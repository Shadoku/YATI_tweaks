//! Messages of an import: printed as the GDScript runtime prints them (`printerr` for errors,
//! `print_rich` in yellow for warnings) and counted.

use godot::global::{print, print_rich, printerr};
use godot::prelude::*;

/// The colour tag of the runtime's warnings.
pub const WARNING_COLOR: &str = "Yellow";

/// A printed line, kept by a quiet [`Diag`] for [`Diag::replay`].
#[derive(Clone, Debug)]
pub enum Line {
    /// `printerr`.
    Error(String),
    /// `print_rich`.
    Rich(String),
    /// `print`.
    Plain(String),
}

/// Error and warning counters of one import.
#[derive(Clone, Debug, Default)]
pub struct Diag {
    pub errors: u32,
    pub warnings: u32,
    /// The messages, in order (`E:` or `W:` prefixed), for `TiledBuilder.get_messages`.
    pub messages: Vec<String>,
    /// Quiet: lines are kept in `lines` instead of printed (a cached TileSet's messages, which
    /// every import that uses it reports again with [`Diag::replay`]).
    pub quiet: bool,
    /// The lines a quiet diag did not print.
    pub lines: Vec<Line>,
}

impl Diag {
    /// A diag that keeps its lines instead of printing them.
    pub fn quiet() -> Self {
        Diag {
            quiet: true,
            ..Default::default()
        }
    }

    fn out(&mut self, line: Line) {
        if self.quiet {
            self.lines.push(line);
            return;
        }
        match &line {
            Line::Error(m) => printerr(&[m.to_variant()]),
            Line::Rich(m) => print_rich(&[m.to_variant()]),
            Line::Plain(m) => print(&[m.to_variant()]),
        }
    }

    /// Reports what a quiet diag kept as if it happened here: its lines printed (or kept, when
    /// this one is quiet too), its counts and messages added.
    pub fn replay(&mut self, from: &Diag) {
        for l in &from.lines {
            self.out(l.clone());
        }
        self.errors += from.errors;
        self.warnings += from.warnings;
        self.messages.extend(from.messages.iter().cloned());
    }

    /// Prints an error (its text as the runtime words it) and counts it.
    pub fn error(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        self.out(Line::Error(msg.clone()));
        self.errors += 1;
        self.messages.push(format!("E:{msg}"));
    }

    /// Prints a warning in the runtime's colour and counts it.
    pub fn warn(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        self.out(Line::Rich(format!("[color={WARNING_COLOR}]{msg}[/color]")));
        self.warnings += 1;
        self.messages.push(format!("W:{msg}"));
    }

    /// Prints a warning in the colour the runtime's `TilesetCreator` uses (`yellow`, not
    /// `TilemapCreator`'s `Yellow`) and counts it.
    pub fn warn_ts(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        self.out(Line::Rich(format!("[color=yellow]{msg}[/color]")));
        self.warnings += 1;
        self.messages.push(format!("W:{msg}"));
    }

    /// Prints a warning without counting it (the runtime prints a few such lines).
    pub fn note(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        self.out(Line::Rich(format!("[color=yellow]{msg}[/color]")));
    }

    /// Prints a plain line.
    pub fn info(&mut self, msg: impl Into<String>) {
        self.out(Line::Plain(msg.into()));
    }
}
