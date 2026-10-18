//! Messages of an import: printed as the GDScript runtime prints them (`printerr` for errors,
//! `print_rich` in yellow for warnings) and counted.

use godot::global::{print, print_rich, printerr};
use godot::prelude::*;

/// The colour tag of the runtime's warnings.
pub const WARNING_COLOR: &str = "Yellow";

/// Error and warning counters of one import.
#[derive(Clone, Debug, Default)]
pub struct Diag {
    pub errors: u32,
    pub warnings: u32,
    /// The messages, in order (`E:` or `W:` prefixed), for `TiledBuilder.get_messages`.
    pub messages: Vec<String>,
}

impl Diag {
    /// Prints an error (its text as the runtime words it) and counts it.
    pub fn error(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        printerr(&[msg.to_variant()]);
        self.errors += 1;
        self.messages.push(format!("E:{msg}"));
    }

    /// Prints a warning in the runtime's colour and counts it.
    pub fn warn(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        print_rich(&[format!("[color={WARNING_COLOR}]{msg}[/color]").to_variant()]);
        self.warnings += 1;
        self.messages.push(format!("W:{msg}"));
    }

    /// Prints a warning in the colour the runtime's `TilesetCreator` uses (`yellow`, not
    /// `TilemapCreator`'s `Yellow`) and counts it.
    pub fn warn_ts(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        print_rich(&[format!("[color=yellow]{msg}[/color]").to_variant()]);
        self.warnings += 1;
        self.messages.push(format!("W:{msg}"));
    }

    /// Prints a warning without counting it (the runtime prints a few such lines).
    pub fn note(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        print_rich(&[format!("[color=yellow]{msg}[/color]").to_variant()]);
    }

    /// Prints a plain line.
    pub fn info(&mut self, msg: impl Into<String>) {
        print(&[msg.into().to_variant()]);
    }
}
