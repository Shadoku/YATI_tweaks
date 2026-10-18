//! [`Value`]: the model as the dictionary the GDScript runtime's `DictionaryBuilder.gd` builds.
//!
//! For a JSON file that dictionary is Godot's JSON parse of the file, in which every number is
//! a float. For an XML file it is what `DictionaryFromXml.gd` makes of it: attributes typed by
//! `insert_attributes` (an integer when `is_valid_int`, a float when `is_valid_float`, else a
//! string; `infinite`, `visible` and `wrap` as bools; keys containing `version` kept as
//! strings), image attributes as `image`/`imagewidth`/`imageheight`/`transparentcolor`, csv
//! payloads as integer arrays, class-typed property values as `{member: typed value}`
//! dictionaries, a tileset's `<transformations>` as a one-element `transformationss` array.
//! [`Map::to_value`] reproduces that for both formats; it is the parity target of the
//! test suite (`tests/parity.rs` against dumps of the GDScript runtime).

use crate::godot_str;
use crate::model::property::Style;
use crate::model::*;
use serde::{Deserialize, Deserializer};
use std::collections::BTreeMap;
use std::fmt::{self, Write as _};

/// A dictionary value: what a Godot `Variant` in the GDScript dictionary holds.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Value {
    /// `null`.
    #[default]
    Null,
    /// A bool.
    Bool(bool),
    /// An int.
    Int(i64),
    /// A float.
    Float(f64),
    /// A string.
    String(String),
    /// An array.
    Array(Vec<Value>),
    /// A dictionary (keys sorted).
    Dict(BTreeMap<String, Value>),
}

impl Value {
    /// The value under a key of a dictionary.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Dict(d) => d.get(key),
            _ => None,
        }
    }

    /// The element at an index of an array.
    pub fn at(&self, index: usize) -> Option<&Value> {
        match self {
            Value::Array(a) => a.get(index),
            _ => None,
        }
    }

    /// The string, if this is one.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    /// The number as `f64`, if this is an int or a float.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }

    /// Every int turned into a float, recursively (what Godot's JSON parser does).
    pub fn floatify(&self) -> Value {
        match self {
            Value::Int(i) => Value::Float(*i as f64),
            Value::Array(a) => Value::Array(a.iter().map(Value::floatify).collect()),
            Value::Dict(d) => {
                Value::Dict(d.iter().map(|(k, v)| (k.clone(), v.floatify())).collect())
            }
            other => other.clone(),
        }
    }

    /// The value as JSON text, keys sorted, floats always with a fraction or an exponent
    /// (`5.0`), so ints and floats stay apart, as `JSON.stringify(v, "", true, true)` prints.
    pub fn to_json(&self) -> String {
        let mut s = String::new();
        self.write_json(&mut s);
        s
    }

    fn write_json(&self, out: &mut String) {
        match self {
            Value::Null => out.push_str("null"),
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Value::Int(i) => {
                let _ = write!(out, "{i}");
            }
            Value::Float(f) => write_float(*f, out),
            Value::String(s) => write_json_string(s, out),
            Value::Array(a) => {
                out.push('[');
                for (i, v) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    v.write_json(out);
                }
                out.push(']');
            }
            Value::Dict(d) => {
                out.push('{');
                for (i, (k, v)) in d.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_json_string(k, out);
                    out.push(':');
                    v.write_json(out);
                }
                out.push('}');
            }
        }
    }
}

fn write_float(f: f64, out: &mut String) {
    if !f.is_finite() {
        out.push_str("null");
        return;
    }
    let start = out.len();
    let _ = write!(out, "{f}");
    if !out[start..].contains(['.', 'e', 'E']) {
        out.push_str(".0");
    }
}

fn write_json_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_json())
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = Value;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("any JSON value")
            }
            fn visit_unit<E>(self) -> Result<Value, E> {
                Ok(Value::Null)
            }
            fn visit_none<E>(self) -> Result<Value, E> {
                Ok(Value::Null)
            }
            fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<Value, D::Error> {
                Value::deserialize(d)
            }
            fn visit_bool<E>(self, v: bool) -> Result<Value, E> {
                Ok(Value::Bool(v))
            }
            fn visit_i64<E>(self, v: i64) -> Result<Value, E> {
                Ok(Value::Int(v))
            }
            fn visit_u64<E>(self, v: u64) -> Result<Value, E> {
                Ok(i64::try_from(v)
                    .map(Value::Int)
                    .unwrap_or(Value::Float(v as f64)))
            }
            fn visit_f64<E>(self, v: f64) -> Result<Value, E> {
                Ok(Value::Float(v))
            }
            fn visit_str<E>(self, v: &str) -> Result<Value, E> {
                Ok(Value::String(v.to_string()))
            }
            fn visit_string<E>(self, v: String) -> Result<Value, E> {
                Ok(Value::String(v))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut a: A) -> Result<Value, A::Error> {
                let mut out = Vec::new();
                while let Some(v) = a.next_element()? {
                    out.push(v);
                }
                Ok(Value::Array(out))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut a: A) -> Result<Value, A::Error> {
                let mut out = BTreeMap::new();
                while let Some((k, v)) = a.next_entry::<String, Value>()? {
                    out.insert(k, v);
                }
                Ok(Value::Dict(out))
            }
        }
        d.deserialize_any(V)
    }
}

/// An XML attribute's value as `insert_attributes` types it, without the key-specific rules.
pub(crate) fn xml_typed(raw: &str) -> Value {
    if godot_str::is_valid_int(raw) {
        Value::Int(godot_str::to_int(raw))
    } else if godot_str::is_valid_float(raw) {
        Value::Float(godot_str::to_float(raw))
    } else {
        Value::String(raw.to_string())
    }
}

/// An XML attribute's value as `insert_attributes` stores it under `key`.
pub(crate) fn xml_attr(key: &str, raw: &str) -> Value {
    match key {
        "infinite" | "visible" | "wrap" => Value::Bool(raw == "1"),
        k if k.contains("version") => Value::String(raw.to_string()),
        _ => xml_typed(raw),
    }
}

/// How the document being turned into a dictionary was read.
#[derive(Clone, Copy)]
struct Cx {
    json: bool,
}

impl Cx {
    fn of(format: Format) -> Cx {
        Cx {
            json: format == Format::Json,
        }
    }
}

/// A dictionary being built for one struct.
struct D {
    m: BTreeMap<String, Value>,
    cx: Cx,
}

impl D {
    fn new(cx: Cx, extra: &Extra) -> D {
        let m = if cx.json {
            extra
                .iter()
                .map(|(k, v)| (k.clone(), v.floatify()))
                .collect()
        } else {
            extra.clone()
        };
        D { m, cx }
    }
    fn put(&mut self, k: &str, v: Value) {
        self.m.insert(k.to_string(), v);
    }
    /// A string field: typed by the XML rules in an XML document.
    fn s(&mut self, k: &str, v: &Option<String>) {
        if let Some(v) = v {
            let val = if self.cx.json {
                Value::String(v.clone())
            } else {
                xml_typed(v)
            };
            self.put(k, val);
        }
    }
    /// A string field the XML side stores untyped.
    fn raw(&mut self, k: &str, v: &Option<String>) {
        if let Some(v) = v {
            self.put(k, Value::String(v.clone()));
        }
    }
    fn i(&mut self, k: &str, v: &Option<i64>) {
        if let Some(v) = *v {
            let val = int(self.cx, v);
            self.put(k, val);
        }
    }
    fn n(&mut self, k: &str, v: &Option<Num>) {
        if let Some(v) = *v {
            let val = num(self.cx, v);
            self.put(k, val);
        }
    }
    fn b(&mut self, k: &str, v: &Option<bool>) {
        if let Some(v) = *v {
            self.put(k, Value::Bool(v));
        }
    }
    /// A bool that TMX writes as `0`/`1` and the XML side therefore types as an int.
    fn flag(&mut self, k: &str, v: &Option<bool>) {
        if let Some(v) = *v {
            let val = if self.cx.json {
                Value::Bool(v)
            } else {
                Value::Int(i64::from(v))
            };
            self.put(k, val);
        }
    }
    fn props(&mut self, v: &Option<Vec<Property>>) {
        if let Some(v) = v {
            let cx = self.cx;
            self.put(
                "properties",
                Value::Array(v.iter().map(|p| property(p, cx)).collect()),
            );
        }
    }
    fn list<T>(&mut self, k: &str, v: &Option<Vec<T>>, f: impl Fn(&T, Cx) -> Value) {
        if let Some(v) = v {
            let cx = self.cx;
            self.put(k, Value::Array(v.iter().map(|t| f(t, cx)).collect()));
        }
    }
    fn one<T>(&mut self, k: &str, v: &Option<T>, f: impl Fn(&T, Cx) -> Value) {
        if let Some(v) = v {
            let val = f(v, self.cx);
            self.put(k, val);
        }
    }
    fn done(self) -> Value {
        Value::Dict(self.m)
    }
}

fn int(cx: Cx, v: i64) -> Value {
    if cx.json {
        Value::Float(v as f64)
    } else {
        Value::Int(v)
    }
}

fn num(cx: Cx, v: Num) -> Value {
    match (cx.json, v) {
        (false, Num::Int(i)) => Value::Int(i),
        (_, n) => Value::Float(n.as_f64()),
    }
}

impl Map {
    /// The map as the GDScript runtime's dictionary (`DictionaryBuilder.get_dictionary`).
    ///
    /// External tilesets and templates are references here, as in the file; their own
    /// dictionaries come from [`Tileset::to_value`] and [`Template::to_value`]. Custom types
    /// the loader merged in (from a project file) are part of it.
    pub fn to_value(&self) -> Value {
        map(self, Cx::of(self.format.0))
    }

    /// [`Map::to_value`] as JSON text.
    pub fn to_json(&self) -> String {
        self.to_value().to_json()
    }
}

impl Tileset {
    /// The tileset as the GDScript runtime's dictionary of its file.
    pub fn to_value(&self) -> Value {
        tileset(self, Cx::of(self.format.0))
    }

    /// [`Tileset::to_value`] as JSON text.
    pub fn to_json(&self) -> String {
        self.to_value().to_json()
    }
}

impl Template {
    /// The template as the GDScript runtime's dictionary of its file.
    pub fn to_value(&self) -> Value {
        let cx = Cx::of(self.format.0);
        let mut d = D::new(cx, &self.extra);
        d.raw("type", &self.kind);
        if cx.json {
            if let Some(t) = self.tilesets.first() {
                d.put("tileset", tileset(t, cx));
            }
            if let Some(o) = self.objects.first() {
                d.put("object", object(o, cx));
            }
        } else {
            if !self.tilesets.is_empty() {
                d.put(
                    "tilesets",
                    Value::Array(self.tilesets.iter().map(|t| tileset(t, cx)).collect()),
                );
            }
            if !self.objects.is_empty() {
                d.put(
                    "objects",
                    Value::Array(self.objects.iter().map(|o| object(o, cx)).collect()),
                );
            }
        }
        d.done()
    }

    /// [`Template::to_value`] as JSON text.
    pub fn to_json(&self) -> String {
        self.to_value().to_json()
    }
}

impl Layer {
    /// The layer as its dictionary in the map's (`json`: true for a TMJ map).
    pub fn to_value(&self, json: bool) -> Value {
        layer(self, Cx { json })
    }
}

impl Object {
    /// The object as its dictionary in the map's (`json`: true for a TMJ map).
    pub fn to_value(&self, json: bool) -> Value {
        object(self, Cx { json })
    }
}

fn map(m: &Map, cx: Cx) -> Value {
    let mut d = D::new(cx, &m.extra);
    d.raw("type", &m.kind);
    d.raw("version", &m.version);
    d.raw("tiledversion", &m.tiledversion);
    d.s("class", &m.class);
    d.s("orientation", &m.orientation);
    d.s("renderorder", &m.renderorder);
    d.i("compressionlevel", &m.compressionlevel);
    d.i("width", &m.width);
    d.i("height", &m.height);
    d.i("tilewidth", &m.tilewidth);
    d.i("tileheight", &m.tileheight);
    d.i("hexsidelength", &m.hexsidelength);
    d.s("staggeraxis", &m.staggeraxis);
    d.s("staggerindex", &m.staggerindex);
    d.n("parallaxoriginx", &m.parallaxoriginx);
    d.n("parallaxoriginy", &m.parallaxoriginy);
    d.s("backgroundcolor", &m.backgroundcolor);
    d.i("nextlayerid", &m.nextlayerid);
    d.i("nextobjectid", &m.nextobjectid);
    d.b("infinite", &m.infinite);
    d.props(&m.properties);
    d.list("tilesets", &m.tilesets, tileset);
    d.list("layers", &m.layers, layer);
    d.done()
}

fn tileset(t: &Tileset, cx: Cx) -> Value {
    let mut d = D::new(cx, &t.extra);
    d.raw("type", &t.kind);
    d.raw("version", &t.version);
    d.raw("tiledversion", &t.tiledversion);
    d.i("firstgid", &t.firstgid);
    d.s("source", &t.source);
    d.s("name", &t.name);
    d.s("class", &t.class);
    d.i("tilewidth", &t.tilewidth);
    d.i("tileheight", &t.tileheight);
    d.i("tilecount", &t.tilecount);
    d.i("columns", &t.columns);
    d.i("spacing", &t.spacing);
    d.i("margin", &t.margin);
    d.s("objectalignment", &t.objectalignment);
    d.s("tilerendersize", &t.tilerendersize);
    d.s("fillmode", &t.fillmode);
    d.s("backgroundcolor", &t.backgroundcolor);
    d.raw("image", &t.image);
    d.i("imagewidth", &t.imagewidth);
    d.i("imageheight", &t.imageheight);
    d.raw("transparentcolor", &t.transparentcolor);
    d.one("tileoffset", &t.tileoffset, |o, cx| {
        let mut d = D::new(cx, &o.extra);
        d.i("x", &o.x);
        d.i("y", &o.y);
        d.done()
    });
    d.one("grid", &t.grid, |g, cx| {
        let mut d = D::new(cx, &g.extra);
        d.s("orientation", &g.orientation);
        d.i("width", &g.width);
        d.i("height", &g.height);
        d.done()
    });
    if let Some(tr) = &t.transformations {
        let mut td = D::new(cx, &tr.extra);
        td.flag("hflip", &tr.hflip);
        td.flag("vflip", &tr.vflip);
        td.flag("rotate", &tr.rotate);
        td.flag("preferuntransformed", &tr.preferuntransformed);
        if cx.json {
            d.put("transformations", td.done());
        } else {
            // DictionaryFromXml.gd treats the element as a list item ("<name>s").
            d.put("transformationss", Value::Array(vec![td.done()]));
        }
    }
    d.props(&t.properties);
    d.list("tiles", &t.tiles, tile);
    d.list("wangsets", &t.wangsets, wangset);
    d.done()
}

fn tile(t: &Tile, cx: Cx) -> Value {
    let mut d = D::new(cx, &t.extra);
    d.i("id", &t.id);
    d.s("type", &t.kind);
    d.s("class", &t.class);
    d.n("probability", &t.probability);
    d.i("x", &t.x);
    d.i("y", &t.y);
    d.i("width", &t.width);
    d.i("height", &t.height);
    d.raw("image", &t.image);
    d.i("imagewidth", &t.imagewidth);
    d.i("imageheight", &t.imageheight);
    d.raw("transparentcolor", &t.transparentcolor);
    d.props(&t.properties);
    d.list("animation", &t.animation, |f, cx| {
        let mut d = D::new(cx, &f.extra);
        d.i("tileid", &f.tileid);
        d.i("duration", &f.duration);
        d.done()
    });
    d.one("objectgroup", &t.objectgroup, layer);
    d.done()
}

fn wangset(w: &WangSet, cx: Cx) -> Value {
    let mut d = D::new(cx, &w.extra);
    d.s("name", &w.name);
    d.s("class", &w.class);
    d.s("type", &w.kind);
    d.i("tile", &w.tile);
    d.props(&w.properties);
    d.list("colors", &w.colors, |c, cx| {
        let mut d = D::new(cx, &c.extra);
        d.s("name", &c.name);
        d.s("class", &c.class);
        d.s("color", &c.color);
        d.i("tile", &c.tile);
        d.n("probability", &c.probability);
        d.props(&c.properties);
        d.done()
    });
    d.list("wangtiles", &w.wangtiles, |t, cx| {
        let mut d = D::new(cx, &t.extra);
        d.i("tileid", &t.tileid);
        if let Some(ids) = &t.wangid {
            d.put(
                "wangid",
                Value::Array(ids.iter().map(|&i| int(cx, i)).collect()),
            );
        }
        d.done()
    });
    d.done()
}

fn layer(l: &Layer, cx: Cx) -> Value {
    let mut d = D::new(cx, &l.extra);
    d.raw("type", &l.kind);
    d.i("id", &l.id);
    d.s("name", &l.name);
    d.s("class", &l.class);
    d.i("x", &l.x);
    d.i("y", &l.y);
    d.i("width", &l.width);
    d.i("height", &l.height);
    d.i("startx", &l.startx);
    d.i("starty", &l.starty);
    d.n("opacity", &l.opacity);
    d.b("visible", &l.visible);
    d.flag("locked", &l.locked);
    d.n("offsetx", &l.offsetx);
    d.n("offsety", &l.offsety);
    d.n("parallaxx", &l.parallaxx);
    d.n("parallaxy", &l.parallaxy);
    d.s("tintcolor", &l.tintcolor);
    d.s("mode", &l.mode);
    d.s("color", &l.color);
    d.s("draworder", &l.draworder);
    d.flag("repeatx", &l.repeatx);
    d.flag("repeaty", &l.repeaty);
    d.raw("image", &l.image);
    d.i("imagewidth", &l.imagewidth);
    d.i("imageheight", &l.imageheight);
    d.raw("transparentcolor", &l.transparentcolor);
    if cx.json {
        d.s("encoding", &l.encoding);
        d.s("compression", &l.compression);
    } else {
        d.raw("encoding", &l.encoding);
        d.raw("compression", &l.compression);
    }
    let csv = l.encoding.as_deref().is_none_or(|e| e == "csv");
    if let Some(p) = &l.data {
        d.put("data", payload(p, csv, cx));
    }
    d.list("chunks", &l.chunks, |c, cx| {
        let mut d = D::new(cx, &c.extra);
        d.i("x", &c.x);
        d.i("y", &c.y);
        d.i("width", &c.width);
        d.i("height", &c.height);
        if let Some(p) = &c.data {
            d.put("data", payload(p, csv, cx));
        }
        d.done()
    });
    d.list("objects", &l.objects, object);
    d.list("layers", &l.layers, layer);
    d.props(&l.properties);
    d.done()
}

fn payload(p: &Payload, csv: bool, cx: Cx) -> Value {
    match p {
        Payload::Text(t) if cx.json || !csv => Value::String(t.clone()),
        Payload::Text(t) => Value::Array(
            crate::data::csv_values(t)
                .into_iter()
                .map(Value::Int)
                .collect(),
        ),
        Payload::Gids(g) => Value::Array(g.iter().map(|&g| int(cx, i64::from(g))).collect()),
    }
}

fn object(o: &Object, cx: Cx) -> Value {
    let mut d = D::new(cx, &o.extra);
    d.i("id", &o.id);
    d.s("name", &o.name);
    d.s("type", &o.kind);
    d.s("class", &o.class);
    d.n("x", &o.x);
    d.n("y", &o.y);
    d.n("width", &o.width);
    d.n("height", &o.height);
    d.n("rotation", &o.rotation);
    d.n("opacity", &o.opacity);
    d.i("gid", &o.gid.map(i64::from));
    d.b("visible", &o.visible);
    d.s("template", &o.template);
    d.b("point", &o.point);
    d.b("ellipse", &o.ellipse);
    d.b("capsule", &o.capsule);
    let points = |pts: &Vec<Point>, _cx: Cx| {
        Value::Array(
            pts.iter()
                .map(|p| {
                    Value::Dict(BTreeMap::from([
                        ("x".to_string(), Value::Float(p.x)),
                        ("y".to_string(), Value::Float(p.y)),
                    ]))
                })
                .collect(),
        )
    };
    d.one("polygon", &o.polygon, points);
    d.one("polyline", &o.polyline, points);
    d.one("text", &o.text, |t, cx| {
        let mut d = D::new(cx, &t.extra);
        d.raw("text", &t.text);
        d.s("fontfamily", &t.fontfamily);
        d.i("pixelsize", &t.pixelsize);
        d.b("wrap", &t.wrap);
        d.s("color", &t.color);
        d.flag("bold", &t.bold);
        d.flag("italic", &t.italic);
        d.flag("underline", &t.underline);
        d.flag("strikeout", &t.strikeout);
        d.flag("kerning", &t.kerning);
        d.s("halign", &t.halign);
        d.s("valign", &t.valign);
        d.done()
    });
    d.props(&o.properties);
    d.done()
}

fn property(p: &Property, cx: Cx) -> Value {
    let style = p.style.0;
    let cx = if style == Style::Json {
        Cx { json: true }
    } else {
        cx
    };
    let mut d = D::new(cx, &p.extra);
    d.s("name", &p.name);
    d.s("type", &p.kind);
    d.s("propertytype", &p.propertytype);
    if let Some(v) = &p.value {
        d.put("value", top_value(v, cx, style));
    }
    d.done()
}

/// A property's own value: the XML side types it like any attribute.
fn top_value(v: &PropertyValue, cx: Cx, style: Style) -> Value {
    if cx.json {
        return member_value(v, cx);
    }
    match v {
        PropertyValue::Bool(b) => Value::String(if *b { "true" } else { "false" }.to_string()),
        PropertyValue::String(s) if style == Style::XmlText => Value::String(s.clone()),
        PropertyValue::String(s) => xml_typed(s),
        other => member_value(other, cx),
    }
}

/// A value inside a class (or any value of a JSON document): typed by its declared type.
fn member_value(v: &PropertyValue, cx: Cx) -> Value {
    match v {
        PropertyValue::Null => Value::Null,
        PropertyValue::Bool(b) => Value::Bool(*b),
        PropertyValue::Number(n) => num(cx, *n),
        PropertyValue::String(s) => Value::String(s.clone()),
        PropertyValue::Class(m) => Value::Dict(
            m.iter()
                .map(|(k, v)| (k.clone(), member_value(v, cx)))
                .collect(),
        ),
        PropertyValue::Array(a) => Value::Array(a.iter().map(|v| member_value(v, cx)).collect()),
        PropertyValue::List(items) => Value::Array(items.iter().map(|p| property(p, cx)).collect()),
    }
}

/// A property as an XML document's dictionary holds it.
pub(crate) fn xml_property(p: &Property) -> Value {
    property(p, Cx { json: false })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_text_keeps_floats_apart() {
        let v = Value::Dict(BTreeMap::from([
            ("a".into(), Value::Int(5)),
            ("b".into(), Value::Float(5.0)),
            ("c".into(), Value::Float(1e-5)),
            ("d".into(), Value::String("x\"\n".into())),
        ]));
        assert_eq!(v.to_json(), r#"{"a":5,"b":5.0,"c":0.00001,"d":"x\"\n"}"#);
    }

    #[test]
    fn xml_typing() {
        assert_eq!(xml_typed("16"), Value::Int(16));
        assert_eq!(xml_typed("0.5"), Value::Float(0.5));
        assert_eq!(xml_typed("#ff0000"), Value::String("#ff0000".into()));
        assert_eq!(xml_attr("visible", "0"), Value::Bool(false));
        assert_eq!(
            xml_attr("tiledversion", "1.11"),
            Value::String("1.11".into())
        );
    }
}
