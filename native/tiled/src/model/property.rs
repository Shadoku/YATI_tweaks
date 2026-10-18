//! Custom properties: `properties` arrays of maps, layers, objects, tilesets, tiles, wang sets
//! and colours, and the members of `.tiled-project` class types.

use super::{Extra, Meta, Num};
use crate::value::Value;
use serde::{Deserialize, Deserializer};
use std::collections::BTreeMap;

/// One custom property (`{"name", "type", "propertytype", "value"}`), or one item of a list
/// property (the same without a name).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(from = "RawProperty")]
pub struct Property {
    /// `name`.
    pub name: Option<String>,
    /// `type`: `string` (also when absent), `int`, `float`, `bool`, `color`, `file`, `object`,
    /// `class` or `list`.
    pub kind: Option<String>,
    /// `propertytype`: the custom type's name, for class and enum values.
    pub propertytype: Option<String>,
    /// `value`, typed by `kind` (see [`PropertyValue`]).
    pub value: Option<PropertyValue>,
    /// Other keys (for example a project member's `propertyType`).
    pub extra: Extra,
    pub(crate) style: Meta<Style>,
}

/// How a property's value appears in the GDScript dictionary (see [`crate::value`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Style {
    /// As the document's format gives it.
    #[default]
    Document,
    /// A string given as the XML element's text (multi-line), kept as a string.
    XmlText,
    /// Merged in from the `.tiled-project` (JSON) file, whatever the document's format.
    Json,
}

/// A property value.
///
/// Scalars: `bool` properties are [`Bool`](Self::Bool), `int`, `float` and `object` ones
/// [`Number`](Self::Number), `string`, `color`, `file` and enum values
/// [`String`](Self::String) (an XML value that does not read as its declared type stays a
/// string). A `class` property's value is [`Class`](Self::Class): its members by name, nested
/// classes as nested `Class`, lists inside classes as [`Array`](Self::Array). A top-level
/// `list` property's value is [`List`](Self::List), its items carrying their own type.
#[derive(Clone, Debug, PartialEq)]
pub enum PropertyValue {
    /// JSON `null`.
    Null,
    /// A boolean.
    Bool(bool),
    /// A number (int, float or object id).
    Number(Num),
    /// A string, colour (`#AARRGGBB`), file path or enum name.
    String(String),
    /// A class value: member name to value.
    Class(BTreeMap<String, PropertyValue>),
    /// The items of a top-level list property, each with its `type` and `value`.
    List(Vec<Property>),
    /// A list inside a class value: the items' values only.
    Array(Vec<PropertyValue>),
}

impl PropertyValue {
    /// The value as a bool (`Bool`, or a `"true"` string).
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            PropertyValue::Bool(b) => Some(*b),
            PropertyValue::String(s) => Some(s == "true"),
            _ => None,
        }
    }

    /// The value as an integer (a number truncated).
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            PropertyValue::Number(n) => Some(n.as_i64()),
            _ => None,
        }
    }

    /// The value as a float.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            PropertyValue::Number(n) => Some(n.as_f64()),
            _ => None,
        }
    }

    /// The value as a string slice.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            PropertyValue::String(s) => Some(s),
            _ => None,
        }
    }

    /// A JSON value as a property value: objects become classes, arrays arrays.
    pub(crate) fn from_json(v: Value) -> PropertyValue {
        match v {
            Value::Null => PropertyValue::Null,
            Value::Bool(b) => PropertyValue::Bool(b),
            Value::Int(i) => PropertyValue::Number(Num::Int(i)),
            Value::Float(f) => PropertyValue::Number(Num::Float(f)),
            Value::String(s) => PropertyValue::String(s),
            Value::Array(a) => PropertyValue::Array(a.into_iter().map(Self::from_json).collect()),
            Value::Dict(d) => PropertyValue::Class(
                d.into_iter()
                    .map(|(k, v)| (k, Self::from_json(v)))
                    .collect(),
            ),
        }
    }
}

impl Property {
    /// A property with a name, a type and a value.
    pub fn new(name: impl Into<String>, kind: impl Into<String>, value: PropertyValue) -> Self {
        Property {
            name: Some(name.into()),
            kind: Some(kind.into()),
            value: Some(value),
            ..Default::default()
        }
    }

    /// The declared type, `string` when absent.
    pub fn kind_or_string(&self) -> &str {
        self.kind.as_deref().unwrap_or("string")
    }

    /// A JSON property dictionary (a list item or a project member) as a property.
    pub(crate) fn from_json_dict(mut d: BTreeMap<String, Value>) -> Property {
        let take_str = |d: &mut BTreeMap<String, Value>, k: &str| match d.remove(k) {
            Some(Value::String(s)) => Some(s),
            Some(other) => {
                d.insert(k.to_string(), other);
                None
            }
            None => None,
        };
        let name = take_str(&mut d, "name");
        let kind = take_str(&mut d, "type");
        let propertytype = take_str(&mut d, "propertytype");
        let value = d.remove("value");
        Property::from_raw(RawProperty {
            name,
            kind,
            propertytype,
            value,
            extra: d,
        })
    }

    fn from_raw(r: RawProperty) -> Property {
        let value = r.value.map(|v| match (r.kind.as_deref(), v) {
            (Some("list"), Value::Array(items))
                if items.iter().all(|i| matches!(i, Value::Dict(_))) =>
            {
                PropertyValue::List(
                    items
                        .into_iter()
                        .map(|i| match i {
                            Value::Dict(d) => Property::from_json_dict(d),
                            _ => unreachable!(),
                        })
                        .collect(),
                )
            }
            (_, v) => PropertyValue::from_json(v),
        });
        Property {
            name: r.name,
            kind: r.kind,
            propertytype: r.propertytype,
            value,
            extra: r.extra,
            style: Meta(Style::Document),
        }
    }
}

/// A property as the JSON formats write it.
#[derive(Deserialize)]
struct RawProperty {
    #[serde(default, deserialize_with = "super::de::opt_string")]
    name: Option<String>,
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    propertytype: Option<String>,
    #[serde(default, deserialize_with = "present")]
    value: Option<Value>,
    #[serde(flatten)]
    extra: Extra,
}

/// A key that is present is `Some`, even when its value is `null`.
fn present<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
}

impl From<RawProperty> for Property {
    fn from(r: RawProperty) -> Self {
        Property::from_raw(r)
    }
}
