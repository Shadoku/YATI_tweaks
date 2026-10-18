//! TMJ, TSJ and TJ (JSON) into the model, with serde.

use super::{strip_bom, Document};
use crate::model::{Format, Map, Meta, Template, Tileset};
use crate::path;
use crate::{Error, ErrorKind};
use serde::Deserialize;

/// Parses a JSON Tiled document. The kind comes from the root's `type` (`map`, `tileset`,
/// `template`), else from the extension (`.tsj`, `.tj`; anything else is a map).
pub fn parse(bytes: &[u8], path: &str) -> Result<Document, Error> {
    let body = strip_bom(bytes);
    let fail = |e: serde_json::Error| {
        Error::new(
            ErrorKind::Parse,
            format!(
                "Error parsing file '{path}' (around line {}): {e}.",
                e.line()
            ),
        )
    };
    #[derive(Deserialize)]
    struct Kind {
        #[serde(rename = "type")]
        kind: Option<String>,
    }
    let kind = match root_type(body) {
        Some(k) => k,
        None => serde_json::from_slice::<Kind>(body)
            .map_err(fail)?
            .kind
            .unwrap_or_default(),
    };
    let kind = match kind.as_str() {
        "map" | "tileset" | "template" => kind,
        _ => match path::extension(path).as_str() {
            "tsj" => "tileset".to_string(),
            "tj" => "template".to_string(),
            _ => "map".to_string(),
        },
    };
    Ok(match kind.as_str() {
        "tileset" => {
            let mut t: Tileset = serde_json::from_slice(body).map_err(fail)?;
            t.format = Meta(Format::Json);
            Document::Tileset(t)
        }
        "template" => {
            let mut t: Template = serde_json::from_slice(body).map_err(fail)?;
            for ts in &mut t.tilesets {
                ts.format = Meta(Format::Json);
            }
            Document::Template(t)
        }
        _ => {
            let mut m: Map = serde_json::from_slice(body).map_err(fail)?;
            m.format = Meta(Format::Json);
            for ts in m.tilesets.iter_mut().flatten() {
                ts.format = Meta(Format::Json);
            }
            Document::Map(m)
        }
    })
}

/// The root's `"type"` when it is the first key (as Tiled writes it), without a full parse.
fn root_type(body: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(body.get(..body.len().min(256))?).ok()?;
    let rest = text.trim_start().strip_prefix('{')?.trim_start();
    let rest = rest
        .strip_prefix("\"type\"")?
        .trim_start()
        .strip_prefix(':')?
        .trim_start();
    let rest = rest.strip_prefix('"')?;
    Some(rest[..rest.find('"')?].to_string())
}
