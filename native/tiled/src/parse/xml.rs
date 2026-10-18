//! TMX, TSX and TX (XML) into the model, as `DictionaryFromXml.gd` reads them.
//!
//! A recursive descent over `quick-xml` events. Each element fills its struct's fields; what
//! the model has no field for goes into the struct's `extra` the way `DictionaryFromXml.gd`
//! would store it (attributes typed by `insert_attributes`, an unknown child element `foo` as a
//! `foos` array of dictionaries), so [`crate::value`] gives back the same dictionary.

use super::Document;
use crate::godot_str::{is_valid_int, to_float, to_int};
use crate::model::property::Style;
use crate::model::*;
use crate::value::{xml_attr, Value};
use crate::{Error, ErrorKind};
use quick_xml::events::{BytesStart, BytesText, Event};
use quick_xml::Reader;
use std::borrow::Cow;
use std::collections::BTreeMap;

type R<T> = Result<T, Error>;

/// Parses an XML Tiled document (map, tileset or template).
pub fn parse(src: &[u8], path: &str) -> R<Document> {
    let mut x = Xml::new(src, path);
    loop {
        match x.next()? {
            Node::Open(e, empty) => {
                let doc = match e.name().as_ref() {
                    b"map" => {
                        let mut m = x.map(&e, empty)?;
                        m.format = Meta(Format::Xml);
                        Document::Map(m)
                    }
                    b"tileset" => {
                        let mut t = x.tileset(&e, empty)?;
                        t.kind = Some("tileset".into());
                        // A root "type" attribute would override it, as in the dictionary.
                        if let Some(Value::String(k)) = t.extra.remove("type") {
                            t.kind = Some(k);
                        }
                        Document::Tileset(t)
                    }
                    b"template" => Document::Template(x.template(&e, empty)?),
                    other => {
                        return Err(x.error(&format!(
                            "'{}' is not a Tiled map, tileset or template",
                            String::from_utf8_lossy(other)
                        )))
                    }
                };
                return Ok(doc);
            }
            Node::Text(_) => {}
            Node::Close | Node::Eof => return Err(x.error("no root element")),
        }
    }
}

enum Node<'a> {
    Open(BytesStart<'a>, bool),
    Close,
    Text(BytesText<'a>),
    Eof,
}

struct Xml<'a> {
    r: Reader<&'a [u8]>,
    src: &'a [u8],
    /// The document as text (validated once), so attribute keys and values that need no
    /// unescaping are sliced out of it instead of being validated one by one.
    text: Option<&'a str>,
    path: &'a str,
}

fn int(v: &str) -> Option<i64> {
    is_valid_int(v).then(|| to_int(v))
}

fn num(v: &str) -> Option<Num> {
    Num::from_xml(v)
}

fn flag(v: &str) -> Option<bool> {
    match v {
        "0" => Some(false),
        "1" => Some(true),
        _ => None,
    }
}

fn s(v: &str) -> Option<String> {
    Some(v.to_string())
}

/// Godot's `strip_edges`: characters up to the space removed at both ends.
fn strip(v: &str) -> &str {
    v.trim_matches(|c: char| c <= ' ')
}

/// Stores a parsed attribute in a field; false when it does not parse (it then goes to `extra`).
macro_rules! set {
    ($dst:expr, $val:expr) => {
        match $val {
            Some(x) => {
                $dst = Some(x);
                true
            }
            None => false,
        }
    };
}

impl<'a> Xml<'a> {
    fn new(src: &'a [u8], path: &'a str) -> Self {
        let mut r = Reader::from_reader(src);
        r.config_mut().trim_text(false);
        Xml {
            r,
            src,
            text: std::str::from_utf8(src).ok(),
            path,
        }
    }

    fn error(&self, detail: &str) -> Error {
        let pos = (self.r.buffer_position() as usize).min(self.src.len());
        let line = self.src[..pos].iter().filter(|&&c| c == b'\n').count() + 1;
        Error::new(
            ErrorKind::Parse,
            format!(
                "Error parsing file '{}' (around line {line}): {detail}.",
                self.path
            ),
        )
    }

    fn next(&mut self) -> R<Node<'a>> {
        loop {
            match self.r.read_event() {
                Ok(Event::Start(e)) => return Ok(Node::Open(e, false)),
                Ok(Event::Empty(e)) => return Ok(Node::Open(e, true)),
                Ok(Event::End(_)) => return Ok(Node::Close),
                Ok(Event::Text(t)) => return Ok(Node::Text(t)),
                Ok(Event::Eof) => return Ok(Node::Eof),
                // Declarations, comments, CDATA (skipped by Godot's parser too), PIs, doctype.
                Ok(_) => continue,
                Err(e) => return Err(self.error(&e.to_string())),
            }
        }
    }

    fn eof(&self) -> Error {
        self.error("unexpected end of file")
    }

    /// Skips the rest of an element.
    fn skip(&mut self, empty: bool) -> R<()> {
        if empty {
            return Ok(());
        }
        let mut depth = 1;
        while depth > 0 {
            match self.next()? {
                Node::Open(_, false) => depth += 1,
                Node::Close => depth -= 1,
                Node::Eof => return Err(self.eof()),
                _ => {}
            }
        }
        Ok(())
    }

    fn text(&self, t: &BytesText<'a>) -> R<Cow<'a, str>> {
        let raw: &[u8] = t;
        if memchr::memchr(b'&', raw).is_none() {
            if let Some(s) = self.slice(raw) {
                return Ok(Cow::Borrowed(s));
            }
        }
        t.unescape().map_err(|e| self.error(&e.to_string()))
    }

    /// The text of a slice of the document, when the document is valid UTF-8 and the slice
    /// lies in it (it does for everything quick-xml borrows from it).
    fn slice(&self, bytes: &[u8]) -> Option<&'a str> {
        let text = self.text?;
        let start = (bytes.as_ptr() as usize).checked_sub(text.as_ptr() as usize)?;
        text.get(start..start.checked_add(bytes.len())?)
    }

    /// Calls `f` with each attribute (name, unescaped value).
    fn attrs(&self, e: &BytesStart<'a>, mut f: impl FnMut(&str, &str)) -> R<()> {
        for a in e.attributes().with_checks(false) {
            let a = a.map_err(|err| self.error(&err.to_string()))?;
            let key = match self.slice(a.key.as_ref()) {
                Some(k) => k,
                None => std::str::from_utf8(a.key.as_ref())
                    .map_err(|err| self.error(&err.to_string()))?,
            };
            let plain = match &a.value {
                Cow::Borrowed(v) if memchr::memchr(b'&', v).is_none() => self.slice(v),
                _ => None,
            };
            match plain {
                Some(v) => f(key, v),
                None => {
                    let value = a
                        .unescape_value()
                        .map_err(|err| self.error(&err.to_string()))?;
                    f(key, &value);
                }
            }
        }
        Ok(())
    }

    /// Attributes into a struct: `set` takes the known ones, the rest go to `extra`.
    fn fill<T>(
        &self,
        e: &BytesStart<'a>,
        target: &mut T,
        set: impl Fn(&mut T, &str, &str) -> bool,
        extra: impl Fn(&mut T) -> &mut Extra,
    ) -> R<()> {
        self.attrs(e, |k, v| {
            if !set(target, k, v) {
                extra(target).insert(k.to_string(), xml_attr(k, v));
            }
        })
    }

    // ----------------------------------------------------------------------------------------
    // Map, tileset, template

    fn map(&mut self, e: &BytesStart<'a>, empty: bool) -> R<Map> {
        let mut m = Map {
            kind: Some("map".into()),
            ..Default::default()
        };
        self.fill(e, &mut m, map_attr, |m| &mut m.extra)?;
        if empty {
            return Ok(m);
        }
        loop {
            match self.next()? {
                Node::Open(c, em) => match c.name().as_ref() {
                    b"properties" => self.properties(em, &mut m.properties)?,
                    b"tileset" => {
                        let t = self.tileset(&c, em)?;
                        m.tilesets.get_or_insert_with(Vec::new).push(t);
                    }
                    b"layer" | b"objectgroup" | b"imagelayer" | b"group" => {
                        let l = self.layer(&c, em, false)?;
                        m.layers.get_or_insert_with(Vec::new).push(l);
                    }
                    _ => self.generic(&c, em, &mut m.extra)?,
                },
                Node::Text(_) => {}
                Node::Close => return Ok(m),
                Node::Eof => return Err(self.eof()),
            }
        }
    }

    fn tileset(&mut self, e: &BytesStart<'a>, empty: bool) -> R<Tileset> {
        let mut t = Tileset::default();
        self.fill(e, &mut t, tileset_attr, |t| &mut t.extra)?;
        if empty {
            return Ok(t);
        }
        loop {
            match self.next()? {
                Node::Open(c, em) => match c.name().as_ref() {
                    b"image" => {
                        let img = self.image(&c, em)?;
                        img.apply(
                            &mut t.image,
                            &mut t.imagewidth,
                            &mut t.imageheight,
                            &mut t.transparentcolor,
                        );
                    }
                    b"tileoffset" => {
                        let mut o = TileOffset::default();
                        self.fill(
                            &c,
                            &mut o,
                            |o, k, v| match k {
                                "x" => set!(o.x, int(v)),
                                "y" => set!(o.y, int(v)),
                                _ => false,
                            },
                            |o| &mut o.extra,
                        )?;
                        self.generic_children(em, &mut o.extra)?;
                        t.tileoffset = Some(o);
                    }
                    b"grid" => {
                        let mut g = Grid::default();
                        self.fill(
                            &c,
                            &mut g,
                            |g, k, v| match k {
                                "orientation" => set!(g.orientation, s(v)),
                                "width" => set!(g.width, int(v)),
                                "height" => set!(g.height, int(v)),
                                _ => false,
                            },
                            |g| &mut g.extra,
                        )?;
                        self.generic_children(em, &mut g.extra)?;
                        t.grid = Some(g);
                    }
                    b"transformations" => {
                        let mut tr = Transformations::default();
                        self.fill(
                            &c,
                            &mut tr,
                            |tr, k, v| match k {
                                "hflip" => set!(tr.hflip, flag(v)),
                                "vflip" => set!(tr.vflip, flag(v)),
                                "rotate" => set!(tr.rotate, flag(v)),
                                "preferuntransformed" => set!(tr.preferuntransformed, flag(v)),
                                _ => false,
                            },
                            |tr| &mut tr.extra,
                        )?;
                        self.generic_children(em, &mut tr.extra)?;
                        t.transformations = Some(tr);
                    }
                    b"properties" => self.properties(em, &mut t.properties)?,
                    b"tile" => {
                        let tile = self.tile(&c, em)?;
                        t.tiles.get_or_insert_with(Vec::new).push(tile);
                    }
                    b"wangsets" if !em => self.wangsets(&mut t)?,
                    _ => self.generic(&c, em, &mut t.extra)?,
                },
                Node::Text(_) => {}
                Node::Close => return Ok(t),
                Node::Eof => return Err(self.eof()),
            }
        }
    }

    fn template(&mut self, e: &BytesStart<'a>, empty: bool) -> R<Template> {
        let mut t = Template {
            kind: Some("template".into()),
            format: Meta(Format::Xml),
            ..Default::default()
        };
        self.attrs(e, |k, v| {
            t.extra.insert(k.to_string(), xml_attr(k, v));
        })?;
        if let Some(Value::String(k)) = t.extra.remove("type") {
            t.kind = Some(k);
        }
        if empty {
            return Ok(t);
        }
        loop {
            match self.next()? {
                Node::Open(c, em) => match c.name().as_ref() {
                    b"tileset" => {
                        let ts = self.tileset(&c, em)?;
                        t.tilesets.push(ts);
                    }
                    b"object" => {
                        let o = self.object(&c, em)?;
                        t.objects.push(o);
                    }
                    _ => self.generic(&c, em, &mut t.extra)?,
                },
                Node::Text(_) => {}
                Node::Close => return Ok(t),
                Node::Eof => return Err(self.eof()),
            }
        }
    }

    fn image(&mut self, e: &BytesStart<'a>, empty: bool) -> R<ImageAttrs> {
        let mut img = ImageAttrs::default();
        self.attrs(e, |k, v| match k {
            "source" => img.source = s(v),
            "width" => img.width = Some(to_int(v)),
            "height" => img.height = Some(to_int(v)),
            "trans" => img.trans = s(v),
            _ => {}
        })?;
        // An embedded image (<data> child) is not supported, as in the GDScript runtime.
        self.skip(empty)?;
        Ok(img)
    }

    fn tile(&mut self, e: &BytesStart<'a>, empty: bool) -> R<Tile> {
        let mut t = Tile::default();
        self.fill(e, &mut t, tile_attr, |t| &mut t.extra)?;
        if empty {
            return Ok(t);
        }
        loop {
            match self.next()? {
                Node::Open(c, em) => match c.name().as_ref() {
                    b"properties" => self.properties(em, &mut t.properties)?,
                    b"image" => {
                        let img = self.image(&c, em)?;
                        img.apply(
                            &mut t.image,
                            &mut t.imagewidth,
                            &mut t.imageheight,
                            &mut t.transparentcolor,
                        );
                    }
                    b"animation" => {
                        let frames = t.animation.get_or_insert_with(Vec::new);
                        if !em {
                            loop {
                                match self.next()? {
                                    Node::Open(f, fem) if f.name().as_ref() == b"frame" => {
                                        let mut fr = Frame::default();
                                        self.fill(
                                            &f,
                                            &mut fr,
                                            |fr, k, v| match k {
                                                "tileid" => set!(fr.tileid, int(v)),
                                                "duration" => set!(fr.duration, int(v)),
                                                _ => false,
                                            },
                                            |fr| &mut fr.extra,
                                        )?;
                                        self.skip(fem)?;
                                        frames.push(fr);
                                    }
                                    Node::Open(_, fem) => self.skip(fem)?,
                                    Node::Text(_) => {}
                                    Node::Close => break,
                                    Node::Eof => return Err(self.eof()),
                                }
                            }
                        }
                    }
                    b"objectgroup" => {
                        let l = self.layer(&c, em, true)?;
                        t.objectgroup = Some(l);
                    }
                    _ => self.generic(&c, em, &mut t.extra)?,
                },
                Node::Text(_) => {}
                Node::Close => return Ok(t),
                Node::Eof => return Err(self.eof()),
            }
        }
    }

    /// The children of `<wangsets>` (it adds no level of its own in the dictionary).
    fn wangsets(&mut self, t: &mut Tileset) -> R<()> {
        loop {
            match self.next()? {
                Node::Open(c, em) if c.name().as_ref() == b"wangset" => {
                    let w = self.wangset(&c, em)?;
                    t.wangsets.get_or_insert_with(Vec::new).push(w);
                }
                Node::Open(c, em) => self.generic(&c, em, &mut t.extra)?,
                Node::Text(_) => {}
                Node::Close => return Ok(()),
                Node::Eof => return Err(self.eof()),
            }
        }
    }

    fn wangset(&mut self, e: &BytesStart<'a>, empty: bool) -> R<WangSet> {
        let mut w = WangSet::default();
        self.fill(
            e,
            &mut w,
            |w, k, v| match k {
                "name" => set!(w.name, s(v)),
                "class" => set!(w.class, s(v)),
                "type" => set!(w.kind, s(v)),
                "tile" => set!(w.tile, int(v)),
                _ => false,
            },
            |w| &mut w.extra,
        )?;
        if empty {
            return Ok(w);
        }
        loop {
            match self.next()? {
                Node::Open(c, em) => match c.name().as_ref() {
                    b"properties" => self.properties(em, &mut w.properties)?,
                    b"wangcolor" => {
                        let mut col = WangColor::default();
                        self.fill(
                            &c,
                            &mut col,
                            |col, k, v| match k {
                                "name" => set!(col.name, s(v)),
                                "class" => set!(col.class, s(v)),
                                "color" => set!(col.color, s(v)),
                                "tile" => set!(col.tile, int(v)),
                                "probability" => set!(col.probability, num(v)),
                                _ => false,
                            },
                            |col| &mut col.extra,
                        )?;
                        if !em {
                            loop {
                                match self.next()? {
                                    Node::Open(p, pem) if p.name().as_ref() == b"properties" => {
                                        self.properties(pem, &mut col.properties)?
                                    }
                                    Node::Open(p, pem) => self.generic(&p, pem, &mut col.extra)?,
                                    Node::Text(_) => {}
                                    Node::Close => break,
                                    Node::Eof => return Err(self.eof()),
                                }
                            }
                        }
                        w.colors.get_or_insert_with(Vec::new).push(col);
                    }
                    b"wangtile" => {
                        let mut wt = WangTile::default();
                        // Only tileid and wangid, each converted with int(), as the runtime does.
                        self.attrs(&c, |k, v| match k {
                            "tileid" => wt.tileid = Some(to_int(v)),
                            "wangid" => wt.wangid = Some(v.split(',').map(to_int).collect()),
                            _ => {}
                        })?;
                        self.skip(em)?;
                        w.wangtiles.get_or_insert_with(Vec::new).push(wt);
                    }
                    _ => self.generic(&c, em, &mut w.extra)?,
                },
                Node::Text(_) => {}
                Node::Close => return Ok(w),
                Node::Eof => return Err(self.eof()),
            }
        }
    }

    // ----------------------------------------------------------------------------------------
    // Layers and objects

    /// A `<layer>`, `<objectgroup>`, `<imagelayer>` or `<group>`; `tile_group` for a tile's
    /// collision `<objectgroup>`, which gets no `type`.
    fn layer(&mut self, e: &BytesStart<'a>, empty: bool, tile_group: bool) -> R<Layer> {
        let mut l = Layer::default();
        self.fill(e, &mut l, layer_attr, |l| &mut l.extra)?;
        match e.name().as_ref() {
            b"objectgroup" if !tile_group => l.kind = Some("objectgroup".into()),
            b"imagelayer" => l.kind = Some("imagelayer".into()),
            b"group" => l.kind = Some("group".into()),
            _ => {}
        }
        if empty {
            return Ok(l);
        }
        loop {
            match self.next()? {
                Node::Open(c, em) => match c.name().as_ref() {
                    b"properties" => self.properties(em, &mut l.properties)?,
                    b"data" if !em => self.data(&c, &mut l)?,
                    b"object" => {
                        let o = self.object(&c, em)?;
                        l.objects.get_or_insert_with(Vec::new).push(o);
                    }
                    b"image" => {
                        let img = self.image(&c, em)?;
                        img.apply(
                            &mut l.image,
                            &mut l.imagewidth,
                            &mut l.imageheight,
                            &mut l.transparentcolor,
                        );
                    }
                    b"layer" | b"objectgroup" | b"imagelayer" | b"group" => {
                        let child = self.layer(&c, em, false)?;
                        l.layers.get_or_insert_with(Vec::new).push(child);
                    }
                    _ => self.generic(&c, em, &mut l.extra)?,
                },
                Node::Text(_) => {}
                Node::Close => return Ok(l),
                Node::Eof => return Err(self.eof()),
            }
        }
    }

    /// A tile layer's `<data>`: its encoding and its text, chunks or legacy `<tile>` list.
    fn data(&mut self, e: &BytesStart<'a>, l: &mut Layer) -> R<()> {
        l.kind = Some("tilelayer".into());
        let mut encoding = None;
        let mut compression = None;
        self.attrs(e, |k, v| match k {
            "encoding" => encoding = s(v),
            "compression" => compression = s(v),
            _ => {}
        })?;
        // No encoding: the deprecated <tile gid="..."/> list, delivered like csv.
        let legacy = encoding.is_none();
        l.encoding = Some(encoding.unwrap_or_else(|| "csv".into()));
        if compression.is_some() {
            l.compression = compression;
        }
        loop {
            match self.next()? {
                Node::Text(t) => {
                    let text = self.text(&t)?;
                    let stripped = strip(&text);
                    if !stripped.is_empty() {
                        l.data = Some(Payload::Text(stripped.to_string()));
                    }
                }
                Node::Open(c, em) => match c.name().as_ref() {
                    b"chunk" => {
                        let ch = self.chunk(&c, em, legacy)?;
                        l.chunks.get_or_insert_with(Vec::new).push(ch);
                    }
                    b"tile" if legacy => {
                        let gid = self.legacy_gid(&c, em)?;
                        push_gid(&mut l.data, gid);
                    }
                    _ => self.generic(&c, em, &mut l.extra)?,
                },
                Node::Close => return Ok(()),
                Node::Eof => return Err(self.eof()),
            }
        }
    }

    fn legacy_gid(&mut self, e: &BytesStart<'a>, empty: bool) -> R<u32> {
        let mut gid = 0;
        self.attrs(e, |k, v| {
            if k == "gid" {
                gid = to_int(v) as u32;
            }
        })?;
        self.skip(empty)?;
        Ok(gid)
    }

    fn chunk(&mut self, e: &BytesStart<'a>, empty: bool, legacy: bool) -> R<Chunk> {
        let mut ch = Chunk::default();
        self.fill(
            e,
            &mut ch,
            |ch, k, v| match k {
                "x" => set!(ch.x, int(v)),
                "y" => set!(ch.y, int(v)),
                "width" => set!(ch.width, int(v)),
                "height" => set!(ch.height, int(v)),
                _ => false,
            },
            |ch| &mut ch.extra,
        )?;
        if empty {
            return Ok(ch);
        }
        loop {
            match self.next()? {
                Node::Text(t) => {
                    let text = self.text(&t)?;
                    let stripped = strip(&text);
                    if !stripped.is_empty() {
                        ch.data = Some(Payload::Text(stripped.to_string()));
                    }
                }
                Node::Open(c, em) if legacy && c.name().as_ref() == b"tile" => {
                    let gid = self.legacy_gid(&c, em)?;
                    push_gid(&mut ch.data, gid);
                }
                Node::Open(c, em) => self.generic(&c, em, &mut ch.extra)?,
                Node::Close => return Ok(ch),
                Node::Eof => return Err(self.eof()),
            }
        }
    }

    fn object(&mut self, e: &BytesStart<'a>, empty: bool) -> R<Object> {
        let mut o = Object::default();
        self.fill(e, &mut o, object_attr, |o| &mut o.extra)?;
        if empty {
            return Ok(o);
        }
        loop {
            match self.next()? {
                Node::Open(c, em) => match c.name().as_ref() {
                    b"properties" => self.properties(em, &mut o.properties)?,
                    b"point" => {
                        o.point = Some(true);
                        self.skip(em)?;
                    }
                    b"ellipse" => {
                        o.ellipse = Some(true);
                        self.skip(em)?;
                    }
                    b"capsule" => {
                        o.capsule = Some(true);
                        self.skip(em)?;
                    }
                    b"polygon" => {
                        o.polygon = Some(self.points(&c, em)?);
                    }
                    b"polyline" => {
                        o.polyline = Some(self.points(&c, em)?);
                    }
                    b"text" => {
                        o.text = Some(Box::new(self.text_object(&c, em)?));
                    }
                    _ => self.generic(&c, em, &mut o.extra)?,
                },
                Node::Text(_) => {}
                Node::Close => return Ok(o),
                Node::Eof => return Err(self.eof()),
            }
        }
    }

    fn points(&mut self, e: &BytesStart<'a>, empty: bool) -> R<Vec<Point>> {
        let mut pts = Vec::new();
        self.attrs(e, |k, v| {
            if k == "points" {
                pts = parse_points(v);
            }
        })?;
        self.skip(empty)?;
        Ok(pts)
    }

    fn text_object(&mut self, e: &BytesStart<'a>, empty: bool) -> R<Text> {
        let mut t = Text::default();
        self.fill(
            e,
            &mut t,
            |t, k, v| match k {
                "fontfamily" => set!(t.fontfamily, s(v)),
                "pixelsize" => set!(t.pixelsize, int(v)),
                "wrap" => set!(t.wrap, Some(v == "1")),
                "color" => set!(t.color, s(v)),
                "bold" => set!(t.bold, flag(v)),
                "italic" => set!(t.italic, flag(v)),
                "underline" => set!(t.underline, flag(v)),
                "strikeout" => set!(t.strikeout, flag(v)),
                "kerning" => set!(t.kerning, flag(v)),
                "halign" => set!(t.halign, s(v)),
                "valign" => set!(t.valign, s(v)),
                _ => false,
            },
            |t| &mut t.extra,
        )?;
        if empty {
            return Ok(t);
        }
        loop {
            match self.next()? {
                Node::Text(x) => {
                    let text = self.text(&x)?;
                    if !strip(&text).is_empty() {
                        t.text = Some(text.replace('\r', ""));
                    }
                }
                Node::Open(c, em) => self.generic(&c, em, &mut t.extra)?,
                Node::Close => return Ok(t),
                Node::Eof => return Err(self.eof()),
            }
        }
    }

    // ----------------------------------------------------------------------------------------
    // Properties

    /// A `<properties>` element (top level): its `<property>` children appended.
    fn properties(&mut self, empty: bool, out: &mut Option<Vec<Property>>) -> R<()> {
        let list = out.get_or_insert_with(Vec::new);
        if empty {
            return Ok(());
        }
        loop {
            match self.next()? {
                Node::Open(c, em) if c.name().as_ref() == b"property" => {
                    let p = self.property(&c, em, false)?;
                    list.push(p);
                }
                Node::Open(_, em) => self.skip(em)?,
                Node::Text(_) => {}
                Node::Close => return Ok(()),
                Node::Eof => return Err(self.eof()),
            }
        }
    }

    /// A top-level `<property>` or (`item`) an item of a top-level list: a dictionary with
    /// its attributes; a class value from the nested `<properties>`, a list's from its items.
    fn property(&mut self, e: &BytesStart<'a>, empty: bool, item: bool) -> R<Property> {
        let mut p = Property::default();
        let mut raw: Option<PropertyValue> = None;
        let mut kind = ValueKind::String;
        self.attrs(e, |k, v| match k {
            "name" => p.name = s(v),
            "type" => {
                kind = ValueKind::of(v);
                p.kind = s(v);
            }
            "propertytype" => p.propertytype = s(v),
            // Typed here when the type came first (as Tiled writes it), else below.
            "value" => raw = Some(typed_top_str(kind, v)),
            _ => {
                p.extra.insert(k.to_string(), xml_attr(k, v));
            }
        })?;
        match kind {
            ValueKind::List => p.value = Some(PropertyValue::List(self.list_items(empty)?)),
            ValueKind::Class => p.value = Some(PropertyValue::Class(self.class_members(empty)?)),
            _ => {
                p.value = raw.map(|r| match r {
                    PropertyValue::String(r) => typed_top(kind, r),
                    other => other,
                });
                if !empty {
                    loop {
                        match self.next()? {
                            Node::Text(t) if !item => {
                                let text = self.text(&t)?;
                                if !strip(&text).is_empty() {
                                    // A multi-line string: the element's text, kept untyped.
                                    p.value = Some(PropertyValue::String(text.replace('\r', "")));
                                    p.style = Meta(Style::XmlText);
                                }
                            }
                            Node::Text(_) => {}
                            Node::Open(_, em) => self.skip(em)?,
                            Node::Close => break,
                            Node::Eof => return Err(self.eof()),
                        }
                    }
                }
            }
        }
        Ok(p)
    }

    /// The `<item>` children of a top-level list property.
    fn list_items(&mut self, empty: bool) -> R<Vec<Property>> {
        let mut items = Vec::new();
        if empty {
            return Ok(items);
        }
        loop {
            match self.next()? {
                Node::Open(c, em) if c.name().as_ref() == b"item" => {
                    let i = self.property(&c, em, true)?;
                    items.push(i);
                }
                Node::Open(_, em) => self.skip(em)?,
                Node::Text(_) => {}
                Node::Close => return Ok(items),
                Node::Eof => return Err(self.eof()),
            }
        }
    }

    /// The members of a class value: the `<property>` children of the nested `<properties>`.
    fn class_members(&mut self, empty: bool) -> R<BTreeMap<String, PropertyValue>> {
        let mut members = BTreeMap::new();
        if empty {
            return Ok(members);
        }
        loop {
            match self.next()? {
                Node::Open(c, em) if c.name().as_ref() == b"properties" => {
                    if em {
                        continue;
                    }
                    loop {
                        match self.next()? {
                            Node::Open(m, mem) if m.name().as_ref() == b"property" => {
                                self.member(&m, mem, &mut members)?
                            }
                            Node::Open(_, mem) => self.skip(mem)?,
                            Node::Text(_) => {}
                            Node::Close => break,
                            Node::Eof => return Err(self.eof()),
                        }
                    }
                }
                Node::Open(_, em) => self.skip(em)?,
                Node::Text(_) => {}
                Node::Close => return Ok(members),
                Node::Eof => return Err(self.eof()),
            }
        }
    }

    /// A class member: `name: typed value` (`DictionaryFromXml.class_member_element`).
    fn member(
        &mut self,
        e: &BytesStart<'a>,
        empty: bool,
        out: &mut BTreeMap<String, PropertyValue>,
    ) -> R<()> {
        let (mut name, mut kind, mut raw) = (String::new(), "string".to_string(), String::new());
        self.attrs(e, |k, v| match k {
            "name" => name = v.to_string(),
            "type" => kind = v.to_string(),
            "value" => raw = v.to_string(),
            _ => {}
        })?;
        let value = match kind.as_str() {
            "class" => PropertyValue::Class(self.class_members(empty)?),
            "list" => PropertyValue::Array(self.member_items(empty)?),
            _ => {
                let mut v = typed_member(&kind, &raw);
                if !empty {
                    loop {
                        match self.next()? {
                            Node::Text(t) => {
                                let text = self.text(&t)?;
                                if !strip(&text).is_empty() {
                                    v = typed_member(&kind, &text.replace('\r', ""));
                                }
                            }
                            Node::Open(_, em) => self.skip(em)?,
                            Node::Close => break,
                            Node::Eof => return Err(self.eof()),
                        }
                    }
                }
                v
            }
        };
        out.insert(name, value);
        Ok(())
    }

    /// The items of a list inside a class value: typed values (`class_member_list_item`).
    fn member_items(&mut self, empty: bool) -> R<Vec<PropertyValue>> {
        let mut items = Vec::new();
        if empty {
            return Ok(items);
        }
        loop {
            match self.next()? {
                Node::Open(c, em) if c.name().as_ref() == b"item" => {
                    let (mut kind, mut raw) = ("string".to_string(), String::new());
                    self.attrs(&c, |k, v| match k {
                        "type" => kind = v.to_string(),
                        "value" => raw = v.to_string(),
                        _ => {}
                    })?;
                    let v = match kind.as_str() {
                        "class" => PropertyValue::Class(self.class_members(em)?),
                        "list" => PropertyValue::Array(self.member_items(em)?),
                        _ => {
                            self.skip(em)?;
                            typed_member(&kind, &raw)
                        }
                    };
                    items.push(v);
                }
                Node::Open(_, em) => self.skip(em)?,
                Node::Text(_) => {}
                Node::Close => return Ok(items),
                Node::Eof => return Err(self.eof()),
            }
        }
    }

    // ----------------------------------------------------------------------------------------
    // Elements the model does not know

    /// An unknown element, stored in `parent` as `DictionaryFromXml.gd` would store it.
    fn generic(&mut self, e: &BytesStart<'a>, empty: bool, parent: &mut Extra) -> R<()> {
        let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
        match name.as_str() {
            "image" => {
                let img = self.image(e, empty)?;
                if let Some(src) = img.source {
                    parent.insert("image".into(), Value::String(src));
                }
                if let Some(w) = img.width {
                    parent.insert("imagewidth".into(), Value::Int(w));
                }
                if let Some(h) = img.height {
                    parent.insert("imageheight".into(), Value::Int(h));
                }
                if let Some(t) = img.trans {
                    parent.insert("transparentcolor".into(), Value::String(t));
                }
            }
            "point" | "ellipse" | "capsule" => {
                parent.insert(name, Value::Bool(true));
                self.skip(empty)?;
            }
            "polygon" | "polyline" => {
                let pts = self.points(e, empty)?;
                let arr = pts
                    .iter()
                    .map(|p| {
                        Value::Dict(BTreeMap::from([
                            ("x".to_string(), Value::Float(p.x)),
                            ("y".to_string(), Value::Float(p.y)),
                        ]))
                    })
                    .collect();
                parent.insert(name, Value::Array(arr));
            }
            "properties" => {
                let mut props = None;
                self.properties(empty, &mut props)?;
                let values: Vec<Value> = props
                    .unwrap_or_default()
                    .iter()
                    .map(crate::value::xml_property)
                    .collect();
                match parent.get_mut("properties") {
                    Some(Value::Array(a)) => a.extend(values),
                    _ => {
                        parent.insert(name, Value::Array(values));
                    }
                }
            }
            "text" | "tileoffset" | "grid" | "objectgroup" => {
                let d = self.generic_dict(e, empty)?;
                parent.insert(name, Value::Dict(d));
            }
            _ => {
                let key = if name == "wangcolor" {
                    "colors".to_string()
                } else {
                    format!("{name}s")
                };
                let d = self.generic_dict(e, empty)?;
                match parent.get_mut(&key) {
                    Some(Value::Array(a)) => a.push(Value::Dict(d)),
                    _ => {
                        parent.insert(key, Value::Array(vec![Value::Dict(d)]));
                    }
                }
            }
        }
        Ok(())
    }

    fn generic_dict(&mut self, e: &BytesStart<'a>, empty: bool) -> R<Extra> {
        let mut d = Extra::new();
        self.attrs(e, |k, v| {
            d.insert(k.to_string(), xml_attr(k, v));
        })?;
        self.generic_children(empty, &mut d)?;
        Ok(d)
    }

    fn generic_children(&mut self, empty: bool, d: &mut Extra) -> R<()> {
        if empty {
            return Ok(());
        }
        loop {
            match self.next()? {
                Node::Open(c, em) => self.generic(&c, em, d)?,
                Node::Text(_) => {}
                Node::Close => return Ok(()),
                Node::Eof => return Err(self.eof()),
            }
        }
    }
}

/// `<image>` attributes as the GDScript runtime keeps them.
#[derive(Default)]
struct ImageAttrs {
    source: Option<String>,
    width: Option<i64>,
    height: Option<i64>,
    trans: Option<String>,
}

impl ImageAttrs {
    fn apply(
        self,
        image: &mut Option<String>,
        width: &mut Option<i64>,
        height: &mut Option<i64>,
        trans: &mut Option<String>,
    ) {
        if self.source.is_some() {
            *image = self.source;
        }
        if self.width.is_some() {
            *width = self.width;
        }
        if self.height.is_some() {
            *height = self.height;
        }
        if self.trans.is_some() {
            *trans = self.trans;
        }
    }
}

fn push_gid(data: &mut Option<Payload>, gid: u32) {
    match data {
        Some(Payload::Gids(g)) => g.push(gid),
        None => *data = Some(Payload::Gids(vec![gid])),
        Some(Payload::Text(_)) => {}
    }
}

/// `points="x,y x,y ..."`: each coordinate with `float()`, a missing y 0.
fn parse_points(v: &str) -> Vec<Point> {
    v.split(' ')
        .filter(|p| !p.is_empty())
        .map(|pt| {
            let mut c = pt.split(',');
            let x = to_float(c.next().unwrap_or(""));
            let y = c.next().map(to_float).unwrap_or(0.0);
            Point { x, y }
        })
        .collect()
}

/// A property's declared type, as far as reading its value goes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ValueKind {
    String,
    Bool,
    Number,
    Class,
    List,
}

impl ValueKind {
    fn of(kind: &str) -> ValueKind {
        match kind {
            "bool" => ValueKind::Bool,
            "int" | "float" | "object" => ValueKind::Number,
            "class" => ValueKind::Class,
            "list" => ValueKind::List,
            _ => ValueKind::String,
        }
    }
}

/// [`typed_top`] without allocating for a number or a bool.
fn typed_top_str(kind: ValueKind, raw: &str) -> PropertyValue {
    match kind {
        ValueKind::Bool if raw == "true" || raw == "false" => PropertyValue::Bool(raw == "true"),
        ValueKind::Number => match Num::from_xml(raw) {
            Some(n) => PropertyValue::Number(n),
            None => PropertyValue::String(raw.to_string()),
        },
        _ => PropertyValue::String(raw.to_string()),
    }
}

/// A top-level property's `value` attribute, read by its declared type; a value that does not
/// read as that type stays a string (the dictionary then types it like any attribute).
fn typed_top(kind: ValueKind, raw: String) -> PropertyValue {
    match kind {
        ValueKind::Bool if raw == "true" || raw == "false" => PropertyValue::Bool(raw == "true"),
        ValueKind::Number => match Num::from_xml(&raw) {
            Some(n) => PropertyValue::Number(n),
            None => PropertyValue::String(raw),
        },
        _ => PropertyValue::String(raw),
    }
}

/// `DictionaryFromXml.get_typed_member_value`.
fn typed_member(kind: &str, raw: &str) -> PropertyValue {
    match kind {
        "bool" => PropertyValue::Bool(raw == "true"),
        "int" | "object" => PropertyValue::Number(Num::Int(to_int(raw))),
        "float" => PropertyValue::Number(Num::Float(to_float(raw))),
        _ => PropertyValue::String(raw.to_string()),
    }
}

fn map_attr(m: &mut Map, k: &str, v: &str) -> bool {
    match k {
        "type" => set!(m.kind, s(v)),
        "version" => set!(m.version, s(v)),
        "tiledversion" => set!(m.tiledversion, s(v)),
        "class" => set!(m.class, s(v)),
        "orientation" => set!(m.orientation, s(v)),
        "renderorder" => set!(m.renderorder, s(v)),
        "compressionlevel" => set!(m.compressionlevel, int(v)),
        "width" => set!(m.width, int(v)),
        "height" => set!(m.height, int(v)),
        "tilewidth" => set!(m.tilewidth, int(v)),
        "tileheight" => set!(m.tileheight, int(v)),
        "hexsidelength" => set!(m.hexsidelength, int(v)),
        "staggeraxis" => set!(m.staggeraxis, s(v)),
        "staggerindex" => set!(m.staggerindex, s(v)),
        "parallaxoriginx" => set!(m.parallaxoriginx, num(v)),
        "parallaxoriginy" => set!(m.parallaxoriginy, num(v)),
        "backgroundcolor" => set!(m.backgroundcolor, s(v)),
        "nextlayerid" => set!(m.nextlayerid, int(v)),
        "nextobjectid" => set!(m.nextobjectid, int(v)),
        "infinite" => set!(m.infinite, Some(v == "1")),
        _ => false,
    }
}

fn tileset_attr(t: &mut Tileset, k: &str, v: &str) -> bool {
    match k {
        "version" => set!(t.version, s(v)),
        "tiledversion" => set!(t.tiledversion, s(v)),
        "firstgid" => set!(t.firstgid, int(v)),
        "source" => set!(t.source, s(v)),
        "name" => set!(t.name, s(v)),
        "class" => set!(t.class, s(v)),
        "tilewidth" => set!(t.tilewidth, int(v)),
        "tileheight" => set!(t.tileheight, int(v)),
        "tilecount" => set!(t.tilecount, int(v)),
        "columns" => set!(t.columns, int(v)),
        "spacing" => set!(t.spacing, int(v)),
        "margin" => set!(t.margin, int(v)),
        "objectalignment" => set!(t.objectalignment, s(v)),
        "tilerendersize" => set!(t.tilerendersize, s(v)),
        "fillmode" => set!(t.fillmode, s(v)),
        "backgroundcolor" => set!(t.backgroundcolor, s(v)),
        _ => false,
    }
}

fn tile_attr(t: &mut Tile, k: &str, v: &str) -> bool {
    match k {
        "id" => set!(t.id, int(v)),
        "type" => set!(t.kind, s(v)),
        "class" => set!(t.class, s(v)),
        "probability" => set!(t.probability, num(v)),
        "x" => set!(t.x, int(v)),
        "y" => set!(t.y, int(v)),
        "width" => set!(t.width, int(v)),
        "height" => set!(t.height, int(v)),
        _ => false,
    }
}

fn layer_attr(l: &mut Layer, k: &str, v: &str) -> bool {
    match k {
        "type" => set!(l.kind, s(v)),
        "id" => set!(l.id, int(v)),
        "name" => set!(l.name, s(v)),
        "class" => set!(l.class, s(v)),
        "x" => set!(l.x, int(v)),
        "y" => set!(l.y, int(v)),
        "width" => set!(l.width, int(v)),
        "height" => set!(l.height, int(v)),
        "opacity" => set!(l.opacity, num(v)),
        "visible" => set!(l.visible, Some(v == "1")),
        "locked" => set!(l.locked, flag(v)),
        "offsetx" => set!(l.offsetx, num(v)),
        "offsety" => set!(l.offsety, num(v)),
        "parallaxx" => set!(l.parallaxx, num(v)),
        "parallaxy" => set!(l.parallaxy, num(v)),
        "tintcolor" => set!(l.tintcolor, s(v)),
        "mode" => set!(l.mode, s(v)),
        "color" => set!(l.color, s(v)),
        "draworder" => set!(l.draworder, s(v)),
        "repeatx" => set!(l.repeatx, flag(v)),
        "repeaty" => set!(l.repeaty, flag(v)),
        _ => false,
    }
}

fn object_attr(o: &mut Object, k: &str, v: &str) -> bool {
    match k {
        "id" => set!(o.id, int(v)),
        "name" => set!(o.name, s(v)),
        "type" => set!(o.kind, s(v)),
        "class" => set!(o.class, s(v)),
        "x" => set!(o.x, num(v)),
        "y" => set!(o.y, num(v)),
        "width" => set!(o.width, num(v)),
        "height" => set!(o.height, num(v)),
        "rotation" => set!(o.rotation, num(v)),
        "opacity" => set!(o.opacity, num(v)),
        "gid" => set!(o.gid, int(v).and_then(|g| u32::try_from(g).ok())),
        "visible" => set!(o.visible, Some(v == "1")),
        "template" => set!(o.template, s(v)),
        _ => false,
    }
}
