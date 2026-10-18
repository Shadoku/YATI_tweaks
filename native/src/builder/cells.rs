//! Tile layers: every tile layer prepared as `tiled_core::prepare` prepares it (payloads
//! decoded into `tile_map_data` blobs whose records wait for their tile fields, one thread per
//! layer, pure Rust), or taken prepared from the map (`TiledMap.prepare_tile_layers`: an
//! asynchronous load prepares them on its worker); then each distinct cell value resolved once
//! on the main thread as `resolve_cell` resolves it (tiles and alternative tiles created in the
//! TileSet on the way), the blob patched with the resolutions, and the layer filled with one
//! `tile_map_data` assignment. Either way the blob is the same, byte for byte.
//!
//! The blob format is engine-internal, so it is checked once per process against the engine
//! (cells set with `set_cell` read back as a blob, and a blob read back cell by cell); should it
//! ever differ, the layers are filled with `set_cell` instead and an error says so.

use super::Build;
use crate::tileset::Built;
use godot::classes::{OccluderPolygon2D, TileData, TileMapLayer, TileSetAtlasSource};
use godot::global::printerr;
use godot::prelude::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use tiled_core::cells::{BlobWriter, TRANSFORM_FLIP_H, TRANSFORM_FLIP_V, TRANSFORM_TRANSPOSE};
use tiled_core::data::{FLIPPED_DIAGONALLY, FLIPPED_HORIZONTALLY, FLIPPED_VERTICALLY, GID_MASK};
use tiled_core::prepare::{PreparedCells, PreparedLayer};
use tiled_core::{Layer, Map};

/// Where a build's tile layers spent their main-thread time, in µs.
#[derive(Clone, Copy, Debug, Default)]
pub struct TileTiming {
    /// Preparing the layers (decoding payloads into blobs) in the build; about 0 when the map
    /// came prepared.
    pub prepare_us: u64,
    /// Resolving the distinct cell values and patching the blobs.
    pub cells_us: u64,
    /// `tile_map_data` assignments (or `set_cell` calls).
    pub assign_us: u64,
}

/// The prepared tile layers of the map being built.
#[derive(Default)]
pub struct Cells {
    prepared: Option<Arc<PreparedCells>>,
    /// Layer address -> its index in `Map::all_layers`.
    index: HashMap<usize, usize>,
}

fn key(l: &Layer) -> usize {
    l as *const Layer as usize
}

impl Cells {
    /// Uses the map's own prepared layers (`TiledMap.prepare_tile_layers`), if any.
    pub fn given(&mut self, prepared: Option<Arc<PreparedCells>>) {
        self.prepared = prepared;
    }

    /// Prepares every tile layer of the map, one thread per layer, unless the map came with
    /// them prepared.
    pub fn prepare(&mut self, map: &Map) {
        let all = map.all_layers();
        self.index = all.iter().enumerate().map(|(i, &l)| (key(l), i)).collect();
        if self
            .prepared
            .as_ref()
            .is_some_and(|p| p.layer_count() == all.len())
        {
            return;
        }
        self.prepared = Some(Arc::new(PreparedCells::prepare(map)));
    }

    /// The prepared cells and a layer's index in them.
    fn of(&self, l: &Layer) -> Option<(Arc<PreparedCells>, usize)> {
        let i = *self.index.get(&key(l))?;
        Some((self.prepared.clone()?, i))
    }
}

/// (source, atlas x, atlas y, alternative) of a resolved cell value.
type Resolved = Option<[i16; 4]>;

/// Whether `tile_map_data` blobs can be used: 0 unchecked, 1 yes, 2 no.
static BLOB_FORMAT: AtomicU8 = AtomicU8::new(0);

fn records(blob: &[u8]) -> Option<Vec<[u8; 12]>> {
    if blob.len() < 2 || !(blob.len() - 2).is_multiple_of(12) || blob[0] != 0 || blob[1] != 0 {
        return None;
    }
    let mut r: Vec<[u8; 12]> = blob[2..]
        .chunks(12)
        .map(|c| c.try_into().expect("12 bytes"))
        .collect();
    r.sort();
    Some(r)
}

/// The format check: two cells through `set_cell` must read back as the core's blob, and the
/// core's blob must read back as those cells.
fn blob_format_ok() -> bool {
    match BLOB_FORMAT.load(Ordering::Relaxed) {
        1 => return true,
        2 => return false,
        _ => {}
    }
    let cells: [(i32, i32, i32, i32, i32, i32); 2] = [
        (
            3,
            -2,
            1,
            2,
            1,
            (TRANSFORM_FLIP_H | TRANSFORM_TRANSPOSE) as i32,
        ),
        (-7, 5, 0, 0, 3, 0),
    ];
    let mut w = BlobWriter::with_capacity(2);
    for c in cells {
        w.push(
            c.0 as i16, c.1 as i16, c.2 as i16, c.3 as i16, c.4 as i16, c.5 as u16,
        );
    }
    let ours = w.finish();
    let mut a = TileMapLayer::new_alloc();
    for c in cells {
        a.set_cell_ex(Vector2i::new(c.0, c.1))
            .source_id(c.2)
            .atlas_coords(Vector2i::new(c.3, c.4))
            .alternative_tile(c.5)
            .done();
    }
    let engine = a.get_tile_map_data_as_array().to_vec();
    a.free();
    let mut ok = records(&engine).is_some() && records(&engine) == records(&ours);
    if ok {
        let mut b = TileMapLayer::new_alloc();
        b.set_tile_map_data_from_array(&PackedByteArray::from(ours.as_slice()));
        for c in cells {
            let at = Vector2i::new(c.0, c.1);
            ok &= b.get_cell_source_id(at) == c.2
                && b.get_cell_atlas_coords(at) == Vector2i::new(c.3, c.4)
                && b.get_cell_alternative_tile(at) == c.5;
        }
        b.free();
    }
    if !ok {
        printerr(&["ERROR: yati_native: the engine's TileMapLayer.tile_map_data format is not the one this build writes; tile layers are filled with set_cell instead (slower). Please report this with your Godot version.".to_variant()]);
    }
    BLOB_FORMAT.store(if ok { 1 } else { 2 }, Ordering::Relaxed);
    ok
}

impl Build<'_> {
    /// `get_first_gid_index`.
    pub(super) fn first_gid_index(&self, gid: u32) -> usize {
        let mut idx = 0;
        for (i, &f) in self.first_gids.iter().enumerate() {
            if gid as i64 >= f {
                idx = i;
            }
        }
        idx
    }

    /// `get_atlas_source_index` (`find_atlas_source_index`, cached).
    pub(super) fn atlas_index(&mut self, gid: u32) -> i32 {
        if let Some(&i) = self.atlas_index_cache.get(&gid) {
            return i;
        }
        let i = self.find_atlas_index(gid);
        self.atlas_index_cache.insert(gid, i);
        i
    }

    fn find_atlas_index(&self, gid: u32) -> i32 {
        let sources = &self.built.sources;
        if sources.is_empty() {
            return -1;
        }
        let ts_first = if self.first_gids.is_empty() {
            -1
        } else {
            self.first_gids[self.first_gid_index(gid)]
        };
        let gid = gid as i64;
        // Animation strips first: a strip stands in for its tile
        for (i, s) in sources.iter().enumerate() {
            if s.is_strip && gid - s.first_gid == s.assigned_id && s.first_gid == ts_first {
                return i as i32;
            }
        }
        for (i, s) in sources.iter().enumerate() {
            let effective = gid - s.first_gid + 1;
            if s.assigned_id < 0 {
                if effective <= s.num_tiles && s.first_gid == ts_first {
                    return i as i32;
                }
            } else if effective == s.assigned_id + 1 {
                return i as i32;
            }
        }
        -1
    }

    /// `get_num_tiles_for_source_id`.
    pub(super) fn num_tiles_of(&self, source_id: i32) -> i64 {
        self.built
            .sources
            .iter()
            .find(|s| s.source_id == source_id)
            .map(|s| s.num_tiles)
            .unwrap_or(-1)
    }

    /// `is_partitioned_tileset`.
    pub(super) fn is_partitioned(&self, source_id: i32) -> bool {
        self.built
            .sources
            .iter()
            .find(|s| s.source_id == source_id)
            .is_some_and(|s| s.assigned_id < 0)
    }

    /// The texture origin that centres a tile of `size` on a map cell (`resolve_cell`).
    fn cell_origin(&self, size: Vector2i) -> Option<Vector2i> {
        let (mw, mh) = (self.tile_w as i32, self.tile_h as i32);
        if size.x != mw || size.y != mh {
            let mut dx = size.x - mw;
            if dx % 2 != 0 {
                dx -= 1;
            }
            let mut dy = size.y - mh;
            if dy % 2 != 0 {
                dy += 1;
            }
            Some(Vector2i::new(-dx / 2, dy / 2))
        } else {
            None
        }
    }

    /// `resolve_cell`: a raw cell value to (source, atlas, alternative), the tile and
    /// alternative tile created in the TileSet when missing.
    pub(super) fn resolve_cell(&mut self, raw: u32) -> Resolved {
        let flip_h = raw & FLIPPED_HORIZONTALLY != 0;
        let flip_v = raw & FLIPPED_VERTICALLY != 0;
        let flip_d = raw & FLIPPED_DIAGONALLY != 0;
        let gid = raw & GID_MASK;
        let idx = self.atlas_index(gid);
        if idx < 0 {
            return None;
        }
        let built: std::rc::Rc<Built> = self.built.clone();
        let item = &built.sources[idx as usize];
        let source_id = item.source_id;
        if source_id < 0 {
            return None;
        }
        let ts = built.tileset.clone();
        if !ts.has_source(source_id) {
            return None;
        }
        let mut src = ts
            .get_source(source_id)?
            .try_cast::<TileSetAtlasSource>()
            .ok()?;
        let grid = src.get_atlas_grid_size();
        if grid.x <= 0 {
            return None;
        }
        let effective = gid as i64 - self.first_gids[self.first_gid_index(gid)];
        let mut coords = Vector2i::ZERO;
        if self.num_tiles_of(source_id) > 1 && grid != Vector2i::ONE {
            coords = Vector2i::new(
                (effective % grid.x as i64) as i32,
                (effective / grid.x as i64) as i32,
            );
        }
        if !src.has_tile(coords) {
            src.create_tile(coords);
            if let Some(mut td) = src.get_tile_data(coords, 0) {
                let size = src.get_texture_region_size();
                let o = match self.cell_origin(size) {
                    Some(o) => o - item.tile_offset,
                    // The tileset's tileoffset applies to every tile
                    None => -item.tile_offset,
                };
                td.set_texture_origin(o);
                if let Some(m) = &item.material {
                    td.set_material(m);
                }
            }
        }
        let mut alt: i32 = 0;
        if flip_h || flip_v || flip_d {
            if self.opts.no_alternative_tiles {
                if flip_h {
                    alt |= TRANSFORM_FLIP_H as i32;
                }
                if flip_v {
                    alt |= TRANSFORM_FLIP_V as i32;
                }
                if flip_d {
                    alt |= TRANSFORM_TRANSPOSE as i32;
                }
            } else {
                alt = i32::from(flip_h) + 2 * i32::from(flip_v) + 4 * i32::from(flip_d);
                if !src.has_alternative_tile(coords, alt) {
                    src.create_alternative_tile_ex(coords)
                        .alternative_id_override(alt)
                        .done();
                    if let (Some(mut td), Some(base_td)) =
                        (src.get_tile_data(coords, alt), src.get_tile_data(coords, 0))
                    {
                        td.set_flip_h(flip_h);
                        td.set_flip_v(flip_v);
                        td.set_transpose(flip_d);
                        let mut size = src.get_texture_region_size();
                        if flip_d {
                            size = Vector2i::new(size.y, size.x);
                        }
                        if let Some(o) = self.cell_origin(size) {
                            td.set_texture_origin(o);
                        }
                        let o = td.get_texture_origin() - item.tile_offset;
                        td.set_texture_origin(o);
                        if let Some(m) = &item.material {
                            td.set_material(m);
                        }
                        polygons_on_alternative(&ts, &base_td, &mut td, alt);
                        for m in base_td.get_meta_list().iter_shared() {
                            let v = base_td.get_meta(&m);
                            td.set_meta(&m, &v);
                        }
                    }
                }
            }
        }
        Some([
            source_id as i16,
            coords.x as i16,
            coords.y as i16,
            alt as i16,
        ])
    }

    /// Fills a tile layer's node with its cells.
    pub(super) fn fill_layer(&mut self, l: &Layer, tml: &mut Gd<TileMapLayer>) {
        let Some((prepared, i)) = self.cells.of(l) else {
            return;
        };
        let Some(pl) = prepared.layer(i) else { return };
        for e in &pl.errors {
            self.diag.error(e.clone());
        }
        let t0 = std::time::Instant::now();
        let resolved: Vec<Resolved> = pl
            .values
            .iter()
            .map(|&raw| match self.cell_cache.get(&raw) {
                Some(r) => *r,
                None => {
                    let r = self.resolve_cell(raw);
                    self.cell_cache.insert(raw, r);
                    r
                }
            })
            .collect();
        if blob_format_ok() {
            let mut bytes = PackedByteArray::from(pl.blob.as_slice());
            let n = PreparedLayer::patch(bytes.as_mut_slice(), &resolved);
            let t1 = std::time::Instant::now();
            self.timing.cells_us += (t1 - t0).as_micros() as u64;
            if n > 2 {
                if n < bytes.len() {
                    bytes.resize(n);
                }
                tml.set_tile_map_data_from_array(&bytes);
            }
            self.timing.assign_us += t1.elapsed().as_micros() as u64;
        } else {
            let t1 = std::time::Instant::now();
            self.timing.cells_us += (t1 - t0).as_micros() as u64;
            for (x, y, index) in pl.records() {
                if let Some(Some([src, ax, ay, alt])) = resolved.get(index as usize) {
                    tml.set_cell_ex(Vector2i::new(x as i32, y as i32))
                        .source_id(*src as i32)
                        .atlas_coords(Vector2i::new(*ax as i32, *ay as i32))
                        .alternative_tile(*alt as u16 as i32)
                        .done();
                }
            }
            self.timing.assign_us += t1.elapsed().as_micros() as u64;
        }
    }
}

/// A polygon of the base tile, flipped as an alternative tile shows it.
fn flip_points(
    pts: &PackedVector2Array,
    origin: Vector2,
    target_origin: Vector2,
    alt: i32,
) -> PackedVector2Array {
    let (h, v, d) = (alt & 1 != 0, alt & 2 != 0, alt & 4 != 0);
    pts.as_slice()
        .iter()
        .map(|&pt| {
            let mut p = pt + origin;
            if d {
                p = Vector2::new(p.y, p.x);
            }
            if h {
                p.x = -p.x;
            }
            if v {
                p.y = -p.y;
            }
            p - target_origin
        })
        .collect()
}

/// `create_polygons_on_alternative_tiles`: collision, navigation and occlusion polygons of the
/// base tile, flipped, on its alternative tile.
fn polygons_on_alternative(
    ts: &Gd<godot::classes::TileSet>,
    from: &Gd<TileData>,
    to: &mut Gd<TileData>,
    alt: i32,
) {
    let o = from.get_texture_origin();
    let origin = Vector2::new(o.x as f32, o.y as f32);
    let t = to.get_texture_origin();
    let target = Vector2::new(t.x as f32, t.y as f32);
    for layer in 0..ts.get_physics_layers_count() {
        for i in 0..from.get_collision_polygons_count(layer) {
            let pts = from.get_collision_polygon_points(layer, i);
            to.add_collision_polygon(layer);
            to.set_collision_polygon_points(layer, i, &flip_points(&pts, origin, target, alt));
        }
    }
    for layer in 0..ts.get_navigation_layers_count() {
        let Some(np) = crate::dynobj::tile_navigation_polygon(from, layer) else {
            continue;
        };
        let pts = flip_points(&crate::dynobj::navigation_outline(&np), origin, target, alt);
        let nav = crate::dynobj::new_navigation_polygon(&pts);
        crate::dynobj::set_tile_navigation_polygon(to, layer, &nav);
    }
    for layer in 0..ts.get_occlusion_layers_count() {
        if from.get_occluder_polygons_count(layer) == 0 {
            continue;
        }
        let Some(occ) = from.get_occluder_polygon(layer, 0) else {
            continue;
        };
        let pts = flip_points(&occ.get_polygon(), origin, target, alt);
        let mut op = OccluderPolygon2D::new_gd();
        op.set_polygon(&pts);
        to.set_occluder_polygons_count(layer, 1);
        to.set_occluder_polygon(layer, 0, &op);
    }
}
