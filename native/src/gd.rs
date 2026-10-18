//! What the builders need of GDScript's semantics: `str()` of a dictionary value, the runtime's
//! typed property values (`CommonUtils.get_right_typed_value`), Tiled colours, bit masks,
//! Godot's path functions, and the conversion of the core's [`Value`] into a `Variant`.

use godot::prelude::*;
use tiled_core::godot_str;
use tiled_core::Value;

/// A property's value as the GDScript runtime holds it in its dictionaries: a value of the
/// parsed file, a `Variant` read back from the engine (tile metadata turned into object
/// properties), or a string (a property forwarded to children with the `__` prefix).
#[derive(Clone, Debug)]
pub enum PV {
    /// From the file, as `DictionaryBuilder` gives it.
    Core(Value),
    /// From the engine.
    Var(Variant),
    /// Already stringified.
    Str(String),
}

impl PV {
    /// GDScript's `str(value)`.
    pub fn gd_str(&self) -> String {
        match self {
            PV::Core(v) => gd_str(v),
            PV::Var(v) => v.stringify().to_string(),
            PV::Str(s) => s.clone(),
        }
    }

    /// The core value, when it is one.
    pub fn core(&self) -> Option<&Value> {
        match self {
            PV::Core(v) => Some(v),
            _ => None,
        }
    }

    /// True for a dictionary value (a class-typed property's members).
    pub fn is_dict(&self) -> bool {
        match self {
            PV::Core(v) => matches!(v, Value::Dict(_)),
            PV::Var(v) => v.get_type() == VariantType::DICTIONARY,
            PV::Str(_) => false,
        }
    }

    /// The value as a `Variant`.
    pub fn to_variant(&self) -> Variant {
        match self {
            PV::Core(v) => value_to_variant(v),
            PV::Var(v) => v.clone(),
            PV::Str(s) => s.to_variant(),
        }
    }
}

/// GDScript's `str()` of a dictionary value.
pub fn gd_str(v: &Value) -> String {
    match v {
        Value::Null => "<null>".to_string(),
        Value::Bool(b) => if *b { "true" } else { "false" }.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => num_real(*f),
        Value::String(s) => s.clone(),
        Value::Array(_) | Value::Dict(_) => value_to_variant(v).stringify().to_string(),
    }
}

/// Godot's `String::num_real(f, true)`: what `str(float)` prints (14 significant digits at
/// most, trailing zeros dropped, `.0` for an integral value).
pub fn num_real(f: f64) -> String {
    if f.is_nan() {
        return "nan".to_string();
    }
    if f.is_infinite() {
        return if f > 0.0 { "inf" } else { "-inf" }.to_string();
    }
    if f == 0.0 {
        return "0.0".to_string();
    }
    let mut decimals: i32 = 14;
    let abs = f.abs();
    if abs > 10.0 {
        decimals -= abs.log10().floor() as i32;
    }
    let mut s = format!("{:.*}", decimals.max(0) as usize, f);
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    if !s.contains('.') && !s.contains('e') {
        s.push_str(".0");
    }
    s
}

/// GDScript's `int(String)` (`String.to_int`).
pub fn to_int(s: &str) -> i64 {
    godot_str::to_int(s)
}

/// GDScript's `float(String)` (`String.to_float`).
pub fn to_float(s: &str) -> f64 {
    godot_str::to_float(s)
}

/// `CommonUtils.get_right_typed_value`.
pub fn typed_value(kind: &str, val: &str) -> Variant {
    match kind {
        "bool" => (val == "true").to_variant(),
        "float" => to_float(val).to_variant(),
        "int" => to_int(val).to_variant(),
        "color" => tiled_color(val, Color::WHITE).to_variant(),
        _ => {
            // JSON parsing since Godot 4.4 adds ".0" to integers so remove that
            if val.ends_with(".0") {
                val.replace(".0", "").to_variant()
            } else {
                val.to_variant()
            }
        }
    }
}

/// `CommonUtils.tiled_color`: a Tiled colour (`#RRGGBB`, or `#AARRGGBB` with the alpha first).
/// An empty or unreadable string gives the fallback.
pub fn tiled_color(s: &str, fallback: Color) -> Color {
    let s = s.trim();
    let hex = s.strip_prefix('#').unwrap_or(s);
    if hex.is_empty() {
        return fallback;
    }
    let hex = if hex.chars().count() == 8 {
        format!("{}{}", &hex[2..], &hex[..2])
    } else {
        hex.to_string()
    };
    if !is_valid_hex_number(&hex) {
        return fallback;
    }
    Color::from_string(hex.as_str()).unwrap_or(fallback)
}

fn is_valid_hex_number(s: &str) -> bool {
    let s = s.strip_prefix(['+', '-']).unwrap_or(s);
    !s.is_empty() && s.bytes().all(|c| c.is_ascii_hexdigit())
}

/// GDScript's `Color(String)`: an HTML code or a colour name.
pub fn color_from_string(s: &str) -> Color {
    Color::from_string(s).unwrap_or(Color::from_rgba(0.0, 0.0, 0.0, 1.0))
}

/// `TilemapCreator.property_color`: a colour-typed value is a Tiled colour, a string one is read
/// as Godot reads it.
pub fn property_color(kind: &str, val: &str) -> Color {
    if kind == "color" {
        tiled_color(val, Color::WHITE)
    } else {
        color_from_string(val)
    }
}

/// `CommonUtils.get_bitmask_integer_from_string`: `"1,3-5"` to the bits 1, 3, 4 and 5.
pub fn bitmask(mask: &str, max_len: i64) -> i64 {
    let mut ret: i64 = 0;
    for s1 in mask.split(',').filter(|s| !s.is_empty()) {
        if s1.contains('-') {
            let mut parts = s1.splitn(2, '-').filter(|s| !s.is_empty());
            let a = parts.next().unwrap_or("");
            let b = parts.next().unwrap_or("");
            let i1 = if godot_str::is_valid_int(a) {
                to_int(a)
            } else {
                0
            };
            let i2 = if godot_str::is_valid_int(b) {
                to_int(b)
            } else {
                0
            };
            if i1 == 0 || i2 == 0 || i1 > i2 {
                continue;
            }
            for i in i1..=i2 {
                if i <= max_len {
                    ret += pow2(i - 1);
                }
            }
        } else if godot_str::is_valid_int(s1) {
            let i = to_int(s1);
            if i <= max_len {
                ret += pow2(i - 1);
            }
        }
    }
    ret
}

/// `int(pow(2, e))`: 0 for a negative exponent.
fn pow2(e: i64) -> i64 {
    if (0..63).contains(&e) {
        1i64 << e
    } else {
        0
    }
}

/// The core's dictionary value as a `Variant` (ints, floats, strings, arrays, dictionaries).
pub fn value_to_variant(v: &Value) -> Variant {
    match v {
        Value::Null => Variant::nil(),
        Value::Bool(b) => b.to_variant(),
        Value::Int(i) => i.to_variant(),
        Value::Float(f) => f.to_variant(),
        Value::String(s) => s.to_variant(),
        Value::Array(a) => {
            let mut arr = VarArray::new();
            for e in a {
                arr.push(&value_to_variant(e));
            }
            arr.to_variant()
        }
        Value::Dict(d) => {
            let mut dict = VarDictionary::new();
            for (k, e) in d {
                dict.set(&k.to_variant(), &value_to_variant(e));
            }
            dict.to_variant()
        }
    }
}

/// Godot's `String.get_file`.
pub fn get_file(path: &str) -> &str {
    match path.rfind(['/', '\\']) {
        Some(i) => &path[i + 1..],
        None => match path.find("://") {
            Some(i) => &path[i + 3..],
            None => path,
        },
    }
}

/// Godot's `String.get_basename` (everything before the last dot).
pub fn get_basename(path: &str) -> &str {
    let slash = path.rfind(['/', '\\']).map(|i| i + 1).unwrap_or(0);
    match path[slash..].rfind('.') {
        Some(i) => &path[..slash + i],
        None => path,
    }
}

/// Godot's `String.get_extension`.
pub fn get_extension(path: &str) -> &str {
    let file = get_file(path);
    match file.rfind('.') {
        Some(i) => &file[i + 1..],
        None => "",
    }
}

/// Godot's `String.is_absolute_path`.
pub fn is_absolute_path(path: &str) -> bool {
    path.starts_with('/') || path.starts_with('\\') || path.contains(":/") || path.contains(":\\")
}

/// Godot's `String.path_join`.
pub fn path_join(base: &str, file: &str) -> String {
    tiled_core::path::join(base, file)
}

/// `CommonUtils.cleanup_path` (on a path without root).
pub fn cleanup_path(path: &str) -> String {
    tiled_core::path::cleanup(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_print_as_godot_prints_them() {
        assert_eq!(num_real(1.0), "1.0");
        assert_eq!(num_real(2.5), "2.5");
        assert_eq!(num_real(0.1), "0.1");
        assert_eq!(num_real(1.0 / 3.0), "0.33333333333333");
        assert_eq!(num_real(1e20), "100000000000000000000.0");
        assert_eq!(num_real(123456789.123), "123456789.123");
        assert_eq!(num_real(1e-7), "0.0000001");
        assert_eq!(num_real(42.0), "42.0");
    }

    #[test]
    fn paths() {
        assert_eq!(get_file("res://a/b.png"), "b.png");
        assert_eq!(get_basename("b.png"), "b");
        assert_eq!(get_basename("res://a/b.c.png"), "res://a/b.c");
        assert_eq!(get_extension("x/y.TMX"), "TMX");
        assert!(is_absolute_path("res://x"));
        assert!(!is_absolute_path("x/y"));
    }

    #[test]
    fn masks() {
        assert_eq!(bitmask("1,3-4", 32), 1 + 4 + 8);
        assert_eq!(bitmask("2", 1), 0);
    }
}
