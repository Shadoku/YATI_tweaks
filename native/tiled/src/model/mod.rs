//! The Tiled data model.
//!
//! The structs mirror the Tiled JSON format (<https://doc.mapeditor.org/en/stable/reference/json-map-format/>):
//! field names are the JSON keys (`type` is spelled `kind`), and every field is optional
//! because the files are: a key absent in the file stays `None`, so the model can give back
//! exactly the dictionary the GDScript runtime builds from the same file ([`crate::value`]).
//! The JSON formats deserialize into these structs with serde; the XML parser fills the same
//! structs (`DictionaryFromXml.gd` makes TMX look like TMJ the same way). Keys the model does
//! not know are kept in each struct's `extra`.
//!
//! Numbers that Tiled writes as floats (positions, sizes, opacity, offsets, parallax factors,
//! probabilities) are a [`Num`], which remembers whether the file wrote an integer or a
//! fraction (the GDScript dictionary types them by how they are written); integer fields are
//! `i64`, gids `u32`.

pub(crate) mod layer;
pub(crate) mod map;
pub(crate) mod project;
pub(crate) mod property;
pub(crate) mod template;
pub(crate) mod tileset;

pub use layer::{Chunk, Layer, Object, Payload, Point, Text};
pub use map::{Instance, Map, TilesetSlot, TilesetSource};
pub use project::{Project, PropertyType};
pub use property::{Property, PropertyValue};
pub use template::Template;
pub use tileset::{
    Frame, Grid, Tile, TileOffset, Tileset, Transformations, WangColor, WangSet, WangTile,
};

use crate::value::Value;
use serde::{Deserialize, Deserializer};
use std::collections::BTreeMap;
use std::fmt;

/// Keys of a struct the model has no field for, with their values as the GDScript dictionary
/// holds them (for XML: typed as `insert_attributes` types attributes).
pub type Extra = BTreeMap<String, Value>;

/// The file format a document was read from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Format {
    /// TMX, TSX or TX.
    #[default]
    Xml,
    /// TMJ, TSJ, TJ (and the `.tiled-project` file).
    Json,
}

/// A number as the file wrote it: an integer or a number with a fraction or exponent.
///
/// Compares by value (`Int(2) == Float(2.0)`), so an XML and a JSON file that hold the same
/// numbers give equal models.
#[derive(Clone, Copy, Debug)]
pub enum Num {
    /// Written as an integer (`16`).
    Int(i64),
    /// Written with a fraction or an exponent (`16.5`, `0.0`, `1e+06`).
    Float(f64),
}

impl Num {
    /// The value as `f64`.
    pub fn as_f64(self) -> f64 {
        match self {
            Num::Int(i) => i as f64,
            Num::Float(f) => f,
        }
    }

    /// The value as `i64` (a fraction truncated).
    pub fn as_i64(self) -> i64 {
        match self {
            Num::Int(i) => i,
            Num::Float(f) => f as i64,
        }
    }

    /// Reads an XML attribute the way `insert_attributes` types it: an integer when
    /// `is_valid_int`, a float when `is_valid_float`, otherwise `None`.
    pub fn from_xml(raw: &str) -> Option<Num> {
        if crate::godot_str::is_valid_int(raw) {
            Some(Num::Int(crate::godot_str::to_int(raw)))
        } else if crate::godot_str::is_valid_float(raw) {
            Some(Num::Float(crate::godot_str::to_float(raw)))
        } else {
            None
        }
    }
}

impl PartialEq for Num {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Num::Int(a), Num::Int(b)) => a == b,
            _ => self.as_f64() == other.as_f64(),
        }
    }
}

impl From<f64> for Num {
    fn from(v: f64) -> Self {
        Num::Float(v)
    }
}

impl From<i64> for Num {
    fn from(v: i64) -> Self {
        Num::Int(v)
    }
}

impl<'de> Deserialize<'de> for Num {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl serde::de::Visitor<'_> for V {
            type Value = Num;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a number")
            }
            fn visit_i64<E>(self, v: i64) -> Result<Num, E> {
                Ok(Num::Int(v))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Num, E> {
                Ok(i64::try_from(v)
                    .map(Num::Int)
                    .unwrap_or(Num::Float(v as f64)))
            }
            fn visit_f64<E>(self, v: f64) -> Result<Num, E> {
                Ok(Num::Float(v))
            }
        }
        d.deserialize_any(V)
    }
}

/// A field that takes no part in comparisons (the format a struct was read from, loader
/// results, a property's emission style): two models are equal when their content is.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Meta<T>(pub T);

impl<T> PartialEq for Meta<T> {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

/// Lenient serde helpers for the JSON formats (Godot's own JSON writer turns every integer into
/// a float: `16.0`; those read as integers here).
pub(crate) mod de {
    use super::Num;
    use serde::de::Error as _;
    use serde::{Deserialize, Deserializer};

    pub fn opt_i64<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
        match Option::<Num>::deserialize(d)? {
            None => Ok(None),
            Some(Num::Int(i)) => Ok(Some(i)),
            Some(Num::Float(f)) if f.fract() == 0.0 => Ok(Some(f as i64)),
            Some(Num::Float(f)) => Err(D::Error::custom(format!("expected an integer, got {f}"))),
        }
    }

    pub fn opt_u32<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u32>, D::Error> {
        match opt_i64(d)? {
            None => Ok(None),
            Some(i) => u32::try_from(i)
                .map(Some)
                .map_err(|_| D::Error::custom(format!("{i} is not a gid"))),
        }
    }

    pub fn vec_i64<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<i64>>, D::Error> {
        let v = Option::<Vec<Num>>::deserialize(d)?;
        Ok(v.map(|v| v.into_iter().map(Num::as_i64).collect()))
    }

    /// A string, or a number written where Tiled used to write one (old `"version": 1.2`).
    pub fn opt_string<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum S {
            S(String),
            N(f64),
        }
        Ok(Option::<S>::deserialize(d)?.map(|s| match s {
            S::S(s) => s,
            S::N(n) => n.to_string(),
        }))
    }
}
