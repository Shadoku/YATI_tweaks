//! Tile layers prepared for a build before the TileSet exists.
//!
//! What a tile layer costs a build, besides the engine's `tile_map_data` assignment, is
//! decoding its payloads (base64, zlib/gzip/zstd, csv) and writing one 12-byte record per
//! non-empty cell. Neither needs the TileSet, except for the record's source id, atlas
//! coordinates and alternative id, which depend on how the TileSet was built (its source ids,
//! the textures' atlas grids). So [`PreparedCells::prepare`] does everything else, anywhere (the
//! asynchronous loader runs it on its worker thread, one thread per layer): each layer becomes
//! a blob in `tile_map_data`'s format whose records hold the cell's coordinates and, in place
//! of the four tile fields, the index of the cell's value in the layer's list of distinct
//! values. The main thread then resolves those few distinct values against the TileSet and
//! [`PreparedLayer::patch`]es the blob in place, one 8-byte write per cell.
//!
//! The records come in the order the builder always wrote them (an infinite map's chunks in
//! file order, else the map's `data` row by row; empty cells left out), so a patched blob is
//! byte for byte the blob a build that decodes its layers itself writes.

use crate::data::{decode, GID_MASK};
use crate::model::{Layer, Map};

/// The value bits a cell's resolution depends on: the gid and the three flip flags (the
/// hexagonal 120° rotation flag is masked off, as the GDScript runtime masks it).
pub const VALUE_MASK: u32 = 0xEFFF_FFFF;

/// One tile layer's cells as a blob waiting for its tile fields.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PreparedLayer {
    /// `tile_map_data`'s format: the `u16` version 0, then 12 bytes per non-empty cell: x and y
    /// (`i16`, little-endian), the index of the cell's value in [`values`](Self::values) (`u32`,
    /// little-endian, where the source id and atlas x go), 4 zero bytes (atlas y, alternative).
    pub blob: Vec<u8>,
    /// The distinct non-empty cell values ([`VALUE_MASK`]ed), in the order they first appear.
    pub values: Vec<u32>,
    /// Why payloads could not be decoded (their cells are left out), one message each.
    pub errors: Vec<String>,
}

/// A segment's origin, row width and cells (or why they could not be decoded).
type RawSegment<'a> = (i32, i32, usize, Result<std::borrow::Cow<'a, [u32]>, String>);

/// Value index tables: dense for gids below this many entries (`gid * 8 + flips`), a map
/// beyond.
const DENSE_LIMIT: usize = 1 << 20;

impl PreparedLayer {
    /// Decodes a tile layer as the builder fills it: an infinite map's chunks (each at its own
    /// origin, in rows of its width), else its `data` in rows of the map's width.
    pub fn of(map: &Map, layer: &Layer) -> PreparedLayer {
        let enc = layer.encoding.as_deref();
        let comp = layer.compression.as_deref();
        let mut segs: Vec<RawSegment> = Vec::new();
        if map.is_infinite() && layer.chunks.is_some() {
            for c in layer.chunks.iter().flatten() {
                let w = c.width.unwrap_or(0).max(0) as usize;
                let h = c.height.unwrap_or(0).max(0) as usize;
                let Some(p) = &c.data else { continue };
                let cells = decode(p, enc, comp, w * h).map_err(|e| e.message);
                segs.push((c.x.unwrap_or(0) as i32, c.y.unwrap_or(0) as i32, w, cells));
            }
        } else if let Some(p) = &layer.data {
            let w = map.width.unwrap_or(0).max(0) as usize;
            let h = map.height.unwrap_or(0).max(0) as usize;
            let cells = decode(p, enc, comp, w * h).map_err(|e| e.message);
            segs.push((0, 0, w, cells));
        }
        let total: usize = segs
            .iter()
            .map(|s| s.3.as_ref().map_or(0, |c| c.len()))
            .sum();
        let mut out = PreparedLayer {
            blob: Vec::with_capacity(2 + total * 12),
            ..Default::default()
        };
        out.blob.extend_from_slice(&0u16.to_le_bytes());
        // Value -> index + 1 (0: not seen yet)
        let mut dense: Vec<u32> = Vec::new();
        let mut sparse: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
        for (x0, y0, width, cells) in segs {
            let cells = match cells {
                Ok(c) => c,
                Err(e) => {
                    out.errors.push(e);
                    continue;
                }
            };
            let width = width.max(1);
            let (mut col, mut row) = (0usize, 0i32);
            for &raw in cells.iter() {
                if raw & GID_MASK != 0 {
                    let i = ((raw & GID_MASK) as usize) * 8 + (raw >> 29) as usize;
                    let slot = if i < DENSE_LIMIT {
                        if dense.len() <= i {
                            dense.resize(i + 1, 0);
                        }
                        &mut dense[i]
                    } else {
                        sparse.entry(raw & VALUE_MASK).or_insert(0)
                    };
                    if *slot == 0 {
                        out.values.push(raw & VALUE_MASK);
                        *slot = out.values.len() as u32;
                    }
                    let index = *slot - 1;
                    let mut rec = [0u8; 12];
                    rec[0..2].copy_from_slice(&((x0 + col as i32) as i16).to_le_bytes());
                    rec[2..4].copy_from_slice(&((y0 + row) as i16).to_le_bytes());
                    rec[4..8].copy_from_slice(&index.to_le_bytes());
                    out.blob.extend_from_slice(&rec);
                }
                col += 1;
                if col == width {
                    col = 0;
                    row += 1;
                }
            }
        }
        out
    }

    /// The number of cell records.
    pub fn len(&self) -> usize {
        self.blob.len().saturating_sub(2) / 12
    }

    /// True when the layer has no non-empty cell.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Every record as (x, y, index into [`values`](Self::values)).
    pub fn records(&self) -> impl Iterator<Item = (i16, i16, u32)> + '_ {
        self.blob.get(2..).unwrap_or(&[]).chunks_exact(12).map(|r| {
            (
                i16::from_le_bytes([r[0], r[1]]),
                i16::from_le_bytes([r[2], r[3]]),
                u32::from_le_bytes([r[4], r[5], r[6], r[7]]),
            )
        })
    }

    /// Fills in the tile fields of a copy of [`blob`](Self::blob), in place: `resolved[i]` is
    /// (source id, atlas x, atlas y, alternative id) of `values[i]`, or `None` for a value the
    /// TileSet has no tile for, whose cells are then removed (the records after them move
    /// down). Returns the patched blob's length (shorter than `blob` only when cells were
    /// removed); `blob` must be a copy of this layer's blob.
    pub fn patch(blob: &mut [u8], resolved: &[Option<[i16; 4]>]) -> usize {
        if blob.len() < 2 {
            return blob.len();
        }
        let n = (blob.len() - 2) / 12;
        let mut write = 2usize;
        for k in 0..n {
            let read = 2 + k * 12;
            let index = u32::from_le_bytes([
                blob[read + 4],
                blob[read + 5],
                blob[read + 6],
                blob[read + 7],
            ]) as usize;
            let Some(Some([src, ax, ay, alt])) = resolved.get(index).copied() else {
                continue;
            };
            if write != read {
                blob.copy_within(read..read + 4, write);
            }
            let r = &mut blob[write + 4..write + 12];
            r[0..2].copy_from_slice(&src.to_le_bytes());
            r[2..4].copy_from_slice(&ax.to_le_bytes());
            r[4..6].copy_from_slice(&ay.to_le_bytes());
            r[6..8].copy_from_slice(&alt.to_le_bytes());
            write += 12;
        }
        write
    }
}

/// A tile layer this large (cells of its data or chunks) is prepared on a thread of its own
/// when the map has several such.
pub const THREAD_CELLS: usize = 1 << 15;

/// A layer and its index in [`Map::all_layers`].
type Indexed<'m> = (usize, &'m Layer);

/// The cells a tile layer's payloads hold.
fn cell_count(map: &Map, l: &Layer) -> usize {
    let n = |w: Option<i64>, h: Option<i64>| {
        w.unwrap_or(0).max(0) as usize * h.unwrap_or(0).max(0) as usize
    };
    if map.is_infinite() && l.chunks.is_some() {
        l.chunks
            .iter()
            .flatten()
            .map(|c| n(c.width, c.height))
            .sum()
    } else {
        n(map.width, map.height)
    }
}

/// Every tile layer of a map, prepared ([`PreparedLayer`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PreparedCells {
    /// By the layer's index in [`Map::all_layers`]; `None` for a layer that is not a tile
    /// layer.
    layers: Vec<Option<PreparedLayer>>,
}

impl PreparedCells {
    /// Prepares every tile layer of the map (groups included): layers of at least
    /// [`THREAD_CELLS`] cells on a thread each when there are several such, the rest on this
    /// thread (a thread costs more than a small layer's decoding, and much more on a busy
    /// machine).
    pub fn prepare(map: &Map) -> PreparedCells {
        let all = map.all_layers();
        let mut layers: Vec<Option<PreparedLayer>> = vec![None; all.len()];
        let (big, small): (Vec<Indexed>, Vec<Indexed>) = all
            .iter()
            .enumerate()
            .filter(|(_, l)| l.kind_or_tile() == "tilelayer")
            .map(|(i, &l)| (i, l))
            .partition(|(_, l)| cell_count(map, l) >= THREAD_CELLS);
        let (threaded, inline) = if big.len() > 1 {
            (big, small)
        } else {
            (Vec::new(), [big, small].concat())
        };
        std::thread::scope(|s| {
            let handles: Vec<_> = threaded
                .iter()
                .map(|&(i, l)| (i, s.spawn(move || PreparedLayer::of(map, l))))
                .collect();
            for (i, l) in inline {
                layers[i] = Some(PreparedLayer::of(map, l));
            }
            for (i, h) in handles {
                // A thread that panicked (it cannot: decoding reports errors) prepares an
                // empty layer.
                layers[i] = Some(h.join().unwrap_or_default());
            }
        });
        PreparedCells { layers }
    }

    /// The number of layers of the map these were prepared for (all kinds).
    pub fn layer_count(&self) -> usize {
        self.layers.len()
    }

    /// The prepared layer at an index of [`Map::all_layers`] (`None`: not a tile layer).
    pub fn layer(&self, index: usize) -> Option<&PreparedLayer> {
        self.layers.get(index).and_then(|l| l.as_ref())
    }

    /// The bytes held (blobs and value lists).
    pub fn byte_size(&self) -> usize {
        self.layers
            .iter()
            .flatten()
            .map(|l| l.blob.len() + l.values.len() * 4)
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cells::{layer_blob, BlobOptions, CellResolver};
    use crate::data::{FLIPPED_DIAGONALLY, FLIPPED_HORIZONTALLY, FLIPPED_VERTICALLY};
    use std::path::Path;

    fn fixture(name: &str) -> Map {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures")
            .join(name);
        crate::load::load_from_fs(&path.to_string_lossy(), None).expect("fixture loads")
    }

    /// A prepared layer patched with the core's resolver equals the core's own blob of it.
    #[test]
    fn patched_blobs_equal_the_core_blobs() {
        for name in [
            "layers.tmx",
            "infinite.tmx",
            "infinite.tmj",
            "encodings.tmx",
            "legacy_xml.tmx",
            "tilesets.tmx",
            "json_map.tmj",
            "hexagonal.tmx",
        ] {
            let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../test/fixtures")
                .join(name);
            assert!(path.exists(), "{name}");
            let map = fixture(name);
            let resolver = CellResolver::new(&map);
            let lookup = resolver.lookup();
            let options = BlobOptions::default();
            let prepared = PreparedCells::prepare(&map);
            assert_eq!(prepared.layer_count(), map.all_layers().len(), "{name}");
            let mut tile_layers = 0;
            for (i, l) in map.all_layers().into_iter().enumerate() {
                let Some(expected) = layer_blob(&map, l, &lookup, options) else {
                    assert!(prepared.layer(i).is_none(), "{name}: layer {i}");
                    continue;
                };
                tile_layers += 1;
                let expected = expected.expect("decodes");
                let p = prepared.layer(i).expect("prepared");
                assert!(p.errors.is_empty(), "{name}: {:?}", p.errors);
                let resolved: Vec<Option<[i16; 4]>> = p
                    .values
                    .iter()
                    .map(|&raw| {
                        let [src, ax, ay] = lookup.get(raw & GID_MASK)?;
                        let flag = |bit: u32, t: u16| if raw & bit != 0 { t } else { 0 };
                        let alt = flag(FLIPPED_HORIZONTALLY, crate::cells::TRANSFORM_FLIP_H)
                            | flag(FLIPPED_VERTICALLY, crate::cells::TRANSFORM_FLIP_V)
                            | flag(FLIPPED_DIAGONALLY, crate::cells::TRANSFORM_TRANSPOSE);
                        Some([src, ax, ay, alt as i16])
                    })
                    .collect();
                let mut blob = p.blob.clone();
                let n = PreparedLayer::patch(&mut blob, &resolved);
                blob.truncate(n);
                assert_eq!(blob, expected, "{name}: layer {i}");
            }
            assert!(tile_layers > 0, "{name} has tile layers");
        }
    }

    /// Distinct values in first-appearance order, records pointing at them, flips kept apart,
    /// the hexagonal rotation bit ignored; unresolved values' cells removed in place.
    #[test]
    fn values_records_and_removal() {
        let rot = crate::data::ROTATED_HEXAGONAL_120;
        let cells = [
            0,
            5,
            5 | FLIPPED_HORIZONTALLY,
            7,
            5 | rot,
            0,
            7,
            (1 << 26) | FLIPPED_VERTICALLY,
        ];
        let csv: Vec<String> = cells.iter().map(|c| c.to_string()).collect();
        let map: Map = crate::parse::map(
            format!(
                r#"{{"type":"map","width":4,"height":2,"tilewidth":16,"tileheight":16,
                "layers":[{{"type":"tilelayer","name":"a","width":4,"height":2,"data":[{}]}}]}}"#,
                csv.join(",")
            )
            .as_bytes(),
            "m.tmj",
        )
        .expect("parses");
        let prepared = PreparedCells::prepare(&map);
        let p = prepared.layer(0).expect("tile layer");
        assert_eq!(
            p.values,
            vec![
                5,
                5 | FLIPPED_HORIZONTALLY,
                7,
                (1 << 26) | FLIPPED_VERTICALLY
            ]
        );
        let recs: Vec<_> = p.records().collect();
        assert_eq!(
            recs,
            vec![
                (1, 0, 0),
                (2, 0, 1),
                (3, 0, 2),
                (0, 1, 0),
                (2, 1, 2),
                (3, 1, 3)
            ]
        );
        // Value 1 (the flipped 5) unresolved: its cell goes, the rest move down
        let resolved = [
            Some([0, 1, 2, 0]),
            None,
            Some([1, 0, 0, 0]),
            Some([0, 3, 3, 0x2000]),
        ];
        let mut blob = p.blob.clone();
        let n = PreparedLayer::patch(&mut blob, &resolved);
        blob.truncate(n);
        let mut expected = crate::cells::BlobWriter::with_capacity(5);
        expected.push(1, 0, 0, 1, 2, 0);
        expected.push(3, 0, 1, 0, 0, 0);
        expected.push(0, 1, 0, 1, 2, 0);
        expected.push(2, 1, 1, 0, 0, 0);
        expected.push(3, 1, 0, 3, 3, 0x2000);
        assert_eq!(blob, expected.finish());
    }
}
