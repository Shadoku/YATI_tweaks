//! Errors and diagnostics of the parsers and the loader.

use std::fmt;

/// What went wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// A file the loader asked for does not exist (the caller reported it with `missing`).
    NotFound,
    /// A file could not be parsed (bad XML or JSON, or not a Tiled file).
    Parse,
    /// A file's type could not be told from its content or extension.
    UnknownType,
    /// A zip archive could not be opened or a member not read.
    Zip,
    /// A layer or chunk payload could not be decoded.
    Data,
}

/// An error with a message close to the GDScript runtime's wording.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    /// The kind of error.
    pub kind: ErrorKind,
    /// The message, as the GDScript runtime would print it (for example
    /// `ERROR: Tileset file 'x.tsx' not found. -> Continuing but result may be unusable`).
    pub message: String,
}

impl Error {
    /// A new error.
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}
