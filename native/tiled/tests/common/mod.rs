//! Helpers shared by the integration tests: fixture paths, a `std::fs` reader for the loader,
//! the expected dumps and a dictionary comparison.
#![allow(dead_code)]

use std::path::PathBuf;
use tiled_core::{Loader, Map, Value};

/// The repository's `test/fixtures/` directory.
pub fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/fixtures")
}

/// A fixture's path as a string (absolute, `..` left in: the loader cleans it).
pub fn fixture(name: &str) -> String {
    fixtures().join(name).to_string_lossy().into_owned()
}

/// Reads a fixture's bytes.
pub fn bytes(name: &str) -> Vec<u8> {
    std::fs::read(fixtures().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// Loads a fixture map through the file-request protocol, reading with `std::fs`.
pub fn load(name: &str, project: Option<&str>) -> Map {
    let project = project.map(fixture);
    Loader::new(&fixture(name), project.as_deref())
        .run(|p| std::fs::read(p).ok())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

/// An expected dump (`test/fixtures/expected/<name>`).
pub fn expected(name: &str) -> Value {
    let text = std::fs::read_to_string(fixtures().join("expected").join(name))
        .unwrap_or_else(|e| panic!("expected/{name}: {e} (run test/gen_expected.sh)"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("expected/{name}: {e}"))
}

/// Compares two dictionaries: same keys, ints exact, floats approximately, arrays in order.
/// Returns every difference, with its path.
pub fn diff(actual: &Value, expected: &Value) -> Vec<String> {
    let mut out = Vec::new();
    walk(actual, expected, "$", &mut out);
    out
}

fn walk(a: &Value, e: &Value, path: &str, out: &mut Vec<String>) {
    match (a, e) {
        (Value::Dict(da), Value::Dict(de)) => {
            for k in de.keys() {
                if !da.contains_key(k) {
                    out.push(format!("{path}.{k}: missing (expected {})", short(&de[k])));
                }
            }
            for (k, va) in da {
                match de.get(k) {
                    Some(ve) => walk(va, ve, &format!("{path}.{k}"), out),
                    None => out.push(format!("{path}.{k}: unexpected {}", short(va))),
                }
            }
        }
        (Value::Array(aa), Value::Array(ae)) => {
            if aa.len() != ae.len() {
                out.push(format!(
                    "{path}: {} elements, expected {}",
                    aa.len(),
                    ae.len()
                ));
            }
            for (i, (va, ve)) in aa.iter().zip(ae).enumerate() {
                walk(va, ve, &format!("{path}[{i}]"), out);
            }
        }
        (Value::Float(x), Value::Float(y)) => {
            if (x - y).abs() > 1e-9 * x.abs().max(y.abs()).max(1.0) {
                out.push(format!("{path}: {x}, expected {y}"));
            }
        }
        (a, e) if a == e => {}
        (a, e) => out.push(format!("{path}: {}, expected {}", short(a), short(e))),
    }
}

fn short(v: &Value) -> String {
    let s = v.to_json();
    let t = match v {
        Value::Int(_) => "int ",
        Value::Float(_) => "float ",
        _ => "",
    };
    if s.len() > 120 {
        format!("{t}{}...", &s[..120])
    } else {
        format!("{t}{s}")
    }
}

/// Panics with the differences when the dictionaries differ.
pub fn assert_parity(what: &str, actual: &Value, expected: &Value) {
    let d = diff(actual, expected);
    assert!(
        d.is_empty(),
        "{what}: {} difference(s):\n{}",
        d.len(),
        d.join("\n")
    );
}
