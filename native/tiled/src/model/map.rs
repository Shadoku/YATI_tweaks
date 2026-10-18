//! The map, and what the loader resolved for it (external tilesets, templates, the project's
//! custom types, image references, diagnostics).

use super::de;
use super::{Extra, Format, Layer, Meta, Num, Object, Project, Property, Template, Tileset};
use crate::Error;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::Arc;

/// A Tiled map (`.tmx` / `.tmj`).
///
/// The public fields are the file's content; [`Map::to_value`](crate::Map::to_value) turns
/// them into the GDScript runtime's dictionary. A map from the [`Loader`](crate::Loader) also
/// carries its external tilesets, templates, custom types, image references and diagnostics
/// (the accessor methods).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Map {
    /// `type`: `map`.
    #[serde(rename = "type")]
    pub kind: Option<String>,
    /// `version` of the format.
    #[serde(deserialize_with = "de::opt_string")]
    pub version: Option<String>,
    /// `tiledversion`.
    #[serde(deserialize_with = "de::opt_string")]
    pub tiledversion: Option<String>,
    /// `class`.
    pub class: Option<String>,
    /// `orientation`: `orthogonal`, `isometric`, `staggered` or `hexagonal`.
    pub orientation: Option<String>,
    /// `renderorder`.
    pub renderorder: Option<String>,
    /// `compressionlevel`.
    #[serde(deserialize_with = "de::opt_i64")]
    pub compressionlevel: Option<i64>,
    /// `width` in cells.
    #[serde(deserialize_with = "de::opt_i64")]
    pub width: Option<i64>,
    /// `height` in cells.
    #[serde(deserialize_with = "de::opt_i64")]
    pub height: Option<i64>,
    /// `tilewidth` in pixels.
    #[serde(deserialize_with = "de::opt_i64")]
    pub tilewidth: Option<i64>,
    /// `tileheight` in pixels.
    #[serde(deserialize_with = "de::opt_i64")]
    pub tileheight: Option<i64>,
    /// `hexsidelength` (hexagonal maps).
    #[serde(deserialize_with = "de::opt_i64")]
    pub hexsidelength: Option<i64>,
    /// `staggeraxis`: `x` or `y`.
    pub staggeraxis: Option<String>,
    /// `staggerindex`: `odd` or `even`.
    pub staggerindex: Option<String>,
    /// `parallaxoriginx`.
    pub parallaxoriginx: Option<Num>,
    /// `parallaxoriginy`.
    pub parallaxoriginy: Option<Num>,
    /// `backgroundcolor`.
    pub backgroundcolor: Option<String>,
    /// `nextlayerid`.
    #[serde(deserialize_with = "de::opt_i64")]
    pub nextlayerid: Option<i64>,
    /// `nextobjectid`.
    #[serde(deserialize_with = "de::opt_i64")]
    pub nextobjectid: Option<i64>,
    /// `infinite`.
    pub infinite: Option<bool>,
    /// `properties`.
    pub properties: Option<Vec<Property>>,
    /// `tilesets` as the file lists them (external ones as `firstgid` + `source`).
    pub tilesets: Option<Vec<Tileset>>,
    /// `layers`, top level (groups hold the rest).
    pub layers: Option<Vec<Layer>>,
    /// Other keys (for example `editorsettings`).
    #[serde(flatten)]
    pub extra: Extra,
    #[serde(skip)]
    pub(crate) format: Meta<Format>,
    #[serde(skip)]
    pub(crate) loaded: Meta<Loaded>,
}

/// What the loader resolved for a map.
#[derive(Clone, Debug, Default)]
pub(crate) struct Loaded {
    pub path: String,
    pub slots: Vec<TilesetSlot>,
    pub templates: BTreeMap<String, Template>,
    pub project: Option<Arc<Project>>,
    pub images: Vec<String>,
    pub errors: Vec<Error>,
    pub warnings: Vec<String>,
}

/// One entry of a map's (or template's) tileset list, resolved.
#[derive(Clone, Debug)]
pub struct TilesetSlot {
    /// The first gid of the tileset in this map.
    pub firstgid: i64,
    /// Where the tileset is.
    pub source: TilesetSource,
    /// The directory the tileset's image references are relative to.
    pub base_dir: String,
}

/// Where a tileset slot's tileset is.
#[derive(Clone, Debug)]
pub enum TilesetSource {
    /// Embedded: the index into the map's (or template's) `tilesets`.
    Embedded(usize),
    /// An external file: its resolved path and its content (`None`: missing or unreadable).
    External {
        /// The resolved path of the tileset file.
        path: String,
        /// The parsed file, shared between the map and its templates.
        tileset: Option<Arc<Tileset>>,
    },
    /// Not a tileset for the importer: Tiled's internal `:/automap...` rules tileset.
    Skipped,
}

impl TilesetSlot {
    /// The slot's tileset, given the list of embedded tilesets it indexes into.
    pub fn tileset<'a>(&'a self, embedded: &'a [Tileset]) -> Option<&'a Tileset> {
        match &self.source {
            TilesetSource::Embedded(i) => embedded.get(*i),
            TilesetSource::External { tileset, .. } => tileset.as_deref(),
            TilesetSource::Skipped => None,
        }
    }
}

/// A map object as the importer creates it: a template instance becomes its template's
/// objects with the instance's overrides.
#[derive(Clone, Debug)]
pub struct Instance<'a> {
    /// The objects to create (the object itself, or the merged template objects).
    pub objects: Vec<Object>,
    /// The template, when the object is a template instance (its tilesets resolve a merged
    /// object's `gid`; its directory is what the object's file properties are relative to).
    pub template: Option<&'a Template>,
}

impl Map {
    /// The format this map was read from.
    pub fn format(&self) -> Format {
        self.format.0
    }

    /// The path the map was loaded from (as given to the loader).
    pub fn path(&self) -> &str {
        &self.loaded.0.path
    }

    /// The directory of the map file (what its references are relative to).
    pub fn base_dir(&self) -> String {
        crate::path::base_dir(&self.loaded.0.path)
    }

    /// True for an infinite map (layers in chunks).
    pub fn is_infinite(&self) -> bool {
        self.infinite.unwrap_or(false)
    }

    /// The tilesets as the loader resolved them, in `tilesets` order.
    pub fn tileset_slots(&self) -> &[TilesetSlot] {
        &self.loaded.0.slots
    }

    /// The tileset of a slot: the embedded one or the loaded external file.
    pub fn slot_tileset<'a>(&'a self, slot: &'a TilesetSlot) -> Option<&'a Tileset> {
        slot.tileset(self.tilesets.as_deref().unwrap_or(&[]))
    }

    /// The tileset at an index of the map's tileset list (embedded or external).
    pub fn tileset(&self, index: usize) -> Option<&Tileset> {
        self.slot_tileset(self.loaded.0.slots.get(index)?)
    }

    /// The template an object's `template` reference names, if it was loaded.
    pub fn template(&self, reference: &str) -> Option<&Template> {
        let path = crate::path::resolve(&self.base_dir(), reference);
        self.loaded.0.templates.get(&path)
    }

    /// Every loaded template by resolved path.
    pub fn templates(&self) -> &BTreeMap<String, Template> {
        &self.loaded.0.templates
    }

    /// The project's custom types, when the map was loaded with a project file.
    pub fn project(&self) -> Option<&Project> {
        self.loaded.0.project.as_deref()
    }

    /// Every image the map refers to (tileset images, image collection tiles, image layers,
    /// the templates' tilesets' images), resolved, each once, in discovery order. The core
    /// never reads them: loading textures is the Godot side's job.
    pub fn images(&self) -> &[String] {
        &self.loaded.0.images
    }

    /// Errors met while loading that did not stop it (a missing tileset or template, ...).
    pub fn errors(&self) -> &[Error] {
        &self.loaded.0.errors
    }

    /// Warnings met while loading.
    pub fn warnings(&self) -> &[String] {
        &self.loaded.0.warnings
    }

    /// Every layer, depth first in file order (groups before their children).
    pub fn all_layers(&self) -> Vec<&Layer> {
        let mut out = Vec::new();
        super::layer::collect(self.layers.as_deref().unwrap_or(&[]), &mut out);
        out
    }

    /// The objects the importer creates for a map object (`TilemapCreator.create_object`'s
    /// template merge): for a template instance, a copy of each template object where the
    /// instance's properties override the template's by name (others appended), the instance's
    /// `id` replaces the template's, a non-empty `class`, `type` and `name` and any `opacity`
    /// of the instance replace the template's, and a position the template lacks is the
    /// instance's; the project's custom types are then merged into each (scope `object`).
    /// A missing template gives no objects. An object without template is returned as is.
    pub fn instantiate<'a>(&'a self, obj: &Object) -> Instance<'a> {
        let Some(reference) = obj.template.as_deref() else {
            return Instance {
                objects: vec![obj.clone()],
                template: None,
            };
        };
        let Some(template) = self.template(reference) else {
            return Instance {
                objects: Vec::new(),
                template: None,
            };
        };
        let objects = template
            .objects
            .iter()
            .map(|t| {
                let mut m = merge_template_object(t, obj);
                if let Some(p) = self.project() {
                    let class = m.class_name().to_string();
                    p.merge(&mut m.properties, &class, "object");
                }
                m
            })
            .collect();
        Instance {
            objects,
            template: Some(template),
        }
    }
}

/// The template object `t` with the instance `obj`'s overrides (see [`Map::instantiate`]).
pub(crate) fn merge_template_object(t: &Object, obj: &Object) -> Object {
    let mut m = t.clone();
    if let Some(props) = &obj.properties {
        match &mut m.properties {
            Some(tprops) => {
                for p in props {
                    match tprops.iter_mut().find(|tp| tp.name == p.name) {
                        Some(tp) => tp.value = p.value.clone(),
                        None => tprops.push(p.clone()),
                    }
                }
            }
            None => m.properties = Some(props.clone()),
        }
    }
    m.id = Some(obj.id.unwrap_or(0));
    for (dst, src) in [
        (&mut m.class, &obj.class),
        (&mut m.kind, &obj.kind),
        (&mut m.name, &obj.name),
    ] {
        if let Some(s) = src.as_deref().filter(|s| !s.is_empty()) {
            *dst = Some(s.to_string());
        }
    }
    if obj.opacity.is_some() {
        m.opacity = obj.opacity;
    }
    if m.x.is_none() {
        m.x = obj.x;
    }
    if m.y.is_none() {
        m.y = obj.y;
    }
    m
}
