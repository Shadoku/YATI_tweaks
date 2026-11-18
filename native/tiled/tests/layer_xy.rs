//! A layer's `x` / `y` attributes (an older Tiled's; Tiled 1.12 offers no way to set them) read as
//! Tiled 1.12 draws them: a tile layer's cells shifted by them (finite and infinite, TMX and JSON),
//! an image layer's from TMX its offset when it has none, every other kind's ignored. Each
//! `layer_xy*` fixture has a `_plain` twin written as Tiled draws it without x/y.

mod common;

use common::*;
use tiled_core::cells::{chunk_blobs, layer_blob, BlobOptions, CellResolver};
use tiled_core::prepare::PreparedCells;
use tiled_core::{Layer, Map};

const PAIRS: [(&str, &str); 3] = [
    ("layer_xy.tmx", "layer_xy_plain.tmx"),
    ("layer_xy.tmj", "layer_xy_plain.tmj"),
    ("layer_xy_infinite.tmx", "layer_xy_infinite_plain.tmx"),
];

fn tile_layers(map: &Map) -> Vec<&Layer> {
    map.all_layers()
        .into_iter()
        .filter(|l| l.kind_or_tile() == "tilelayer")
        .collect()
}

fn layer<'a>(map: &'a Map, name: &str) -> &'a Layer {
    map.all_layers()
        .into_iter()
        .find(|l| l.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("layer {name}"))
}

#[test]
fn cell_shift_is_a_tile_layers_x_and_y_only() {
    let map = load("layer_xy.tmx", None);
    assert_eq!(layer(&map, "Tiles").cell_shift(), (2, 1));
    assert_eq!(layer(&map, "Inner").cell_shift(), (1, 0));
    for name in ["Group", "GroupObjects", "Objects", "ImgOffset", "ImgLegacy"] {
        assert_eq!(layer(&map, name).cell_shift(), (0, 0), "{name}");
    }
}

#[test]
fn tile_layer_blobs_equal_the_plain_twins() {
    for (shifted, plain) in PAIRS {
        let (a, b) = (load(shifted, None), load(plain, None));
        let (ra, rb) = (CellResolver::new(&a), CellResolver::new(&b));
        let (la, lb) = (ra.lookup(), rb.lookup());
        let (ta, tb) = (tile_layers(&a), tile_layers(&b));
        assert_eq!(ta.len(), tb.len(), "{shifted}");
        for (x, y) in ta.iter().zip(tb.iter()) {
            let what = format!("{shifted}: {}", x.name.as_deref().unwrap_or(""));
            let bx = layer_blob(&a, x, &la, BlobOptions::default())
                .unwrap()
                .unwrap();
            let by = layer_blob(&b, y, &lb, BlobOptions::default())
                .unwrap()
                .unwrap();
            assert!(bx.len() > 2, "{what}: no cells");
            assert_eq!(bx, by, "{what}: layer_blob");
            if a.is_infinite() {
                let cx: Vec<_> = chunk_blobs(x, &la, BlobOptions::default())
                    .unwrap()
                    .into_iter()
                    .map(|c| (c.x, c.y, c.blob))
                    .collect();
                let cy: Vec<_> = chunk_blobs(y, &lb, BlobOptions::default())
                    .unwrap()
                    .into_iter()
                    .map(|c| (c.x, c.y, c.blob))
                    .collect();
                assert_eq!(cx, cy, "{what}: chunk_blobs");
            }
        }
        let (pa, pb) = (PreparedCells::prepare(&a), PreparedCells::prepare(&b));
        for i in 0..pa.layer_count() {
            let records = |p: &PreparedCells| p.layer(i).map(|l| l.records().collect::<Vec<_>>());
            assert_eq!(records(&pa), records(&pb), "{shifted}: prepared layer {i}");
        }
    }
}

#[test]
fn an_infinite_maps_extent_takes_the_shift() {
    // Shifted's chunks at (-16, 0) and (0, 0) moved by (2, 3), Still's at (0, 0)
    for name in ["layer_xy_infinite.tmx", "layer_xy_infinite_plain.tmx"] {
        let m = load(name, None);
        assert_eq!(m.tile_rect(), (-14, 0, 32, 19), "{name}");
        assert_eq!(m.map_rect(), (-224.0, 0.0, 512.0, 304.0), "{name}");
    }
}

#[test]
fn a_tmx_image_layers_x_and_y_are_its_offset_when_it_has_none() {
    let offset = |l: &Layer| (l.offsetx.map(|n| n.as_f64()), l.offsety.map(|n| n.as_f64()));
    let tmx = load("layer_xy.tmx", None);
    assert_eq!(offset(layer(&tmx, "ImgLegacy")), (Some(5.0), Some(7.0)));
    assert_eq!(offset(layer(&tmx, "ImgLegacyX")), (Some(9.0), Some(0.0)));
    assert_eq!(offset(layer(&tmx, "ImgOffset")), (Some(4.0), Some(6.0)));
    // An object layer's and a group's x/y stay what the file says (the builders ignore them)
    assert_eq!(offset(layer(&tmx, "Objects")), (Some(4.0), None));
    assert_eq!(offset(layer(&tmx, "Group")), (Some(10.0), Some(20.0)));
    // Tiled's JSON reader ignores them
    let tmj = load("layer_xy.tmj", None);
    assert_eq!(offset(layer(&tmj, "ImgLegacy")), (None, None));
}
