//! The builder as a streamer uses it (`crate::streamer`): the tree of a map without its content
//! (every layer node, configured as the one-shot build configures it, tile layers empty, object
//! layers without objects: the [`Skeleton`]), then, frame after frame on the suspended build,
//! cell values resolved, objects created one by one by the one-shot build's own code, and object
//! references resolved again whenever their targets come or go.

use super::{Build, Suspended};
use crate::diag::Diag;
use crate::map::TiledMap;
use crate::options::Options;
use crate::tileset::{Built, TiledTileSets};
use godot::classes::{Node, Node2D, TileMapLayer, TileSetAtlasSource};
use godot::prelude::*;
use std::collections::HashSet;
use std::rc::Rc;
use tiled_core::{Layer, Map, Object};

/// A layer's identity within its map: its address (the map is shared and never changes while
/// a streamer holds it).
pub(crate) fn layer_key(l: &Layer) -> usize {
    l as *const Layer as usize
}

/// The layer nodes of a streamed map, by layer (`layer_key`), in the map's order.
#[derive(Default)]
pub(crate) struct Skeleton {
    pub tile_layers: Vec<(usize, Gd<TileMapLayer>)>,
    pub object_layers: Vec<(usize, Gd<Node2D>)>,
}

/// A resolved cell value: (source, atlas x, atlas y, alternative) and whether its tile has a
/// collision polygon.
pub(crate) type Cell = ([i16; 4], bool);

impl<'m> Build<'m> {
    /// A build for a streamer: `run_skeleton` makes the map's tree without its content.
    pub(crate) fn for_stream(
        map: &'m Map,
        zip: &str,
        tilesets: &'m mut TiledTileSets,
        opts: Options,
    ) -> Self {
        let mut b = Build::new(map, zip, tilesets, opts);
        b.skel = Some(Skeleton::default());
        b
    }

    /// The map's tree as `run` makes it, but with empty tile layers and object layers without
    /// objects (recorded in the returned [`Skeleton`]), always under a root named after the map
    /// (no single-layer unwrapping), its properties and metadata set. `built`: a TileSet the
    /// caller already holds for this map's tilesets; `None` takes it from the cache.
    pub(crate) fn run_skeleton(
        mut self,
        built: Option<Rc<Built>>,
    ) -> (Gd<Node2D>, Skeleton, Suspended) {
        let map = self.map;
        for t in map.tilesets.iter().flatten() {
            self.first_gids.push(t.firstgid.unwrap_or(0));
        }
        let zip = self.zip.clone();
        let opts = self.opts.clone();
        self.built = match built {
            Some(b) => b,
            None => self
                .tilesets
                .built_for_map(map, &zip, &opts, &mut self.diag),
        };
        let mut base = self.base_node.clone();
        base.set_name(self.base_name.as_str());
        if self.parallax_mode == "legacy" {
            let mut pbg = godot::classes::ParallaxBackground::new_alloc();
            base.add_child(&pbg);
            pbg.set_name(format!("{} (PBG)", self.base_name).as_str());
            self.own(&mut pbg);
            self.parallax_background = Some(pbg);
        }
        if let Some(bg) = map.backgroundcolor.as_deref().filter(|c| !c.is_empty()) {
            let mut rect = godot::classes::ColorRect::new_alloc();
            rect.set_color(crate::gd::tiled_color(bg, Color::WHITE));
            rect.set_size(Vector2::new(
                (self.map_w * self.tile_w) as f32,
                (self.map_h * self.tile_h) as f32,
            ));
            base.add_child(&rect);
            rect.set_name("Background Color");
            self.own(&mut rect);
            self.background = Some(rect);
        }
        let parent: Gd<Node> = base.clone().upcast();
        for l in map.layers.iter().flatten() {
            self.handle_layer(l, &parent);
        }
        if let Some(p) = &super::gprops(&map.properties, self.json) {
            let mut n: Gd<Node> = base.clone().upcast();
            self.handle_properties(&mut n, p, "");
        }
        self.update_refs(&HashSet::new());
        if let Some(pbg) = self.parallax_background.take() {
            if pbg.get_child_count() == 0 {
                base.remove_child(&pbg);
                pbg.free();
            }
        }
        let mut root: Gd<Node> = base.clone().upcast();
        self.root_metadata(&mut root);
        let skel = std::mem::take(self.skel.as_mut().expect("a streaming build"));
        (base, skel, self.suspend())
    }

    /// The TileSet and registry the build uses.
    pub(crate) fn built(&self) -> Rc<Built> {
        self.built.clone()
    }

    /// A raw cell value (flip flags included) resolved as the one-shot build resolves it (the
    /// tile, and with `no_alternative_tiles` off the alternative tile, created in the TileSet on
    /// first use), with whether its tile has a collision polygon; `None` for a value no tileset
    /// has.
    pub(crate) fn resolve_value(&mut self, raw: u32) -> Option<Cell> {
        let r = match self.cell_cache.get(&raw) {
            Some(r) => *r,
            None => {
                let r = self.resolve_cell(raw);
                self.cell_cache.insert(raw, r);
                r
            }
        }?;
        let ts = &self.built.tileset;
        let mut collision = false;
        let layers = ts.get_physics_layers_count();
        if layers > 0 {
            let src = ts
                .get_source(r[0] as i32)
                .and_then(|s| s.try_cast::<TileSetAtlasSource>().ok());
            // With transform flags (no_alternative_tiles) the alternative's high bits are flags
            let alt = (r[3] as u16 as i32) & 0x0FFF;
            if let Some(td) =
                src.and_then(|s| s.get_tile_data(Vector2i::new(r[1] as i32, r[2] as i32), alt))
            {
                collision = (0..layers).any(|l| td.get_collision_polygons_count(l) > 0);
            }
        }
        Some((r, collision))
    }

    /// Creates the nodes of one object of an object layer under the layer's node, exactly as
    /// the one-shot build creates them; returns the nodes added to the layer node (a template
    /// instance can add several).
    pub(crate) fn spawn_object(&mut self, o: &Object, layer_node: &Gd<Node>) -> Vec<Gd<Node>> {
        let before = layer_node.get_child_count();
        let obj = self.map_object(o);
        let ts = self.built.clone();
        self.handle_object(obj, layer_node, &ts, Vector2::ZERO);
        (before..layer_node.get_child_count())
            .filter_map(|i| layer_node.get_child(i))
            .collect()
    }

    /// Forgets the nodes of a despawned object (id `id`): it no longer stands for the id.
    pub(crate) fn forget_object(&mut self, id: i64, nodes: &[Gd<Node>]) {
        if let Some(n) = self.nodes_by_id.get(&id) {
            if !n.is_instance_valid() || nodes.contains(n) {
                self.nodes_by_id.remove(&id);
            }
        }
    }

    /// Resolves the object references made since the last call, and again those whose targets
    /// are among `changed` (objects spawned or despawned since); references of freed nodes are
    /// dropped. A target that is not spawned gives an empty path, without a warning.
    pub(crate) fn update_refs(&mut self, changed: &HashSet<i64>) {
        let fresh = std::mem::take(&mut self.pending_refs);
        let mut all = std::mem::take(&mut self.stream_refs);
        if !changed.is_empty() {
            all.retain(|r| r.node.is_instance_valid());
            for r in &all {
                if r.targets().iter().any(|id| changed.contains(id)) {
                    self.resolve_ref(r);
                }
            }
        }
        for r in fresh {
            self.resolve_ref(&r);
            all.push(r);
        }
        self.stream_refs = all;
    }

    /// The `_tiled_import` hooks of the objects created since the last call.
    pub(crate) fn run_hooks(&mut self) {
        self.call_import_hooks();
    }

    /// The messages since the last call (printed already; counted here).
    pub(crate) fn take_diag(&mut self) -> Diag {
        std::mem::take(&mut self.diag)
    }
}

/// Every gid the map's tilesets define (`tilecount` tiles from each first gid, and every listed
/// tile id beyond), for creating their tiles before any cell is set: a TileSet changed while a
/// layer holds cells makes the engine rebuild every cell of every layer using it.
pub(crate) fn tileset_gids(map: &Map) -> Vec<u32> {
    let mut out = Vec::new();
    for slot in map.tileset_slots() {
        let Some(ts) = map.slot_tileset(slot) else {
            continue;
        };
        let count = ts.tilecount.unwrap_or(0).max(0);
        let max_id = ts
            .tiles
            .iter()
            .flatten()
            .filter_map(|t| t.id)
            .max()
            .map(|m| m + 1)
            .unwrap_or(0);
        for local in 0..count.max(max_id) {
            let gid = slot.firstgid + local;
            if gid > 0 && gid <= tiled_core::data::GID_MASK as i64 {
                out.push(gid as u32);
            }
        }
    }
    out
}

/// The map a `TiledMap` holds, if loaded.
pub(crate) fn map_of(m: &Gd<TiledMap>) -> Option<(std::sync::Arc<Map>, String)> {
    let b = m.bind();
    b.inner().map(|i| (i, b.zip_file.clone()))
}
