//! Layer payloads in every encoding, chunks, and the `tile_map_data` blobs.

mod common;

use common::*;
use tiled_core::cells::{
    layer_blob, merge_blobs, tile_map_data, AtlasSource, BlobOptions, BlobWriter, CellResolver,
    TRANSFORM_FLIP_H, TRANSFORM_FLIP_V, TRANSFORM_TRANSPOSE,
};
use tiled_core::data::{decode, Gid};
use tiled_core::{Layer, Map, Payload};

fn tile_layer<'a>(map: &'a Map, name: &str) -> &'a Layer {
    map.all_layers()
        .into_iter()
        .find(|l| l.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("layer {name}"))
}

/// A blob's cells back as (x, y, source, atlas x, atlas y, alternative).
fn cells_of(blob: &[u8]) -> Vec<(i16, i16, i16, i16, i16, u16)> {
    assert_eq!(&blob[..2], &[0, 0], "format version");
    assert_eq!((blob.len() - 2) % 12, 0);
    blob[2..]
        .as_chunks::<12>()
        .0
        .iter()
        .map(|c| {
            let v = |i: usize| i16::from_le_bytes([c[i], c[i + 1]]);
            (
                v(0),
                v(2),
                v(4),
                v(6),
                v(8),
                u16::from_le_bytes([c[10], c[11]]),
            )
        })
        .collect()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[test]
fn every_encoding_decodes_to_the_same_gids() {
    let map = load("encodings.tmx", None);
    let reference = tile_layer(&map, "CSV")
        .decode()
        .unwrap()
        .unwrap()
        .into_owned();
    assert_eq!(
        reference,
        vec![
            1,
            2,
            3,
            4,
            9,
            10,
            11,
            12,
            0,
            0,
            17,
            18,
            25,
            26,
            27,
            2_147_483_650
        ]
    );
    for name in ["B64", "B64zlib", "B64gzip", "B64zstd"] {
        if name == "B64zstd" && cfg!(not(feature = "zstd")) {
            continue;
        }
        let l = tile_layer(&map, name);
        assert!(
            matches!(l.data, Some(Payload::Text(_))),
            "{name} stays undecoded in the model"
        );
        assert_eq!(
            l.decode().unwrap().unwrap().as_ref(),
            reference.as_slice(),
            "{name}"
        );
    }
    let flipped = Gid(reference[15]);
    assert!(flipped.flip_h() && !flipped.flip_v() && !flipped.flip_d());
    assert_eq!(flipped.id(), 2);
}

#[test]
#[cfg(feature = "zstd")]
fn the_zstd_layer_of_classes_decodes() {
    let map = load("classes.tmx", None);
    let cells = tile_layer(&map, "Zstd").decode().unwrap().unwrap();
    assert_eq!(cells.len(), 16);
    assert_eq!(cells[15], 2_147_483_650);
}

#[test]
fn legacy_tile_gid_list() {
    let map = load("legacy_xml.tmx", None);
    let l = tile_layer(&map, "LegacyXml");
    assert_eq!(l.encoding.as_deref(), Some("csv"));
    assert_eq!(
        l.decode().unwrap().unwrap().as_ref(),
        &[1, 2, 0, 4, 9, 0, 0, 2]
    );
    let after = tile_layer(&map, "AfterLegacy").decode().unwrap().unwrap();
    assert_eq!(after.as_ref(), &[3; 8]);
}

#[test]
fn bad_payloads_are_errors_not_panics() {
    let p = Payload::Text("not base64!".into());
    assert!(decode(&p, Some("base64"), None, 4).is_err());
    let p = Payload::Text("AQAAAA==".into());
    assert!(decode(&p, Some("base64"), Some("zlib"), 1).is_err());
    assert!(decode(&p, Some("base64"), Some("lzma"), 1)
        .unwrap_err()
        .message
        .contains("not yet implemented"));
    assert!(decode(&p, Some("hex"), None, 1).is_err());
}

#[test]
fn blob_format_matches_the_engine() {
    // set_cell(Vector2i(3, -2), 7, Vector2i(5, 1), TRANSFORM_FLIP_H)
    // + set_cell(Vector2i(100, 200), 7, Vector2i(7, 7), 0), read back from tile_map_data.
    const ENGINE: &str = "00000300feff07000500010000106400c8000700070007000000";
    let mut w = BlobWriter::with_capacity(2);
    w.push(3, -2, 7, 5, 1, TRANSFORM_FLIP_H);
    w.push(100, 200, 7, 7, 7, 0);
    assert_eq!(w.len(), 2);
    assert_eq!(hex(&w.finish()), ENGINE);

    // The same through gid resolution: one 8 x 8 atlas as source 7.
    let resolver = CellResolver::from_sources(
        vec![1],
        vec![AtlasSource {
            source_id: 7,
            slot: 0,
            firstgid: 1,
            tile: None,
            num_tiles: 64,
            grid_width: 8,
            single: false,
        }],
    );
    let lookup = resolver.lookup();
    let opts = BlobOptions::default();
    let a = tile_map_data(&[(1 + 8 + 5) | 0x8000_0000], 3, -2, 1, &lookup, opts);
    let b = tile_map_data(&[0, 1 + 63], 99, 200, 2, &lookup, opts);
    assert_eq!(hex(&merge_blobs(&[a, b])), ENGINE);
}

#[test]
fn infinite_chunks_and_their_offsets() {
    let map = load("infinite.tmx", None);
    let resolver = CellResolver::new(&map);
    let lookup = resolver.lookup();
    for name in ["ChunksCSV", "ChunksZlib"] {
        let l = tile_layer(&map, name);
        let chunks = l.chunks.as_ref().unwrap();
        assert_eq!(chunks.len(), 2);
        assert_eq!((chunks[0].x, chunks[0].y), (Some(-16), Some(0)));
        let blob = layer_blob(&map, l, &lookup, BlobOptions::default())
            .unwrap()
            .unwrap();
        assert_eq!(
            cells_of(&blob),
            vec![
                (-16, 0, 0, 0, 0, 0),
                (-1, 15, 0, 1, 0, 0),
                (0, 16, 0, 2, 0, 0),
                (1, 17, 0, 3, 0, 0)
            ],
            "{name}"
        );
        // Per chunk, for streaming: the same cells split at the chunk border.
        let per_chunk = tiled_core::cells::chunk_blobs(l, &lookup, BlobOptions::default()).unwrap();
        assert_eq!(per_chunk.len(), 2);
        assert_eq!((per_chunk[1].x, per_chunk[1].y), (0, 16));
        assert_eq!(cells_of(&per_chunk[0].blob).len(), 2);
        let merged = merge_blobs(&per_chunk.iter().map(|c| c.blob.clone()).collect::<Vec<_>>());
        assert_eq!(merged, blob);
    }
}

#[test]
fn json_infinite_chunks_equal_the_tmx_ones() {
    let tmx = load("infinite.tmx", None);
    let tmj = load("infinite.tmj", None);
    for name in ["ChunksCSV", "ChunksZlib"] {
        let (a, b) = (tile_layer(&tmx, name), tile_layer(&tmj, name));
        let ca = a.chunks.as_ref().unwrap();
        let cb = b.chunks.as_ref().unwrap();
        assert_eq!(ca.len(), cb.len());
        for (x, y) in ca.iter().zip(cb) {
            assert_eq!((x.x, x.y, x.width, x.height), (y.x, y.y, y.width, y.height));
            assert_eq!(
                x.decode(a).unwrap().unwrap(),
                y.decode(b).unwrap().unwrap(),
                "{name}"
            );
        }
    }
}

#[test]
fn cells_resolve_as_the_runtime_resolves_them() {
    let map = load("tilesets.tmx", None);
    let resolver = CellResolver::new(&map);
    let lookup = resolver.lookup();
    let main = tile_layer(&map, "Main");
    let blob = layer_blob(&map, main, &lookup, BlobOptions::default())
        .unwrap()
        .unwrap();
    let cells = cells_of(&blob);
    let at = |x: i16, y: i16| {
        cells
            .iter()
            .find(|c| c.0 == x && c.1 == y)
            .map(|c| (c.2, c.3, c.4))
            .unwrap_or_else(|| panic!("no cell at {x},{y}"))
    };
    // test/run_tests.gd's expectations for the same map.
    for (x, y, source, ax, ay) in [
        (0, 0, 0, 0, 0),
        (1, 0, 0, 4, 0),
        (2, 0, 0, 0, 1),
        (3, 0, 0, 0, 2),
        (4, 0, 1, 0, 0),
        (5, 0, 1, 1, 0),
        (6, 0, 2, 0, 0),
        (7, 0, 3, 0, 0),
        (0, 1, 0, 3, 0),
        (4, 1, 1, 2, 0),
        (5, 1, 1, 3, 0),
        (6, 1, 4, 0, 0),
    ] {
        assert_eq!(at(x, y), (source, ax, ay), "cell {x},{y}");
    }
    let r = resolver.resolve(43).unwrap();
    assert_eq!((r.tileset, r.local_id, r.source_id), (2, 2, 4));

    // Every flip combination of gid 2, with transform flags and with alternative ids.
    let flips = tile_layer(&map, "Flips");
    let h = TRANSFORM_FLIP_H;
    let v = TRANSFORM_FLIP_V;
    let t = TRANSFORM_TRANSPOSE;
    let blob = layer_blob(&map, flips, &lookup, BlobOptions::default())
        .unwrap()
        .unwrap();
    let alts: Vec<u16> = cells_of(&blob).iter().map(|c| c.5).collect();
    assert_eq!(alts, vec![0, h, v, t, h | v, h | t, v | t, h | v | t]);
    let blob = layer_blob(
        &map,
        flips,
        &lookup,
        BlobOptions {
            no_alternative_tiles: false,
        },
    )
    .unwrap()
    .unwrap();
    let alts: Vec<u16> = cells_of(&blob).iter().map(|c| c.5).collect();
    assert_eq!(alts, vec![0, 1, 2, 4, 3, 5, 6, 7]);
    assert!(cells_of(&blob).iter().all(|c| (c.2, c.3, c.4) == (0, 1, 0)));
}

#[test]
fn encodings_layer_cells() {
    let map = load("encodings.tmx", None);
    let lookup = CellResolver::new(&map).lookup();
    let mut blobs = tiled_core::cells::layer_blobs(&map, &lookup, BlobOptions::default());
    assert_eq!(blobs.len(), 5);
    let reference = cells_of(blobs.remove(0).1.as_ref().unwrap());
    assert_eq!(reference.len(), 14);
    assert!(reference.contains(&(1, 1, 0, 1, 1, 0)));
    assert!(reference.contains(&(2, 2, 0, 0, 2, 0)));
    assert!(reference.contains(&(3, 3, 0, 1, 0, TRANSFORM_FLIP_H)));
    for (l, b) in blobs {
        if l.compression.as_deref() == Some("zstd") && cfg!(not(feature = "zstd")) {
            assert!(b.is_err());
            continue;
        }
        assert_eq!(cells_of(b.as_ref().unwrap()), reference, "{:?}", l.name);
    }
    if cfg!(not(feature = "zstd")) {
        return;
    }
    let parallel = tiled_core::cells::layer_blobs_parallel(&map, &lookup, BlobOptions::default());
    assert_eq!(parallel.len(), 5);
    assert!(parallel
        .iter()
        .all(|(_, b)| cells_of(b.as_ref().unwrap()) == reference));
}
