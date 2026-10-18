//! Custom properties assigned to node properties of the same name (F1 of
//! `docs/runtime-improvement-plan.md`: `get_assignable_properties`, `set_property_or_meta`,
//! `convert_for_property`, `get_typed_array_for`, `get_enum_value`, `is_of_class`).

use super::Build;
use crate::files::load_resource;
use godot::classes::{ClassDb, Script};
use godot::global::type_string;
use godot::prelude::*;
use godot::register::info::{PropertyHint, PropertyUsageFlags};
use std::collections::HashMap;
use std::rc::Rc;

/// What a property's info dictionary says about it.
#[derive(Clone, Debug)]
pub(crate) struct PropInfo {
    pub ty: VariantType,
    pub class_name: String,
    pub hint: i64,
    pub hint_string: String,
}

/// A node's assignable properties by name.
pub(crate) type PropInfos = HashMap<String, PropInfo>;

thread_local! {
    /// The assignable properties of engine classes (nodes without a script), by class.
    static ENGINE_PROPS: std::cell::RefCell<HashMap<String, Rc<PropInfos>>> =
        std::cell::RefCell::new(HashMap::new());
}

/// The assignable properties of a node, from its property list.
fn property_infos(node: &Gd<Node>) -> PropInfos {
    let skip = PropertyUsageFlags::CATEGORY.ord()
        | PropertyUsageFlags::GROUP.ord()
        | PropertyUsageFlags::SUBGROUP.ord();
    let want = PropertyUsageFlags::STORAGE.ord()
        | PropertyUsageFlags::EDITOR.ord()
        | PropertyUsageFlags::SCRIPT_VARIABLE.ord();
    let mut ret = PropInfos::new();
    for p in node.get_property_list().iter_shared() {
        let usage = p.get("usage").map(|v| v.to::<i64>()).unwrap_or(0) as u64;
        if usage & skip != 0 || usage & want == 0 {
            continue;
        }
        let name = p
            .get("name")
            .map(|v| v.stringify().to_string())
            .unwrap_or_default();
        if name == "script" || name.starts_with("metadata/") {
            continue;
        }
        let text = |k: &str| {
            p.get(k)
                .map(|v| v.stringify().to_string())
                .unwrap_or_default()
        };
        let ty = p.get("type").map(|v| v.to::<i64>()).unwrap_or(0);
        ret.insert(
            name,
            PropInfo {
                ty: VariantType::from_ord(ty as i32),
                class_name: text("class_name"),
                hint: p.get("hint").map(|v| v.to::<i64>()).unwrap_or(0),
                hint_string: text("hint_string"),
            },
        );
    }
    ret
}

/// `is_of_class`: an engine class, or a script's `class_name` along its base scripts.
pub(crate) fn is_of_class(obj: &Gd<Object>, cls: &str) -> bool {
    if cls.is_empty() || obj.is_class(cls) {
        return true;
    }
    let mut script: Option<Gd<Script>> = obj.get_script();
    while let Some(s) = script {
        if s.get_global_name() == cls {
            return true;
        }
        script = s.get_base_script();
    }
    false
}

/// `get_typed_array_for`: `list` as the array type of the property `name` of `node` (`info`),
/// or `None` when an item does not fit. NodePath items fit a NodePath array, and (`paths`) an
/// array of nodes as the nodes they point at from `node`. The array is made from the
/// property's own (typed) value, so it has the property's element type.
pub(crate) fn typed_array_for(
    info: &PropInfo,
    list: &VarArray,
    node: &Gd<Node>,
    name: &str,
    paths: bool,
) -> Option<Variant> {
    let hs = info.hint_string.as_str();
    let (elem_type, elem_class): (i64, String) =
        if info.hint == PropertyHint::TYPE_STRING.ord() as i64 && hs.contains(':') {
            // A script's typed array: "<type>:" or "<type>/<hint>:<class>"
            let head = hs.split(':').next().unwrap_or("");
            let t = crate::gd::to_int(head.split('/').next().unwrap_or(""));
            (t, hs.split(':').nth(1).unwrap_or("").to_string())
        } else if info.hint == PropertyHint::ARRAY_TYPE.ord() as i64 && !hs.is_empty() {
            // An engine class's typed array (or a script's): the type or class name
            let builtin = (0..VariantType::MAX.ord() as i64).find(|t| type_string(*t) == hs);
            match builtin {
                Some(t) => (t, String::new()),
                None => (VariantType::OBJECT.ord() as i64, hs.to_string()),
            }
        } else {
            return Some(list.to_variant());
        };
    let object = VariantType::OBJECT.ord() as i64;
    let mut items = VarArray::new();
    for item in list.iter_shared() {
        let mut item = item;
        let it = item.get_type();
        if elem_type == object && it == VariantType::NODE_PATH && paths {
            item = node.get_node_or_null(&item.to::<NodePath>())?.to_variant();
        }
        let it = item.get_type();
        if elem_type == VariantType::FLOAT.ord() as i64 && it == VariantType::INT {
            item = (item.to::<i64>() as f64).to_variant();
        }
        if elem_type == VariantType::STRING_NAME.ord() as i64 && it == VariantType::STRING {
            item = StringName::from(&item.to::<GString>()).to_variant();
        }
        if elem_type == VariantType::NODE_PATH.ord() as i64 && it == VariantType::STRING {
            item = NodePath::from(&item.to::<GString>()).to_variant();
        }
        if item.get_type().ord() as i64 != elem_type {
            return None;
        }
        if elem_type == object {
            let o = item.try_to::<Gd<Object>>().ok()?;
            if !is_of_class(&o, &elem_class) {
                return None;
            }
        }
        items.push(&item);
    }
    if elem_type == object && !ClassDb::singleton().class_exists(elem_class.as_str()) {
        return None;
    }
    // An array of the property's element type: its current value's, emptied and refilled
    let current = node.get(name);
    match Some(current).filter(|c| c.get_type() == VariantType::ARRAY) {
        Some(c) => {
            let fresh = c.call("duplicate", &[]);
            fresh.call("clear", &[]);
            fresh.call("assign", &[items.to_variant()]);
            Some(fresh)
        }
        None => Some(items.to_variant()),
    }
}

/// `get_enum_value`: the value of the constant `name` in an enum property's hint string
/// (`"A,B:5,C"`), names compared ignoring case, blanks and underscores (the editor shows
/// `WALK_FAST` as "Walk Fast").
fn enum_value(hint_string: &str, name: &str) -> Option<i64> {
    let norm = |s: &str| s.to_lowercase().replace(['_', ' '], "");
    let wanted = norm(name);
    let mut next = 0i64;
    for entry in hint_string.split(',').filter(|e| !e.is_empty()) {
        let parts: Vec<&str> = entry.split(':').collect();
        let value = match parts.get(1) {
            Some(v) if tiled_core::godot_str::is_valid_int(v) => crate::gd::to_int(v),
            _ => next,
        };
        if norm(parts[0]) == wanted {
            return Some(value);
        }
        next = value + 1;
    }
    None
}

/// `get_property_type_name`.
fn type_name(info: &PropInfo) -> String {
    if info.ty == VariantType::OBJECT && !info.class_name.is_empty() {
        return info.class_name.clone();
    }
    type_string(info.ty.ord() as i64).to_string()
}

impl Build<'_> {
    /// `get_assignable_properties`: the properties of a node (of class `class`) that a custom
    /// property of the same name may set (stored or shown in the editor, and script variables;
    /// not categories and groups, not `script`), cached per class and script. The list of an
    /// engine class without a script is the same in every import: it is kept for the process.
    pub(super) fn assignable(&mut self, node: &Gd<Node>, class: &str) -> Rc<PropInfos> {
        let script = node
            .get_script()
            .map(|s| s.instance_id().to_i64())
            .unwrap_or(0);
        if script == 0 {
            if let Some(p) = ENGINE_PROPS.with_borrow(|c| c.get(class).cloned()) {
                return p;
            }
        } else if let Some(p) = self.prop_info.get(class).and_then(|m| m.get(&script)) {
            return p.clone();
        }
        let ret = Rc::new(property_infos(node));
        if script == 0 {
            ENGINE_PROPS.with_borrow_mut(|c| c.insert(class.to_string(), ret.clone()));
        } else {
            self.prop_info
                .entry(class.to_string())
                .or_default()
                .insert(script, ret.clone());
        }
        ret
    }

    /// `set_property_or_meta` (for a property the node has, `info`): assigned when the value
    /// fits the property's type, else metadata with a warning.
    pub(super) fn set_property_or_meta(
        &mut self,
        node: &mut Gd<Node>,
        name: &str,
        ty: &str,
        value: Variant,
        file_base: &str,
        info: &PropInfo,
    ) {
        if let Some(v) = self.convert_for_property(node, name, info, ty, &value, file_base) {
            node.set(name, &v);
            return;
        }
        self.diag.warn(format!(
            " -- Property '{name}' of '{}': a Tiled '{ty}' value does not fit its type {}. -> Stored as metadata",
            node.get_name(),
            type_name(info)
        ));
        let key = self.sn(name);
        node.set_meta(&key, &value);
    }

    /// `convert_for_property`: `value` (as metadata holds it) converted to the property's type,
    /// `None` when it does not fit. A Tiled int also fits float and enum properties, a string
    /// an enum by its name, StringName and NodePath; a file loads its resource for a Resource
    /// property; a list fits an array (a typed one when every item does); an untyped variable
    /// takes any value.
    fn convert_for_property(
        &mut self,
        node: &Gd<Node>,
        name: &str,
        info: &PropInfo,
        ty: &str,
        value: &Variant,
        file_base: &str,
    ) -> Option<Variant> {
        let pt = info.ty;
        if pt == VariantType::NIL {
            return Some(value.clone());
        }
        if ty == "file" && pt == VariantType::OBJECT {
            let res = load_resource(&value.stringify().to_string(), file_base, &mut self.diag)?;
            let cls = if info.class_name.is_empty() {
                info.hint_string.as_str()
            } else {
                info.class_name.as_str()
            };
            return is_of_class(&res.clone().upcast(), cls).then(|| res.to_variant());
        }
        let vt = value.get_type();
        if pt == VariantType::INT
            && info.hint == PropertyHint::ENUM.ord() as i64
            && vt == VariantType::STRING
        {
            return enum_value(&info.hint_string, &value.stringify().to_string())
                .map(|v| v.to_variant());
        }
        if pt == VariantType::ARRAY && vt == VariantType::ARRAY {
            let list = value.try_to::<VarArray>().ok()?;
            return typed_array_for(info, &list, node, name, false);
        }
        if vt == pt {
            return Some(value.clone());
        }
        if pt == VariantType::FLOAT && vt == VariantType::INT {
            return Some((value.to::<i64>() as f64).to_variant());
        }
        if pt == VariantType::STRING_NAME && vt == VariantType::STRING {
            return Some(StringName::from(&value.to::<GString>()).to_variant());
        }
        if pt == VariantType::NODE_PATH && vt == VariantType::STRING {
            return Some(NodePath::from(&value.to::<GString>()).to_variant());
        }
        None
    }
}
