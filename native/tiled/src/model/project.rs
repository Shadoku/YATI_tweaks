//! The `.tiled-project` file's custom types, and their merge into maps, layers, objects,
//! tilesets and tiles (`CustomTypes.gd`).

use super::property::Style;
use super::{Extra, Property};
use crate::value::Value;
use serde::Deserialize;
use std::collections::HashMap;

/// A Tiled project file: what the importer uses of it is `propertyTypes`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Project {
    /// `propertyTypes`: the custom classes and enums.
    #[serde(rename = "propertyTypes")]
    pub property_types: Option<Vec<PropertyType>>,
    /// Other keys (`folders`, `commands`, ...).
    #[serde(flatten)]
    pub extra: Extra,
    /// `"name|scope"` -> the members merged into a struct of that class and scope.
    #[serde(skip)]
    index: HashMap<String, Vec<Property>>,
}

/// A custom type: a class (with members) or an enum (with values).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct PropertyType {
    /// `id`.
    #[serde(deserialize_with = "super::de::opt_i64")]
    pub id: Option<i64>,
    /// `name`.
    pub name: Option<String>,
    /// `type`: `class` or `enum`.
    #[serde(rename = "type")]
    pub kind: Option<String>,
    /// `useAs`: where a class applies (`property`, `map`, `layer`, `object`, `tile`,
    /// `tileset`, `wangcolor`, `wangset`, `project`).
    #[serde(rename = "useAs")]
    pub use_as: Option<Vec<String>>,
    /// `members` of a class, with their default values.
    pub members: Option<Vec<Property>>,
    /// `values` of an enum.
    pub values: Option<Vec<Value>>,
    /// Other keys (`color`, `drawFill`, `storageType`, `valuesAsFlags`, ...).
    #[serde(flatten)]
    pub extra: Extra,
}

impl Project {
    /// Parses a project file (JSON).
    pub fn from_json(bytes: &[u8]) -> Result<Project, serde_json::Error> {
        let mut p: Project = serde_json::from_slice(crate::parse::strip_bom(bytes))?;
        for t in p.property_types.iter_mut().flatten() {
            for m in t.members.iter_mut().flatten() {
                m.style.0 = Style::Json;
            }
        }
        p.build_index();
        Ok(p)
    }

    /// `CustomTypes._build_index`: the members of every class, by `name|scope` for each scope
    /// in its `useAs`; a class defined twice keeps both member lists in definition order.
    fn build_index(&mut self) {
        self.index.clear();
        for t in self.property_types.iter().flatten() {
            if t.kind.as_deref() != Some("class") {
                continue;
            }
            let name = t.name.as_deref().unwrap_or("");
            let members = t.members.as_deref().unwrap_or(&[]);
            for scope in t.use_as.iter().flatten() {
                self.index
                    .entry(format!("{name}|{scope}"))
                    .or_default()
                    .extend(members.iter().cloned());
            }
        }
    }

    /// The members a struct of class `class` gets in `scope`, if the project defines any.
    pub fn members(&self, class: &str, scope: &str) -> Option<&[Property]> {
        self.index
            .get(&format!("{class}|{scope}"))
            .map(Vec::as_slice)
    }

    /// `CustomTypes.merge_custom_properties`: appends every member of the class (in `scope`)
    /// that `properties` does not already have by name. Returns true when anything was added.
    pub fn merge(&self, properties: &mut Option<Vec<Property>>, class: &str, scope: &str) -> bool {
        if self.index.is_empty() {
            return false;
        }
        let Some(members) = self.members(class, scope) else {
            return false;
        };
        let mut added = false;
        for m in members {
            let name = m.name.as_deref().unwrap_or("");
            let present = properties
                .iter()
                .flatten()
                .any(|p| p.name.as_deref().unwrap_or("") == name);
            if present {
                continue;
            }
            properties.get_or_insert_with(Vec::new).push(m.clone());
            added = true;
        }
        added
    }
}
