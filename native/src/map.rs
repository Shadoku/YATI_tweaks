//! `TiledMap`: a parsed map, loaded through the core's file-request protocol with the files
//! read on the main thread through Godot (`FileAccess`, so `res://` inside a .pck works).

use crate::gd::value_to_variant;
use godot::classes::FileAccess;
use godot::global::{print_rich, printerr, Error as GdError};
use godot::prelude::*;
use std::sync::Arc;
use tiled_core::{Loader, Map};

/// A Tiled map (`.tmx` / `.tmj`) with its external tilesets, templates and custom types,
/// parsed by the native core. `load()` or `from_zip()` it, then hand it to
/// `TiledTileSets.tileset_for` and `TiledBuilder.build`, or read it with the queries.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct TiledMap {
    base: Base<RefCounted>,
    pub(crate) map: Option<Arc<Map>>,
    /// The zip archive the map was read from ("" for the file system).
    pub(crate) zip_file: String,
    errors: Vec<String>,
    warnings: Vec<String>,
}

#[godot_api]
impl TiledMap {
    /// Loads a map from the file system (`res://`, `user://` or an absolute path), with an
    /// optional `.tiled-project` file for custom types. Errors and warnings are printed with the
    /// GDScript runtime's wording and kept (`get_errors`, `get_warnings`). Returns `OK`, or
    /// `FAILED` when the map could not be loaded at all.
    #[func]
    fn load(&mut self, path: GString, project_file: GString) -> GdError {
        let path = path.to_string();
        let project = project_file.to_string();
        let loader = Loader::new(&path, (!project.is_empty()).then_some(project.as_str()));
        self.zip_file.clear();
        self.finish(loader.run(read_file))
    }

    /// Loads a map from a zip archive: `path_in_zip` and `project_in_zip` are paths inside the
    /// archive, which is read for this map only.
    #[func]
    #[allow(clippy::wrong_self_convention)] // the plan's name for it
    fn from_zip(
        &mut self,
        zip_file: GString,
        path_in_zip: GString,
        project_in_zip: GString,
    ) -> GdError {
        let zip = zip_file.to_string();
        self.zip_file = zip.clone();
        self.map = None;
        self.errors.clear();
        self.warnings.clear();
        let bytes = if FileAccess::file_exists(zip.as_str()) {
            Some(FileAccess::get_file_as_bytes(zip.as_str()).to_vec())
        } else {
            None
        };
        let Some(bytes) = bytes.filter(|b| !b.is_empty()) else {
            let msg = format!("ERROR: Unable to open zip file '{zip}'.");
            printerr(&[msg.to_variant()]);
            self.errors.push(msg);
            return GdError::FAILED;
        };
        let project = project_in_zip.to_string();
        let loader = Loader::from_zip(
            bytes,
            &path_in_zip.to_string(),
            (!project.is_empty()).then_some(project.as_str()),
        );
        self.finish(loader.run(|_| None))
    }

    /// True after a successful `load` or `from_zip`.
    #[func]
    fn is_loaded(&self) -> bool {
        self.map.is_some()
    }

    /// The path the map was loaded from.
    #[func]
    fn get_path(&self) -> GString {
        self.map
            .as_ref()
            .map(|m| m.path().into())
            .unwrap_or_default()
    }

    /// The zip archive the map was read from, "" for the file system.
    #[func]
    fn get_zip_file(&self) -> GString {
        self.zip_file.as_str().into()
    }

    /// The map's size in cells (`width`, `height`).
    #[func]
    fn get_size(&self) -> Vector2i {
        self.map.as_ref().map_or(Vector2i::ZERO, |m| {
            Vector2i::new(m.width.unwrap_or(0) as i32, m.height.unwrap_or(0) as i32)
        })
    }

    /// The map's tile size in pixels.
    #[func]
    fn get_tile_size(&self) -> Vector2i {
        self.map.as_ref().map_or(Vector2i::ZERO, |m| {
            Vector2i::new(
                m.tilewidth.unwrap_or(0) as i32,
                m.tileheight.unwrap_or(0) as i32,
            )
        })
    }

    /// `orientation`: `orthogonal`, `isometric`, `staggered` or `hexagonal`.
    #[func]
    fn get_orientation(&self) -> GString {
        self.map
            .as_ref()
            .and_then(|m| m.orientation.as_deref().map(GString::from))
            .unwrap_or_default()
    }

    /// True for an infinite map (tile layers in chunks).
    #[func]
    fn is_infinite(&self) -> bool {
        self.map.as_ref().is_some_and(|m| m.is_infinite())
    }

    /// Every layer, depth first in file order, as `{name, type, id, depth}` dictionaries.
    #[func]
    fn get_layers(&self) -> VarArray {
        let mut out = VarArray::new();
        if let Some(m) = &self.map {
            fn walk(layers: &[tiled_core::Layer], depth: i64, out: &mut VarArray) {
                for l in layers {
                    let mut d = VarDictionary::new();
                    d.set("name", l.name.as_deref().unwrap_or(""));
                    d.set("type", l.kind_or_tile());
                    d.set("id", l.id.unwrap_or(0));
                    d.set("depth", depth);
                    out.push(&d.to_variant());
                    walk(l.layers.as_deref().unwrap_or(&[]), depth + 1, out);
                }
            }
            walk(m.layers.as_deref().unwrap_or(&[]), 0, &mut out);
        }
        out
    }

    /// The map as the GDScript runtime's dictionary (`DictionaryBuilder.get_dictionary`), the
    /// project's custom types merged in. Empty when nothing is loaded.
    #[func]
    fn to_dictionary(&self) -> VarDictionary {
        match &self.map {
            Some(m) => value_to_variant(&m.to_value()).to::<VarDictionary>(),
            None => VarDictionary::new(),
        }
    }

    /// The errors of the last load (each as printed).
    #[func]
    fn get_errors(&self) -> PackedStringArray {
        self.errors
            .iter()
            .map(|s| GString::from(s.as_str()))
            .collect()
    }

    /// The warnings of the last load.
    #[func]
    fn get_warnings(&self) -> PackedStringArray {
        self.warnings
            .iter()
            .map(|s| GString::from(s.as_str()))
            .collect()
    }
}

impl TiledMap {
    fn finish(&mut self, result: Result<Map, Vec<tiled_core::Error>>) -> GdError {
        self.errors.clear();
        self.warnings.clear();
        match result {
            Ok(map) => {
                for e in map.errors() {
                    self.errors.push(e.message.clone());
                }
                for w in map.warnings() {
                    self.warnings.push(w.clone());
                }
                self.print_messages();
                self.map = Some(Arc::new(map));
                GdError::OK
            }
            Err(errors) => {
                self.map = None;
                self.errors = errors.into_iter().map(|e| e.message).collect();
                self.print_messages();
                GdError::FAILED
            }
        }
    }

    fn print_messages(&self) {
        for e in &self.errors {
            printerr(&[e.to_variant()]);
        }
        for w in &self.warnings {
            print_rich(
                &[format!("[color={}]{w}[/color]", crate::diag::WARNING_COLOR).to_variant()],
            );
        }
    }

    /// The parsed map.
    pub fn inner(&self) -> Option<Arc<Map>> {
        self.map.clone()
    }
}

/// The main thread's file reader: `FileAccess`, so `res://` inside an exported .pck works.
fn read_file(path: &str) -> Option<Vec<u8>> {
    if !FileAccess::file_exists(path) {
        return None;
    }
    Some(FileAccess::get_file_as_bytes(path).to_vec())
}
