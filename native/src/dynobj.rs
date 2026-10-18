//! Classes godot-rust lists as experimental (`Parallax2D`, `NavigationPolygon`,
//! `NavigationRegion2D` and TileData's navigation polygons): reached through `ClassDB` and
//! dynamic calls, so the crate needs no experimental API feature.

use godot::classes::{ClassDb, Node2D, Object, Resource, TileData};
use godot::prelude::*;

fn instantiate(class: &str) -> Variant {
    ClassDb::singleton().instantiate(class)
}

/// A new `Parallax2D`.
pub fn new_parallax2d() -> Gd<Node2D> {
    instantiate("Parallax2D").to::<Gd<Node2D>>()
}

/// A new `NavigationRegion2D`.
pub fn new_navigation_region() -> Gd<Node2D> {
    instantiate("NavigationRegion2D").to::<Gd<Node2D>>()
}

/// A `NavigationPolygon` with one outline, its vertices and one polygon over all of them
/// (what the runtime does in place of the deprecated `make_polygons_from_outlines`).
pub fn new_navigation_polygon(points: &PackedVector2Array) -> Gd<Resource> {
    let mut np = instantiate("NavigationPolygon").to::<Gd<Resource>>();
    np.call("add_outline", &[points.to_variant()]);
    np.call("set_vertices", &[points.to_variant()]);
    let idx: PackedInt32Array = (0..points.len() as i32).collect();
    np.call("add_polygon", &[idx.to_variant()]);
    np
}

/// The first outline of a navigation polygon.
pub fn navigation_outline(np: &Gd<Resource>) -> PackedVector2Array {
    np.clone()
        .call("get_outline", &[0.to_variant()])
        .try_to::<PackedVector2Array>()
        .unwrap_or_default()
}

/// `TileData.get_navigation_polygon(layer)`.
pub fn tile_navigation_polygon(td: &Gd<TileData>, layer: i32) -> Option<Gd<Resource>> {
    td.clone()
        .call("get_navigation_polygon", &[layer.to_variant()])
        .try_to::<Gd<Resource>>()
        .ok()
}

/// `TileData.set_navigation_polygon(layer, polygon)`.
pub fn set_tile_navigation_polygon(td: &mut Gd<TileData>, layer: i32, np: &Gd<Resource>) {
    td.call(
        "set_navigation_polygon",
        &[layer.to_variant(), np.to_variant()],
    );
}

/// A Vector2 property of an object.
pub fn get_vec(o: &Gd<impl Inherits<Object>>, prop: &str) -> Vector2 {
    o.clone()
        .upcast::<Object>()
        .get(prop)
        .try_to::<Vector2>()
        .unwrap_or(Vector2::ZERO)
}

/// Sets a property of an object.
pub fn set(o: &mut Gd<impl Inherits<Object>>, prop: &str, v: impl ToGodot) {
    o.clone().upcast::<Object>().set(prop, &v.to_variant());
}
