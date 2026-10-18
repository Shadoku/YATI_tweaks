//! `TiledWorld`: a Tiled world file (`.world`) read by the core's `WorldLoader`, its requests
//! answered on the main thread through Godot (`FileAccess` for files, `DirAccess` for the
//! directory a pattern lists), or from a zip archive (F3 of `docs/runtime-improvement-plan.md`;
//! the facade's `WorldLoader.gd` is the runtime's API over it).

use godot::classes::{DirAccess, FileAccess};
use godot::global::{printerr, Error as GdError};
use godot::prelude::*;
use tiled_core::world::{WorldLoader, WorldMap};

/// A Tiled world: `load(world_file, zip_file)`, then `get_maps()`, each map's path (resolved
/// against the world file's directory) and rect in pixels, in the order of the file's `maps`
/// entries, then the maps its `patterns` list (sorted by file name).
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct TiledWorld {
    base: Base<RefCounted>,
    maps: Vec<WorldMap>,
    errors: Vec<String>,
}

#[godot_api]
impl TiledWorld {
    /// Reads a world file and every map its patterns list whose size the pattern does not give.
    /// `zip_file`: read the world file, its directory and the maps from that archive (all paths
    /// are paths inside it); `""`: from the file system. Errors are printed as the GDScript
    /// runtime's `WorldLoader` words them and kept (`get_errors`); what could be read is kept
    /// too. Returns `OK`, or `FAILED` when the world file could not be read at all.
    #[func]
    fn load(&mut self, world_file: GString, zip_file: GString) -> GdError {
        self.maps.clear();
        self.errors.clear();
        let path = world_file.to_string();
        let zip = zip_file.to_string();
        let result = if zip.is_empty() {
            WorldLoader::new(&path).run(read_file, list_dir)
        } else {
            let bytes = FileAccess::file_exists(zip.as_str())
                .then(|| FileAccess::get_file_as_bytes(zip.as_str()).to_vec())
                .filter(|b| !b.is_empty());
            match bytes {
                Some(b) => WorldLoader::from_zip(b, &path).run(|_| None, |_| None),
                None => {
                    self.error(format!("ERROR: Unable to open zip file '{zip}'."));
                    return GdError::FAILED;
                }
            }
        };
        match result {
            Ok(world) => {
                for e in world.errors {
                    self.error(e.message);
                }
                self.maps = world.maps;
                GdError::OK
            }
            Err(errors) => {
                for e in errors {
                    self.error(e.message);
                }
                GdError::FAILED
            }
        }
    }

    /// The maps, as `{"path": String, "rect": Rect2i}` dictionaries (`WorldLoader.parse`'s).
    #[func]
    fn get_maps(&self) -> VarArray {
        let mut out = VarArray::new();
        for m in &self.maps {
            let (x, y, w, h) = m.rect;
            let mut d = VarDictionary::new();
            d.set("path", m.path.as_str());
            d.set(
                "rect",
                Rect2i::new(
                    Vector2i::new(x as i32, y as i32),
                    Vector2i::new(w as i32, h as i32),
                ),
            );
            out.push(&d.to_variant());
        }
        out
    }

    /// The errors of the last `load` (each as printed).
    #[func]
    fn get_errors(&self) -> PackedStringArray {
        self.errors
            .iter()
            .map(|s| GString::from(s.as_str()))
            .collect()
    }
}

impl TiledWorld {
    fn error(&mut self, msg: String) {
        printerr(&[msg.to_variant()]);
        self.errors.push(msg);
    }
}

/// A file's bytes through `FileAccess` (so `res://` inside an exported .pck works).
fn read_file(path: &str) -> Option<Vec<u8>> {
    if !FileAccess::file_exists(path) {
        return None;
    }
    Some(FileAccess::get_file_as_bytes(path).to_vec())
}

/// The names of the files directly in a directory (`DataLoader.list_files`).
fn list_dir(dir: &str) -> Option<Vec<String>> {
    let dir = if dir.is_empty() { "." } else { dir };
    if !DirAccess::dir_exists_absolute(dir) {
        return None;
    }
    Some(
        DirAccess::get_files_at(dir)
            .as_slice()
            .iter()
            .map(|s| s.to_string())
            .collect(),
    )
}
