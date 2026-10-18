//! Custom properties as the GDScript runtime holds them: `{name, type, value}` dictionaries of
//! the parsed file (the core gives them back exactly), on which its property handling works.

use crate::diag::Diag;
use crate::gd::{bitmask, to_int, PV};
use std::collections::HashMap;
use tiled_core::{Project, Property, Value};

/// One property dictionary.
#[derive(Clone, Debug)]
pub struct GProp {
    /// `name` (`""` when absent).
    pub name: String,
    /// `type` (`"string"` when absent, as `property.get("type", "string")` reads it).
    pub kind: String,
    /// `value`, `None` when the dictionary has none.
    pub value: Option<PV>,
    /// `propertytype`: the custom type (class or enum) the value is of, if any.
    pub propertytype: Option<String>,
    /// Merged in from the project file (a custom class's member the object did not override).
    pub from_project: bool,
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
            propertytype: p.propertytype.clone(),
            from_project: p.from_project(),
        }
    }

    /// A property made by the builder (tile metadata as an object property, a forwarded one).
    pub fn new(name: String, kind: String, value: Option<PV>) -> GProp {
        GProp {
            name,
            kind,
            value,
            propertytype: None,
            from_project: false,
        }
    }
}

/// The value names of the project file's enums (`CustomTypes._enum_values_by_name`; of an enum
/// defined twice the later definition).
#[derive(Clone, Debug, Default)]
pub struct Enums(HashMap<String, Vec<String>>);

impl Enums {
    /// The enums of a project (`None`: no project file, no enums).
    pub fn of(project: Option<&Project>) -> Enums {
        let mut e = HashMap::new();
        for t in project
            .and_then(|p| p.property_types.as_deref())
            .unwrap_or(&[])
        {
            if t.kind.as_deref() != Some("enum") {
                continue;
            }
            let values = t
                .values
                .iter()
                .flatten()
                .map(|v| match v {
                    Value::String(s) => s.clone(),
                    other => crate::gd::gd_str(other),
                })
                .collect();
            e.insert(t.name.clone().unwrap_or_default(), values);
        }
        Enums(e)
    }

    /// `CustomTypes.get_enum_mask`: the bits of a string of value names of the enum `name`
    /// (each value's index its bit), `None` when there is no such enum. A name that is no value
    /// (or past `max_len`) is reported and ignored.
    fn mask(&self, name: &str, names: &str, max_len: i64, diag: &mut Diag) -> Option<i64> {
        let values = self.0.get(name)?;
        let mut mask = 0i64;
        for n in names.split(',').filter(|n| !n.is_empty()) {
            let n = n.trim();
            match values.iter().position(|v| v == n) {
                Some(i) if (i as i64) < max_len => mask |= 1i64 << i,
                _ => diag.warn_ts(format!(
                    " -- '{n}' is no layer of enum '{name}'. -> Ignored"
                )),
            }
        }
        Some(mask)
    }
}

/// `CommonUtils.get_mask_value`: the bitmask of a layer-mask property (`collision_layer`,
/// `light_mask`, `navigation_layers`, ...; F5). An `int` is the mask itself (what a Tiled flags
/// enum stored as int saves); a string is value names of the enum its `propertytype` names in
/// the project file, each value's index its bit (`"Player,Enemy"`), or else layer numbers and
/// ranges (`"1,3,5-7"`, 1-based). `max_len`: the number of layers (bits) of the mask.
pub fn mask_value(p: &GProp, max_len: i64, enums: &Enums, diag: &mut Diag) -> i64 {
    let val = p.val();
    if p.kind == "int" {
        return to_int(&val) & ((1i64 << max_len) - 1);
    }
    if let Some(e) = p.propertytype.as_deref().filter(|e| !e.is_empty()) {
        if let Some(mask) = enums.mask(e, &val, max_len, diag) {
            return mask;
        }
    }
    bitmask(&val, max_len)
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
