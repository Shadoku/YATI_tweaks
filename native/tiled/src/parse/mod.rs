//! Parsing Tiled files into the model.
//!
//! The format is told by content (`<` for XML, `{` for JSON, after a byte-order mark and white
//! space), the file extension only breaking a tie (`DictionaryBuilder.gd` goes by the
//! extension first; content first is the same for every Tiled file and copes with a wrong
//! extension). Both parsers fill the same structs.

pub mod json;
pub mod xml;

use crate::model::{Format, Map, Template, Tileset};
use crate::{path, Error, ErrorKind};

/// A parsed Tiled file.
#[derive(Clone, Debug)]
pub enum Document {
    /// A map (`.tmx`, `.tmj`).
    Map(Map),
    /// A tileset (`.tsx`, `.tsj`).
    Tileset(Tileset),
    /// An object template (`.tx`, `.tj`).
    Template(Template),
}

/// The bytes without a UTF-8 byte-order mark.
pub fn strip_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes)
}

/// Tells a file's format from its content, then its extension.
pub fn detect(bytes: &[u8], path: &str) -> Result<Format, Error> {
    let body = strip_bom(bytes);
    match body.iter().find(|c| !c.is_ascii_whitespace()) {
        Some(b'<') => return Ok(Format::Xml),
        Some(b'{') => return Ok(Format::Json),
        _ => {}
    }
    match path::extension(path).as_str() {
        "tmx" | "tsx" | "tx" | "xml" => Ok(Format::Xml),
        "tmj" | "tsj" | "tj" | "json" | "tiled-project" => Ok(Format::Json),
        _ => Err(Error::new(
            ErrorKind::UnknownType,
            format!("ERROR: File '{path}' has an unknown type. -> Continuing but result may be unusable"),
        )),
    }
}

/// Parses any Tiled document (map, tileset, template), XML or JSON.
pub fn document(bytes: &[u8], path: &str) -> Result<Document, Error> {
    match detect(bytes, path)? {
        Format::Xml => xml::parse(bytes, path),
        Format::Json => json::parse(bytes, path),
    }
}

fn wrong(path: &str, what: &str) -> Error {
    Error::new(
        ErrorKind::Parse,
        format!("Error parsing file '{path}': not a Tiled {what}."),
    )
}

/// Parses a map file.
pub fn map(bytes: &[u8], path: &str) -> Result<Map, Error> {
    match document(bytes, path)? {
        Document::Map(m) => Ok(m),
        _ => Err(wrong(path, "map")),
    }
}

/// Parses a tileset file.
pub fn tileset(bytes: &[u8], path: &str) -> Result<Tileset, Error> {
    match document(bytes, path)? {
        Document::Tileset(t) => Ok(t),
        _ => Err(wrong(path, "tileset")),
    }
}

/// Parses a template file.
pub fn template(bytes: &[u8], path: &str) -> Result<Template, Error> {
    match document(bytes, path)? {
        Document::Template(t) => Ok(t),
        _ => Err(wrong(path, "template")),
    }
}

impl Map {
    /// Parses a map from bytes (no external files read; see [`crate::Loader`] for that).
    pub fn parse(bytes: &[u8], path: &str) -> Result<Map, Error> {
        map(bytes, path)
    }
}

impl Tileset {
    /// Parses a tileset file from bytes.
    pub fn parse(bytes: &[u8], path: &str) -> Result<Tileset, Error> {
        tileset(bytes, path)
    }
}

impl Template {
    /// Parses a template file from bytes (its tileset not read).
    pub fn parse(bytes: &[u8], path: &str) -> Result<Template, Error> {
        template(bytes, path)
    }
}
