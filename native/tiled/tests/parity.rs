//! Parity of the model with the GDScript runtime: `to_value()` of every fixture map, tileset
//! and template, loaded through the [`Loader`], against `DictionaryBuilder`'s dictionary of the
//! same file (`test/fixtures/expected/*.json`, written by `test/dump_dictionary.gd`).

mod common;

use common::*;
use tiled_core::{Loader, Map, Template, Tileset, TilesetSource};

fn map_parity(name: &str, project: Option<&str>, dump: &str) -> Map {
    let map = load(name, project);
    assert_parity(name, &map.to_value(), &expected(dump));
    map
}

#[test]
fn every_map_matches_the_runtime_dictionary() {
    for name in [
        "background.tmx",
        "classes.tmx",
        "encodings.tmx",
        "hexagonal.tmx",
        "infinite.tmx",
        "infinite.tmj",
        "isometric.tmx",
        "json_map.tmj",
        "json_twin.tmx",
        "layers.tmx",
        "legacy_xml.tmx",
        "missing_tileset.tmx",
        "objects.tmx",
        "staggered.tmx",
        "tilesets.tmx",
        // The runtime improvement plan's part A fixtures
        "animations.tmx",
        "area_gravity.tmx",
        "clip.tmj",
        "clip.tmx",
        "colors.tmx",
        "layer_class.tmx",
        "parallax.tmx",
        "single_layer.tmx",
        "template_files.tmx",
        "templates.tmx",
        "unknown_class.tmx",
        // Its part B fixtures (object references, scenes, masks, the world's maps)
        "object_refs.tmx",
        "scenes.tmx",
        "masks.tmx",
        "world/hub.tmx",
        "world/area_0_0.tmx",
        "world/area_0_1.tmj",
        "world/area_1_0.tmx",
    ] {
        map_parity(name, None, &format!("{}.json", name.replace('/', "_")));
    }
}

#[test]
fn maps_with_custom_types_match() {
    map_parity(
        "objects.tmx",
        Some("custom.tiled-project"),
        "objects.tmx+custom.json",
    );
    map_parity(
        "classes.tmx",
        Some("classes.tiled-project"),
        "classes.tmx+classes.json",
    );
    map_parity(
        "scenes.tmx",
        Some("scenes.tiled-project"),
        "scenes.tmx+scenes.json",
    );
    map_parity(
        "masks.tmx",
        Some("masks.tiled-project"),
        "masks.tmx+masks.json",
    );
}

#[test]
fn every_tileset_matches() {
    for name in ["terrain.tsx", "iso.tsx", "json_tiles.tsj"] {
        let t = Tileset::parse(&bytes(name), &fixture(name)).unwrap();
        assert_parity(name, &t.to_value(), &expected(&format!("{name}.json")));
    }
}

/// The external tilesets as the loader resolved them for a map, merged with the project.
#[test]
fn loaded_tilesets_match_with_and_without_project() {
    let check = |map: &Map, file: &str, dump: &str| {
        let slot = map
            .tileset_slots()
            .iter()
            .find(|s| matches!(&s.source, TilesetSource::External { path, .. } if path.ends_with(file)))
            .unwrap_or_else(|| panic!("{file} not in the map"));
        let ts = map.slot_tileset(slot).expect("loaded");
        assert_parity(dump, &ts.to_value(), &expected(dump));
    };
    let plain = load("objects.tmx", None);
    check(&plain, "terrain.tsx", "terrain.tsx.json");
    let json = load("infinite.tmj", Some("classes.tiled-project"));
    check(&json, "json_tiles.tsj", "json_tiles.tsj+classes.json");
    check(&json, "terrain.tsx", "terrain.tsx+classes.json");
}

#[test]
fn every_template_matches() {
    for name in [
        "enemy.tx",
        "crate.tx",
        "json_enemy.tj",
        "probe_rect.tx",
        "probe_text.tx",
        "probe_tpl.tx",
        "tpl/door.tx",
    ] {
        let t = Template::parse(&bytes(name), &fixture(name)).unwrap();
        let dump = format!("{}.json", name.replace('/', "_"));
        assert_parity(name, &t.to_value(), &expected(&dump));
    }
    // And as the loader found them.
    let map = load("classes.tmx", None);
    let t = map.template("crate.tx").expect("crate.tx loaded");
    assert_parity(
        "crate.tx (loaded)",
        &t.to_value(),
        &expected("crate.tx.json"),
    );
    let map = load("infinite.tmj", None);
    let t = map.template("json_enemy.tj").expect("json_enemy.tj loaded");
    assert_parity(
        "json_enemy.tj (loaded)",
        &t.to_value(),
        &expected("json_enemy.tj.json"),
    );
}

#[test]
fn zip_members_match() {
    let map = Loader::from_zip(bytes("bundle.zip"), "maps/zipmap.tmx", None)
        .run(|p| panic!("zip mode asked for {p}"))
        .expect("zip map");
    assert_parity(
        "zipmap.tmx",
        &map.to_value(),
        &expected("bundle.zip#maps_zipmap.tmx.json"),
    );
    let ts = map.tileset(0).expect("zip tileset loaded");
    assert_parity(
        "zip_tiles.tsx",
        &ts.to_value(),
        &expected("bundle.zip#maps_zip_tiles.tsx.json"),
    );
}

/// The benchmark map (generated, not committed): run `test/benchmark.gd` once to create
/// `test/out/bench/`, then dump its dictionaries next to it:
/// `godot --headless -s test/dump_dictionary.gd -- test/out/bench/bench.tmx --out=test/out/bench/bench.tmx.json`
/// (and the same for `bench_a.tsx` and `bench_tpl.tx`), then
/// `cargo test --release -- --ignored bench_map`.
#[test]
#[ignore = "needs the generated benchmark map and its dumps in test/out/bench/"]
fn bench_map_matches_the_runtime_dictionary() {
    let dir = fixtures().join("../out/bench");
    let read = |name: &str| -> tiled_core::Value {
        let text =
            std::fs::read_to_string(dir.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        serde_json::from_str(&text).unwrap()
    };
    let path = dir.join("bench.tmx").to_string_lossy().into_owned();
    let map = Loader::new(&path, None)
        .run(|p| std::fs::read(p).ok())
        .unwrap();
    assert!(map.errors().is_empty(), "{:?}", map.errors());
    assert_parity("bench.tmx", &map.to_value(), &read("bench.tmx.json"));
    let ts = map.tileset(0).unwrap();
    assert_parity("bench_a.tsx", &ts.to_value(), &read("bench_a.tsx.json"));
    let t = map.template("bench_tpl.tx").unwrap();
    assert_parity("bench_tpl.tx", &t.to_value(), &read("bench_tpl.tx.json"));
}
