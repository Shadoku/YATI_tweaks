// lib/configure.mjs: the fields, the initial values, the validation and what it stores, the
// --version reading. Pure: the file checks are a set of paths.
import { test } from "node:test";
import assert from "node:assert/strict";
import {
	FIELDS, TEXTS, OPTION_FIELDS, fieldByKey, initialValues, managedSummary, godotProjectStatus, godotVersionStatus,
	validateConfiguration, configuredLines, optionFieldByName, optionTooltip, optionChoices, optionsToValues, valuesToOptions,
} from "../yati-godot/lib/configure.mjs";
import { readSettings, SETTING_NAMES, IMPORT_OPTIONS, IMPORT_OPTION_VALUES } from "../yati-godot/lib/settings.mjs";

const props = (o) => (name) => o[name];
const existing = (paths) => ({ exists: (p) => paths.indexOf(p) >= 0 });
const PROJECT = "/g/tiled/game.tiled-project";
const GAME = ["/g/game", "/g/game/project.godot", "/g/game/scenes", "/g/game/addons/x", "/g/tiled/tools/godot4", "/opt/elsewhere"];

test("fields: one per setting, in the dialog's order, each naming its property in the tooltip", () => {
	assert.deepEqual(FIELDS.map((f) => f.key), ["godotProject", "godotBinary", "scenesDir", "importOptions", "validate"]);
	assert.deepEqual(FIELDS.map((f) => f.property), ["godot_project", "godot_binary", "godot_scenes_dir", "yati_import_options", "yati_validate"]);
	assert.deepEqual(FIELDS.map((f) => f.kind), ["directory", "text", "directory", "text", "bool"]);
	for (const f of FIELDS) {
		assert.ok(f.tooltip.indexOf("Saved as " + f.property) >= 0, f.key);
		assert.ok(f.label.length > 0);
	}
	assert.equal(fieldByKey("validate").label, "Validate maps and tilesets on save");
	assert.equal(fieldByKey("nope"), null);
	assert.match(fieldByKey("importOptions").tooltip, /\n  use_default_filter=true\|false\n/);
	assert.match(fieldByKey("importOptions").tooltip, /\n  parallax_mode=parallax2d\|legacy\n/);
	assert.match(TEXTS.intro(PROJECT), /^These settings are custom properties of the project file \(game\.tiled-project\)/);
});

test("option fields: one per import option but class_scenes, kinds matching the options table, a tooltip with the default", () => {
	const names = Object.keys(IMPORT_OPTIONS).filter((n) => IMPORT_OPTIONS[n] !== "dictionary");
	assert.deepEqual(OPTION_FIELDS.map((f) => f.name), names);
	for (const f of OPTION_FIELDS) {
		if (IMPORT_OPTIONS[f.name] === "bool")
			assert.equal(f.kind, "bool", f.name);
		else if (IMPORT_OPTION_VALUES[f.name] !== undefined)
			assert.equal(f.kind, "choice", f.name);
		else
			assert.equal(f.kind, "text", f.name);
		if (f.kind === "choice")
			assert.ok(IMPORT_OPTION_VALUES[f.name].indexOf(f.def) >= 0, f.name + "'s default is one of its values");
		if (f.kind === "bool")
			assert.ok(f.def === "true" || f.def === "false", f.name);
		assert.ok(f.text.length > 20, f.name);
		assert.equal(optionTooltip(f).indexOf(f.name + "\n\n" + f.text + "\n\nDefault: "), 0);
	}
	assert.equal(optionTooltip(optionFieldByName("save_tileset_to")), "save_tileset_to\n\nIf set, the generated TileSet is saved to this resource path.\n\nDefault: (empty).");
	assert.equal(optionFieldByName("class_scenes"), null);
});

test("option choices: the default first, the values, and an unlisted current value kept", () => {
	assert.deepEqual(optionChoices(optionFieldByName("warn_unknown_classes"), ""), { labels: ["(importer default: true)", "true", "false"], values: ["", "true", "false"], index: 0 });
	assert.equal(optionChoices(optionFieldByName("warn_unknown_classes"), "false").index, 2);
	const u = optionChoices(optionFieldByName("unknown_class_fallback"), "area");
	assert.equal(u.values[u.index], "area");
	assert.equal(u.labels[0], "(importer default: staticbody)");
	const odd = optionChoices(optionFieldByName("animation_mode"), "weird");
	assert.deepEqual(odd.labels, ["(importer default: strip)", "strip", "atlas", "weird (not a listed value)"]);
	assert.equal(odd.index, 3);
	assert.equal(odd.values[3], "weird");
});

test("options text to values and back: the table's order, defaults left out, unknown text kept, quoting", () => {
	const empty = optionsToValues("");
	assert.deepEqual(empty.extra, []);
	assert.ok(Object.keys(empty.values).every((k) => empty.values[k] === ""));
	assert.equal(valuesToOptions(empty.values, empty.extra), "");
	const v = optionsToValues("object_refs=both  warn_unknown_classes=FALSE bogus=1 custom_data_prefix='x y' junk here use_default_filter=true");
	assert.equal(v.values.object_refs, "both");
	assert.equal(v.values.warn_unknown_classes, "false");
	assert.equal(v.values.custom_data_prefix, "x y");
	assert.equal(v.values.use_default_filter, "true");
	assert.deepEqual(v.extra, ["bogus=1", "junk here"]);
	assert.equal(valuesToOptions(v.values, v.extra), "use_default_filter=true custom_data_prefix=\"x y\" warn_unknown_classes=false object_refs=both bogus=1 junk here");
	// Round trip of the written form; a value changed to "" leaves the option out; spaces trimmed.
	const again = optionsToValues(valuesToOptions(v.values, v.extra));
	assert.deepEqual(again.values, v.values);
	assert.deepEqual(again.extra, v.extra);
	again.values.object_refs = "";
	again.values.save_tileset_to = "  res://a.tres ";
	assert.equal(valuesToOptions(again.values, []), "use_default_filter=true custom_data_prefix=\"x y\" save_tileset_to=res://a.tres warn_unknown_classes=false");
	// An unknown pair with a quoted value keeps its quotes; a known option given twice: the last wins.
	assert.deepEqual(optionsToValues("other=\"a b\" object_refs=both object_refs=geometry").extra, ["other=\"a b\""]);
	assert.equal(optionsToValues("object_refs=both object_refs=geometry").values.object_refs, "geometry");
});

test("initial values: resolved folders, the binary and options as written, the scenes folder empty for the whole project", () => {
	const empty = initialValues(readSettings(props({}), PROJECT));
	assert.deepEqual(empty, { godotProject: "", godotBinary: "", scenesDir: "", importOptions: "", validate: true });
	const set = initialValues(readSettings(props({
		godot_project: "../game", godot_binary: "tools/godot4", godot_scenes_dir: "res://scenes",
		yati_import_options: " warn_unknown_classes=false ", yati_validate: false,
	}), PROJECT));
	assert.deepEqual(set, { godotProject: "/g/game", godotBinary: "tools/godot4", scenesDir: "/g/game/scenes", importOptions: "warn_unknown_classes=false", validate: false });
	assert.equal(managedSummary(readSettings(props({}), PROJECT)), "No types made by the extension yet: run Sync Godot project and Import scenes as classes.");
	const managed = readSettings(props({
		yati_generated: "{\"Enemy\":[\"godot_scene\",\"hp\"],\"GodotPhysicsLayers\":[]}",
		yati_fixups: "{\"flags\":[\"GodotPhysicsLayers\"],\"floats\":{\"Enemy\":[\"speed\"]}}",
	}), PROJECT);
	assert.equal(managedSummary(managed), "Made by the extension: 2 type(s) with 2 member(s) (yati_generated), 2 project-file fix(es) (yati_fixups).");
});

test("the Godot project's status: empty, missing, without project.godot, right; relative to the project file", () => {
	const s = readSettings(props({}), PROJECT);
	const io = existing(GAME);
	assert.deepEqual(godotProjectStatus("", s, io.exists), { ok: false, text: "Choose the folder that holds project.godot.", dir: "" });
	assert.deepEqual(godotProjectStatus("/g/nope", s, io.exists), { ok: false, text: "'/g/nope' does not exist.", dir: "/g/nope" });
	assert.deepEqual(godotProjectStatus("/g/game/scenes", s, io.exists), { ok: false, text: "'/g/game/scenes' holds no project.godot. Choose the Godot project's own folder.", dir: "/g/game/scenes" });
	assert.deepEqual(godotProjectStatus("/g/game/", s, io.exists), { ok: true, text: "project.godot found.", dir: "/g/game" });
	assert.deepEqual(godotProjectStatus("../game", s, io.exists), { ok: true, text: "project.godot found.", dir: "/g/game" });
	assert.equal(godotProjectStatus("C:\\game", s, (p) => p === "C:/game" || p === "C:/game/project.godot").ok, true);
});

test("--version: Godot 4, Godot 3, not Godot, not started, not run", () => {
	assert.deepEqual(godotVersionStatus("godot", { code: 0, output: "4.7.1.stable.official.abcdef123\n" }), { ok: true, text: "Godot 4.7.1.stable.official.abcdef123", warning: "" });
	assert.deepEqual(godotVersionStatus("", { code: 0, output: "\n4.5.rc1\n" }).text, "Godot 4.5.rc1");
	const three = godotVersionStatus("godot3", { code: 0, output: "3.5.3.stable.official\n" });
	assert.equal(three.ok, false);
	assert.equal(three.text, "Godot 3.5.3.stable.official: YATI needs Godot 4.");
	assert.equal(three.warning, "'godot3' is Godot 3.5.3.stable.official; Check map in Godot needs Godot 4.");
	const other = godotVersionStatus("python", { code: 0, output: "Python 3.12.1\n" });
	assert.equal(other.text, "'python' started, but printed no Godot version: Python 3.12.1");
	assert.equal(godotVersionStatus("x", { code: 0, output: "" }).text, "'x' started, but printed no Godot version.");
	const none = godotVersionStatus("", { code: -1, output: "" });
	assert.equal(none.text, "Could not start 'godot'.");
	assert.match(none.warning, /^Could not start 'godot': Check map in Godot will not work/);
	assert.deepEqual(godotVersionStatus("godot", null), { ok: false, text: "", warning: "" });
});

test("validation: errors for what cannot work, warnings for what may; what is stored", () => {
	const s = readSettings(props({}), PROJECT);
	const io = existing(GAME);
	const versions = { "/g/tiled/tools/godot4": { code: 0, output: "4.7.1.stable\n" }, godot: { code: -1, output: "" } };
	const run = (values, godotVersion) => validateConfiguration(values, s, { exists: io.exists, godotVersion: godotVersion === undefined ? (b) => versions[b] || { code: 0, output: "?" } : godotVersion });

	const nothing = run({ godotProject: "", godotBinary: "", scenesDir: "", importOptions: "", validate: true });
	assert.deepEqual(nothing.errors, ["Godot project: Choose the folder that holds project.godot."]);
	assert.deepEqual(nothing.warnings, ["Godot binary: Could not start 'godot': Check map in Godot will not work until the Godot binary is a full path or a command on PATH."]);
	assert.equal(nothing.stored, null);

	const good = run({ godotProject: "/g/game", godotBinary: " tools/godot4 ", scenesDir: "/g/game/scenes", importOptions: " object_refs=both ", validate: true });
	assert.deepEqual(good.errors, []);
	assert.deepEqual(good.warnings, []);
	assert.deepEqual(good.stored, {
		godot_project: "../game", godot_binary: "tools/godot4", godot_scenes_dir: "res://scenes", yati_import_options: "object_refs=both", yati_validate: true,
	});
	assert.deepEqual(good.resolved, { godotProject: "/g/game", godotBinary: "/g/tiled/tools/godot4", scenesDir: "/g/game/scenes", validate: true });
	assert.deepEqual(configuredLines(good), [
		"Godot project: /g/game", "Godot binary: /g/tiled/tools/godot4", "Scenes scanned under: /g/game/scenes",
		"Import options: object_refs=both", "Validate on save: yes",
	]);

	// The scenes folder: the project itself is "", res:// typed in, relative to the Godot project, outside it.
	assert.equal(run({ godotProject: "/g/game", godotBinary: "tools/godot4", scenesDir: "/g/game/", importOptions: "", validate: true }).stored.godot_scenes_dir, "");
	assert.equal(run({ godotProject: "/g/game", godotBinary: "tools/godot4", scenesDir: "res://addons/x", importOptions: "", validate: true }).stored.godot_scenes_dir, "res://addons/x");
	assert.equal(run({ godotProject: "/g/game", godotBinary: "tools/godot4", scenesDir: "scenes", importOptions: "", validate: true }).stored.godot_scenes_dir, "res://scenes");
	const outside = run({ godotProject: "/g/game", godotBinary: "tools/godot4", scenesDir: "/opt/elsewhere", importOptions: "", validate: "false" });
	assert.deepEqual(outside.errors, []);
	assert.deepEqual(outside.warnings, ["Scenes folder: '/opt/elsewhere' is outside the Godot project; scenes found there get absolute paths, which the game cannot load as res://."]);
	assert.equal(outside.stored.godot_scenes_dir, "/opt/elsewhere");
	assert.equal(outside.stored.yati_validate, false);
	assert.equal(configuredLines(outside)[4], "Validate on save: no");
	assert.equal(configuredLines(outside)[5], outside.warnings[0]);

	// Every error at once; a missing binary path is not run.
	let ran = 0;
	const bad = run({ godotProject: "/g/nope", godotBinary: "../nowhere/godot", scenesDir: "/g/game/none", importOptions: "add_id_as_metadata=maybe x", validate: true }, () => { ran++; return { code: 0, output: "4.7\n" }; });
	assert.deepEqual(bad.errors, [
		"Godot project: '/g/nope' does not exist.",
		"Godot binary: '/g/nowhere/godot' does not exist.",
		"Scenes folder: '/g/game/none' does not exist.",
		"Import options: Option 'add_id_as_metadata' takes true or false, not 'maybe'.",
		"Import options: Ignored in the import options: 'x' (expected name=value pairs).",
	]);
	assert.equal(ran, 0);
	// With no project, a relative scenes folder is not judged (it is relative to the project).
	const noProject = run({ godotProject: "", godotBinary: "tools/godot4", scenesDir: "scenes", importOptions: "", validate: true });
	assert.deepEqual(noProject.errors, ["Godot project: Choose the folder that holds project.godot."]);
	// No version check when the dialog does not ask for one (null): no warning about the binary.
	const unchecked = run({ godotProject: "/g/game", godotBinary: "godot", scenesDir: "", importOptions: "bogus=1", validate: true }, () => null);
	assert.deepEqual(unchecked.errors, []);
	assert.deepEqual(unchecked.warnings, ["Import options: Unknown import option 'bogus' (passed on as it is)."]);
	assert.equal(unchecked.stored[SETTING_NAMES.godotBinary], "godot");
});
