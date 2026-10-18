//! The YATI runtime importer as a GDExtension, loaded by Godot through
//! `addons/yati_native/yati_native.gdextension`.
//!
//! Stage 0 of `docs/native-importer-plan.md`: the scaffold. One class, `TiledNative`, proves
//! that the library loads and that values cross the boundary both ways; the parsers (stage 1),
//! the builders (stage 2), asynchronous loading (stage 3) and the streamer (stage 4) come after.

use godot::prelude::*;

struct YatiNative;

/// The extension's entry point. godot-rust exports it as `gdext_rust_init`, the
/// `entry_symbol` in `yati_native.gdextension`.
#[gdextension]
unsafe impl ExtensionLibrary for YatiNative {}

/// The native importer's handle for GDScript. For now only what proves the extension works:
/// its version and a round trip of a packed array.
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
