//! Custom properties as the GDScript runtime holds them: `{name, type, value}` dictionaries of
//! the parsed file (the core gives them back exactly), on which its property handling works.

use crate::gd::PV;
use tiled_core::Property;

/// One property dictionary.
#[derive(Clone, Debug)]
pub struct GProp {
    /// `name` (`""` when absent).
    pub name: String,
    /// `type` (`"string"` when absent, as `property.get("type", "string")` reads it).
    pub kind: String,
    /// `value`, `None` when the dictionary has none.
    pub value: Option<PV>,
}

impl GProp {
    /// `str(property.get("value", ""))`.
    pub fn val(&self) -> String {
        self.value.as_ref().map(PV::gd_str).unwrap_or_default()
    }

    /// The property dictionary of a parsed property (`json`: its document is a JSON file).
    pub fn from_property(p: &Property, json: bool) -> GProp {
        GProp {
            name: dict_name(p.name.as_deref().unwrap_or(""), json || p.from_project()),
            kind: p.kind.clone().unwrap_or_else(|| "string".to_string()),
            value: p.dict_value(json).map(PV::Core),
        }
    }
}

/// `str(property["name"])`: an XML document's name attribute is typed like any attribute, so a
/// numeric name comes back as Godot prints the number (`"007"` is `"7"`).
fn dict_name(name: &str, json: bool) -> String {
    if json || !name.starts_with(|c: char| c.is_ascii_digit() || c == '-' || c == '+' || c == '.') {
        return name.to_string();
    }
    if tiled_core::godot_str::is_valid_int(name) {
        tiled_core::godot_str::to_int(name).to_string()
    } else if tiled_core::godot_str::is_valid_float(name) {
        crate::gd::num_real(tiled_core::godot_str::to_float(name))
    } else {
        name.to_string()
    }
}

/// A `properties` array as dictionaries (`None`: the key is absent).
pub fn gprops(props: &Option<Vec<Property>>, json: bool) -> Option<Vec<GProp>> {
    props
        .as_ref()
        .map(|v| v.iter().map(|p| GProp::from_property(p, json)).collect())
}

/// `get_property`: the value (as `str()`) of the first property named `name` (compared in
/// lower case) of type `kind`, `""` when there is none.
pub fn get_property(props: Option<&[GProp]>, name: &str, kind: &str) -> String {
    for p in props.unwrap_or(&[]) {
        if p.name.to_lowercase() == name && p.kind == kind {
            return p.val();
        }
    }
    String::new()
}

/// `get_properties`: the first value of each wanted `name:type` key.
pub fn get_properties(props: Option<&[GProp]>, wanted: &[&str]) -> Vec<(String, String)> {
    let mut ret: Vec<(String, String)> = Vec::new();
    for p in props.unwrap_or(&[]) {
        let key = format!("{}:{}", p.name.to_lowercase(), p.kind);
        if wanted.contains(&key.as_str()) && !ret.iter().any(|(k, _)| *k == key) {
            ret.push((key, p.val()));
        }
    }
    ret
}

/// The value of a key of [`get_properties`]' result, `""` when absent.
pub fn lookup<'a>(found: &'a [(String, String)], key: &str) -> &'a str {
    found
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
        .unwrap_or("")
}

/// `get_godot_node_type_property`: the value of the `godot_node_type` string property and
/// whether there is one.
pub fn godot_node_type_property(props: Option<&[GProp]>) -> (String, bool) {
    for p in props.unwrap_or(&[]) {
        if p.name.to_lowercase() == "godot_node_type" && p.kind == "string" {
            return (p.val(), true);
        }
    }
    (String::new(), false)
}
