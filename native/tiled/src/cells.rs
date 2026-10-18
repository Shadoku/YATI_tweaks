//! Cells: gid resolution and the byte blobs of Godot's `TileMapLayer.tile_map_data`.
//!
//! [`CellResolver`] maps a gid to its tileset, local tile id, atlas coordinates and flips the
//! way the GDScript runtime's `TilemapCreator.resolve_cell` does, over the atlas sources its
//! `TilesetCreator` registers: one per atlas tileset, one per image collection tile, numbered
//! in creation order (a `godot_atlas_id` int property on the tileset or tile picks the id, as
//! `TileSet.add_source` takes it). The atlas geometry comes from the files (`imagewidth`,
//! `imageheight`, `margin`, `spacing`); [`CellResolver::set_texture_size`] puts the loaded
//! texture's size in instead, which is what the runtime measures.
//!
//! [`tile_map_data`] writes cells in the engine's format: a `u16` format version (0), then per
//! cell six little-endian `i16`: x, y, source id, atlas x, atlas y, alternative id.

use crate::data::{FLIPPED_DIAGONALLY, FLIPPED_HORIZONTALLY, FLIPPED_VERTICALLY, GID_MASK};
use crate::model::{Layer, Map, PropertyValue, Tileset, TilesetSource};
use crate::Error;

/// `TileSetAtlasSource.TRANSFORM_FLIP_H`.
pub const TRANSFORM_FLIP_H: u16 = 1 << 12;
/// `TileSetAtlasSource.TRANSFORM_FLIP_V`.
pub const TRANSFORM_FLIP_V: u16 = 1 << 13;
/// `TileSetAtlasSource.TRANSFORM_TRANSPOSE`.
pub const TRANSFORM_TRANSPOSE: u16 = 1 << 14;

/// One atlas source as the GDScript runtime registers it.
#[derive(Clone, Debug, PartialEq)]
pub struct AtlasSource {
    /// The Godot source id (-1 when `TileSet.add_source` would refuse it: a duplicate
    /// `godot_atlas_id`).
    pub source_id: i32,
    /// The index of the tileset in the map's tileset list.
    pub slot: usize,
    /// The tileset's first gid.
    pub firstgid: u32,
    /// For an image collection tile: its local id (the source holds that tile only).
    pub tile: Option<u32>,
    /// The number of tiles the source covers (`numTiles`).
    pub num_tiles: u32,
    /// Width of the source's atlas grid, in tiles.
    pub grid_width: u32,
    /// True when the atlas grid is a single tile (coordinates are then always 0, 0).
    pub single: bool,
}

/// A resolved cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedCell {
    /// The Godot source id.
    pub source_id: i32,
    /// The index of the tileset in the map's tileset list.
    pub tileset: usize,
    /// The tile's local id in its tileset.
    pub local_id: u32,
    /// The atlas coordinates.
    pub atlas: (i32, i32),
    /// Flipped horizontally.
    pub flip_h: bool,
    /// Flipped vertically.
    pub flip_v: bool,
    /// Flipped anti-diagonally.
    pub flip_d: bool,
}

impl ResolvedCell {
    /// The alternative id the runtime gives the cell: the transform flags with
    /// `no_alternative_tiles` (its default), else the generated alternative `h + 2v + 4d`.
    pub fn alternative(&self, no_alternative_tiles: bool) -> u16 {
        alternative(self.flip_h, self.flip_v, self.flip_d, no_alternative_tiles)
    }
}

fn alternative(h: bool, v: bool, d: bool, no_alternative_tiles: bool) -> u16 {
    if no_alternative_tiles {
        (if h { TRANSFORM_FLIP_H } else { 0 })
            | (if v { TRANSFORM_FLIP_V } else { 0 })
            | (if d { TRANSFORM_TRANSPOSE } else { 0 })
    } else {
        u16::from(h) + 2 * u16::from(v) + 4 * u16::from(d)
    }
}

/// Resolves gids of one map (its tileset list and atlas sources).
#[derive(Clone, Debug, Default)]
pub struct CellResolver {
    first_gids: Vec<u32>,
    /// Sorted by source id, as the runtime sorts its registered sources.
    sources: Vec<AtlasSource>,
}

impl CellResolver {
    /// The resolver of a loaded map, with the atlas geometry its files give.
    pub fn new(map: &Map) -> Self {
        let slots = map.tileset_slots();
        let mut first_gids = Vec::with_capacity(slots.len());
        let mut sources = Vec::new();
        let mut next_id: i32 = 0;
        let mut used: Vec<i32> = Vec::new();
        let mut add = |requested: i32| -> i32 {
            if requested >= 0 && used.contains(&requested) {
                return -1;
            }
            let id = if requested >= 0 { requested } else { next_id };
            used.push(id);
            next_id = next_id.max(id) + 1;
            id
        };
        for (i, slot) in slots.iter().enumerate() {
            let firstgid = slot.firstgid.max(0) as u32;
            first_gids.push(firstgid);
            if matches!(slot.source, TilesetSource::Skipped) {
                continue;
            }
            let Some(ts) = map.slot_tileset(slot) else {
                continue;
            };
            if ts.name.as_deref() == Some("AutoMap Rules") {
                continue;
            }
            if ts.image.is_some() {
                let id = add(atlas_id(&ts.properties));
                let (grid_width, grid_height, num_tiles) = atlas_geometry(ts);
                sources.push(AtlasSource {
                    source_id: id,
                    slot: i,
                    firstgid,
                    tile: None,
                    num_tiles,
                    grid_width,
                    single: grid_width == 1 && grid_height == 1,
                });
            }
            for tile in ts.tiles.iter().flatten() {
                if tile.image.is_none() {
                    continue;
                }
                let id = add(atlas_id(&tile.properties));
                sources.push(AtlasSource {
                    source_id: id,
                    slot: i,
                    firstgid,
                    tile: Some(tile.id.unwrap_or(0).max(0) as u32),
                    num_tiles: 1,
                    grid_width: 1,
                    single: true,
                });
            }
        }
        sources.sort_by_key(|s| s.source_id);
        CellResolver {
            first_gids,
            sources,
        }
    }

    /// A resolver over explicit first gids and sources.
    pub fn from_sources(first_gids: Vec<u32>, mut sources: Vec<AtlasSource>) -> Self {
        sources.sort_by_key(|s| s.source_id);
        CellResolver {
            first_gids,
            sources,
        }
    }

    /// The atlas sources, sorted by source id.
    pub fn sources(&self) -> &[AtlasSource] {
        &self.sources
    }

    /// Puts the real texture size of an atlas tileset's image in (what the runtime measures):
    /// `numTiles` becomes `(width / tile width) * height / tile height`, the grid Godot's
    /// `get_atlas_grid_size()` for the tileset's margin and spacing.
    pub fn set_texture_size(&mut self, slot: usize, tileset: &Tileset, width: i64, height: i64) {
        let (gw, gh, n) = geometry(tileset, width, height);
        for s in self
            .sources
            .iter_mut()
            .filter(|s| s.slot == slot && s.tile.is_none())
        {
            s.grid_width = gw;
            s.single = gw == 1 && gh == 1;
            s.num_tiles = n;
        }
    }

    /// `get_first_gid_index`: the last tileset whose first gid is at most `gid` (0 if none).
    fn first_gid_index(&self, gid: u32) -> usize {
        let mut index = 0;
        for (i, &f) in self.first_gids.iter().enumerate() {
            if gid >= f {
                index = i;
            }
        }
        index
    }

    /// Resolves a raw cell value (gid with flip flags), as `resolve_cell` does; `None` for an
    /// empty cell or one the runtime skips.
    pub fn resolve(&self, raw: u32) -> Option<ResolvedCell> {
        let gid = raw & GID_MASK;
        if gid == 0 || self.first_gids.is_empty() {
            return None;
        }
        let covering = self.first_gids[self.first_gid_index(gid)];
        let src = self.sources.iter().find(|s| {
            let effective = i64::from(gid) - i64::from(s.firstgid) + 1;
            match s.tile {
                None => effective <= i64::from(s.num_tiles) && s.firstgid == covering,
                Some(t) => effective == i64::from(t) + 1,
            }
        })?;
        if src.source_id < 0 || src.grid_width == 0 {
            return None;
        }
        let local = gid.wrapping_sub(covering);
        let atlas = if src.num_tiles > 1 && !src.single {
            (
                (local % src.grid_width) as i32,
                (local / src.grid_width) as i32,
            )
        } else {
            (0, 0)
        };
        Some(ResolvedCell {
            source_id: src.source_id,
            tileset: src.slot,
            local_id: src.tile.unwrap_or(local),
            atlas,
            flip_h: raw & FLIPPED_HORIZONTALLY != 0,
            flip_v: raw & FLIPPED_VERTICALLY != 0,
            flip_d: raw & FLIPPED_DIAGONALLY != 0,
        })
    }

    /// A lookup table for blob emission.
    pub fn lookup(&self) -> GidLookup {
        GidLookup::new(self)
    }
}

/// The `godot_atlas_id` int property, -1 when there is none.
fn atlas_id(props: &Option<Vec<crate::model::Property>>) -> i32 {
    for p in props.iter().flatten() {
        let name = p.name.as_deref().unwrap_or("");
        if name.is_empty() {
            continue;
        }
        if name.to_lowercase() == "godot_atlas_id" && p.kind_or_string() == "int" {
            return match &p.value {
                Some(PropertyValue::Number(n)) => n.as_i64() as i32,
                Some(PropertyValue::String(s)) => crate::godot_str::to_int(s) as i32,
                _ => 0,
            };
        }
    }
    -1
}

/// (grid width, grid height, numTiles) of an atlas tileset from its file.
fn atlas_geometry(ts: &Tileset) -> (u32, u32, u32) {
    match (ts.imagewidth, ts.imageheight) {
        (Some(w), Some(h)) => geometry(ts, w, h),
        _ => {
            let columns = ts.columns.unwrap_or(0).max(0) as u32;
            let count = ts.tilecount.unwrap_or(0).max(0) as u32;
            let rows = if columns > 0 {
                count.div_ceil(columns)
            } else {
                0
            };
            (columns, rows, count)
        }
    }
}

fn geometry(ts: &Tileset, width: i64, height: i64) -> (u32, u32, u32) {
    let tw = ts.tilewidth.unwrap_or(0);
    let th = ts.tileheight.unwrap_or(0);
    if tw <= 0 || th <= 0 {
        return (0, 0, 0);
    }
    let margin = ts.margin.unwrap_or(0);
    let spacing = ts.spacing.unwrap_or(0);
    // Godot's TileSetAtlasSource.get_atlas_grid_size().
    let grid = |size: i64, region: i64| {
        let valid = size - margin;
        if valid >= region {
            1 + (valid - region) / (region + spacing)
        } else {
            0
        }
    };
    let (gw, gh) = if width - margin >= tw && height - margin >= th {
        (grid(width, tw), grid(height, th))
    } else {
        (0, 0)
    };
    // TilesetCreator: columns = width / tile width; tile count = columns * height / tile height.
    let columns = width / tw;
    let count = columns * height / th;
    (gw.max(0) as u32, gh.max(0) as u32, count.max(0) as u32)
}

/// Gid to (source, atlas x, atlas y), precomputed for the gids the map's tilesets cover.
#[derive(Clone, Debug)]
pub struct GidLookup {
    table: Vec<[i16; 3]>,
    resolver: CellResolver,
}

const NO_TILE: [i16; 3] = [-1, 0, 0];
const TABLE_LIMIT: u32 = 1 << 20;

impl GidLookup {
    fn new(resolver: &CellResolver) -> Self {
        let max_gid = resolver
            .sources
            .iter()
            .map(|s| match s.tile {
                Some(t) => s.firstgid.saturating_add(t),
                None => s.firstgid.saturating_add(s.num_tiles),
            })
            .max()
            .unwrap_or(0)
            .min(TABLE_LIMIT);
        let table = (0..=max_gid)
            .map(|gid| match resolver.resolve(gid) {
                Some(c) => [c.source_id as i16, c.atlas.0 as i16, c.atlas.1 as i16],
                None => NO_TILE,
            })
            .collect();
        GidLookup {
            table,
            resolver: resolver.clone(),
        }
    }

    /// (source id, atlas x, atlas y) of a gid without flags.
    #[inline]
    pub fn get(&self, gid: u32) -> Option<[i16; 3]> {
        let t = match self.table.get(gid as usize) {
            Some(t) => *t,
            None => {
                let c = self.resolver.resolve(gid)?;
                [c.source_id as i16, c.atlas.0 as i16, c.atlas.1 as i16]
            }
        };
        (t[0] >= 0).then_some(t)
    }
}

/// How cells become blobs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlobOptions {
    /// Flips as transform flags in the alternative id (the runtime's default), else the
    /// generated alternative tile's id `h + 2v + 4d`.
    pub no_alternative_tiles: bool,
}

impl Default for BlobOptions {
    fn default() -> Self {
        BlobOptions {
            no_alternative_tiles: true,
        }
    }
}

/// Writes cells in `tile_map_data`'s format, header included.
#[derive(Clone, Debug)]
pub struct BlobWriter {
    out: Vec<u8>,
}

impl BlobWriter {
    /// A writer with room for `cells` cells.
    pub fn with_capacity(cells: usize) -> Self {
        let mut out = Vec::with_capacity(2 + cells * 12);
        out.extend_from_slice(&0u16.to_le_bytes());
        BlobWriter { out }
    }

    /// Appends one cell.
    #[inline]
    pub fn push(
        &mut self,
        x: i16,
        y: i16,
        source: i16,
        atlas_x: i16,
        atlas_y: i16,
        alternative: u16,
    ) {
        let mut rec = [0u8; 12];
        rec[0..2].copy_from_slice(&x.to_le_bytes());
        rec[2..4].copy_from_slice(&y.to_le_bytes());
        rec[4..6].copy_from_slice(&source.to_le_bytes());
        rec[6..8].copy_from_slice(&atlas_x.to_le_bytes());
        rec[8..10].copy_from_slice(&atlas_y.to_le_bytes());
        rec[10..12].copy_from_slice(&alternative.to_le_bytes());
        self.out.extend_from_slice(&rec);
    }

    /// The number of cells written.
    pub fn len(&self) -> usize {
        (self.out.len() - 2) / 12
    }

    /// True when no cell was written.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The blob.
    pub fn finish(self) -> Vec<u8> {
        self.out
    }
}

/// The blob of a run of cells laid out in rows of `width`, the first at (`x0`, `y0`):
/// `create_map_from_data` without the engine. Empty cells and cells the runtime skips are left
/// out. Coordinates are `i16` in the engine's format.
pub fn tile_map_data(
    cells: &[u32],
    x0: i32,
    y0: i32,
    width: usize,
    lookup: &GidLookup,
    options: BlobOptions,
) -> Vec<u8> {
    let mut w = BlobWriter::with_capacity(cells.len());
    write_cells(&mut w, cells, x0, y0, width, lookup, options);
    w.finish()
}

fn write_cells(
    w: &mut BlobWriter,
    cells: &[u32],
    x0: i32,
    y0: i32,
    width: usize,
    lookup: &GidLookup,
    options: BlobOptions,
) {
    let width = width.max(1);
    let (mut col, mut row) = (0usize, 0i32);
    for &raw in cells {
        let gid = raw & GID_MASK;
        if gid != 0 {
            if let Some([src, ax, ay]) = lookup.get(gid) {
                let alt = alternative(
                    raw & FLIPPED_HORIZONTALLY != 0,
                    raw & FLIPPED_VERTICALLY != 0,
                    raw & FLIPPED_DIAGONALLY != 0,
                    options.no_alternative_tiles,
                );
                w.push(
                    (x0 + col as i32) as i16,
                    (y0 + row) as i16,
                    src,
                    ax,
                    ay,
                    alt,
                );
            }
        }
        col += 1;
        if col == width {
            col = 0;
            row += 1;
        }
    }
}

/// Joins blobs into one (one header, every blob's cells in order).
pub fn merge_blobs<B: AsRef<[u8]>>(blobs: &[B]) -> Vec<u8> {
    let total: usize = blobs
        .iter()
        .map(|b| b.as_ref().len().saturating_sub(2))
        .sum();
    let mut out = Vec::with_capacity(2 + total);
    out.extend_from_slice(&0u16.to_le_bytes());
    for b in blobs {
        out.extend_from_slice(b.as_ref().get(2..).unwrap_or(&[]));
    }
    out
}

/// A chunk's blob, at the chunk's cell coordinates.
#[derive(Clone, Debug)]
pub struct ChunkBlob {
    /// The chunk's x in cells.
    pub x: i64,
    /// The chunk's y in cells.
    pub y: i64,
    /// The chunk's cells.
    pub blob: Vec<u8>,
}

/// The blobs of an infinite layer's chunks, one per chunk (for streaming).
pub fn chunk_blobs(
    layer: &Layer,
    lookup: &GidLookup,
    options: BlobOptions,
) -> Result<Vec<ChunkBlob>, Error> {
    let mut out = Vec::new();
    for c in layer.chunks.iter().flatten() {
        let Some(cells) = c.decode(layer) else {
            continue;
        };
        let cells = cells?;
        let (x, y) = (c.x.unwrap_or(0), c.y.unwrap_or(0));
        let width = c.width.unwrap_or(0).max(0) as usize;
        out.push(ChunkBlob {
            x,
            y,
            blob: tile_map_data(&cells, x as i32, y as i32, width, lookup, options),
        });
    }
    Ok(out)
}

/// The blob of a whole tile layer as the runtime fills it: the chunks of an infinite map's
/// layer, else its `data` laid out in rows of the map's width. `None` for a layer that is not
/// a tile layer.
pub fn layer_blob(
    map: &Map,
    layer: &Layer,
    lookup: &GidLookup,
    options: BlobOptions,
) -> Option<Result<Vec<u8>, Error>> {
    if layer.kind_or_tile() != "tilelayer" {
        return None;
    }
    if map.is_infinite() && layer.chunks.is_some() {
        let mut w = BlobWriter::with_capacity(0);
        for c in layer.chunks.iter().flatten() {
            let Some(cells) = c.decode(layer) else {
                continue;
            };
            let cells = match cells {
                Ok(c) => c,
                Err(e) => return Some(Err(e)),
            };
            let width = c.width.unwrap_or(0).max(0) as usize;
            write_cells(
                &mut w,
                &cells,
                c.x.unwrap_or(0) as i32,
                c.y.unwrap_or(0) as i32,
                width,
                lookup,
                options,
            );
        }
        return Some(Ok(w.finish()));
    }
    let cells = match layer.decode()? {
        Ok(c) => c,
        Err(e) => return Some(Err(e)),
    };
    let width = map.width.unwrap_or(0).max(0) as usize;
    Some(Ok(tile_map_data(&cells, 0, 0, width, lookup, options)))
}

/// Every tile layer of the map (depth first) with its blob, one thread per layer.
pub fn layer_blobs_parallel<'m>(
    map: &'m Map,
    lookup: &GidLookup,
    options: BlobOptions,
) -> Vec<(&'m Layer, Result<Vec<u8>, Error>)> {
    let layers: Vec<&Layer> = map
        .all_layers()
        .into_iter()
        .filter(|l| l.kind_or_tile() == "tilelayer")
        .collect();
    std::thread::scope(|s| {
        let handles: Vec<_> = layers
            .iter()
            .map(|&l| s.spawn(move || layer_blob(map, l, lookup, options)))
            .collect();
        layers
            .iter()
            .zip(handles)
            .filter_map(|(&l, h)| h.join().expect("blob thread panicked").map(|r| (l, r)))
            .collect()
    })
}

/// Every tile layer of the map with its blob, on this thread.
pub fn layer_blobs<'m>(
    map: &'m Map,
    lookup: &GidLookup,
    options: BlobOptions,
) -> Vec<(&'m Layer, Result<Vec<u8>, Error>)> {
    map.all_layers()
        .into_iter()
        .filter_map(|l| layer_blob(map, l, lookup, options).map(|r| (l, r)))
        .collect()
}
