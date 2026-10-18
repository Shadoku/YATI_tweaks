//! `TiledTileSets`: the TileSet builder (the GDScript runtime's `TilesetCreator`, ported) and
//! its cache.
//!
//! A map's TileSet is built from its ordered list of tilesets: one atlas source per atlas
//! tileset, one per image collection tile, animation strips after them, collision, navigation
//! and occlusion polygons, terrains from wang sets, custom data layers and metadata. Next to
//! the TileSet the builder keeps what the runtime registers for its gid lookups (the atlas
//! sources in source id order, the tiles' collision object groups); the runtime stores the
//! latter in an internal custom data layer that it removes at the end, this builder keeps it
//! on the Rust side and never adds the layer.

use crate::diag::Diag;
use crate::files::{clear_texture_cache, load_resource, texture_cache_len, Textures};
use crate::gd::{bitmask, get_basename, get_extension, get_file, to_int, typed_value, PV};
use crate::map::TiledMap;
use crate::options::{options_of, Options, TiledImportOptions};
use crate::props::{gprops, GProp};
use godot::classes::image::Format as ImageFormat;
use godot::classes::tile_set::{CellNeighbor, TerrainMode, TileLayout, TileOffsetAxis, TileShape};
use godot::classes::{
    Image, ImageTexture, OccluderPolygon2D, PlaceholderTexture2D, Shader, ShaderMaterial,
    Texture2D, TileData, TileSet, TileSetAtlasSource,
};
use godot::prelude::*;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::sync::Arc;
use tiled_core::{Format, Map, Object, Tileset, TilesetSlot, TilesetSource, Value};

/// An atlas source as the runtime registers it (`register_atlas_source`).
#[derive(Clone, Debug)]
pub struct AtlasReg {
    pub source_id: i32,
    pub num_tiles: i64,
    /// The tile id of an image collection tile or a strip, -1 for an atlas.
    pub assigned_id: i64,
    pub tile_offset: Vector2i,
    pub orientation: String,
    pub alignment: String,
    pub first_gid: i64,
    pub material: Option<Gd<ShaderMaterial>>,
    pub is_strip: bool,
}

/// A built TileSet and what the runtime registered while building it.
pub struct Built {
    /// The TileSet (an empty one when the list had no tileset).
    pub tileset: Gd<TileSet>,
    /// The registered atlas sources, sorted by source id.
    pub sources: Vec<AtlasReg>,
    /// Object groups (a tile's collision shapes) by their 1-based index.
    pub object_groups: HashMap<i32, Arc<Vec<Object>>>,
    /// The object group index of a tile: (source id, atlas x, atlas y) -> index.
    pub internal: HashMap<(i32, i32, i32), i32>,
}

impl Built {
    /// The object group index of a tile (the runtime's internal custom data), 0 for none.
    pub fn group_index(&self, source_id: i32, coords: Vector2i) -> i32 {
        self.internal
            .get(&(source_id, coords.x, coords.y))
            .copied()
            .unwrap_or(0)
    }
}

/// One tileset of the list to build from.
pub struct SlotRef<'a> {
    pub firstgid: i64,
    pub tileset: Option<&'a Tileset>,
    pub skipped: bool,
    pub base_dir: String,
}

/// The parameters a TileSet's content depends on besides its tilesets.
#[derive(Clone)]
pub struct BuildParams {
    pub map_tile_size: Vector2i,
    pub map_wangset_to_terrain: bool,
    pub custom_data_prefix: String,
    pub animation_mode: String,
    /// Whether an unsupported `animation_mode` is reported (the map's builder does, the
    /// templates' do not).
    pub report_animation_mode: bool,
    pub zip: String,
}

struct PendingStrip {
    tile_id: i64,
    frames: Vec<(i64, f32)>,
    collection: usize,
    atlas: Option<(Gd<TileSetAtlasSource>, i32)>,
    columns: i64,
    tile_count: i64,
    tile_offset: Vector2i,
    orientation: String,
    alignment: String,
    first_gid: i64,
    material: Option<Gd<ShaderMaterial>>,
}

const PEERING_BITS: [CellNeighbor; 8] = [
    CellNeighbor::TOP_SIDE,
    CellNeighbor::TOP_RIGHT_CORNER,
    CellNeighbor::RIGHT_SIDE,
    CellNeighbor::BOTTOM_RIGHT_CORNER,
    CellNeighbor::BOTTOM_SIDE,
    CellNeighbor::BOTTOM_LEFT_CORNER,
    CellNeighbor::LEFT_SIDE,
    CellNeighbor::TOP_LEFT_CORNER,
];

fn terrain_mode(kind: Option<&str>) -> TerrainMode {
    match kind {
        Some("edge") => TerrainMode::SIDES,
        Some("mixed") => TerrainMode::CORNERS_AND_SIDES,
        _ => TerrainMode::CORNERS,
    }
}

fn custom_data_type(kind: &str) -> VariantType {
    match kind {
        "bool" => VariantType::BOOL,
        "int" => VariantType::INT,
        "float" => VariantType::FLOAT,
        "color" => VariantType::COLOR,
        "class" => VariantType::DICTIONARY,
        _ => VariantType::STRING,
    }
}

#[derive(Clone, Copy)]
enum LayerType {
    Physics,
    Navigation,
    Occlusion,
}

/// Where a [`Creator`] stands in its list of tilesets.
#[derive(Clone, Copy, Debug)]
enum Cursor {
    /// At a tileset of the list: `tile` is `None` before its header (atlas source, texture),
    /// else the index of its next tile.
    Slot { slot: usize, tile: Option<usize> },
    /// Every tileset done; the animation strips are next.
    Strips,
    /// The TileSet is complete.
    Done,
}

/// `TilesetCreator`, one per TileSet built. It owns what it builds and its messages (kept
/// quietly, reported by every import that uses the TileSet), so a creation can be spread over
/// several calls ([`Creator::step`], a budget of tiles each: `TiledTileSets.build_step`).
struct Creator {
    params: BuildParams,
    diag: Diag,
    textures: Textures,
    cursor: Cursor,
    tileset: Option<Gd<TileSet>>,
    append: bool,
    atlas: Option<(Gd<TileSetAtlasSource>, i32)>,
    max: (i32, i32),
    tile_size: Vector2i,
    tile_count: i64,
    columns: i64,
    grid_size: Vector2i,
    tile_offset: Vector2i,
    orientation: String,
    alignment: String,
    first_gid: i64,
    material: Option<Gd<ShaderMaterial>>,
    terrain_sets: i32,
    terrains: i32,
    physics_layers: i32,
    navigation_layers: i32,
    occlusion_layers: i32,
    base_dir: String,
    json: bool,
    collections: Vec<HashMap<i64, (Gd<TileSetAtlasSource>, i32)>>,
    pending: Vec<PendingStrip>,
    sources: Vec<AtlasReg>,
    object_groups: HashMap<i32, Arc<Vec<Object>>>,
    groups_counter: i32,
    internal: HashMap<(i32, i32, i32), i32>,
}

impl Creator {
    fn new(params: BuildParams, diag: Diag) -> Self {
        Creator {
            params,
            diag,
            textures: Textures::default(),
            cursor: Cursor::Slot {
                slot: 0,
                tile: None,
            },
            tileset: None,
            append: false,
            atlas: None,
            max: (0, 0),
            tile_size: Vector2i::ZERO,
            tile_count: 0,
            columns: 0,
            grid_size: Vector2i::ZERO,
            tile_offset: Vector2i::ZERO,
            orientation: "orthogonal".into(),
            alignment: "unspecified".into(),
            first_gid: -1,
            material: None,
            terrain_sets: -1,
            terrains: 0,
            physics_layers: -1,
            navigation_layers: -1,
            occlusion_layers: -1,
            base_dir: String::new(),
            json: false,
            collections: Vec::new(),
            pending: Vec::new(),
            sources: Vec::new(),
            object_groups: HashMap::new(),
            groups_counter: 0,
            internal: HashMap::new(),
        }
    }

    fn ts(&mut self) -> &mut Gd<TileSet> {
        self.tileset.as_mut().expect("tileset created")
    }

    /// `create_from_dictionary_array`, resumable: works through `slots` (the same list on every
    /// call) until `budget` (tiles, a tileset's header and end counting one each, the
    /// animation strips one each) is spent. True once the TileSet is complete.
    fn step(&mut self, slots: &[SlotRef], budget: &mut usize) -> bool {
        loop {
            match self.cursor {
                Cursor::Done => return true,
                _ if *budget == 0 => return false,
                Cursor::Strips => {
                    *budget = budget.saturating_sub(self.pending.len().max(1));
                    self.create_animation_strips();
                    self.cursor = Cursor::Done;
                }
                Cursor::Slot { slot, tile } => {
                    let Some(s) = slots.get(slot) else {
                        self.cursor = Cursor::Strips;
                        continue;
                    };
                    let next = Cursor::Slot {
                        slot: slot + 1,
                        tile: None,
                    };
                    // Skipped, or a missing or unreadable file: the loader reported it.
                    let Some(ts) = s.tileset.filter(|_| !s.skipped) else {
                        self.cursor = next;
                        continue;
                    };
                    match tile {
                        None => {
                            *budget -= 1;
                            self.base_dir = s.base_dir.clone();
                            if self.begin_tileset(ts, s.firstgid) {
                                self.cursor = Cursor::Slot {
                                    slot,
                                    tile: Some(0),
                                };
                            } else {
                                self.append = true;
                                self.cursor = next;
                            }
                        }
                        Some(i) => {
                            let tiles = ts.tiles.as_deref().unwrap_or(&[]);
                            if i < tiles.len() {
                                let n = (*budget).min(tiles.len() - i);
                                self.handle_tiles(&tiles[i..i + n]);
                                *budget -= n;
                                self.cursor = Cursor::Slot {
                                    slot,
                                    tile: Some(i + n),
                                };
                            } else {
                                *budget -= 1;
                                self.end_tileset(ts);
                                self.append = true;
                                self.cursor = next;
                            }
                        }
                    }
                }
            }
        }
    }

    /// The finished TileSet and its registry, and the creation's messages.
    fn finish(mut self) -> (Built, Diag) {
        self.textures.close_zip();
        let tileset = self.tileset.take().unwrap_or_else(TileSet::new_gd);
        let mut sources = std::mem::take(&mut self.sources);
        sources.sort_by_key(|s| s.source_id);
        let built = Built {
            tileset,
            sources,
            object_groups: std::mem::take(&mut self.object_groups),
            internal: std::mem::take(&mut self.internal),
        };
        (built, self.diag)
    }

    /// The first half of `create_or_append`: the tileset's atlas source and texture. False
    /// when the rest of the tileset is skipped (AutoMap rules, or no texture).
    fn begin_tileset(&mut self, t: &Tileset, firstgid: i64) -> bool {
        if t.name.as_deref() == Some("AutoMap Rules") {
            return false;
        }
        self.json = t.format() == Format::Json;
        if !self.append {
            self.tileset = Some(TileSet::new_gd());
        }
        self.tile_size = Vector2i::new(
            t.tilewidth.unwrap_or(0) as i32,
            t.tileheight.unwrap_or(0) as i32,
        );
        if !self.append {
            let s = self.params.map_tile_size;
            self.ts().set_tile_size(s);
        }
        self.tile_count = t.tilecount.unwrap_or(0);
        self.columns = t.columns.unwrap_or(0);
        self.orientation = "orthogonal".into();
        self.grid_size = self.tile_size;
        self.tile_offset = match &t.tileoffset {
            Some(o) => Vector2i::new(o.x.unwrap_or(0) as i32, o.y.unwrap_or(0) as i32),
            None => Vector2i::ZERO,
        };
        self.first_gid = firstgid;
        if let Some(g) = &t.grid {
            if let Some(o) = &g.orientation {
                self.orientation = o.clone();
            }
            self.grid_size.x = g.width.map(|v| v as i32).unwrap_or(self.tile_size.x);
            self.grid_size.y = g.height.map(|v| v as i32).unwrap_or(self.tile_size.y);
        }
        self.alignment = t
            .objectalignment
            .clone()
            .unwrap_or_else(|| "unspecified".into());
        if self.append {
            self.terrains = 0;
        }
        self.collections.push(HashMap::new());

        self.material = t.transparentcolor.as_deref().map(transparent_material);

        if let Some(image) = &t.image {
            let mut src = TileSetAtlasSource::new_gd();
            let requested = atlas_id_property(&gprops(&t.properties, self.json));
            let id = self
                .ts()
                .add_source_ex(&src)
                .atlas_source_id_override(requested)
                .done();
            src.set_texture_region_size(self.tile_size);
            if let Some(m) = t.margin {
                src.set_margins(Vector2i::new(m as i32, m as i32));
            }
            if let Some(s) = t.spacing {
                src.set_separation(Vector2i::new(s as i32, s as i32));
            }
            self.atlas = Some((src.clone(), id));
            let base = self.base_dir.clone();
            let zip = self.params.zip.clone();
            let Some(texture) = self.textures.load(image, &base, &zip, &mut self.diag) else {
                // Can't continue without texture
                return false;
            };
            src.set_texture(&texture);
            let (w, h) = (texture.get_width() as i64, texture.get_height() as i64);
            if self.tile_size.x > 0 && self.tile_size.y > 0 {
                self.columns = w / self.tile_size.x as i64;
                self.tile_count = self.columns * h / self.tile_size.y as i64;
            }
            self.register(id, self.tile_count, -1);
            let grid = src.get_atlas_grid_size();
            self.max = (grid.x - 1, grid.y - 1);
        }
        true
    }

    /// The second half of `create_or_append`, after the tiles (`handle_tiles`): wang sets,
    /// the tileset's properties and class.
    fn end_tileset(&mut self, t: &Tileset) {
        let props = gprops(&t.properties, self.json);
        if let Some(w) = &t.wangsets {
            if self.params.map_wangset_to_terrain {
                self.handle_wangsets_old_mapping(w);
            } else {
                self.handle_wangsets(w);
            }
        }
        if let Some(p) = &props {
            self.handle_tileset_properties(p);
        }
        // The last tileset with a class wins (one TileSet for all).
        let class = t.class.as_deref().unwrap_or("");
        if !class.is_empty() {
            self.ts().set_meta("class", &class.to_variant());
        }
    }

    fn register(&mut self, source_id: i32, num_tiles: i64, assigned_id: i64) {
        self.sources.push(AtlasReg {
            source_id,
            num_tiles,
            assigned_id,
            tile_offset: self.tile_offset,
            orientation: self.orientation.clone(),
            alignment: self.alignment.clone(),
            first_gid: self.first_gid,
            material: self.material.clone(),
            is_strip: false,
        });
    }

    /// `create_tile_if_not_existing_and_get_tiledata`.
    fn tile_data_for(&mut self, tile_id: i64) -> Option<(Gd<TileData>, i32, Vector2i)> {
        if tile_id < self.tile_count {
            if self.columns <= 0 {
                return None;
            }
            let row = (tile_id / self.columns) as i32;
            let col = (tile_id % self.columns) as i32;
            let coords = Vector2i::new(col, row);
            if col > self.max.0 || row > self.max.1 {
                self.diag.warn_ts(format!(
                    " -- Tile {tile_id} at {col},{row} outside texture range. -> Skipped"
                ));
                return None;
            }
            let (mut src, id) = self.atlas.clone()?;
            let at = src.get_tile_at_coords(coords);
            if at == Vector2i::new(-1, -1) {
                src.create_tile(coords);
            } else if at != coords {
                self.diag
                    .note("WARNING: tile_at_coords not equal tile_coords!");
                self.diag
                    .note(format!("         tile_coords:   {col},{row}"));
                self.diag
                    .note(format!("         tile_at_coords: {},{}", at.x, at.x));
                self.diag.warn_ts("-> Tile skipped");
                return None;
            }
            return src.get_tile_data(coords, 0).map(|d| (d, id, coords));
        }
        self.diag.warn_ts(format!(
            " -- Tile id {tile_id} outside tile count range (0-{}). -> Skipped.",
            self.tile_count - 1
        ));
        None
    }

    fn origin_for_size(&self, size: Vector2i) -> Option<Vector2i> {
        let m = self.params.map_tile_size;
        if size.x != m.x || size.y != m.y {
            let mut dx = size.x - m.x;
            if dx % 2 != 0 {
                dx -= 1;
            }
            let mut dy = size.y - m.y;
            if dy % 2 != 0 {
                dy += 1;
            }
            Some(Vector2i::new(-dx / 2, dy / 2) - self.tile_offset)
        } else {
            None
        }
    }

    fn handle_tiles(&mut self, tiles: &[tiled_core::Tile]) {
        let ci = self.collections.len() - 1;
        for tile in tiles {
            let tile_id = tile.id.unwrap_or(0);
            let tile_class = tile.class_name().to_string();
            let props = gprops(&tile.properties, self.json);
            let (mut td, src_id, coords) = if let Some(image) = &tile.image {
                let ext = get_extension(image).to_lowercase();
                let texture: Option<Gd<Texture2D>> = if ext == "tmx" || ext == "tmj" {
                    let mut p = PlaceholderTexture2D::new_gd();
                    p.set_size(Vector2::new(
                        tile.imagewidth.unwrap_or(0) as f32,
                        tile.imageheight.unwrap_or(0) as f32,
                    ));
                    Some(p.upcast())
                } else {
                    let base = self.base_dir.clone();
                    let zip = self.params.zip.clone();
                    self.textures.load(image, &base, &zip, &mut self.diag)
                };
                let Some(texture) = texture else {
                    continue;
                };
                let mut src = TileSetAtlasSource::new_gd();
                let requested = atlas_id_property(&props);
                let id = self
                    .ts()
                    .add_source_ex(&src)
                    .atlas_source_id_override(requested)
                    .done();
                self.register(id, 1, tile_id);
                self.collections[ci].insert(tile_id, (src.clone(), id));
                self.atlas = Some((src.clone(), id));
                src.set_texture(&texture);
                src.set_name(get_basename(get_file(image)));
                let w = tile.width.map(|v| v as i32).unwrap_or(texture.get_width());
                let h = tile
                    .height
                    .map(|v| v as i32)
                    .unwrap_or(texture.get_height());
                src.set_texture_region_size(Vector2i::new(w, h));
                src.set_margins(Vector2i::new(
                    tile.x.unwrap_or(0) as i32,
                    tile.y.unwrap_or(0) as i32,
                ));
                // For single image tiles the tileset's tile size is misleading: the texture's
                self.tile_size = Vector2i::new(w, h);
                src.create_tile(Vector2i::ZERO);
                let Some(mut td) = src.get_tile_data(Vector2i::ZERO, 0) else {
                    continue;
                };
                td.set_probability(tile.probability.map(|p| p.as_f64()).unwrap_or(1.0) as f32);
                (td, id, Vector2i::ZERO)
            } else {
                match self.tile_data_for(tile_id) {
                    Some(t) => t,
                    None => continue,
                }
            };

            if let Some(o) = self.origin_for_size(self.tile_size) {
                td.set_texture_origin(o);
            }
            if self.tile_offset != Vector2i::ZERO && td.get_texture_origin() == Vector2i::ZERO {
                let o = td.get_texture_origin() - self.tile_offset;
                td.set_texture_origin(o);
            }
            if let Some(m) = &self.material {
                td.set_material(m);
            }
            if let Some(p) = tile.probability {
                td.set_probability(p.as_f64() as f32);
            }
            if let Some(frames) = &tile.animation {
                self.handle_animation(frames, tile_id);
            }
            if let Some(g) = &tile.objectgroup {
                self.handle_objectgroup(g, &mut td, tile_id, src_id, coords);
            }
            if !tile_class.is_empty() {
                td.set_meta("class", &tile_class.to_variant());
            }
            if let Some(p) = &props {
                self.handle_tile_properties(p, &mut td);
            }
        }
    }

    fn handle_animation(&mut self, frames: &[tiled_core::Frame], tile_id: i64) {
        if frames.is_empty() {
            return;
        }
        let frames: Vec<(i64, f32)> = frames
            .iter()
            .map(|f| {
                let d = match f.duration {
                    Some(d) => d as f64 / 1000.0,
                    None => 1.0,
                };
                (f.tileid.unwrap_or(0), d as f32)
            })
            .collect();
        if self.params.animation_mode == "strip" {
            self.queue_strip(frames, tile_id);
            return;
        }
        let ci = self.collections.len() - 1;
        if self.collections[ci].contains_key(&tile_id) || self.columns <= 0 {
            self.diag.warn_ts(format!(
                " -- Animated tile {tile_id} of an image collection has no atlas to animate in. -> Using a strip"
            ));
            self.queue_strip(frames, tile_id);
            return;
        }
        if frames[0].0 != tile_id {
            self.diag.warn_ts(format!(
                " -- Animated tile {tile_id}: the first frame (tile {}) is not the tile itself. -> Using a strip",
                frames[0].0
            ));
            self.queue_strip(frames, tile_id);
            return;
        }
        let cols = self.columns;
        let coords = Vector2i::new((tile_id % cols) as i32, (tile_id / cols) as i32);
        let max_dx = cols - coords.x as i64;
        let max_dy = self.tile_count / cols - coords.y as i64;
        let mut separation = Vector2i::ZERO;
        let mut anim_columns = 0;
        if frames.len() > 1 {
            let step = frames[1].0 - tile_id;
            let dx = step % cols;
            let dy = step / cols;
            let mut valid = false;
            if dx == 0 && dy > 0 && dy < max_dy {
                separation = Vector2i::new(0, (dy - 1) as i32);
                anim_columns = 1;
                valid = true;
            } else if dy == 0 && dx > 0 && dx < max_dx {
                separation = Vector2i::new((dx - 1) as i32, 0);
                anim_columns = 0;
                valid = true;
            }
            for i in 1..frames.len() {
                if frames[i].0 - frames[i - 1].0 != step {
                    valid = false;
                }
            }
            if !valid {
                self.diag.warn_ts(format!(
                    " -- Animated tile {tile_id}: Succession of tiles not supported in Godot 4 (use animation_mode 'strip'). -> Skipped"
                ));
                return;
            }
        }
        let Some((mut src, _)) = self.atlas.clone() else {
            return;
        };
        let room = src
            .has_room_for_tile_ex(
                coords,
                Vector2i::ONE,
                anim_columns,
                separation,
                frames.len() as i32,
            )
            .ignored_tile(coords)
            .done();
        if !room {
            self.diag.warn_ts(format!(
                " -- TileId {tile_id}: Not enough room for all {} animation frames (use animation_mode 'strip'). -> Skipped",
                frames.len()
            ));
            return;
        }
        src.set_tile_animation_separation(coords, separation);
        src.set_tile_animation_columns(coords, anim_columns);
        src.set_tile_animation_frames_count(coords, frames.len() as i32);
        for (i, f) in frames.iter().enumerate() {
            src.set_tile_animation_frame_duration(coords, i as i32, f.1);
        }
    }

    fn queue_strip(&mut self, frames: Vec<(i64, f32)>, tile_id: i64) {
        let ci = self.collections.len() - 1;
        let atlas = if self.collections[ci].contains_key(&tile_id) {
            None
        } else {
            self.atlas.clone()
        };
        self.pending.push(PendingStrip {
            tile_id,
            frames,
            collection: ci,
            atlas,
            columns: self.columns,
            tile_count: self.tile_count,
            tile_offset: self.tile_offset,
            orientation: self.orientation.clone(),
            alignment: self.alignment.clone(),
            first_gid: self.first_gid,
            material: self.material.clone(),
        });
    }

    /// `get_strip_frame_tile`.
    fn strip_frame_tile(
        &self,
        item: &PendingStrip,
        tile_id: i64,
    ) -> Option<(Gd<TileSetAtlasSource>, i32, Vector2i)> {
        if let Some((s, id)) = self.collections[item.collection].get(&tile_id) {
            return Some((s.clone(), *id, Vector2i::ZERO));
        }
        let (src, id) = item.atlas.clone()?;
        if item.columns <= 0 || tile_id < 0 || tile_id >= item.tile_count {
            return None;
        }
        Some((
            src,
            id,
            Vector2i::new(
                (tile_id % item.columns) as i32,
                (tile_id / item.columns) as i32,
            ),
        ))
    }

    /// `create_animation_strips`.
    fn create_animation_strips(&mut self) {
        if self.tileset.is_none() {
            self.pending.clear();
            return;
        }
        let pending = std::mem::take(&mut self.pending);
        let mut images: HashMap<i64, Option<Gd<Image>>> = HashMap::new();
        for item in &pending {
            let tile_id = item.tile_id;
            let base = self.strip_frame_tile(item, tile_id);
            let Some((base_src, base_id, base_coords)) = base.filter(|(s, _, c)| s.has_tile(*c))
            else {
                self.diag.warn_ts(format!(
                    " -- Animated tile {tile_id} not found. -> Animation skipped"
                ));
                continue;
            };
            let mut regions: Vec<(Gd<Image>, Rect2i)> = Vec::new();
            let mut frame_size = Vector2i::ZERO;
            let mut failed = String::new();
            for &(ftid, _) in &item.frames {
                let Some((fs, _, fc)) = self.strip_frame_tile(item, ftid) else {
                    failed = format!("frame tile {ftid} does not exist");
                    break;
                };
                let Some(img) = rgba_image(fs.get_texture(), &mut images) else {
                    failed = format!("the image of frame tile {ftid} can't be read");
                    break;
                };
                let rs = fs.get_texture_region_size();
                let sep = fs.get_separation();
                let pos =
                    fs.get_margins() + Vector2i::new(fc.x * (rs.x + sep.x), fc.y * (rs.y + sep.y));
                let region = Rect2i::new(pos, rs);
                if frame_size == Vector2i::ZERO {
                    frame_size = rs;
                } else if rs != frame_size {
                    failed = "the frames differ in size".into();
                    break;
                }
                regions.push((img, region));
            }
            if failed.is_empty() && frame_size.x <= 0 {
                failed = "the frames are empty".into();
            }
            if !failed.is_empty() {
                self.diag.warn_ts(format!(
                    " -- Animated tile {tile_id}: {failed}. -> Animation skipped"
                ));
                continue;
            }
            let n = item.frames.len() as i32;
            let Some(mut strip) =
                Image::create(frame_size.x * n, frame_size.y, false, ImageFormat::RGBA8)
            else {
                continue;
            };
            for (i, (img, region)) in regions.iter().enumerate() {
                strip.blit_rect(img, *region, Vector2i::new(i as i32 * frame_size.x, 0));
            }
            let mut strip_src = TileSetAtlasSource::new_gd();
            if let Some(t) = ImageTexture::create_from_image(&strip) {
                strip_src.set_texture(&t);
            }
            strip_src.set_texture_region_size(frame_size);
            let mut base_name = base_src.get_name().to_string();
            if base_name.is_empty() {
                if let Some(t) = base_src.get_texture() {
                    base_name = get_basename(get_file(&t.get_path().to_string())).to_string();
                }
            }
            strip_src.set_name(base_name.as_str());
            let strip_id = self.ts().add_source(&strip_src);
            strip_src.create_tile(Vector2i::ZERO);
            if let (Some(from), Some(mut to)) = (
                base_src.get_tile_data(base_coords, 0),
                strip_src.get_tile_data(Vector2i::ZERO, 0),
            ) {
                copy_tile_data(&from, &mut to);
            }
            strip_src.set_tile_animation_columns(Vector2i::ZERO, 0);
            strip_src.set_tile_animation_frames_count(Vector2i::ZERO, n);
            for (i, f) in item.frames.iter().enumerate() {
                strip_src.set_tile_animation_frame_duration(Vector2i::ZERO, i as i32, f.1);
            }
            if let Some(&g) = self.internal.get(&(base_id, base_coords.x, base_coords.y)) {
                self.internal.insert((strip_id, 0, 0), g);
            }
            self.sources.push(AtlasReg {
                source_id: strip_id,
                num_tiles: 1,
                assigned_id: tile_id,
                tile_offset: item.tile_offset,
                orientation: item.orientation.clone(),
                alignment: item.alignment.clone(),
                first_gid: item.first_gid,
                material: item.material.clone(),
                is_strip: true,
            });
        }
    }

    fn transpose(&self, x: f64, y: f64) -> Vector2 {
        if self.orientation == "isometric" {
            let tx = (x - y) * self.grid_size.x as f64 / self.grid_size.y as f64 / 2.0;
            let ty = (x + y) * 0.5;
            return Vector2::new(tx as f32, ty as f32);
        }
        Vector2::new(x as f32, y as f32)
    }

    fn handle_objectgroup(
        &mut self,
        group: &tiled_core::Layer,
        td: &mut Gd<TileData>,
        tile_id: i64,
        src_id: i32,
        coords: Vector2i,
    ) {
        self.groups_counter += 1;
        let objects = group.objects.clone().unwrap_or_default();
        self.object_groups
            .insert(self.groups_counter, Arc::new(objects.clone()));
        self.internal
            .insert((src_id, coords.x, coords.y), self.groups_counter);

        let mut polygon_indices: HashMap<i64, i32> = HashMap::new();
        for obj in &objects {
            if obj.point == Some(true) {
                continue;
            }
            let ox = obj.x.map(|v| v.as_f64()).unwrap_or(0.0);
            let oy = obj.y.map(|v| v.as_f64()).unwrap_or(0.0);
            let origin = td.get_texture_origin();
            let mut base = self.transpose(ox, oy) - Vector2::new(origin.x as f32, origin.y as f32);
            if self.orientation == "isometric" {
                base.y -= self.grid_size.y as f32 / 2.0;
                if self.grid_size.y != self.tile_size.y {
                    base.y += (self.tile_size.y - self.grid_size.y) as f32 / 2.0;
                }
            } else {
                base -= Vector2::new(self.tile_size.x as f32 / 2.0, self.tile_size.y as f32 / 2.0);
            }
            let rot = obj.rotation.map(|v| v.as_f64()).unwrap_or(0.0);
            let (sin_a, cos_a) = (rot * std::f64::consts::PI / 180.0).sin_cos();
            let rotate = |p: Vector2| -> Vector2 {
                let (x, y) = (p.x as f64, p.y as f64);
                Vector2::new(
                    (x * cos_a - y * sin_a) as f32,
                    (x * sin_a + y * cos_a) as f32,
                )
            };
            let width = obj.width.map(|v| v.as_f64()).unwrap_or(0.0);
            let height = obj.height.map(|v| v.as_f64()).unwrap_or(0.0);
            let polygon: Vec<Vector2> = if obj.polygon.is_some() || obj.polyline.is_some() {
                let pts = obj
                    .polygon
                    .as_ref()
                    .or(obj.polyline.as_ref())
                    .expect("polygon or polyline");
                if pts.len() < 3 {
                    self.diag.warn_ts(format!(
                        " -- Skipped invalid polygon on tile {tile_id} (less than 3 points)"
                    ));
                    break;
                }
                pts.iter()
                    .map(|p| base + rotate(self.transpose(p.x, p.y)))
                    .collect()
            } else if obj.ellipse == Some(true) {
                let cx = width / 2.0;
                let cy = height / 2.0;
                (0..12)
                    .map(|i| {
                        let a = std::f64::consts::TAU * i as f64 / 12.0;
                        let p = (cx + cx * a.cos(), cy + cy * a.sin());
                        base + rotate(self.transpose(p.0, p.1))
                    })
                    .collect()
            } else if obj.capsule == Some(true) {
                capsule_polygon(width, height)
                    .into_iter()
                    .map(|p| base + rotate(self.transpose(p.0, p.1)))
                    .collect()
            } else {
                [(0.0, 0.0), (0.0, height), (width, height), (width, 0.0)]
                    .iter()
                    .map(|&(x, y)| base + rotate(self.transpose(x, y)))
                    .collect()
            };
            let polygon = PackedVector2Array::from(polygon.as_slice());

            let props = gprops(&obj.properties, self.json);
            let (mut nav, mut occ, mut phys) = (-1i64, -1i64, -1i64);
            for p in props.iter().flatten() {
                if p.kind != "int" {
                    continue;
                }
                let v = prop_int(p);
                match p.name.to_lowercase().as_str() {
                    "navigation_layer" if nav < 0 => nav = v,
                    "occlusion_layer" if occ < 0 => occ = v,
                    "physics_layer" if phys < 0 => phys = v,
                    _ => {}
                }
            }
            if nav >= 0 {
                let np = crate::dynobj::new_navigation_polygon(&polygon);
                self.ensure_layer(LayerType::Navigation, nav as i32);
                crate::dynobj::set_tile_navigation_polygon(td, nav as i32, &np);
            }
            if occ >= 0 {
                let mut op = OccluderPolygon2D::new_gd();
                op.set_polygon(&polygon);
                self.ensure_layer(LayerType::Occlusion, occ as i32);
                td.set_occluder_polygons_count(occ as i32, 1);
                td.set_occluder_polygon(occ as i32, 0, &op);
            }
            // No layer property: collision (the default)
            if phys < 0 && nav < 0 && occ < 0 {
                phys = 0;
            }
            if phys < 0 {
                continue;
            }
            let index = *polygon_indices.get(&phys).unwrap_or(&0);
            polygon_indices.insert(phys, index + 1);
            self.ensure_layer(LayerType::Physics, phys as i32);
            td.add_collision_polygon(phys as i32);
            td.set_collision_polygon_points(phys as i32, index, &polygon);
            for p in props.iter().flatten() {
                if p.name.is_empty() {
                    continue;
                }
                let lname = p.name.to_lowercase();
                if lname == "one_way" && p.kind == "bool" {
                    td.set_collision_polygon_one_way(
                        phys as i32,
                        index,
                        p.val().to_lowercase() == "true",
                    );
                } else if lname == "one_way_margin" && p.kind == "int" {
                    td.set_collision_polygon_one_way_margin(
                        phys as i32,
                        index,
                        to_int(&p.val()) as f32,
                    );
                }
            }
        }
    }

    fn ensure_layer(&mut self, t: LayerType, layer: i32) {
        let mut ts = self.ts().clone();
        match t {
            LayerType::Physics => {
                while self.physics_layers < layer {
                    ts.add_physics_layer();
                    self.physics_layers += 1;
                }
            }
            LayerType::Navigation => {
                while self.navigation_layers < layer {
                    ts.add_navigation_layer();
                    self.navigation_layers += 1;
                }
            }
            LayerType::Occlusion => {
                while self.occlusion_layers < layer {
                    ts.add_occlusion_layer();
                    self.occlusion_layers += 1;
                }
            }
        }
    }

    /// A property's value as `handle_tile_properties` reads it: the list, the class
    /// dictionary, or `str(value)`.
    fn split_value(p: &GProp) -> (Variant, Variant, String) {
        match p.kind.as_str() {
            "list" => {
                let mut arr = VarArray::new();
                if let Some(Value::Array(items)) = p.value.as_ref().and_then(PV::core) {
                    for it in items {
                        let kind = it.get("type").map(crate::gd::gd_str).unwrap_or_default();
                        let val = it.get("value").map(crate::gd::gd_str).unwrap_or_default();
                        arr.push(&typed_value(&kind, &val));
                    }
                }
                (arr.to_variant(), Variant::nil(), String::new())
            }
            "class" => {
                let d = match &p.value {
                    Some(v) if v.is_dict() => v.to_variant(),
                    _ => VarDictionary::new().to_variant(),
                };
                (Variant::nil(), d, String::new())
            }
            _ => (Variant::nil(), Variant::nil(), p.val()),
        }
    }

    fn handle_tile_properties(&mut self, props: &[GProp], td: &mut Gd<TileData>) {
        for p in props {
            let (list_val, class_val, val) = Self::split_value(p);
            if p.name.is_empty() {
                continue;
            }
            let lname = p.name.to_lowercase();
            let ty = p.kind.as_str();
            let num = ty == "int" || ty == "float";
            if lname == "texture_origin_x" && ty == "int" {
                let o = td.get_texture_origin();
                td.set_texture_origin(Vector2i::new(to_int(&val) as i32, o.y));
            } else if lname == "texture_origin_y" && ty == "int" {
                let o = td.get_texture_origin();
                td.set_texture_origin(Vector2i::new(o.x, to_int(&val) as i32));
            } else if lname == "modulate" && ty == "color" {
                td.set_modulate(crate::gd::tiled_color(&val, Color::WHITE));
            } else if lname == "modulate" && ty == "string" {
                td.set_modulate(crate::gd::color_from_string(&val));
            } else if lname == "material" && ty == "file" {
                let base = self.base_dir.clone();
                let m = load_resource(&val, &base, &mut self.diag)
                    .and_then(|r| r.try_cast::<godot::classes::Material>().ok());
                td.set_material(m.as_ref());
            } else if lname == "z_index" && ty == "int" {
                td.set_z_index(to_int(&val) as i32);
            } else if lname == "y_sort_origin" && ty == "int" {
                td.set_y_sort_origin(to_int(&val) as i32);
            } else if lname == "linear_velocity_x" && num {
                self.ensure_layer(LayerType::Physics, 0);
                let mut v = td.get_constant_linear_velocity(0);
                v.x = crate::gd::to_float(&val) as f32;
                td.set_constant_linear_velocity(0, v);
            } else if lname.starts_with("linear_velocity_x_") && num {
                let Some(i) = layer_suffix(&p.name, 18) else {
                    continue;
                };
                self.ensure_layer(LayerType::Physics, i);
                let mut v = td.get_constant_linear_velocity(i);
                v.x = crate::gd::to_float(&val) as f32;
                td.set_constant_linear_velocity(i, v);
            } else if lname == "linear_velocity_y" && num {
                self.ensure_layer(LayerType::Physics, 0);
                let mut v = td.get_constant_linear_velocity(0);
                v.y = crate::gd::to_float(&val) as f32;
                td.set_constant_linear_velocity(0, v);
            } else if lname.starts_with("linear_velocity_y_") && num {
                let Some(i) = layer_suffix(&p.name, 18) else {
                    continue;
                };
                self.ensure_layer(LayerType::Physics, i);
                let mut v = td.get_constant_linear_velocity(i);
                v.y = crate::gd::to_float(&val) as f32;
                td.set_constant_linear_velocity(i, v);
            } else if lname == "angular_velocity" && num {
                self.ensure_layer(LayerType::Physics, 0);
                td.set_constant_angular_velocity(0, crate::gd::to_float(&val) as f32);
            } else if lname.starts_with("angular_velocity_") && num {
                let Some(i) = layer_suffix(&p.name, 17) else {
                    continue;
                };
                self.ensure_layer(LayerType::Physics, i);
                td.set_constant_angular_velocity(i, crate::gd::to_float(&val) as f32);
            } else if lname != "godot_atlas_id" {
                // Prefixed properties become custom data (prefix stripped), the others metadata;
                // an empty prefix makes every property both.
                let prefix = self.params.custom_data_prefix.clone();
                let is_custom = prefix.is_empty() || lname.starts_with(&prefix);
                if is_custom {
                    let data_name: String = p.name.chars().skip(prefix.chars().count()).collect();
                    let mut ts = self.ts().clone();
                    let mut layer = ts.get_custom_data_layer_by_name(data_name.as_str());
                    if layer < 0 {
                        ts.add_custom_data_layer();
                        layer = ts.get_custom_data_layers_count() - 1;
                        ts.set_custom_data_layer_name(layer, data_name.as_str());
                        ts.set_custom_data_layer_type(layer, custom_data_type(ty));
                    }
                    if ty == "class" {
                        td.set_custom_data(data_name.as_str(), &class_val);
                    } else {
                        td.set_custom_data(data_name.as_str(), &typed_value(ty, &val));
                    }
                }
                if !is_custom || prefix.is_empty() {
                    let v = match ty {
                        "list" => list_val,
                        "class" => class_val,
                        _ => typed_value(ty, &val),
                    };
                    td.set_meta(p.name.as_str(), &v);
                }
            }
        }
    }

    fn handle_tileset_properties(&mut self, props: &[GProp]) {
        for p in props {
            let (list_val, class_val, val) = Self::split_value(p);
            if p.name.is_empty() {
                continue;
            }
            let lname = p.name.to_lowercase();
            let ty = p.kind.as_str();
            let mut ts = self.ts().clone();
            if lname == "collision_layer" && ty == "string" {
                self.ensure_layer(LayerType::Physics, 0);
                ts.set_physics_layer_collision_layer(0, bitmask(&val, 32) as u32);
            } else if lname.starts_with("collision_layer_") && ty == "string" {
                let Some(i) = layer_suffix(&p.name, 16) else {
                    continue;
                };
                self.ensure_layer(LayerType::Physics, i);
                ts.set_physics_layer_collision_layer(i, bitmask(&val, 32) as u32);
            } else if lname == "collision_mask" && ty == "string" {
                self.ensure_layer(LayerType::Physics, 0);
                ts.set_physics_layer_collision_mask(0, bitmask(&val, 32) as u32);
            } else if lname.starts_with("collision_mask_") && ty == "string" {
                let Some(i) = layer_suffix(&p.name, 15) else {
                    continue;
                };
                self.ensure_layer(LayerType::Physics, i);
                ts.set_physics_layer_collision_mask(i, bitmask(&val, 32) as u32);
            } else if lname == "layers" && ty == "string" {
                self.ensure_layer(LayerType::Navigation, 0);
                ts.set_navigation_layer_layers(0, bitmask(&val, 32) as u32);
            } else if lname.starts_with("layers_") && ty == "string" {
                let Some(i) = layer_suffix(&p.name, 7) else {
                    continue;
                };
                self.ensure_layer(LayerType::Navigation, i);
                ts.set_navigation_layer_layers(i, bitmask(&val, 32) as u32);
            } else if lname == "light_mask" && ty == "string" {
                self.ensure_layer(LayerType::Occlusion, 0);
                ts.set_occlusion_layer_light_mask(0, bitmask(&val, 20) as i32);
            } else if lname.starts_with("light_mask_") && ty == "string" {
                let Some(i) = layer_suffix(&p.name, 11) else {
                    continue;
                };
                self.ensure_layer(LayerType::Occlusion, i);
                ts.set_occlusion_layer_light_mask(i, bitmask(&val, 20) as i32);
            } else if lname == "sdf_collision_" && ty == "bool" {
                self.ensure_layer(LayerType::Occlusion, 0);
                ts.set_occlusion_layer_sdf_collision(0, val.to_lowercase() == "true");
            } else if lname.starts_with("sdf_collision_") && ty == "bool" {
                let Some(i) = layer_suffix(&p.name, 14) else {
                    continue;
                };
                self.ensure_layer(LayerType::Occlusion, i);
                ts.set_occlusion_layer_sdf_collision(i, val.to_lowercase() == "true");
            } else if lname == "uv_clipping" && ty == "bool" {
                ts.set_uv_clipping(val.to_lowercase() == "true");
            } else if lname == "resource_local_to_scene" && ty == "bool" {
                ts.set_local_to_scene(val.to_lowercase() == "true");
            } else if lname == "resource_name" && ty == "string" {
                ts.set_name(val.as_str());
            } else if lname != "godot_atlas_id" {
                let v = match ty {
                    "list" => list_val,
                    "class" => class_val,
                    _ => typed_value(ty, &val),
                };
                ts.set_meta(p.name.as_str(), &v);
            }
        }
    }

    fn handle_wangsets_old_mapping(&mut self, wangsets: &[tiled_core::WangSet]) {
        let mut ts = self.ts().clone();
        ts.add_terrain_set();
        self.terrain_sets += 1;
        for w in wangsets {
            let set = self.terrain_sets;
            ts.add_terrain(set);
            let terrain = self.terrains;
            if let Some(n) = &w.name {
                ts.set_terrain_name(set, self.terrains, n.as_str());
            }
            ts.set_terrain_set_mode(set, terrain_mode(w.kind.as_deref()));
            if let Some(c) = w.colors.as_ref().and_then(|c| c.first()) {
                ts.set_terrain_color(
                    set,
                    self.terrains,
                    crate::gd::tiled_color(c.color.as_deref().unwrap_or(""), Color::WHITE),
                );
            }
            for wt in w.wangtiles.iter().flatten() {
                let Some((mut td, _, _)) = self.tile_data_for(wt.tileid.unwrap_or(0)) else {
                    break;
                };
                if let Some(o) = self.origin_for_size(self.tile_size) {
                    td.set_texture_origin(o);
                }
                td.set_terrain_set(set);
                td.set_terrain(terrain);
                for (i, &wi) in wt.wangid.iter().flatten().enumerate() {
                    let bit = PEERING_BITS
                        .get(i)
                        .copied()
                        .unwrap_or(CellNeighbor::TOP_SIDE);
                    if wi > 0 {
                        td.set_terrain_peering_bit(bit, terrain);
                    }
                }
            }
            self.terrains += 1;
        }
    }

    fn handle_wangsets(&mut self, wangsets: &[tiled_core::WangSet]) {
        let mut ts = self.ts().clone();
        for w in wangsets {
            ts.add_terrain_set();
            self.terrain_sets += 1;
            self.terrains = -1;
            let set = self.terrain_sets;
            let set_name = w.name.clone().unwrap_or_default();
            ts.set_terrain_set_mode(set, terrain_mode(w.kind.as_deref()));
            for c in w.colors.iter().flatten() {
                self.terrains += 1;
                ts.add_terrain(set);
                ts.set_terrain_color(
                    set,
                    self.terrains,
                    crate::gd::tiled_color(c.color.as_deref().unwrap_or(""), Color::WHITE),
                );
                let mut name = set_name.clone();
                if let Some(n) = c.name.as_deref().filter(|n| !n.is_empty()) {
                    name = n.to_string();
                }
                ts.set_terrain_name(set, self.terrains, name.as_str());
            }
            for wt in w.wangtiles.iter().flatten() {
                let Some((mut td, _, _)) = self.tile_data_for(wt.tileid.unwrap_or(0)) else {
                    break;
                };
                if let Some(o) = self.origin_for_size(self.tile_size) {
                    td.set_texture_origin(o);
                }
                td.set_terrain_set(set);
                for (i, &wi) in wt.wangid.iter().flatten().enumerate() {
                    let bit = PEERING_BITS
                        .get(i)
                        .copied()
                        .unwrap_or(CellNeighbor::TOP_SIDE);
                    if wi > 0 {
                        td.set_terrain((wi - 1) as i32);
                        td.set_terrain_peering_bit(bit, (wi - 1) as i32);
                    }
                }
            }
        }
    }
}

/// The capsule outline of `handle_objectgroup` (12 segments, stretched along the long axis).
fn capsule_polygon(width: f64, height: f64) -> Vec<(f64, f64)> {
    let r = if width > height {
        height / 2.0
    } else {
        width / 2.0
    };
    const SEG: usize = 12;
    let mut p: Vec<(f64, f64)> = (0..SEG)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / SEG as f64;
            (width / 2.0 + r * a.cos(), height / 2.0 + r * a.sin())
        })
        .collect();
    if (width - height).abs() > 0.01 {
        p.push((0.0, 0.0));
        p.push((0.0, 0.0));
    }
    if height > width && p.len() > SEG {
        let y = (height - width) / 2.0;
        p[SEG + 1] = (p[0].0, p[0].1 - y);
        for i in (SEG / 2 + 1..=SEG).rev() {
            p[i] = (p[i - 1].0, p[i - 1].1 - y);
        }
        for q in p.iter_mut().take(SEG / 2 + 1) {
            q.1 += y;
        }
    }
    if width > height && p.len() > SEG {
        let x = (width - height) / 2.0;
        for i in (SEG * 3 / 4 + 2..=SEG + 1).rev() {
            p[i] = (p[i - 2].0 + x, p[i - 2].1);
        }
        for i in (SEG / 4 + 1..=SEG * 3 / 4 + 1).rev() {
            p[i] = (p[i - 1].0 - x, p[i - 1].1);
        }
        for q in p.iter_mut().take(SEG / 4 + 1) {
            q.0 += x;
        }
    }
    p
}

/// `name.substr(n)` as a layer index, when it is a valid int.
fn layer_suffix(name: &str, n: usize) -> Option<i32> {
    let s: String = name.chars().skip(n).collect();
    tiled_core::godot_str::is_valid_int(&s).then(|| to_int(&s) as i32)
}

/// `int(property.get("value", 0))`.
fn prop_int(p: &GProp) -> i64 {
    match p.value.as_ref().and_then(PV::core) {
        Some(Value::Int(i)) => *i,
        Some(Value::Float(f)) => *f as i64,
        Some(Value::Bool(b)) => i64::from(*b),
        Some(Value::String(s)) => to_int(s),
        _ => 0,
    }
}

/// `get_special_property(dict, "godot_atlas_id")`: the int property's value, -1 for none.
fn atlas_id_property(props: &Option<Vec<GProp>>) -> i32 {
    for p in props.iter().flatten() {
        if p.name.is_empty() {
            continue;
        }
        if p.name.to_lowercase() == "godot_atlas_id" && p.kind == "int" {
            return prop_int(p) as i32;
        }
    }
    -1
}

/// The shader that makes a tileset's transparent colour transparent.
fn transparent_material(color: &str) -> Gd<ShaderMaterial> {
    let c = crate::gd::color_from_string(color);
    let f = |v: f32| format!("{v:.6}").chars().take(4).collect::<String>();
    let code = format!(
        "shader_type canvas_item;\nconst vec3 transparent = vec3({}, {}, {});\n\nvoid fragment() {{\n\tvec4 col = texture(TEXTURE, UV);\n\tif (col.r >= transparent.r && col.r <= (transparent.r + 0.01) &&\n\t    col.g >= transparent.g && col.g <= (transparent.g + 0.01) &&\n\t\tcol.b >= transparent.b && col.b <= (transparent.b + 0.01))\n\t\tcol.a=0.0;\n\tCOLOR=col;\n}}",
        f(c.r),
        f(c.g),
        f(c.b)
    );
    let mut shader = Shader::new_gd();
    shader.set_code(code.as_str());
    let mut m = ShaderMaterial::new_gd();
    m.set_shader(&shader);
    m
}

/// `get_rgba_image`: a texture's image, decompressed and in RGBA8, once per texture.
fn rgba_image(
    texture: Option<Gd<Texture2D>>,
    cache: &mut HashMap<i64, Option<Gd<Image>>>,
) -> Option<Gd<Image>> {
    let texture = texture?;
    let key = texture.instance_id().to_i64();
    if let Some(i) = cache.get(&key) {
        return i.clone();
    }
    let img = texture.get_image().map(|mut img| {
        if img.is_compressed() {
            img.decompress();
        }
        if img.get_format() != ImageFormat::RGBA8 {
            img.convert(ImageFormat::RGBA8);
        }
        img
    });
    cache.insert(key, img.clone());
    img
}

/// `copy_tile_data`: every stored property of a TileData (collision, navigation, occlusion,
/// custom data, terrains, origin, material, ...) and its metadata onto another.
pub fn copy_tile_data(from: &Gd<TileData>, to: &mut Gd<TileData>) {
    for prop in from.get_property_list().iter_shared() {
        let usage = prop.get("usage").map(|u| u.to::<i64>()).unwrap_or(0);
        if usage & 2 == 0 {
            continue;
        }
        let name = prop
            .get("name")
            .map(|n| n.to::<GString>().to_string())
            .unwrap_or_default();
        if name == "script" || name.starts_with("metadata/") {
            continue;
        }
        let v = from.get(name.as_str());
        to.set(name.as_str(), &v);
    }
    for m in from.get_meta_list().iter_shared() {
        let v = from.get_meta(&m);
        to.set_meta(&m, &v);
    }
}

/// The slots of a map's tileset list.
fn map_slots(map: &Map) -> Vec<SlotRef<'_>> {
    map.tileset_slots()
        .iter()
        .map(|s| slot_ref(s, map.slot_tileset(s)))
        .collect()
}

fn slot_ref<'a>(s: &'a TilesetSlot, ts: Option<&'a Tileset>) -> SlotRef<'a> {
    SlotRef {
        firstgid: s.firstgid,
        tileset: ts,
        skipped: matches!(s.source, TilesetSource::Skipped),
        base_dir: s.base_dir.clone(),
    }
}

/// A stable key of a slot list (paths of external tilesets, a content hash of embedded ones).
fn slots_key(slots: &[&TilesetSlot], embedded: &[Tileset]) -> String {
    let mut key = String::new();
    for s in slots {
        match &s.source {
            TilesetSource::External { path, tileset } => key.push_str(&format!(
                "{}:{}{};",
                s.firstgid,
                path,
                if tileset.is_some() { "" } else { "?" }
            )),
            TilesetSource::Embedded(i) => {
                let mut h = DefaultHasher::new();
                format!("{:?}", embedded.get(*i)).hash(&mut h);
                key.push_str(&format!("{}:#{:x}@{};", s.firstgid, h.finish(), s.base_dir));
            }
            TilesetSource::Skipped => key.push_str(&format!("{}:-;", s.firstgid)),
        }
    }
    key
}

/// What a TileSet is built from: the map's tileset list, or a template's.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Source {
    Map,
    Template(String),
}

impl Source {
    fn slots<'m>(&self, map: &'m Map) -> Vec<SlotRef<'m>> {
        match self {
            Source::Map => map_slots(map),
            Source::Template(p) => map
                .templates()
                .get(p)
                .map(|t| {
                    t.tileset_slots()
                        .iter()
                        .map(|s| slot_ref(s, t.slot_tileset(s)))
                        .collect()
                })
                .unwrap_or_default(),
        }
    }
}

/// A TileSet being created, possibly over several `build_step` calls. It is continued with
/// whichever map asks next for the same key (the key names the same tileset list), so it keeps
/// no map of its own.
struct Creation {
    creator: Creator,
}

impl Creation {
    /// Works on the TileSet of `source` in `map` until `budget` is spent; true when complete.
    fn step(&mut self, map: &Map, source: &Source, budget: &mut usize) -> bool {
        let slots = source.slots(map);
        self.creator.step(&slots, budget)
    }

    /// The finished TileSet (a map's with its tile shape and layout) and its messages.
    fn finish(self, map: &Map, source: &Source) -> (Built, Diag) {
        let tile_size = self.creator.params.map_tile_size;
        let (built, diag) = self.creator.finish();
        if *source == Source::Map {
            shape_for_map(&mut built.tileset.clone(), map, tile_size);
        }
        (built, diag)
    }
}

/// The map's tile size, shape, layout and offset axis on its TileSet.
fn shape_for_map(ts: &mut Gd<TileSet>, map: &Map, tile_size: Vector2i) {
    ts.set_tile_size(tile_size);
    let stagger_x = map.staggeraxis.as_deref().unwrap_or("y") == "x";
    let odd = map.staggerindex.as_deref().unwrap_or("odd") == "odd";
    let layout = if odd {
        TileLayout::STACKED
    } else {
        TileLayout::STACKED_OFFSET
    };
    let axis = if stagger_x {
        TileOffsetAxis::VERTICAL
    } else {
        TileOffsetAxis::HORIZONTAL
    };
    match map.orientation.as_deref() {
        Some("isometric") => {
            ts.set_tile_shape(TileShape::ISOMETRIC);
            ts.set_tile_layout(TileLayout::DIAMOND_DOWN);
        }
        Some("staggered") => {
            ts.set_tile_shape(TileShape::ISOMETRIC);
            ts.set_tile_layout(layout);
            ts.set_tile_offset_axis(axis);
        }
        Some("hexagonal") => {
            ts.set_tile_shape(TileShape::HEXAGON);
            ts.set_tile_layout(layout);
            ts.set_tile_offset_axis(axis);
        }
        _ => {}
    }
}

/// A cached TileSet and the messages its creation printed (reported again by every import
/// that uses it, so an import's counts do not depend on what the cache holds).
struct Entry {
    built: Rc<Built>,
    diag: Diag,
}

impl Entry {
    /// Whether anything besides the cache holds the TileSet (a layer of a live tree, a build in
    /// progress, a script's variable).
    fn in_use(&self) -> bool {
        Rc::strong_count(&self.built) > 1 || self.built.tileset.get_reference_count() > 1
    }
}

/// The TileSet cache: one TileSet per tileset combination (the ordered tilesets with their
/// first gids, the map's tile size, orientation and stagger, the zip archive, and the options
/// that shape a TileSet: `custom_data_prefix`, `map_wangset_to_terrain`, `animation_mode`,
/// `no_alternative_tiles`; and the project file's content, whose custom types and enums go into
/// the tilesets),
/// reused **by reference** by every later map with the same combination; templates' TileSets
/// alike. A cached TileSet is what the maps that use it make of it: the builder adds the tiles
/// and alternative tiles they need, and a change a game makes to it shows in every map that
/// shares it. `clear()` forgets them all. With `exclusive` on, a TileSet is handed out only
/// while no tree holds it, so two live trees never share one (a second one is built for the
/// key meanwhile); a map loaded after the previous tree was freed still reuses it.
///
/// `build_step(map, options, tiles_per_step)` spreads the creation of a map's TileSets over
/// calls (frames); `tileset_for` and `TiledBuilder.build` finish whatever is left. Textures are
/// cached per process (`clear_textures()`).
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct TiledTileSets {
    base: Base<RefCounted>,
    cache: HashMap<String, Vec<Entry>>,
    creating: HashMap<String, Creation>,
    pub(crate) textures: Textures,
    /// Hand a cached TileSet only to a map no live tree shares it with (default false: shared
    /// by every map with the same tilesets, alive or not).
    #[var]
    exclusive: bool,
    created: i64,
    last_errors: u32,
    last_warnings: u32,
}

#[godot_api]
impl TiledTileSets {
    /// The TileSet of a loaded map (built on first use, cached afterwards); `null` when the map
    /// is not loaded. Its messages are printed and counted (`get_last_error_count`) on every
    /// call, cached or not.
    #[func]
    fn tileset_for(
        &mut self,
        map: Gd<TiledMap>,
        options: Option<Gd<TiledImportOptions>>,
    ) -> Option<Gd<TileSet>> {
        let (inner, zip) = {
            let m = map.bind();
            (m.inner(), m.zip_file.clone())
        };
        let inner = inner?;
        let opts = options_of(options);
        let mut diag = Diag::default();
        let built = self.built_for_map(&inner, &zip, &opts, &mut diag);
        self.last_errors = diag.errors;
        self.last_warnings = diag.warnings;
        Some(built.tileset.clone())
    }

    /// Works on the TileSets a loaded map needs (its own and its templates') for at most
    /// `tiles_per_step` tiles (at least one), so a large TileSet is created over several frames.
    /// True when every one is ready (cached), at once when they already were; then `build` or
    /// `tileset_for` takes them from the cache. Messages are printed when the map's build uses
    /// them, not here.
    #[func]
    fn build_step(
        &mut self,
        map: Gd<TiledMap>,
        options: Option<Gd<TiledImportOptions>>,
        tiles_per_step: i64,
    ) -> bool {
        let (inner, zip) = {
            let m = map.bind();
            (m.inner(), m.zip_file.clone())
        };
        let Some(inner) = inner else { return true };
        let opts = options_of(options);
        let mut budget = tiles_per_step.max(1) as usize;
        let mut targets = vec![Source::Map];
        for (p, t) in inner.templates() {
            if !t.tilesets.is_empty() {
                targets.push(Source::Template(p.clone()));
            }
        }
        for source in targets {
            let Some(key) = Self::key_for(&inner, &source, &zip, &opts) else {
                continue;
            };
            if self.available(&key).is_some() {
                continue;
            }
            let creation = self
                .creating
                .entry(key.clone())
                .or_insert_with(|| Self::start(&inner, &source, &zip, &opts));
            if !creation.step(&inner, &source, &mut budget) {
                return false;
            }
            let c = self.creating.remove(&key).expect("in creation");
            self.store(key, c, &inner, &source);
        }
        true
    }

    /// Forgets every cached TileSet (and those half created). The TileSets stay alive while
    /// trees use them; later maps get new ones.
    #[func]
    fn clear(&mut self) {
        self.cache.clear();
        self.creating.clear();
        self.textures.clear();
    }

    /// Forgets the per-process texture cache (every `TiledTileSets`' textures): later imports
    /// load their images again, from files that may have changed.
    #[func]
    fn clear_textures(&mut self) {
        clear_texture_cache();
    }

    /// The number of cached TileSets (maps' and templates').
    #[func]
    fn get_cached_count(&self) -> i64 {
        self.cache.values().map(|v| v.len() as i64).sum()
    }

    /// The number of TileSets this cache has created so far (maps' and templates').
    #[func]
    fn get_created_count(&self) -> i64 {
        self.created
    }

    /// The number of textures in the per-process cache.
    #[func]
    fn get_texture_count(&self) -> i64 {
        texture_cache_len() as i64
    }

    /// Errors of the last `tileset_for`.
    #[func]
    fn get_last_error_count(&self) -> i64 {
        self.last_errors as i64
    }

    /// Warnings of the last `tileset_for`.
    #[func]
    fn get_last_warning_count(&self) -> i64 {
        self.last_warnings as i64
    }
}

impl TiledTileSets {
    fn params(map: &Map, zip: &str, opts: &Options, template: bool) -> BuildParams {
        let mode = opts.animation_mode.to_lowercase();
        let valid = mode == "strip" || mode == "atlas";
        BuildParams {
            map_tile_size: Vector2i::new(
                map.tilewidth.unwrap_or(0) as i32,
                map.tileheight.unwrap_or(0) as i32,
            ),
            map_wangset_to_terrain: opts.map_wangset_to_terrain,
            // A template's TilesetCreator is given no prefix: every tile property is both
            custom_data_prefix: if template {
                String::new()
            } else {
                opts.custom_data_prefix.clone()
            },
            animation_mode: if valid { mode } else { "strip".into() },
            report_animation_mode: !template && !valid,
            zip: zip.to_string(),
        }
    }

    fn key(prefix: &str, map: &Map, params: &BuildParams, slots: &str) -> String {
        format!(
            "{prefix}|{}|{}|{}|{}|{}x{}|{}|{}|{}|{}",
            params.zip,
            map.orientation.as_deref().unwrap_or(""),
            map.staggeraxis.as_deref().unwrap_or(""),
            map.staggerindex.as_deref().unwrap_or(""),
            params.map_tile_size.x,
            params.map_tile_size.y,
            params.map_wangset_to_terrain,
            params.custom_data_prefix,
            params.animation_mode,
            slots
        )
    }

    /// The cache key of a map's TileSet or a template's; `None` for a template without
    /// tilesets (it has no TileSet).
    fn key_for(map: &Map, source: &Source, zip: &str, opts: &Options) -> Option<String> {
        // The builder adds alternative tiles to a TileSet when `no_alternative_tiles` is off,
        // so a TileSet shared across imports differs by it too.
        // The project file's custom types are merged into the tilesets (and its enums shape
        // their properties), so maps loaded with different projects differ too: keyed by a hash
        // of the project's content.
        let key = Self::key_inner(map, source, zip, opts)?;
        let project = map.project().map_or(0, |p| {
            let mut h = DefaultHasher::new();
            // The types only: the project's lookup index is a HashMap (no stable order).
            format!("{:?}", p.property_types).hash(&mut h);
            h.finish()
        });
        Some(format!(
            "{key}|alt={}|project={project:x}",
            !opts.no_alternative_tiles
        ))
    }

    fn key_inner(map: &Map, source: &Source, zip: &str, opts: &Options) -> Option<String> {
        match source {
            Source::Map => {
                let params = Self::params(map, zip, opts, false);
                let slots: Vec<&TilesetSlot> = map.tileset_slots().iter().collect();
                Some(Self::key(
                    "map",
                    map,
                    &params,
                    &slots_key(&slots, map.tilesets.as_deref().unwrap_or(&[])),
                ))
            }
            Source::Template(p) => {
                let t = map.templates().get(p)?;
                if t.tilesets.is_empty() {
                    return None;
                }
                let params = Self::params(map, zip, opts, true);
                let slots: Vec<&TilesetSlot> = t.tileset_slots().iter().collect();
                Some(Self::key(
                    "template",
                    map,
                    &params,
                    &slots_key(&slots, &t.tilesets),
                ))
            }
        }
    }

    /// A cached entry of `key` that may be handed out (any, or one no tree holds when
    /// `exclusive`).
    fn available(&self, key: &str) -> Option<&Entry> {
        let entries = self.cache.get(key)?;
        entries.iter().find(|e| !self.exclusive || !e.in_use())
    }

    /// A new creation of a map's or a template's TileSet.
    fn start(map: &Map, source: &Source, zip: &str, opts: &Options) -> Creation {
        let template = *source != Source::Map;
        let params = Self::params(map, zip, opts, template);
        let mut diag = Diag::quiet();
        if params.report_animation_mode && map.tilesets.is_some() {
            diag.warn_ts(format!(
                " -- Option animation_mode '{}' is not supported. -> Using 'strip'",
                opts.animation_mode
            ));
        }
        Creation {
            creator: Creator::new(params, diag),
        }
    }

    /// Caches a finished creation.
    fn store(&mut self, key: String, creation: Creation, map: &Map, source: &Source) -> Rc<Built> {
        let (built, diag) = creation.finish(map, source);
        let built = Rc::new(built);
        self.created += 1;
        self.cache.entry(key).or_default().push(Entry {
            built: built.clone(),
            diag,
        });
        built
    }

    /// The TileSet of `key`: a cached one, else the creation `build_step` started, finished,
    /// else a new one, created now. Its messages are reported into `diag` either way.
    fn get_or_create(
        &mut self,
        key: String,
        map: &Map,
        source: Source,
        zip: &str,
        opts: &Options,
        diag: &mut Diag,
    ) -> Rc<Built> {
        if let Some(e) = self.available(&key) {
            diag.replay(&e.diag);
            return e.built.clone();
        }
        let mut creation = match self.creating.remove(&key) {
            Some(c) => c,
            None => Self::start(map, &source, zip, opts),
        };
        let mut unbounded = usize::MAX;
        creation.step(map, &source, &mut unbounded);
        let built = self.store(key.clone(), creation, map, &source);
        if let Some(e) = self.cache.get(&key).and_then(|v| v.last()) {
            diag.replay(&e.diag);
        }
        built
    }

    /// The map's TileSet and registry, built on first use; its messages reported into `diag`.
    pub(crate) fn built_for_map(
        &mut self,
        map: &Map,
        zip: &str,
        opts: &Options,
        diag: &mut Diag,
    ) -> Rc<Built> {
        let key = Self::key_for(map, &Source::Map, zip, opts).expect("a map has a key");
        self.get_or_create(key, map, Source::Map, zip, opts, diag)
    }

    /// A template's TileSet (`load_template`'s own TilesetCreator), `None` when the template has
    /// no tilesets.
    pub(crate) fn built_for_template(
        &mut self,
        map: &Map,
        template: &tiled_core::Template,
        zip: &str,
        opts: &Options,
        diag: &mut Diag,
    ) -> Option<Rc<Built>> {
        if template.tilesets.is_empty() {
            return None;
        }
        let source = Source::Template(template.path().to_string());
        let key = Self::key_for(map, &source, zip, opts)?;
        Some(self.get_or_create(key, map, source, zip, opts, diag))
    }
}
