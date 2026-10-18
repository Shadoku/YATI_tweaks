// lib/settings.mjs: paths, the settings from project properties, the import options, the records.
import { test } from "node:test";
import assert from "node:assert/strict";
import {
	cleanPath, joinPath, dirName, baseName, stem, relativePath, fileUrl, godotToLocal, localToGodot,
	isAbsolutePath, readSettings, settingsProblem, parseImportOptions, describeImportOptions, parseGenerated,
	stringifyGenerated, parseFixups, stringifyFixups, storedPath, SETTING_NAMES,
} from "../yati-godot/lib/settings.mjs";

test("paths: clean, join, parts", () => {
	assert.equal(cleanPath("a/./b/../c/"), "a/c");
	assert.equal(cleanPath("/x//y/../z"), "/x/z");
	assert.equal(cleanPath("../../a"), "../../a");
	assert.equal(cleanPath("C:\\games\\x\\..\\y"), "C:/games/y");
	assert.equal(cleanPath(""), ".");
	assert.equal(joinPath("/a/b", "../c.tscn"), "/a/c.tscn");
	assert.equal(joinPath("/a/b", "/abs/x"), "/abs/x");
	assert.equal(joinPath("C:/a", "b"), "C:/a/b");
	assert.equal(dirName("/a/b/c.tmx"), "/a/b");
	assert.equal(dirName("/c.tmx"), "/");
	assert.equal(dirName("C:/c.tmx"), "C:/");
	assert.equal(baseName("/a/b/c.tmx"), "c.tmx");
	assert.equal(stem("/a/big_slime.tscn"), "big_slime");
	assert.ok(isAbsolutePath("/a") && isAbsolutePath("D:/a") && !isAbsolutePath("a/b"));
});

test("paths: relative, URLs, res://", () => {
	assert.equal(relativePath("/g/tiled", "/g/godot/scenes/a.tscn"), "../godot/scenes/a.tscn");
	assert.equal(relativePath("/g", "/g"), ".");
	assert.equal(relativePath("/g/t", "/g/t/maps/a.tmx"), "maps/a.tmx");
	assert.equal(relativePath("C:/g", "D:/x"), "D:/x");
	assert.equal(fileUrl("/a/b c"), "file:///a/b c");
	assert.equal(fileUrl("C:/a"), "file:///C:/a");
	assert.equal(godotToLocal("/g", "res://scenes/a.tscn"), "/g/scenes/a.tscn");
	assert.equal(localToGodot("/g", "/g/scenes"), "res://scenes");
	assert.equal(localToGodot("/g", "/other"), null);
	assert.equal(storedPath("/home/me/game/tiled", "/home/me/game"), "..");
	assert.equal(storedPath("/tmp/x", "/home/me/game"), "/home/me/game");
});

function props(map) {
	return (name) => map[name];
}

test("settings: defaults and relative paths", () => {
	const s = readSettings(props({}), "/g/tiled/game.tiled-project");
	assert.equal(s.projectDir, "/g/tiled");
	assert.equal(s.godotProject, "");
	assert.deepEqual(s.missing, [SETTING_NAMES.godotProject]);
	assert.equal(s.godotBinary, "godot");
	assert.equal(s.validate, true);
	assert.match(settingsProblem(s, false), /not set/);
	assert.match(settingsProblem(readSettings(props({}), ""), false), /No Tiled project/);

	const t = readSettings(props({
		godot_project: "..", godot_scenes_dir: "res://scenes", godot_binary: "tools/godot4",
		yati_import_options: "warn_unknown_classes=false object_refs=\"both\"", yati_validate: false,
	}), "/g/tiled/game.tiled-project");
	assert.equal(t.godotProject, "/g");
	assert.equal(t.scenesDir, "/g/scenes");
	assert.equal(t.godotBinary, "/g/tiled/tools/godot4");
	assert.equal(t.validate, false);
	assert.deepEqual(t.importOptions.map((o) => o.arg), ["--warn_unknown_classes=false", "--object_refs=both"]);
	assert.equal(settingsProblem(t, true), "");
	assert.match(settingsProblem(t, false), /No project\.godot/);
	// A relative scenes dir is relative to the Godot project; "godot" stays a PATH lookup.
	const u = readSettings(props({ godot_project: "/abs/game", godot_scenes_dir: "levels", godot_binary: "godot4" }), "/g/t.tiled-project");
	assert.equal(u.scenesDir, "/abs/game/levels");
	assert.equal(u.godotBinary, "godot4");
	assert.equal(readSettings(props({ yati_validate: "false" }), "/p.tiled-project").validate, false);
});

test("import options: name=value pairs, warnings", () => {
	const r = parseImportOptions("assign_node_properties=FALSE  bogus=1 add_id_as_metadata=maybe class_scenes={} junk");
	assert.deepEqual(r.options.map((o) => o.arg), ["--assign_node_properties=false", "--bogus=1", "--add_id_as_metadata=maybe"]);
	assert.equal(r.warnings.length, 4);
	assert.match(r.warnings[0], /Unknown import option 'bogus'/);
	assert.match(r.warnings[1], /true or false/);
	assert.match(r.warnings[2], /Dictionary/);
	assert.match(r.warnings[3], /junk/);
	// What the Configure dialog refuses: the bad boolean, the Dictionary, the junk; not the unknown name.
	assert.deepEqual(r.blocking, [r.warnings[1], r.warnings[2], r.warnings[3]]);
	assert.deepEqual(parseImportOptions("").options, []);
	assert.deepEqual(parseImportOptions("").blocking, []);
	// A string option with a listed value set: an unlisted value warns, without blocking.
	const v = parseImportOptions("parallax_mode=sideways object_refs=both unknown_class_fallback=area");
	assert.deepEqual(v.options.map((o) => o.arg), ["--parallax_mode=sideways", "--object_refs=both", "--unknown_class_fallback=area"]);
	assert.deepEqual(v.warnings, ["Option 'parallax_mode' takes parallax2d, legacy; 'sideways' is passed on as it is."]);
	assert.deepEqual(v.blocking, []);
	const lines = describeImportOptions();
	assert.ok(lines.indexOf("use_default_filter=true|false") >= 0);
	assert.ok(lines.indexOf("object_refs=nodepath|geometry|both") >= 0);
	assert.ok(lines.indexOf("custom_data_prefix=<text>") >= 0);
	assert.ok(!lines.some((l) => l.indexOf("class_scenes") === 0));
});

test("records: yati_generated and yati_fixups round trip, junk is empty", () => {
	const g = parseGenerated("{\"B\":[\"y\",\"x\"],\"A\":[],\"bad\":3}");
	assert.deepEqual(g, { B: ["y", "x"], A: [] });
	assert.equal(stringifyGenerated(g), "{\"A\":[],\"B\":[\"x\",\"y\"]}");
	assert.deepEqual(parseGenerated("not json"), {});
	assert.deepEqual(parseGenerated(undefined), {});
	const f = parseFixups("{\"flags\":[\"B\",\"A\"],\"floats\":{\"T\":[\"y\",\"x\"],\"E\":[]}}");
	assert.equal(f.written, 0);
	assert.equal(stringifyFixups(f), "{\"flags\":[\"A\",\"B\"],\"floats\":{\"T\":[\"x\",\"y\"]},\"written\":0}");
	assert.deepEqual(parseFixups("[1]"), { flags: [], floats: {}, written: 0 });
	const w = parseFixups("{\"flags\":[],\"floats\":{},\"written\":3}");
	assert.equal(w.written, 3);
	assert.equal(stringifyFixups(w), "{\"flags\":[],\"floats\":{},\"written\":3}");
	assert.equal(parseFixups("{\"written\":\"x\"}").written, 0);
});
