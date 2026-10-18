// lib/scene_exports.mjs: .tscn roots, .gd exports and enums, literals, the scan.
import { test } from "node:test";
import assert from "node:assert/strict";
import path from "node:path";
import {
	parseTscn, parseGdScript, parseLiteral, parseVarDeclaration, takeAnnotations, parseEnum,
	logicalLines, scanScenes, indexClassNames, walkFiles, resolveScript, engineClassOf, pascalCase,
} from "../yati-godot/lib/scene_exports.mjs";
import { io, GODOT_FIXTURE, IMPORTER_FIXTURES } from "./helpers/node_io.mjs";

const read = (...p) => io.readText(path.join(...p));

test("tscn: the root's script, not a child's; ext_resource forms", () => {
	const t = parseTscn(read(GODOT_FIXTURE, "scenes", "enemy.tscn"));
	assert.equal(t.root.name, "Enemy");
	assert.equal(t.root.type, "CharacterBody2D");
	assert.equal(t.rootScript, "res://scripts/enemy.gd");
	assert.equal(t.extResources["1_enemy"].uid, "uid://c1enemyscript");
	// The importer's fixture (format 3, id="1").
	assert.equal(parseTscn(read(IMPORTER_FIXTURES, "enemy.tscn")).rootScript, "res://test/fixtures/enemy.gd");
	// Godot 3's unquoted ids.
	const g3 = parseTscn("[ext_resource path=\"res://a.gd\" type=\"Script\" id=1]\n[node name=\"A\" type=\"Node2D\"]\nscript = ExtResource( 1 )\n");
	assert.equal(g3.rootScript, "res://a.gd");
	const inherited = parseTscn(read(GODOT_FIXTURE, "scenes", "boss.tscn"));
	assert.equal(inherited.rootScript, null);
	assert.equal(inherited.rootInstance, "res://scenes/enemy.tscn");
	assert.equal(parseTscn(read(GODOT_FIXTURE, "scenes", "builtin.tscn")).builtinScript, true);
	assert.equal(parseTscn(read(GODOT_FIXTURE, "scenes", "decor.tscn")).rootScript, null);
	assert.equal(parseTscn("not a scene").root, null);
});

test("gd: logical lines join brackets and backslashes, drop comments", () => {
	const ll = logicalLines("enum E {\n\tA, # one\n\tB\n}\nvar s = \"# not a comment\" # a comment\nvar x = 1 + \\\n\t2\n\tindented()\n");
	assert.equal(ll.length, 4);
	assert.equal(ll[0].text.replace(/\s+/g, " "), "enum E { A, B }");
	assert.equal(ll[1].text, "var s = \"# not a comment\"");
	assert.equal(ll[2].text, "var x = 1 + 2");
	assert.equal(ll[3].text, "indented()");
	assert.equal(ll[3].indented, true);
});

test("gd: annotations, declarations, enums", () => {
	const a = takeAnnotations("@export_range(0, 10, \"or_greater\") @onready var x: int = 1");
	assert.deepEqual(a.annotations, [{ name: "export_range", args: ["0", "10", "\"or_greater\""] }, { name: "onready", args: [] }]);
	assert.equal(a.rest, "var x: int = 1");
	assert.deepEqual(parseVarDeclaration("var lives: int = 3:"), { name: "lives", type: "int", inferred: false, defaultText: "3" });
	assert.deepEqual(parseVarDeclaration("var d := {\"a\": 1}"), { name: "d", type: "", inferred: true, defaultText: "{\"a\": 1}" });
	assert.deepEqual(parseVarDeclaration("var s: int = 4: set = _set_s"), { name: "s", type: "int", inferred: false, defaultText: "4" });
	assert.deepEqual(parseVarDeclaration("var c: Dictionary[String, int] = {}"), { name: "c", type: "Dictionary[String, int]", inferred: false, defaultText: "{}" });
	assert.deepEqual(parseVarDeclaration("var settings: Settings"), { name: "settings", type: "Settings", inferred: false, defaultText: "" });
	assert.deepEqual(parseEnum("enum Mood { CALM, ANGRY = 5, SLEEPY, BIT = 1 << 4, HEX = 0x10 }").values.map((v) => v.value), [0, 5, 6, 16, 16]);
	assert.equal(parseEnum("enum { A, B }").name, "");
});

test("gd: the fixture's every export form", () => {
	const s = parseGdScript(read(GODOT_FIXTURE, "scripts", "enemy.gd"));
	assert.equal(s.className, "Enemy");
	assert.equal(s.extendsPath, "res://scripts/base_unit.gd");
	assert.deepEqual(Object.keys(s.enums), ["Mood"]);
	const names = s.exports.map((e) => e.name);
	assert.deepEqual(names, [
		"speed", "level", "boss", "faction", "tag", "mood", "loot_kind", "tint", "outline_color",
		"plain_color", "offset", "cell", "direction", "jump", "lives", "cruise", "patrol", "target",
		"arena", "spawn_point", "icon", "loot", "dialog", "sounds", "description", "nickname", "gait",
		"heading", "hit_layers", "elements", "drops", "path_points", "extra", "counts",
		"untyped_number", "inferred_text", "setter_inline", "godot_scene",
	]);
	assert.deepEqual(s.skipped.map((x) => x.name), ["hidden_state"]);
	const by = (n) => s.exports.find((e) => e.name === n);
	assert.equal(by("drops").annotation, "export");			// annotation on the line before
	assert.equal(by("drops").type, "Array[int]");
	assert.equal(by("gait").annotation, "export_enum");
	assert.deepEqual(by("gait").args, ["\"Walk\"", "\"Run:5\"", "\"Fly\""]);
	assert.equal(by("spawn_point").annotation, "export_node_path");
	assert.equal(by("lives").defaultText, "3");
	assert.equal(by("direction").inferred, true);
	// class_name and extends on one line, @tool before them.
	const item = parseGdScript(read(GODOT_FIXTURE, "scripts", "item.gd"));
	assert.equal(item.className, "Item");
	assert.equal(item.extendsName, "Resource");
	assert.deepEqual(item.enums.Kind, [{ name: "KEY", value: 0 }, { name: "GEM", value: 4 }, { name: "COIN", value: 5 }]);
	assert.equal(parseGdScript(read(GODOT_FIXTURE, "scripts", "arena.gd")).extendsName, "StaticBody2D");
	// The importer's fixture script.
	const imp = parseGdScript(read(IMPORTER_FIXTURES, "enemy.gd"));
	assert.deepEqual(imp.exports.map((e) => e.name + ":" + e.type), [
		"hp:int", "faction:String", "patrol:NodePath", "speed:float", "mood:Mood", "tag:StringName",
		"icon:Texture2D", "loot:Dictionary", "drops:Array[int]",
	]);
	assert.deepEqual(parseGdScript(read(IMPORTER_FIXTURES, "ref_target.gd")).exports.map((e) => e.type), ["NodePath", "Node2D", "Array[NodePath]"]);
});

test("literals", () => {
	assert.deepEqual(parseLiteral("3"), { kind: "int", value: 3 });
	assert.deepEqual(parseLiteral("-0x1F"), { kind: "int", value: -31 });
	assert.deepEqual(parseLiteral("1_000"), { kind: "int", value: 1000 });
	assert.deepEqual(parseLiteral("2.5"), { kind: "float", value: 2.5 });
	assert.deepEqual(parseLiteral(".5"), { kind: "float", value: 0.5 });
	assert.deepEqual(parseLiteral("1e3"), { kind: "float", value: 1000 });
	assert.deepEqual(parseLiteral("true"), { kind: "bool", value: true });
	assert.deepEqual(parseLiteral("\"a\\\"b\""), { kind: "string", value: "a\"b", prefix: "" });
	assert.deepEqual(parseLiteral("&\"n\""), { kind: "string", value: "n", prefix: "&" });
	assert.deepEqual(parseLiteral("Color(1, 0.5, 0)"), { kind: "color", value: { r: 1, g: 0.5, b: 0, a: 1 } });
	assert.deepEqual(parseLiteral("Color(\"#ff000080\")"), { kind: "color", value: { r: 1, g: 0, b: 0, a: 128 / 255 } });
	assert.deepEqual(parseLiteral("Color.RED"), { kind: "color", value: { r: 1, g: 0, b: 0, a: 1 } });
	assert.deepEqual(parseLiteral("Color8(255, 0, 0)"), { kind: "color", value: { r: 1, g: 0, b: 0, a: 1 } });
	assert.deepEqual(parseLiteral("Vector2(4, -8.5)"), { kind: "vector2", value: { x: 4, y: -8.5 } });
	assert.deepEqual(parseLiteral("Vector2i.UP"), { kind: "vector2i", value: { x: 0, y: -1 } });
	assert.deepEqual(parseLiteral("preload(\"res://icon.png\")"), { kind: "resource", value: "res://icon.png" });
	assert.deepEqual(parseLiteral("Mood.ANGRY"), { kind: "enumref", path: ["Mood", "ANGRY"] });
	assert.equal(parseLiteral("[1, 2]").kind, "array");
	assert.equal(parseLiteral("PackedVector2Array()").kind, "array");
	assert.equal(parseLiteral("{}").kind, "dictionary");
	assert.equal(parseLiteral("a + b").kind, "unknown");
	assert.equal(parseLiteral("").kind, "none");
});

test("scan: scenes with a root script; skips, inheritance, addons and .gdignore", () => {
	const scripts = walkFiles(GODOT_FIXTURE, io.listDir, (n) => n.endsWith(".gd"), null).map((p) => ({ path: p, text: io.readText(p) }));
	const index = indexClassNames(scripts);
	assert.deepEqual(Object.keys(index).sort(), ["Arena", "Enemy", "Item"]);
	assert.equal(engineClassOf("Arena", index), "StaticBody2D");
	assert.equal(engineClassOf("Texture2D", index), "Texture2D");
	const scan = scanScenes(GODOT_FIXTURE, GODOT_FIXTURE, io, index);
	assert.deepEqual(scan.scenes.map((s) => s.sceneRes), ["res://scenes/area.tscn", "res://scenes/boss.tscn", "res://scenes/door.tscn", "res://scenes/enemy.tscn"]);
	assert.deepEqual(scan.skipped.map((s) => s.sceneRes + ": " + s.reason), [
		"res://scenes/builtin.tscn: its root's script is built into the scene",
		"res://scenes/decor.tscn: its root has no script",
	]);
	const boss = scan.scenes[1];
	assert.equal(boss.scriptRes, "res://scripts/enemy.gd");	// from the inherited scene
	// Inherited exports first (base_unit.gd by path), the enum's owner by file name.
	assert.deepEqual(boss.resolved.exports.slice(0, 3).map((e) => e.name + "@" + e.owner), ["max_hp@BaseUnit", "team@BaseUnit", "speed@Enemy"]);
	assert.equal(boss.resolved.enumOwners.Team, "BaseUnit");
	assert.equal(boss.resolved.engineBase, "CharacterBody2D");
	// Scanning the addons folder itself finds its scene.
	const addons = scanScenes(GODOT_FIXTURE, path.join(GODOT_FIXTURE, "addons"), io, index);
	assert.deepEqual(addons.scenes.map((s) => s.sceneRes), ["res://addons/some_plugin/dock.tscn"]);
	assert.equal(pascalCase("big_slime"), "BigSlime");
	assert.equal(pascalCase("hud-panel 2"), "HudPanel2");
	assert.equal(pascalCase("2d"), "Scene2d");
});

test("scan: a missing script is skipped, not fatal", () => {
	const files = {
		"/g/a.tscn": "[ext_resource type=\"Script\" path=\"res://gone.gd\" id=\"1\"]\n[node name=\"A\" type=\"Node2D\"]\nscript = ExtResource(\"1\")\n",
	};
	const fakeIo = {
		listDir: (d) => d === "/g" ? { dirs: [], files: ["a.tscn"] } : null,
		readText: (p) => files[p] === undefined ? null : files[p],
	};
	const scan = scanScenes("/g", "/g", fakeIo, {});
	assert.equal(scan.scenes.length, 0);
	assert.match(scan.skipped[0].reason, /cannot be read/);
	const resolved = resolveScript("/g/x.gd", "/g", () => null, {});
	assert.equal(resolved.script, null);
});
