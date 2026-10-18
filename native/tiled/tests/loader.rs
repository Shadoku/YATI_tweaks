//! The loader: the file-request protocol, path resolution, errors, zip archives, the template
//! and custom type merges, image references, and one model from both formats.

mod common;

use common::*;
use tiled_core::{ErrorKind, Loader, Map, Payload, PropertyValue, Status, TilesetSource, Value};

/// Pumps a loader, recording the paths of each round; `res://` mapped to the repository root.
fn pump(mut loader: Loader) -> (Result<Map, Vec<tiled_core::Error>>, Vec<Vec<String>>) {
    let root = fixtures().join("../..");
    let mut rounds = Vec::new();
    loop {
        match loader.step() {
            Status::NeedFiles => {
                let needs = loader.needs().to_vec();
                assert!(!needs.is_empty(), "NeedFiles without needs");
                for p in &needs {
                    let fs = match p.strip_prefix("res://") {
                        Some(rest) => root.join(rest),
                        None => p.into(),
                    };
                    match std::fs::read(fs) {
                        Ok(b) => loader.supply(p, b),
                        Err(_) => loader.missing(p),
                    }
                }
                assert!(loader.needs().is_empty());
                rounds.push(needs);
            }
            Status::Done(m) => return (Ok(*m), rounds),
            Status::Failed(e) => return (Err(e), rounds),
        }
    }
}

#[test]
fn files_are_requested_in_rounds_relative_to_their_referrer() {
    let (map, rounds) = pump(Loader::new(
        "res://test/fixtures/../fixtures/./classes.tmx",
        None,
    ));
    let map = map.unwrap();
    assert_eq!(
        rounds,
        vec![
            vec!["res://test/fixtures/../fixtures/./classes.tmx".to_string()],
            vec!["res://test/fixtures/crate.tx".to_string()],
            vec!["res://test/fixtures/terrain.tsx".to_string()],
        ]
    );
    assert!(map.errors().is_empty(), "{:?}", map.errors());
    let t = map.template("crate.tx").unwrap();
    assert_eq!(t.path(), "res://test/fixtures/crate.tx");
    match &t.tileset_slots()[0].source {
        TilesetSource::External { path, tileset } => {
            assert_eq!(path, "res://test/fixtures/terrain.tsx");
            assert_eq!(tileset.as_ref().unwrap().name.as_deref(), Some("terrain"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_tileset_shared_by_map_and_template_is_read_once() {
    let (map, rounds) = pump(Loader::new(
        "res://test/fixtures/objects.tmx",
        Some("res://test/fixtures/custom.tiled-project"),
    ));
    let map = map.unwrap();
    assert_eq!(rounds.len(), 2);
    assert_eq!(rounds[0].len(), 2, "map and project in the first round");
    let mut second = rounds[1].clone();
    second.sort();
    assert_eq!(
        second,
        vec![
            "res://test/fixtures/enemy.tx",
            "res://test/fixtures/terrain.tsx"
        ]
    );
    assert!(map.project().is_some());
}

#[test]
fn a_missing_tileset_is_an_error_but_not_fatal() {
    let map = load("missing_tileset.tmx", None);
    assert_eq!(map.errors().len(), 1);
    assert_eq!(map.errors()[0].kind, ErrorKind::NotFound);
    assert_eq!(
        map.errors()[0].message,
        "ERROR: Tileset file 'does_not_exist.tsx' not found. -> Continuing but result may be unusable"
    );
    assert!(map.tileset(0).is_none());
    let lookup = tiled_core::cells::CellResolver::new(&map).lookup();
    for (_, blob) in tiled_core::cells::layer_blobs(&map, &lookup, Default::default()) {
        assert_eq!(blob.unwrap().len(), 2, "no cell resolves");
    }
}

#[test]
fn fatal_errors() {
    let (r, _) = pump(Loader::new("res://test/fixtures/nope.tmx", None));
    let e = r.unwrap_err();
    assert_eq!(
        e[0].message,
        "ERROR: Tiled map file 'res://test/fixtures/nope.tmx' not found."
    );

    let (r, _) = pump(Loader::new(
        "res://test/fixtures/objects.tmx",
        Some("res://nope.tiled-project"),
    ));
    assert_eq!(
        r.unwrap_err()[0].message,
        "ERROR: Tiled project file 'res://nope.tiled-project' not found."
    );

    let mut l = Loader::new("broken.tmx", None);
    assert!(matches!(l.step(), Status::NeedFiles));
    l.supply(
        "broken.tmx",
        b"<?xml version=\"1.0\"?>\n<map>\n <layer>\n</map>".to_vec(),
    );
    match l.step() {
        Status::Failed(e) => {
            assert_eq!(e[0].kind, ErrorKind::Parse);
            assert!(
                e[0].message
                    .starts_with("Error parsing file 'broken.tmx' (around line"),
                "{}",
                e[0].message
            );
        }
        other => panic!("{other:?}"),
    }
    assert!(
        matches!(l.step(), Status::Failed(_)),
        "a finished loader stays finished"
    );

    let mut l = Loader::new("x.bin", None);
    l.step();
    l.supply("x.bin", b"\x00\x01".to_vec());
    match l.step() {
        Status::Failed(e) => assert_eq!(e[0].kind, ErrorKind::UnknownType),
        other => panic!("{other:?}"),
    }
}

#[test]
fn zip_archives_are_read_directly() {
    let map = Loader::from_zip(bytes("bundle.zip"), "maps/zipmap.tmx", None)
        .run(|p| panic!("asked for {p}"))
        .unwrap();
    assert!(map.errors().is_empty());
    assert_eq!(map.images(), &["maps/zip_tiles.png".to_string()]);
    match &map.tileset_slots()[0].source {
        TilesetSource::External { path, tileset } => {
            assert_eq!(path, "maps/zip_tiles.tsx");
            assert!(tileset.is_some());
        }
        other => panic!("{other:?}"),
    }

    let missing = Loader::from_zip(bytes("bundle.zip"), "maps/none.tmx", None).run(|_| None);
    assert_eq!(missing.unwrap_err()[0].kind, ErrorKind::NotFound);
    let bad = Loader::from_zip(b"not a zip".to_vec(), "maps/zipmap.tmx", None).run(|_| None);
    assert_eq!(bad.unwrap_err()[0].kind, ErrorKind::Zip);
    // A project missing inside an archive is an error, not fatal (as in the runtime).
    let m = Loader::from_zip(
        bytes("bundle.zip"),
        "maps/zipmap.tmx",
        Some("none.tiled-project"),
    )
    .run(|_| None)
    .unwrap();
    assert_eq!(m.errors().len(), 1);
}

fn object(map: &Map, id: i64) -> &tiled_core::Object {
    map.all_layers()
        .into_iter()
        .flat_map(|l| l.objects.iter().flatten())
        .find(|o| o.id == Some(id))
        .unwrap_or_else(|| panic!("object {id}"))
}

fn prop<'a>(o: &'a tiled_core::Object, name: &str) -> Option<&'a PropertyValue> {
    o.properties
        .iter()
        .flatten()
        .find(|p| p.name.as_deref() == Some(name))
        .and_then(|p| p.value.as_ref())
}

#[test]
fn templates_merge_into_their_instances() {
    let map = load("objects.tmx", None);
    let t1 = map.instantiate(object(&map, 25));
    assert_eq!(t1.objects.len(), 1);
    let o = &t1.objects[0];
    assert_eq!(
        (o.id, o.name.as_deref(), o.gid),
        (Some(25), Some("T1"), Some(3))
    );
    assert_eq!(
        (o.x.unwrap().as_f64(), o.y.unwrap().as_f64()),
        (32.0, 240.0)
    );
    assert_eq!(prop(o, "hp").unwrap().as_i64(), Some(10));
    assert_eq!(prop(o, "faction").unwrap().as_str(), Some("orcs"));
    // The template's tileset resolves the merged object's gid.
    let tpl = t1.template.unwrap();
    assert_eq!(
        tpl.slot_tileset(&tpl.tileset_slots()[0])
            .unwrap()
            .name
            .as_deref(),
        Some("terrain")
    );

    let t2 = map.instantiate(object(&map, 26));
    assert_eq!(
        prop(&t2.objects[0], "hp").unwrap().as_i64(),
        Some(99),
        "instance overrides"
    );
    let t3 = map.instantiate(object(&map, 27));
    assert_eq!(
        t3.objects[0].name.as_deref(),
        Some("Enemy"),
        "no instance name: the template's"
    );

    let plain = map.instantiate(object(&map, 1));
    assert!(plain.template.is_none());
    assert_eq!(plain.objects[0], *object(&map, 1));
}

#[test]
fn template_instances_get_custom_types_after_the_merge() {
    let map = load("classes.tmx", Some("classes.tiled-project"));
    let big = map.instantiate(object(&map, 11));
    let o = &big.objects[0];
    let names: Vec<&str> = o
        .properties
        .iter()
        .flatten()
        .filter_map(|p| p.name.as_deref())
        .collect();
    // template's (faction, hp overridden), instance's extra, then the Crate class's members
    // not yet there (both definitions of Crate, in order).
    assert_eq!(
        names,
        vec!["faction", "hp", "extra", "breakable", "loot", "weight"]
    );
    assert_eq!(prop(o, "hp").unwrap().as_i64(), Some(50));
    assert_eq!(prop(o, "faction").unwrap().as_str(), Some("elves"));
    assert_eq!(o.opacity.unwrap().as_f64(), 0.5);
    assert_eq!(o.kind.as_deref(), Some("Crate"));
    assert_eq!(o.gid, Some(11));
}

#[test]
fn custom_types_merge_into_the_model() {
    let without = load("objects.tmx", None);
    assert!(object(&without, 28).properties.is_none());
    let with = load("objects.tmx", Some("custom.tiled-project"));
    let enemy = object(&with, 28);
    assert_eq!(prop(enemy, "hp").unwrap().as_i64(), Some(100));
    assert_eq!(prop(enemy, "speed").unwrap().as_f64(), Some(1.5));
    // Off: the project is there but nothing merged.
    let off = Loader::new(
        &fixture("objects.tmx"),
        Some(&fixture("custom.tiled-project")),
    )
    .merge_custom_types(false)
    .run(|p| std::fs::read(p).ok())
    .unwrap();
    assert!(object(&off, 28).properties.is_none());
    assert!(off.project().unwrap().members("Enemy", "object").is_some());

    let classes = load("classes.tmx", Some("classes.tiled-project"));
    let level = classes
        .properties
        .iter()
        .flatten()
        .find(|p| p.name.as_deref() == Some("level"));
    assert_eq!(
        level.unwrap().value.as_ref().unwrap().as_i64(),
        Some(3),
        "the map's own value stays"
    );
    assert!(classes
        .properties
        .iter()
        .flatten()
        .any(|p| p.name.as_deref() == Some("ambient")));
}

#[test]
fn images_are_collected_resolved() {
    let map = load("tilesets.tmx", None);
    let names: Vec<String> = map
        .images()
        .iter()
        .map(|p| p.rsplit('/').next().unwrap().to_string())
        .collect();
    assert_eq!(
        names,
        vec![
            "tiles.png",
            "tiles_ms.png",
            "small.png",
            "big.png",
            "obj.png"
        ]
    );
    assert!(
        map.images().iter().all(|p| !p.contains("..")),
        "{:?}",
        map.images()
    );
    let map = load("infinite.tmj", None);
    let names: Vec<&str> = map
        .images()
        .iter()
        .map(|p| p.rsplit('/').next().unwrap())
        .collect();
    assert_eq!(names, vec!["tiles.png", "bg.png"]);
}

/// Floats for every number: how the two formats' dictionaries compare once typed.
fn numbers(v: &Value) -> Value {
    v.floatify()
}

#[test]
fn both_formats_give_the_same_model() {
    let mut j = load("json_map.tmj", None);
    let mut x = load("json_twin.tmx", None);
    // What differs is only how the payload is stored (an array vs csv text, encoding implied).
    for m in [&mut j, &mut x] {
        for l in m.layers.iter_mut().flatten() {
            if let Some(cells) = l.decode() {
                l.data = Some(Payload::Gids(cells.unwrap().into_owned()));
                l.encoding = None;
            }
        }
    }
    assert_eq!(j.tilesets, x.tilesets);
    assert_eq!(j.layers, x.layers);
    assert_eq!(j, x);

    // The class-typed property's dictionary, from each format.
    let class_of = |m: &Map| {
        let o = object(m, 1);
        let p = o
            .properties
            .iter()
            .flatten()
            .find(|p| p.name.as_deref() == Some("stats"))
            .unwrap();
        o.to_value(m.format() == tiled_core::Format::Json)
            .get("properties")
            .and_then(|ps| ps.at(2))
            .and_then(|p| p.get("value"))
            .cloned()
            .map(|v| (v, p.value.clone()))
            .unwrap()
    };
    let (jv, jm) = class_of(&j);
    let (xv, xm) = class_of(&x);
    assert_eq!(jm, xm);
    assert_eq!(numbers(&jv), numbers(&xv));
    assert_eq!(xv.get("hp"), Some(&Value::Int(5)));
    assert_eq!(jv.get("hp"), Some(&Value::Float(5.0)));
    // The same against the class property in objects.tmx (another value, same shape).
    let o = load("objects.tmx", None);
    let p = object(&o, 24)
        .properties
        .iter()
        .flatten()
        .find(|p| p.name.as_deref() == Some("p_class"))
        .unwrap();
    match p.value.as_ref().unwrap() {
        PropertyValue::Class(m) => {
            assert_eq!(m.get("hp").unwrap().as_i64(), Some(7));
            assert_eq!(m.get("label").unwrap().as_str(), Some("boss"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn class_values_nest_lists_and_classes() {
    let map = load("classes.tmx", None);
    let loot = map
        .properties
        .iter()
        .flatten()
        .find(|p| p.name.as_deref() == Some("loot"))
        .unwrap();
    let PropertyValue::Class(m) = loot.value.as_ref().unwrap() else {
        panic!()
    };
    let PropertyValue::Class(stats) = &m["stats"] else {
        panic!()
    };
    assert_eq!(stats["label"].as_str(), Some("multi\nline"));
    let PropertyValue::Array(tags) = &m["tags"] else {
        panic!()
    };
    assert_eq!(tags.len(), 3);
    assert_eq!(tags[1].as_f64(), Some(2.0));
    let drops = map
        .properties
        .iter()
        .flatten()
        .find(|p| p.name.as_deref() == Some("drops"))
        .unwrap();
    let PropertyValue::List(items) = drops.value.as_ref().unwrap() else {
        panic!()
    };
    assert_eq!(items.len(), 5);
    assert_eq!(items[3].kind.as_deref(), Some("class"));
    let notes = map
        .properties
        .iter()
        .flatten()
        .find(|p| p.name.as_deref() == Some("notes"))
        .unwrap();
    assert_eq!(
        notes.value.as_ref().unwrap().as_str(),
        Some("first line\nsecond line & more")
    );
}
