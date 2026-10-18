//! The YATI runtime importer as a GDExtension, loaded by Godot through
//! `addons/yati_native/yati_native.gdextension` (`docs/native-importer-plan.md`).
//!
//! The Tiled format itself (model, parsers, payloads, the loader and its file protocol) is the
//! pure-Rust core `tiled_core` (`native/tiled/`); this crate is the Godot side:
//!
//! - [`TiledNative`]: the library's version (the facade checks it once per process).
//! - [`TiledImportOptions`](options::TiledImportOptions): every import option.
//! - [`TiledMap`](map::TiledMap): a map loaded through the core, its files read on the main
//!   thread with `FileAccess` (or from a zip archive); queries and `to_dictionary()`.
//! - [`TiledTileSets`](tileset::TiledTileSets): the TileSet builder (the runtime's
//!   `TilesetCreator`) and its cache.
//! - [`TiledBuilder`](builder::TiledBuilder): the node tree of a map in one call (the runtime's
//!   `TilemapCreator`), tile layers filled through `tile_map_data`.
//!
//! Everything here runs on the main thread except the decoding of tile layer payloads, which
//! runs on one thread per layer and touches no Godot object.

use godot::prelude::*;

mod builder;
mod diag;
mod dynobj;
mod files;
mod gd;
mod map;
mod options;
mod props;
mod tileset;

struct YatiNative;

/// The extension's entry point. godot-rust exports it as `gdext_rust_init`, the
/// `entry_symbol` in `yati_native.gdextension`.
#[gdextension]
unsafe impl ExtensionLibrary for YatiNative {}

/// The native importer's handle for GDScript: its version, and a round trip of a packed array
/// for the smoke test.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct TiledNative {
    base: Base<RefCounted>,
}

#[godot_api]
impl TiledNative {
    /// The native importer's version (the crate's), e.g. "0.1.0".
    #[func]
    fn version(&self) -> GString {
        GString::from(env!("CARGO_PKG_VERSION"))
    }

    /// The sum of `values`, as a 64-bit integer: a round trip through the FFI boundary for the
    /// smoke test (a packed array in, a scalar out).
    #[func]
    fn sum(&self, values: PackedInt32Array) -> i64 {
        values.as_slice().iter().map(|&v| i64::from(v)).sum()
    }
}
