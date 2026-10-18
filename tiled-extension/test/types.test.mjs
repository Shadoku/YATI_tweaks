// lib/types.mjs: the generated types as data, compared with expected .tiled-project fragments.
import { test } from "node:test";
import assert from "node:assert/strict";
import path from "node:path";
import {
	syncDefinitions, sceneClassDefinitions, toPropertyTypesJson, memberForExport, stableColor,
	tiledColor, isNodeClass, isResourceClass, MAPPING_TABLE_CLASSES, tiledJsonText,
} from "../yati-godot/lib/types.mjs";
import { readGodotProject } from "../yati-godot/lib/godot_project.mjs";
import { scanScenes, indexClassNames, walkFiles, parseGdScript } from "../yati-godot/lib/scene_exports.mjs";
import { io, GODOT_FIXTURE, FIXTURES, expectedJson } from "./helpers/node_io.mjs";

const TILED_DIR = path.join(FIXTURES, "tiled");	// where a .tiled-project next to the fixture would be

function fixtureScan() {
	const scripts = walkFiles(GODOT_FIXTURE, io.listDir, (n) => n.endsWith(".gd"), null).map((p) => ({ path: p, text: io.readText(p) }));
	const classIndex = indexClassNames(scripts);
	return { classIndex: classIndex, scan: scanScenes(GODOT_FIXTURE, GODOT_FIXTURE, io, classIndex) };
}

test("the mapping table is the importers' (GODOT_TYPE_MAP without \"\")", () => {
	assert.deepEqual(MAPPING_TABLE_CLASSES, [
		"collision", "staticbody", "characterbody", "rigidbody", "animatablebody", "area",
		"navigation", "occluder", "line", "path", "polygon", "instance", "parallax",
	]);
});

test("sync: layer enums, GodotNodeType, MapRef equal the expected fragment", () => {
	const sync = syncDefinitions(readGodotProject(io.readText(path.join(GODOT_FIXTURE, "project.godot"))));
	const json = toPropertyTypesJson(sync.defs);
	assert.deepEqual(json, expectedJson("sync_types.json", json));
	assert.deepEqual(sync.skipped, []);
	// A project without names: no layer enums, and why.
	const bare = syncDefinitions(readGodotProject("[application]\nconfig/name=\"x\"\n"));
	assert.deepEqual(bare.defs.map((d) => d.name), ["GodotNodeType", "MapRef"]);
	assert.equal(bare.skipped.length, 3);
});

test("scenes: classes, enums and vectors equal the expected fragment", () => {
	const { classIndex, scan } = fixtureScan();
	const built = sceneClassDefinitions(scan, {
		godotProjectDir: GODOT_FIXTURE, tiledProjectDir: TILED_DIR, classIndex: classIndex,
		knownEnums: { GodotPhysicsLayers: true },
	});
	const json = toPropertyTypesJson(built.defs);
	assert.deepEqual(json, expectedJson("scene_types.json", json));
	assert.deepEqual(built.classes.map((c) => c.name), ["AreaScene", "Boss", "Door", "Enemy"]);
	assert.deepEqual(built.warnings, [
		"res://scenes/area.tscn: 'Area' is a class the importer or the extension already gives a meaning; the class is named AreaScene",
		"res://scenes/boss.tscn: class_name Enemy is the root script of 2 scenes; the class is named after the file, Boss",
		"res://scenes/enemy.tscn: class_name Enemy is the root script of 2 scenes; the class is named after the file, Enemy",
		"res://scenes/builtin.tscn: skipped, its root's script is built into the scene",
		"res://scenes/decor.tscn: skipped, its root has no script",
	]);
	const enemy = built.classes.find((c) => c.name === "Enemy");
	assert.deepEqual(enemy.skipped.map((s) => s.name), ["extra", "counts", "godot_scene"]);
	assert.deepEqual(enemy.notes.map((n) => n.split(":")[0]), ["drops", "path_points"]);
});

// One export (a line of GDScript) -> its member, as the README's table says.
function member(line, extra) {
	const s = parseGdScript("class_name Probe\nextends Node2D\nenum Mood { CALM, ANGRY = 5 }\n" + line + "\n");
	const resolved = { enums: s.enums, enumOwners: { Mood: "Probe" } };
	const ctx = Object.assign({
		godotProjectDir: "/g", tiledProjectDir: "/g/tiled", classIndex: {}, knownEnums: {}, className: "Probe",
	}, extra || {});
	const ex = s.exports[0];
	ex.owner = "Probe";
	const r = memberForExport(ex, resolved, ctx, "/g/scripts/probe.gd");
	return r.skip !== undefined ? "skip: " + r.skip : r.member;
}

test("export forms -> member types (the README's table)", () => {
	assert.deepEqual(member("@export var a: int = 4"), { name: "a", type: "int", value: 4 });
	assert.deepEqual(member("@export var a: float"), { name: "a", type: "float", value: 0 });
	assert.deepEqual(member("@export var a := 1.5"), { name: "a", type: "float", value: 1.5 });
	assert.deepEqual(member("@export var a = true"), { name: "a", type: "bool", value: true });
	assert.deepEqual(member("@export var a: StringName = &\"x\""), { name: "a", type: "string", value: "x" });
	assert.deepEqual(member("@export_multiline var a: String = \"l1\\nl2\""), { name: "a", type: "string", value: "l1\nl2" });
	assert.deepEqual(member("@export_placeholder(\"hint\") var a: String"), { name: "a", type: "string", value: "" });
	assert.deepEqual(member("@export_dir var a: String = \"res://d\""), { name: "a", type: "string", value: "res://d" });
	assert.deepEqual(member("@export var a: Color = Color(0, 0, 1, 0.5)"), { name: "a", type: "color", value: "#800000ff" });
	assert.deepEqual(member("@export_color_no_alpha var a: Color = Color.WHITE"), { name: "a", type: "color", value: "#ffffffff" });
	assert.deepEqual(member("@export var a: Color = some_color()"), { name: "a", type: "color", value: "" });
	assert.deepEqual(member("@export var a: NodePath"), { name: "a", type: "object", value: 0 });
	assert.deepEqual(member("@export var a: Node2D"), { name: "a", type: "object", value: 0 });
	assert.deepEqual(member("@export_node_path(\"Marker2D\") var a: NodePath"), { name: "a", type: "object", value: 0 });
	assert.deepEqual(member("@export var a: Texture2D = preload(\"res://art/a.png\")"), { name: "a", type: "file", value: "../art/a.png" });
	assert.deepEqual(member("@export var a: PackedScene"), { name: "a", type: "file", value: "" });
	assert.deepEqual(member("@export_file(\"*.json\") var a: String = \"res://data/a.json\""), { name: "a", type: "file", value: "../data/a.json" });
	assert.deepEqual(member("@export_file var a: String = \"user://save.json\""), { name: "a", type: "file", value: "" });
	assert.deepEqual(member("@export var a: Mood = Mood.ANGRY"), { name: "a", type: "string", value: "ANGRY", propertyType: "Probe.Mood" });
	assert.deepEqual(member("@export var a: Mood = 5"), { name: "a", type: "string", value: "ANGRY", propertyType: "Probe.Mood" });
	assert.deepEqual(member("@export_enum(\"A\", \"B:7\") var a: int = 7"), { name: "a", type: "string", value: "B", propertyType: "Probe.a" });
	assert.deepEqual(member("@export_enum(\"A,B\") var a: String"), { name: "a", type: "string", value: "A", propertyType: "Probe.a" });
	assert.deepEqual(member("@export var a: Vector2 = Vector2(1, 2.5)"), { name: "a", type: "class", value: { x: 1, y: 2.5 }, propertyType: "Vector2" });
	assert.deepEqual(member("@export var a: Vector2i"), { name: "a", type: "class", value: { x: 0, y: 0 }, propertyType: "Vector2i" });
	assert.deepEqual(member("@export_flags_2d_physics var a: int = 3", { knownEnums: { GodotPhysicsLayers: true } }), { name: "a", type: "int", value: 3, propertyType: "GodotPhysicsLayers" });
	assert.deepEqual(member("@export_flags_2d_physics var a: int = 3"), { name: "a", type: "int", value: 3 });
	assert.deepEqual(member("@export_flags(\"A\", \"B\") var a: int = 2"), { name: "a", type: "int", value: 2 });
	assert.deepEqual(member("@export var a: Array[int] = [1]"), { name: "a", type: "string", value: "" });
	assert.deepEqual(member("@export var a: PackedStringArray"), { name: "a", type: "string", value: "" });
	assert.equal(member("@export var a: Dictionary = {}"), "skip: a Dictionary has no Tiled member type");
	assert.equal(member("@export var a"), "skip: untyped without a default to infer the type from");
	assert.equal(member("@export var a: Rect2"), "skip: type Rect2 is neither a known Node nor a Resource class");
	assert.equal(member("@export var a: Callable"), "skip: type Callable is neither a known Node nor a Resource class");
	// A script class is judged by the engine class it extends.
	const classIndex = { Arena: { path: "/g/arena.gd", script: { extendsName: "StaticBody2D", extendsPath: "", enums: {} } },
		Loot: { path: "/g/loot.gd", script: { extendsName: "Resource", extendsPath: "", enums: { Kind: [{ name: "A", value: 0 }] } } } };
	assert.deepEqual(member("@export var a: Arena", { classIndex: classIndex }), { name: "a", type: "object", value: 0 });
	assert.deepEqual(member("@export var a: Loot", { classIndex: classIndex }), { name: "a", type: "file", value: "" });
	assert.deepEqual(member("@export var a: Loot.Kind", { classIndex: classIndex }), { name: "a", type: "string", value: "A", propertyType: "Loot.Kind" });
});

test("node and resource classes", () => {
	for (const n of ["Node2D", "Area2D", "Sprite2D", "Control", "Label", "AudioStreamPlayer2D", "TileMapLayer", "MyThing2D"])
		assert.ok(isNodeClass(n), n);
	for (const n of ["Texture2D", "PackedScene", "RectangleShape2D", "Curve2D", "AudioStream", "ShaderMaterial", "NavigationPolygon", "MyMaterial"])
		assert.ok(isResourceClass(n) && !isNodeClass(n), n);
});

test("colours: stable per name, Tiled's #AARRGGBB", () => {
	assert.equal(stableColor("Enemy"), stableColor("Enemy"));
	assert.notEqual(stableColor("Enemy"), stableColor("Door"));
	assert.match(stableColor("Enemy"), /^#ff[0-9a-f]{6}$/);
	assert.equal(tiledColor({ r: 1, g: 0.5, b: 0, a: 1 }), "#ffff8000");
	assert.equal(tiledColor({ r: 0, g: 0, b: 0, a: 0 }), "#00000000");
});

test("Tiled's JSON text: sorted keys, four spaces, a final newline", () => {
	assert.equal(tiledJsonText({ b: [], a: { d: 1, c: "x" } }), "{\n    \"a\": {\n        \"c\": \"x\",\n        \"d\": 1\n    },\n    \"b\": []\n}\n");
});
