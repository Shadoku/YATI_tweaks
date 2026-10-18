//! World files (`.world`) and map bounds: the same fixtures (`test/fixtures/world/`) and the
//! same values as the GDScript runtime's `test_new_world_*` and `test_new_map_rect_*` tests.

mod common;

use common::*;
use std::io::{Cursor, Write};
use tiled_core::world::{load_world_from_fs, World, WorldLoader, WorldRequest, WorldStatus};
use tiled_core::Map;

fn file_name(p: &str) -> &str {
    p.rsplit('/').next().unwrap_or(p)
}

fn names(world: &World) -> Vec<&str> {
    world.maps.iter().map(|m| file_name(&m.path)).collect()
}

fn rect_of(world: &World, name: &str) -> (i64, i64, i64, i64) {
    world
        .maps
        .iter()
        .find(|m| file_name(&m.path) == name)
        .unwrap_or_else(|| panic!("{name} not in {:?}", names(world)))
        .rect
}

fn listing(dir: &str) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{dir}: {e}"))
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .collect()
}

#[test]
fn explicit_maps_are_parsed_relative_to_the_world_file() {
    let world = World::parse(
        &bytes("world/test.world"),
        "res://test/fixtures/world/test.world",
    )
    .expect("parses");
    assert_eq!(world.maps.len(), 1);
    assert_eq!(world.maps[0].path, "res://test/fixtures/world/hub.tmx");
    assert_eq!(world.maps[0].rect, (-160, 0, 160, 160));
    assert_eq!(world.patterns.len(), 1);
    let p = &world.patterns[0];
    assert_eq!(p.regexp, r"area_(\d+)_(\d+)\.tm[xj]");
    assert_eq!((p.multiplier_x, p.multiplier_y), (160, 160));
    assert_eq!((p.offset_x, p.offset_y), (16, -32));
    assert_eq!((p.map_width, p.map_height), (None, None));
    assert!(world.errors.is_empty());
}

#[test]
fn patterns_list_the_matching_maps_with_their_own_size() {
    let world = load_world_from_fs(&fixture("world/test.world")).expect("loads");
    // explicit first, then pattern matches by file name; area_5_5.tmx.bak is no map file
    assert_eq!(
        names(&world),
        ["hub.tmx", "area_0_0.tmx", "area_0_1.tmj", "area_1_0.tmx"]
    );
    assert_eq!(rect_of(&world, "hub.tmx"), (-160, 0, 160, 160));
    assert_eq!(rect_of(&world, "area_0_0.tmx"), (16, -32, 160, 160));
    assert_eq!(rect_of(&world, "area_1_0.tmx"), (176, -32, 160, 128));
    assert_eq!(rect_of(&world, "area_0_1.tmj"), (16, 128, 192, 160));
    assert!(world.errors.is_empty(), "{:?}", world.errors);
}

#[test]
fn the_world_loader_asks_for_the_file_the_listing_and_the_headers() {
    let world_path = "res://test/fixtures/world/test.world";
    let root = fixtures().join("../..");
    let fs = |p: &str| root.join(p.strip_prefix("res://").unwrap_or(p));
    let mut loader = WorldLoader::new(world_path);
    let mut rounds: Vec<Vec<WorldRequest>> = Vec::new();
    let world = loop {
        match loader.step() {
            WorldStatus::NeedFiles => {
                let needs = loader.needs().to_vec();
                assert!(!needs.is_empty());
                for r in &needs {
                    match r {
                        WorldRequest::File(p) => match std::fs::read(fs(p)) {
                            Ok(b) => loader.supply(p, b),
                            Err(_) => loader.missing(p),
                        },
                        WorldRequest::ListDir(d) => {
                            loader.supply_listing(d, listing(&fs(d).to_string_lossy()))
                        }
                    }
                }
                assert!(loader.needs().is_empty());
                rounds.push(needs);
            }
            WorldStatus::Done(w) => break w,
            WorldStatus::Failed(e) => panic!("{e:?}"),
        }
    };
    let dir = "res://test/fixtures/world";
    assert_eq!(
        rounds,
        vec![
            vec![WorldRequest::File(world_path.into())],
            vec![WorldRequest::ListDir(dir.into())],
            vec![
                WorldRequest::File(format!("{dir}/area_0_0.tmx")),
                WorldRequest::File(format!("{dir}/area_0_1.tmj")),
                WorldRequest::File(format!("{dir}/area_1_0.tmx")),
            ],
        ]
    );
    assert_eq!(world.maps[3].path, format!("{dir}/area_1_0.tmx"));
    assert_eq!(world.maps[3].rect, (176, -32, 160, 128));
}

#[test]
fn a_world_in_a_zip_archive() {
    let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for f in [
        "test.world",
        "hub.tmx",
        "area_0_0.tmx",
        "area_1_0.tmx",
        "area_0_1.tmj",
        "area_5_5.tmx.bak",
    ] {
        zw.start_file(format!("w/{f}"), opts).unwrap();
        zw.write_all(&bytes(&format!("world/{f}"))).unwrap();
    }
    let zip_bytes = zw.finish().unwrap().into_inner();
    let world = WorldLoader::from_zip(zip_bytes, "w/test.world")
        .run(
            |_| panic!("asked for a file"),
            |_| panic!("asked for a listing"),
        )
        .expect("loads");
    let paths: Vec<&str> = world.maps.iter().map(|m| m.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "w/hub.tmx",
            "w/area_0_0.tmx",
            "w/area_0_1.tmj",
            "w/area_1_0.tmx"
        ]
    );
    assert_eq!(world.maps[3].rect, (176, -32, 160, 128));
}

#[test]
fn pattern_sizes_bad_regexps_and_unreadable_maps() {
    let json = br#"{
        "patterns": [
            {"regexp": "(", "multiplierX": 1, "multiplierY": 1},
            {"regexp": "fixed_(\\d+)_(\\d+)", "multiplierX": 64, "multiplierY": 32, "mapWidth": 64, "mapHeight": 32},
            {"regexp": "lost_(\\d+)\\.tmx", "multiplierX": 100}
        ],
        "type": "world"
    }"#;
    let mut loader = WorldLoader::new("maps/w.world");
    let mut asked = Vec::new();
    let world = loop {
        match loader.step() {
            WorldStatus::NeedFiles => {
                for r in loader.needs().to_vec() {
                    asked.push(r.clone());
                    match r {
                        WorldRequest::File(p) if p == "maps/w.world" => {
                            loader.supply(&p, json.to_vec())
                        }
                        WorldRequest::File(p) => loader.missing(&p),
                        WorldRequest::ListDir(d) => loader.supply_listing(
                            &d,
                            vec![
                                "lost_3.tmx".into(),
                                "fixed_2_5.tmj".into(),
                                "fixed_1_1.png".into(),
                            ],
                        ),
                    }
                }
            }
            WorldStatus::Done(w) => break w,
            WorldStatus::Failed(e) => panic!("{e:?}"),
        }
    };
    // the pattern's own size needs no header; the lost map is asked for, then skipped
    assert!(!asked.contains(&WorldRequest::File("maps/fixed_2_5.tmj".into())));
    assert!(asked.contains(&WorldRequest::File("maps/lost_3.tmx".into())));
    assert_eq!(world.maps.len(), 1);
    assert_eq!(world.maps[0].path, "maps/fixed_2_5.tmj");
    assert_eq!(world.maps[0].rect, (128, 160, 64, 32));
    let messages: Vec<&str> = world.errors.iter().map(|e| e.message.as_str()).collect();
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(messages[0].contains("pattern '(' is not a valid regular expression"));
    assert!(messages[1].contains("'maps/lost_3.tmx'") && messages[1].contains("could not be read"));
}

#[test]
fn a_missing_or_broken_world_file_fails() {
    assert!(WorldLoader::new("nowhere.world")
        .run(|_| None, |_| None)
        .is_err());
    assert!(World::parse(b"[1, 2]", "x.world").is_err());
    assert!(World::parse(b"not json", "x.world").is_err());
}

fn map(name: &str) -> Map {
    Map::parse(&bytes(name), &fixture(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// The GDScript runtime's `map_rect` metadata for these fixtures (computed by hand from the
/// fixtures' headers and chunks, and asserted by `test_new_map_rect_finite` and
/// `test_new_map_rect_infinite` in `test/run_tests.gd`, which pass against the runtime).
#[test]
fn map_rect_matches_the_gdscript_runtime() {
    for (name, rect, tiles) in [
        ("objects.tmx", (0.0, 0.0, 400.0, 320.0), (0, 0, 25, 20)),
        ("isometric.tmx", (0.0, 0.0, 96.0, 48.0), (0, 0, 3, 3)),
        ("staggered.tmx", (0.0, 0.0, 144.0, 40.0), (0, 0, 4, 4)),
        ("hexagonal.tmx", (0.0, 0.0, 40.0, 56.0), (0, 0, 3, 3)),
        ("single_layer.tmx", (0.0, 0.0, 32.0, 32.0), (0, 0, 2, 2)),
        (
            "infinite.tmx",
            (-256.0, 0.0, 512.0, 512.0),
            (-16, 0, 32, 32),
        ),
        (
            "infinite.tmj",
            (-256.0, 0.0, 512.0, 512.0),
            (-16, 0, 32, 32),
        ),
        (
            "world/area_1_0.tmx",
            (0.0, 0.0, 160.0, 128.0),
            (0, 0, 10, 8),
        ),
    ] {
        let m = map(name);
        assert_eq!(m.map_rect(), rect, "{name}");
        assert_eq!(m.tile_rect(), tiles, "{name}");
    }
}

#[test]
fn isometric_pixel_rect_of_a_tile_rect() {
    // An infinite isometric map's chunk union (x0, y0, w, h) = (-2, 1, 4, 3) on a map of height 5,
    // tiles 32 x 16: x = (-2 - 1 - 3 + 5) * 16, y = (-2 + 1) * 8, size (4 + 3) * (16, 8).
    let m = map("isometric.tmx");
    let mut m5 = m.clone();
    m5.height = Some(5);
    assert_eq!(m5.pixel_rect((-2, 1, 4, 3)), (-16.0, -8.0, 112.0, 56.0));
}
