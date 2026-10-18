//! `tiled_core`: the Tiled map format in pure Rust, for the YATI native importer.
//!
//! This crate holds everything of the importer that does not touch Godot, so it can run on
//! any thread and be tested without the engine:
//!
//! - [`model`]: the Tiled data model, structs mirroring the Tiled JSON format (map, tileset,
//!   layers, objects, properties, templates, the `.tiled-project` custom types).
//! - [`parse`]: TMX/TSX/TX (XML, `quick-xml`) and TMJ/TSJ/TJ (JSON, `serde_json`) into that
//!   one model, detected by content.
//! - [`data`]: tile layer and chunk payloads (csv, base64, zlib/gzip/zstd, the legacy
//!   `<tile gid>` list), decoded on demand.
//! - [`cells`]: gid resolution as the GDScript runtime's `resolve_cell` does it, and the byte
//!   blobs Godot's `TileMapLayer.tile_map_data` takes.
//! - [`load`]: the [`Loader`], driven by a file-request protocol (the caller reads files, the
//!   loader says which), with templates and custom types merged as the GDScript runtime merges
//!   them; zip archives read directly.
//! - [`worker`]: the [`Loader`] on a thread of its own, the protocol's rounds as messages (what
//!   the native importer's asynchronous load runs; nothing Godot crosses the thread).
//! - [`value`]: [`Value`], the model as the dictionary the GDScript runtime's
//!   `DictionaryBuilder.gd` produces (the parity target).
//!
//! Nothing here depends on Godot: the GDExtension crate builds nodes and resources from the
//! model on the main thread.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod cells;
pub mod data;
pub mod error;
pub mod godot_str;
pub mod load;
pub mod model;
pub mod parse;
pub mod path;
pub mod value;
pub mod worker;
pub mod world;

pub use error::{Error, ErrorKind};
pub use load::{Loader, Status};
pub use model::{
    Chunk, Format, Frame, Grid, Instance, Layer, Map, Num, Object, Payload, Point, Project,
    Property, PropertyType, PropertyValue, Template, Text, Tile, TileOffset, Tileset, TilesetSlot,
    TilesetSource, Transformations, WangColor, WangSet, WangTile,
};
pub use value::Value;
