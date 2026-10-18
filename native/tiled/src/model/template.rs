//! Object templates (`.tx`, `.tj`).

use super::{Extra, Format, Meta, Object, Tileset, TilesetSlot};
use crate::value::Value;
use serde::Deserialize;

/// An object template: the object and, for a tile object, the tileset its gid refers to.
///
/// The TX format writes `<tileset>` and `<object>` children (the GDScript dictionary has
/// `tilesets` and `objects` arrays), the TJ format a `tileset` and an `object`; both land here
/// as lists.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(from = "RawTemplate")]
pub struct Template {
    /// `type`: `template`.
    pub kind: Option<String>,
    /// The template's tilesets (normally one external reference, or none).
    pub tilesets: Vec<Tileset>,
    /// The template's objects (normally exactly one).
    pub objects: Vec<Object>,
    /// Other keys.
    pub extra: Extra,
    pub(crate) format: Meta<Format>,
    pub(crate) loaded: Meta<TemplateLoaded>,
}

/// What the loader found for a template.
#[derive(Clone, Debug, Default)]
pub(crate) struct TemplateLoaded {
    pub path: String,
    pub slots: Vec<TilesetSlot>,
}

impl Template {
    /// The format the template was read from.
    pub fn format(&self) -> Format {
        self.format.0
    }

    /// The resolved path the template was loaded from (empty when parsed directly).
    pub fn path(&self) -> &str {
        &self.loaded.0.path
    }

    /// The directory of the template file (what its references are relative to).
    pub fn base_dir(&self) -> String {
        crate::path::base_dir(&self.loaded.0.path)
    }

    /// The template's tilesets as the loader resolved them, in `tilesets` order.
    pub fn tileset_slots(&self) -> &[TilesetSlot] {
        &self.loaded.0.slots
    }

    /// The tileset of a slot: the embedded one or the loaded external file.
    pub fn slot_tileset<'a>(&'a self, slot: &'a TilesetSlot) -> Option<&'a Tileset> {
        slot.tileset(&self.tilesets)
    }
}

#[derive(Deserialize)]
struct RawTemplate {
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    tileset: Option<Tileset>,
    #[serde(default)]
    object: Option<Object>,
    #[serde(flatten)]
    extra: std::collections::BTreeMap<String, Value>,
}

impl From<RawTemplate> for Template {
    fn from(r: RawTemplate) -> Self {
        Template {
            kind: r.kind,
            tilesets: r.tileset.into_iter().collect(),
            objects: r.object.into_iter().collect(),
            extra: r.extra,
            format: Meta(Format::Json),
            loaded: Meta::default(),
        }
    }
}
