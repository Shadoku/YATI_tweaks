//! Layers (tile, object, image, group), chunks, layer payloads and objects.

use super::de;
use super::{Extra, Num, Property};
use serde::Deserialize;

/// A layer: `kind` is `tilelayer`, `objectgroup`, `imagelayer` or `group`.
///
/// A tile object group (a tile's collision shapes) is a `Layer` too; read from XML it has no
/// `kind`, as in the GDScript dictionary.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Layer {
    /// `type`: `tilelayer`, `objectgroup`, `imagelayer` or `group`.
    #[serde(rename = "type")]
    pub kind: Option<String>,
    /// `id`.
    #[serde(deserialize_with = "de::opt_i64")]
    pub id: Option<i64>,
    /// `name`.
    #[serde(deserialize_with = "de::opt_string")]
    pub name: Option<String>,
    /// `class` (Tiled 1.9+).
    pub class: Option<String>,
    /// `x` (cells; always 0 in Tiled).
    #[serde(deserialize_with = "de::opt_i64")]
    pub x: Option<i64>,
    /// `y` (cells; always 0 in Tiled).
    #[serde(deserialize_with = "de::opt_i64")]
    pub y: Option<i64>,
    /// `width` in cells (tile layers).
    #[serde(deserialize_with = "de::opt_i64")]
    pub width: Option<i64>,
    /// `height` in cells (tile layers).
    #[serde(deserialize_with = "de::opt_i64")]
    pub height: Option<i64>,
    /// `startx` of an infinite tile layer (JSON only).
    #[serde(deserialize_with = "de::opt_i64")]
    pub startx: Option<i64>,
    /// `starty` of an infinite tile layer (JSON only).
    #[serde(deserialize_with = "de::opt_i64")]
    pub starty: Option<i64>,
    /// `opacity`, 0 to 1.
    pub opacity: Option<Num>,
    /// `visible`.
    pub visible: Option<bool>,
    /// `locked`.
    pub locked: Option<bool>,
    /// `offsetx` in pixels.
    pub offsetx: Option<Num>,
    /// `offsety` in pixels.
    pub offsety: Option<Num>,
    /// `parallaxx` factor.
    pub parallaxx: Option<Num>,
    /// `parallaxy` factor.
    pub parallaxy: Option<Num>,
    /// `tintcolor` (`#RRGGBB` or `#AARRGGBB`).
    pub tintcolor: Option<String>,
    /// `mode`: the blend mode (Tiled 1.12: `normal`, `add`, `multiply`, ...).
    pub mode: Option<String>,
    /// `color` of an object group.
    pub color: Option<String>,
    /// `draworder` of an object group: `topdown` or `index`.
    pub draworder: Option<String>,
    /// `repeatx` of an image layer.
    pub repeatx: Option<bool>,
    /// `repeaty` of an image layer.
    pub repeaty: Option<bool>,
    /// `image` of an image layer (as written, relative to the map).
    pub image: Option<String>,
    /// `imagewidth` of an image layer.
    #[serde(deserialize_with = "de::opt_i64")]
    pub imagewidth: Option<i64>,
    /// `imageheight` of an image layer.
    #[serde(deserialize_with = "de::opt_i64")]
    pub imageheight: Option<i64>,
    /// `transparentcolor` of an image layer.
    pub transparentcolor: Option<String>,
    /// `encoding` of a tile layer's payload: `csv` (default) or `base64`.
    pub encoding: Option<String>,
    /// `compression` of a base64 payload: `zlib`, `gzip`, `zstd` or empty.
    pub compression: Option<String>,
    /// `data`: a finite tile layer's cells (undecoded, see [`crate::data`]).
    pub data: Option<Payload>,
    /// `chunks` of an infinite tile layer.
    pub chunks: Option<Vec<Chunk>>,
    /// `objects` of an object group.
    pub objects: Option<Vec<Object>>,
    /// `layers` of a group.
    pub layers: Option<Vec<Layer>>,
    /// `properties`.
    pub properties: Option<Vec<Property>>,
    /// Other keys.
    #[serde(flatten)]
    pub extra: Extra,
}

impl Layer {
    /// The layer's `kind` (`tilelayer` when absent, as the GDScript runtime assumes).
    pub fn kind_or_tile(&self) -> &str {
        self.kind.as_deref().unwrap_or("tilelayer")
    }

    /// Every layer in this group and below it, depth first, this one excluded.
    pub fn descendants(&self) -> Vec<&Layer> {
        let mut out = Vec::new();
        collect(self.layers.as_deref().unwrap_or(&[]), &mut out);
        out
    }
}

pub(crate) fn collect<'a>(layers: &'a [Layer], out: &mut Vec<&'a Layer>) {
    for l in layers {
        out.push(l);
        collect(l.layers.as_deref().unwrap_or(&[]), out);
    }
}

/// A tile layer's or chunk's cell data as the file holds it.
///
/// Kept undecoded: an infinite map costs its file size in memory, not four bytes per cell of
/// its bounding box. Decode with [`crate::data::decode`] (or [`Layer::decode`],
/// [`Chunk::decode`]).
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum Payload {
    /// Text: csv (XML) or base64 (both formats), white space around it stripped.
    Text(String),
    /// Gids as numbers: a JSON array, or the XML legacy `<tile gid="..."/>` list.
    Gids(Vec<u32>),
}

/// A chunk of an infinite tile layer.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Chunk {
    /// `x` in cells.
    #[serde(deserialize_with = "de::opt_i64")]
    pub x: Option<i64>,
    /// `y` in cells.
    #[serde(deserialize_with = "de::opt_i64")]
    pub y: Option<i64>,
    /// `width` in cells.
    #[serde(deserialize_with = "de::opt_i64")]
    pub width: Option<i64>,
    /// `height` in cells.
    #[serde(deserialize_with = "de::opt_i64")]
    pub height: Option<i64>,
    /// `data`, encoded as the layer's `encoding` and `compression` say.
    pub data: Option<Payload>,
    /// Other keys.
    #[serde(flatten)]
    pub extra: Extra,
}

/// A polygon or polyline vertex, relative to its object.
#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Point {
    /// `x`.
    pub x: f64,
    /// `y`.
    pub y: f64,
}

/// An object: rectangle (the default), ellipse, capsule, point, polygon, polyline, text, tile
/// (`gid`) or template instance (`template`).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Object {
    /// `id`.
    #[serde(deserialize_with = "de::opt_i64")]
    pub id: Option<i64>,
    /// `name`.
    #[serde(deserialize_with = "de::opt_string")]
    pub name: Option<String>,
    /// `type`: the class in the JSON format and in TMX from Tiled 1.10 on.
    #[serde(rename = "type")]
    pub kind: Option<String>,
    /// `class` (TMX of Tiled 1.9).
    pub class: Option<String>,
    /// `x` in pixels.
    pub x: Option<Num>,
    /// `y` in pixels.
    pub y: Option<Num>,
    /// `width` in pixels.
    pub width: Option<Num>,
    /// `height` in pixels.
    pub height: Option<Num>,
    /// `rotation` in degrees, clockwise.
    pub rotation: Option<Num>,
    /// `opacity`.
    pub opacity: Option<Num>,
    /// `gid` of a tile object, flip flags included.
    #[serde(deserialize_with = "de::opt_u32")]
    pub gid: Option<u32>,
    /// `visible`.
    pub visible: Option<bool>,
    /// `template`: the template file (as written, relative to the referencing file).
    pub template: Option<String>,
    /// `point`.
    pub point: Option<bool>,
    /// `ellipse`.
    pub ellipse: Option<bool>,
    /// `capsule`.
    pub capsule: Option<bool>,
    /// `polygon` vertices.
    pub polygon: Option<Vec<Point>>,
    /// `polyline` vertices.
    pub polyline: Option<Vec<Point>>,
    /// `text` of a text object.
    pub text: Option<Box<Text>>,
    /// `properties`.
    pub properties: Option<Vec<Property>>,
    /// Other keys.
    #[serde(flatten)]
    pub extra: Extra,
}

impl Object {
    /// The class: `class` when set and not empty, else `type` (the GDScript runtime's rule).
    pub fn class_name(&self) -> &str {
        match self.class.as_deref() {
            Some(c) if !c.is_empty() => c,
            _ => self.kind.as_deref().unwrap_or(""),
        }
    }
}

/// A text object's text and font.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Text {
    /// `text`.
    pub text: Option<String>,
    /// `fontfamily`.
    pub fontfamily: Option<String>,
    /// `pixelsize`.
    #[serde(deserialize_with = "de::opt_i64")]
    pub pixelsize: Option<i64>,
    /// `wrap`.
    pub wrap: Option<bool>,
    /// `color`.
    pub color: Option<String>,
    /// `bold`.
    pub bold: Option<bool>,
    /// `italic`.
    pub italic: Option<bool>,
    /// `underline`.
    pub underline: Option<bool>,
    /// `strikeout`.
    pub strikeout: Option<bool>,
    /// `kerning`.
    pub kerning: Option<bool>,
    /// `halign`: `left`, `center`, `right` or `justify`.
    pub halign: Option<String>,
    /// `valign`: `top`, `center` or `bottom`.
    pub valign: Option<String>,
    /// Other keys.
    #[serde(flatten)]
    pub extra: Extra,
}
