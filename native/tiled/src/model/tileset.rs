//! Tilesets: atlas (one image) and image collection (an image per tile), their tiles, wang
//! sets, grid, tile offset and transformations.

use super::de;
use super::{Extra, Format, Layer, Meta, Num, Property};
use serde::Deserialize;

/// A tileset, embedded in a map or template or an external `.tsx`/`.tsj` file.
///
/// In a map's `tilesets` array an external tileset is only `firstgid` + `source`; the loader
/// reads the file and keeps it in the map's [`TilesetSlot`](super::TilesetSlot)s.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Tileset {
    /// `type` (`tileset` at the root of a tileset file).
    #[serde(rename = "type")]
    pub kind: Option<String>,
    /// `version` of the format.
    #[serde(deserialize_with = "de::opt_string")]
    pub version: Option<String>,
    /// `tiledversion`.
    #[serde(deserialize_with = "de::opt_string")]
    pub tiledversion: Option<String>,
    /// `firstgid` (in a map's or template's list).
    #[serde(deserialize_with = "de::opt_i64")]
    pub firstgid: Option<i64>,
    /// `source`: the external tileset file (in a map's or template's list).
    pub source: Option<String>,
    /// `name`.
    #[serde(deserialize_with = "de::opt_string")]
    pub name: Option<String>,
    /// `class`.
    pub class: Option<String>,
    /// `tilewidth`.
    #[serde(deserialize_with = "de::opt_i64")]
    pub tilewidth: Option<i64>,
    /// `tileheight`.
    #[serde(deserialize_with = "de::opt_i64")]
    pub tileheight: Option<i64>,
    /// `tilecount`.
    #[serde(deserialize_with = "de::opt_i64")]
    pub tilecount: Option<i64>,
    /// `columns` (0 for an image collection).
    #[serde(deserialize_with = "de::opt_i64")]
    pub columns: Option<i64>,
    /// `spacing` between tiles in the image.
    #[serde(deserialize_with = "de::opt_i64")]
    pub spacing: Option<i64>,
    /// `margin` around the tiles in the image.
    #[serde(deserialize_with = "de::opt_i64")]
    pub margin: Option<i64>,
    /// `objectalignment`.
    pub objectalignment: Option<String>,
    /// `tilerendersize`: `tile` or `grid`.
    pub tilerendersize: Option<String>,
    /// `fillmode`: `stretch` or `preserve-aspect-fit`.
    pub fillmode: Option<String>,
    /// `backgroundcolor`.
    pub backgroundcolor: Option<String>,
    /// `image` of an atlas tileset (as written, relative to the tileset's file).
    pub image: Option<String>,
    /// `imagewidth`.
    #[serde(deserialize_with = "de::opt_i64")]
    pub imagewidth: Option<i64>,
    /// `imageheight`.
    #[serde(deserialize_with = "de::opt_i64")]
    pub imageheight: Option<i64>,
    /// `transparentcolor` (TMX writes it without the `#`).
    pub transparentcolor: Option<String>,
    /// `tileoffset`.
    pub tileoffset: Option<TileOffset>,
    /// `grid`.
    pub grid: Option<Grid>,
    /// `properties`.
    pub properties: Option<Vec<Property>>,
    /// `tiles`: the tiles that have anything set (and every tile of an image collection).
    pub tiles: Option<Vec<Tile>>,
    /// `wangsets`.
    pub wangsets: Option<Vec<WangSet>>,
    /// `transformations`.
    pub transformations: Option<Transformations>,
    /// Other keys.
    #[serde(flatten)]
    pub extra: Extra,
    #[serde(skip)]
    pub(crate) format: Meta<Format>,
}

impl Tileset {
    /// The format this tileset was read from (an embedded tileset: its map's).
    pub fn format(&self) -> Format {
        self.format.0
    }

    /// True for an image collection (no tileset image; tiles have their own).
    pub fn is_collection(&self) -> bool {
        self.image.is_none()
    }

    /// The tile with this local id, if the tileset lists it.
    pub fn tile(&self, id: i64) -> Option<&Tile> {
        self.tiles.as_ref()?.iter().find(|t| t.id == Some(id))
    }

    /// The class: `class` when set and not empty, else `type`.
    pub fn class_name(&self) -> &str {
        match self.class.as_deref() {
            Some(c) if !c.is_empty() => c,
            _ => self.kind.as_deref().unwrap_or(""),
        }
    }
}

/// `tileoffset`: drawing offset of every tile, in pixels (positive y down).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct TileOffset {
    /// `x`.
    #[serde(deserialize_with = "de::opt_i64")]
    pub x: Option<i64>,
    /// `y`.
    #[serde(deserialize_with = "de::opt_i64")]
    pub y: Option<i64>,
    /// Other keys.
    #[serde(flatten)]
    pub extra: Extra,
}

/// `grid`: the tile grid of an image collection or isometric tileset.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Grid {
    /// `orientation`: `orthogonal` or `isometric`.
    pub orientation: Option<String>,
    /// `width`.
    #[serde(deserialize_with = "de::opt_i64")]
    pub width: Option<i64>,
    /// `height`.
    #[serde(deserialize_with = "de::opt_i64")]
    pub height: Option<i64>,
    /// Other keys.
    #[serde(flatten)]
    pub extra: Extra,
}

/// `transformations`: which transformations the tileset allows.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Transformations {
    /// `hflip`.
    pub hflip: Option<bool>,
    /// `vflip`.
    pub vflip: Option<bool>,
    /// `rotate`.
    pub rotate: Option<bool>,
    /// `preferuntransformed`.
    pub preferuntransformed: Option<bool>,
    /// Other keys.
    #[serde(flatten)]
    pub extra: Extra,
}

/// A tile definition (only tiles with something set are listed).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Tile {
    /// `id`: the local tile id.
    #[serde(deserialize_with = "de::opt_i64")]
    pub id: Option<i64>,
    /// `type`: the class (JSON, and TMX from Tiled 1.10 on).
    #[serde(rename = "type")]
    pub kind: Option<String>,
    /// `class` (TMX of Tiled 1.9).
    pub class: Option<String>,
    /// `probability` for the terrain and random tools.
    pub probability: Option<Num>,
    /// `x` of the sub-rectangle in the tile's image.
    #[serde(deserialize_with = "de::opt_i64")]
    pub x: Option<i64>,
    /// `y` of the sub-rectangle.
    #[serde(deserialize_with = "de::opt_i64")]
    pub y: Option<i64>,
    /// `width` of the sub-rectangle.
    #[serde(deserialize_with = "de::opt_i64")]
    pub width: Option<i64>,
    /// `height` of the sub-rectangle.
    #[serde(deserialize_with = "de::opt_i64")]
    pub height: Option<i64>,
    /// `image` of an image collection tile (relative to the tileset's file).
    pub image: Option<String>,
    /// `imagewidth`.
    #[serde(deserialize_with = "de::opt_i64")]
    pub imagewidth: Option<i64>,
    /// `imageheight`.
    #[serde(deserialize_with = "de::opt_i64")]
    pub imageheight: Option<i64>,
    /// `transparentcolor` of the tile's image.
    pub transparentcolor: Option<String>,
    /// `properties`.
    pub properties: Option<Vec<Property>>,
    /// `animation` frames.
    pub animation: Option<Vec<Frame>>,
    /// `objectgroup`: the tile's collision shapes.
    pub objectgroup: Option<Layer>,
    /// Other keys (for example the old `terrain`).
    #[serde(flatten)]
    pub extra: Extra,
}

impl Tile {
    /// The class: `class` when set and not empty, else `type`.
    pub fn class_name(&self) -> &str {
        match self.class.as_deref() {
            Some(c) if !c.is_empty() => c,
            _ => self.kind.as_deref().unwrap_or(""),
        }
    }
}

/// An animation frame.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Frame {
    /// `tileid`: the local id of the frame's tile.
    #[serde(deserialize_with = "de::opt_i64")]
    pub tileid: Option<i64>,
    /// `duration` in milliseconds.
    #[serde(deserialize_with = "de::opt_i64")]
    pub duration: Option<i64>,
    /// Other keys.
    #[serde(flatten)]
    pub extra: Extra,
}

/// A wang set (terrain set).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct WangSet {
    /// `name`.
    #[serde(deserialize_with = "de::opt_string")]
    pub name: Option<String>,
    /// `class`.
    pub class: Option<String>,
    /// `type`: `corner`, `edge` or `mixed`.
    #[serde(rename = "type")]
    pub kind: Option<String>,
    /// `tile`: the local id of the tile representing the set (-1 for none).
    #[serde(deserialize_with = "de::opt_i64")]
    pub tile: Option<i64>,
    /// `colors` (XML `<wangcolor>`).
    pub colors: Option<Vec<WangColor>>,
    /// `wangtiles`.
    pub wangtiles: Option<Vec<WangTile>>,
    /// `properties`.
    pub properties: Option<Vec<Property>>,
    /// Other keys.
    #[serde(flatten)]
    pub extra: Extra,
}

/// A wang colour (terrain).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct WangColor {
    /// `name`.
    #[serde(deserialize_with = "de::opt_string")]
    pub name: Option<String>,
    /// `class`.
    pub class: Option<String>,
    /// `color`.
    pub color: Option<String>,
    /// `tile` representing the colour (-1 for none).
    #[serde(deserialize_with = "de::opt_i64")]
    pub tile: Option<i64>,
    /// `probability`.
    pub probability: Option<Num>,
    /// `properties`.
    pub properties: Option<Vec<Property>>,
    /// Other keys.
    #[serde(flatten)]
    pub extra: Extra,
}

/// A tile's wang colours.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct WangTile {
    /// `tileid`.
    #[serde(deserialize_with = "de::opt_i64")]
    pub tileid: Option<i64>,
    /// `wangid`: eight colour indices, clockwise from the top edge.
    #[serde(deserialize_with = "de::vec_i64")]
    pub wangid: Option<Vec<i64>>,
    /// Other keys (JSON only; the XML parser keeps only `tileid` and `wangid`, as the
    /// GDScript runtime does).
    #[serde(flatten)]
    pub extra: Extra,
}
