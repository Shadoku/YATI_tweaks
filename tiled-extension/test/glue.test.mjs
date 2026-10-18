// yati-godot.mjs, the glue, against the fake API: installation, the four actions, the save hook,
// the Issues' activation, and the model it reads from the API (checked against the test reader).
import { test, after } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { ACTIONS, ACTION_TEXTS, mapModel } from "../yati-godot/yati-godot.mjs";
import { tmxModel, projectTypes, enumsOf } from "./helpers/tmx_model.mjs";
import { scenario, CHECK_OUTPUT, gameProjectTypes } from "./helpers/scenario.mjs";

const scenarios = [];
function make(options) {
	const s = scenario(options);
	scenarios.push(s);
	return s;
}
after(() => { for (const s of scenarios) s.cleanup(); });

const configured = (extra) => [{ name: "godot_project", type: "string", value: "godot_project" }].concat(extra || []);
// For a project in maps/ with game.tiled-project's types (their paths are relative to maps/).
const inMaps = (extra) => [{ name: "godot_project", type: "string", value: "../godot_project" }].concat(extra || []);

test("install: four actions in the Map menu after a separator, the save hook", () => {
	const s = make();
	const out = s.fake.out;
	assert.deepEqual(Object.keys(out.actions), [ACTIONS.configure, ACTIONS.sync, ACTIONS.scenes, ACTIONS.check]);
	assert.deepEqual(Object.values(out.actions).map((a) => a.text), [
		"YATI: Configure…", "YATI: Sync Godot project…", "YATI: Import scenes as classes…", "YATI: Check map in Godot",
	]);
	assert.deepEqual(Object.values(ACTION_TEXTS), Object.values(out.actions).map((a) => a.text));
	assert.deepEqual(out.menus.Map, [{ separator: true }, { action: "YatiConfigure" }, { action: "YatiSyncGodotProject" }, { action: "YatiImportScenes" }, { action: "YatiCheckMap" }]);
});

// A --version run that answers like Godot 4 for the check's fake process, the check output for the rest.
const godot4 = (binary, args) => (args[0] === "--version" ? { code: 0, stdout: "4.7.1.stable.official.abcdef123\n", stderr: "" } : { code: 1, stdout: CHECK_OUTPUT, stderr: "" });

test("configure: one dialog with a field per setting, each with its tooltip; Cancel writes nothing", () => {
	const none = make({ noProject: true });
	none.fake.trigger(ACTIONS.configure);
	assert.match(none.fake.out.alerts[0], /No Tiled project is open/);
	assert.equal(none.fake.out.dialogs.length, 0);

	const s = make({ fakeOptions: { process: godot4 } });
	let seen = null;
	s.fake.answers.dialogs.push((d) => { seen = d; });
	s.fake.trigger(ACTIONS.configure);
	assert.equal(seen.windowTitle, "YATI: Configure");
	assert.equal(seen.newRowMode, 1);	// ManualRows
	assert.equal(seen.execs, 1);
	assert.equal(seen.result, 0);		// closed without Save: rejected
	assert.deepEqual(seen.widgets.filter((w) => ["file", "text", "check"].indexOf(w.kind) >= 0).map((w) => w.label),
		["Godot project", "Godot binary", "Scenes folder", "Import options", "Validate maps and tilesets on save"]);
	assert.deepEqual(seen.widgets.filter((w) => w.kind === "button").map((w) => w.label), ["Browse…", "Test", "Save", "Cancel"]);
	for (const w of seen.widgets.filter((x) => ["file", "text", "check", "button"].indexOf(x.kind) >= 0))
		assert.ok(w.widget.toolTip.length > 20, w.label + " has a tooltip");
	assert.match(seen.field("Godot project").toolTip, /project\.godot/);
	assert.match(seen.field("Import options").toolTip, /warn_unknown_classes=true\|false/);
	assert.match(seen.field("Validate maps and tilesets on save").toolTip, /Issues view/);
	assert.equal(seen.field("Godot project").isDirectory, true);
	assert.equal(seen.field("Scenes folder").isDirectory, true);
	assert.equal(seen.field("Godot binary").placeholderText, "godot");
	assert.match(seen.widgets[1].widget.text, /custom properties of the project file \(game\.tiled-project\)/);
	assert.match(seen.labels()[seen.labels().length - 2].text, /^No types made by the extension yet/);
	assert.deepEqual(s.fake.out.alerts, []);
	assert.equal(s.property("godot_project"), undefined);
	assert.equal(s.projectJson().properties.length, 0);
	// An explicit Cancel.
	s.fake.answers.dialogs.push((d) => { d.field("Godot project").fileName = s.godot; d.button("Cancel").click(); });
	s.fake.trigger(ACTIONS.configure);
	assert.equal(s.projectJson().properties.length, 0);
	assert.equal(s.fake.out.dialogs[1].result, 0);
});

test("configure: Save validates first and keeps the dialog open; the status says what is wrong", () => {
	const s = make({ fakeOptions: { process: godot4 } });
	let d = null;
	s.fake.answers.dialogs.push((dialog) => {
		d = dialog;
		const status = () => d.labels()[d.labels().length - 1];
		// Nothing set: the Godot project is required; live status under the field.
		assert.equal(d.labels()[1].text, "Choose the folder that holds project.godot.");
		d.button("Save").click();
		assert.equal(d.result, null);
		assert.match(status().text, /^Godot project: Choose the folder that holds project\.godot\.$/);
		assert.match(status().styleSheet, /color/);
		// A folder without project.godot.
		d.field("Godot project").fileName = s.root;
		assert.match(d.labels()[1].text, /holds no project\.godot/);
		d.button("Save").click();
		assert.equal(d.result, null);
		assert.match(status().text, /holds no project\.godot/);
		// The right folder, a binary path that does not exist, a scenes folder that does not exist,
		// options with a bad boolean and junk: every error at once.
		d.field("Godot project").fileName = s.godot;
		assert.equal(d.labels()[1].text, "project.godot found.");
		d.field("Godot binary").text = "tools/godot4";
		d.field("Scenes folder").fileName = path.join(s.godot, "nowhere");
		d.field("Import options").text = "add_id_as_metadata=maybe junk class_scenes={}";
		d.button("Save").click();
		assert.equal(d.result, null);
		assert.deepEqual(status().text.split("\n"), [
			"Godot binary: '" + path.join(s.root, "tools", "godot4") + "' does not exist.",
			"Scenes folder: '" + path.join(s.godot, "nowhere") + "' does not exist.",
			"Import options: Option 'add_id_as_metadata' takes true or false, not 'maybe'.",
			"Import options: Option 'class_scenes' is a Dictionary and cannot be given here; set it in the game's code.",
			"Import options: Ignored in the import options: 'junk' (expected name=value pairs).",
		]);
		assert.equal(s.projectJson().properties.length, 0);
	});
	s.fake.trigger(ACTIONS.configure);
	assert.equal(d.execs, 1);
	assert.equal(d.result, 0);
	assert.equal(s.projectJson().properties.length, 0);
});

test("configure: Save stores the settings relative to the project file; defaults are left unset", () => {
	const s = make({ fakeOptions: { process: godot4 } });
	s.fake.answers.dialogs.push((d) => {
		d.field("Godot project").fileName = s.godot;
		d.field("Godot binary").text = "godot4";
		d.field("Scenes folder").fileName = path.join(s.godot, "scenes");
		d.field("Import options").text = " warn_unknown_classes=false bogus=1 ";
		d.button("Save").click();
		// The unknown option is a warning: shown, and Save asks to be pressed again.
		assert.equal(d.result, null);
		assert.equal(d.button("Save").text, "Save anyway");
		assert.match(d.labels()[d.labels().length - 1].text, /^Import options: Unknown import option 'bogus' \(passed on as it is\)\.\nPress Save anyway/);
		d.button("Save").click();
	});
	s.fake.trigger(ACTIONS.configure);
	const d = s.fake.out.dialogs[0];
	assert.equal(d.result, 1);
	assert.equal(s.property("godot_project"), "godot_project");
	assert.equal(s.property("godot_binary"), "godot4");
	assert.equal(s.property("godot_scenes_dir"), "res://scenes");
	assert.equal(s.property("yati_import_options"), "warn_unknown_classes=false bogus=1");
	assert.equal(s.property("yati_validate"), undefined);
	assert.deepEqual(s.fake.out.alerts, []);
	assert.ok(s.fake.out.logs.indexOf("YATI: Configured.") >= 0);
	assert.ok(s.fake.out.logs.indexOf("YATI:   Godot binary: godot4") >= 0);
	assert.ok(s.fake.out.logs.indexOf("YATI:   Scenes scanned under: " + path.join(s.godot, "scenes")) >= 0);
	assert.ok(s.fake.out.logs.indexOf("YATI:   Import options: Unknown import option 'bogus' (passed on as it is).") >= 0);
	// --version was run on Save, once, for the typed binary.
	assert.deepEqual(s.fake.out.processes, [{ binary: "godot4", args: ["--version"] }]);
	// Again: the dialog starts from the saved values; the scenes folder cleared and validation off.
	s.fake.answers.dialogs.push((d2) => {
		assert.equal(d2.field("Godot project").fileName, s.godot);
		assert.equal(d2.field("Godot binary").text, "godot4");
		assert.equal(d2.field("Scenes folder").fileName, path.join(s.godot, "scenes"));
		assert.equal(d2.field("Import options").text, "warn_unknown_classes=false bogus=1");
		assert.equal(d2.field("Validate maps and tilesets on save").checked, true);
		d2.field("Scenes folder").fileName = "";
		d2.field("Validate maps and tilesets on save").checked = false;
		d2.button("Save").click();
	});
	s.fake.trigger(ACTIONS.configure);
	assert.equal(s.property("godot_scenes_dir"), "");
	assert.equal(s.property("yati_validate"), false);
	assert.equal(s.property("godot_binary"), "godot4");
	// A third time with nothing changed: no write (the project is not saved again).
	const saves = s.fake.project.saves;
	s.fake.answers.dialogs.push((d3) => { d3.button("Save").click(); });
	s.fake.trigger(ACTIONS.configure);
	assert.equal(s.fake.project.saves, saves);
	assert.equal(s.fake.out.dialogs[2].result, 1);
});

test("configure: Browse and Test; a binary that does not start warns, and Save asks to be pressed again", () => {
	const s = make({ fakeOptions: { files: ["/usr/bin/godot4"], process: (binary, args) => (binary === "/usr/bin/godot4" ? { code: 0, stdout: "4.7.1.stable.official\n" } : { code: -1 }) } });
	s.fake.answers.dialogs.push((d) => {
		d.field("Godot project").fileName = s.godot;
		d.button("Browse…").click();
		assert.equal(d.field("Godot binary").text, "/usr/bin/godot4");
		d.button("Test").click();
		assert.equal(d.labels()[2].text, "Godot 4.7.1.stable.official");
		// A command PATH does not have: Test says so; Save warns and waits for a second press.
		d.field("Godot binary").text = "godot-nowhere";
		d.button("Test").click();
		assert.equal(d.labels()[2].text, "Could not start 'godot-nowhere'.");
		d.button("Save").click();
		assert.equal(d.result, null);
		const status = d.labels()[d.labels().length - 1];
		assert.deepEqual(status.text.split("\n"), [
			"Godot binary: Could not start 'godot-nowhere': Check map in Godot will not work until the Godot binary is a full path or a command on PATH.",
			"Press Save anyway to keep these settings regardless.",
		]);
		assert.equal(d.button("Save").text, "Save anyway");
		d.button("Save").click();
	});
	s.fake.trigger(ACTIONS.configure);
	assert.equal(s.fake.out.dialogs[0].result, 1);
	assert.equal(s.property("godot_binary"), "godot-nowhere");
	// The Test button's run was reused by Save: two --version runs in all, one per binary.
	assert.deepEqual(s.fake.out.processes.map((p) => p.binary + " " + p.args.join(" ")), ["/usr/bin/godot4 --version", "godot-nowhere --version"]);
	// A Godot 3 and a non-Godot answer: warnings too (a binary with a slash must exist).
	const t = make({ fakeOptions: { process: (binary) => (binary === "godot3" ? { code: 0, stdout: "3.5.3.stable.official\n" } : { code: 0, stdout: "usage: thing\n" }) } });
	t.fake.answers.dialogs.push((d) => {
		d.field("Godot project").fileName = t.godot;
		d.field("Godot binary").text = "godot3";
		d.button("Save").click();
		assert.match(d.labels()[d.labels().length - 1].text, /is Godot 3\.5\.3\.stable\.official; Check map in Godot needs Godot 4/);
		d.field("Godot binary").text = "thing";
		d.button("Save").click();
		assert.match(d.labels()[d.labels().length - 1].text, /does not look like Godot \(no version in its --version output: usage: thing\)/);
		d.button("Save").click();
	});
	t.fake.trigger(ACTIONS.configure);
	assert.equal(t.property("godot_binary"), "thing");
});

test("configure: a scenes folder outside the Godot project warns and is stored absolute; res:// typed in is accepted", () => {
	const s = make({ fakeOptions: { process: godot4 } });
	const outside = path.join(s.root, "importer");
	s.fake.answers.dialogs.push((d) => {
		d.field("Godot project").fileName = s.godot;
		d.field("Scenes folder").fileName = outside;
		d.button("Save").click();
		assert.match(d.labels()[d.labels().length - 1].text, /is outside the Godot project; scenes found there get absolute paths/);
		d.button("Save").click();
	});
	s.fake.trigger(ACTIONS.configure);
	assert.equal(s.property("godot_scenes_dir"), outside);
	s.fake.answers.dialogs.push((d) => { d.field("Scenes folder").fileName = "res://scenes"; d.button("Save").click(); });
	s.fake.trigger(ACTIONS.configure);
	assert.equal(s.property("godot_scenes_dir"), "res://scenes");
	// The live status under the options field.
	s.fake.answers.dialogs.push((d) => {
		d.field("Import options").text = "object_refs=both parallax_mode=sideways";
		assert.equal(d.labels()[3].text, "Option 'parallax_mode' takes parallax2d, legacy; 'sideways' is passed on as it is.");
		d.field("Import options").text = "";
		assert.equal(d.labels()[3].text, "");
	});
	s.fake.trigger(ACTIONS.configure);
});

// Tiled starts without a project unless its session restores one, and tiled.project is null then
// (the first real Tiled reported "Cannot read property 'fileName' of null" on every save).
test("without a project (tiled.project is null): the actions say so, a save still validates, nothing throws", () => {
	const s = make({ noProject: true });
	assert.equal(s.fake.api.tiled.project, null);
	const map = s.fake.openFile(path.join(s.root, "importer", "unknown_class.tmx"));
	s.fake.trigger(ACTIONS.configure);
	s.fake.trigger(ACTIONS.sync);
	s.fake.trigger(ACTIONS.scenes);
	s.fake.trigger(ACTIONS.check);
	assert.equal(s.fake.out.alerts.length, 4);
	for (const a of s.fake.out.alerts)
		assert.match(a, /^No Tiled project is open\. Open or create one/);
	s.fake.emitAboutToBeSaved(map);
	assert.equal(s.fake.out.issues.length, 4);
	assert.ok(s.fake.out.issues.every((i) => i.severity === "warning"));
	assert.deepEqual(s.fake.out.errors, []);
});

test("sync, scenes and check offer the Configure dialog until the Godot project is set, and go on with it", () => {
	const s = make({ fakeOptions: { process: godot4 } });
	// Declined: nothing happens. Accepted but cancelled: nothing happens either.
	s.fake.answers.confirms.push(false);
	s.fake.trigger(ACTIONS.sync);
	s.fake.trigger(ACTIONS.scenes);
	s.fake.trigger(ACTIONS.check);
	assert.deepEqual(s.fake.out.alerts, ["Open the map to check first (Check map in Godot runs on the active map)."]);
	const offers = s.fake.out.logs.filter((l) => l.indexOf("confirm: The Godot project is not set") === 0);
	assert.equal(offers.length, 2);
	assert.match(offers[0], /YATI needs the folder that holds project\.godot\.\n\nOpen YATI: Configure… now\?$/);
	assert.equal(s.fake.out.dialogs.length, 1);	// the second offer was accepted (the fake's default), then cancelled
	assert.equal(s.projectJson().propertyTypes.length, 0);
	// Accepted and saved: the sync runs at once with the new settings.
	s.fake.answers.dialogs.push((d) => { d.field("Godot project").fileName = s.godot; d.button("Save").click(); });
	s.fake.trigger(ACTIONS.sync);
	assert.equal(s.property("godot_project"), "godot_project");
	assert.match(s.fake.out.alerts[1], /^Synced with the Godot project 'Extension Fixture'/);
	// A project folder that went missing: the same offer, naming the folder.
	const t = make({ properties: [{ name: "godot_project", type: "string", value: "nowhere" }] });
	t.fake.answers.confirms.push(false);
	t.fake.trigger(ACTIONS.sync);
	assert.deepEqual(t.fake.out.alerts, []);
	assert.ok(t.fake.out.logs.some((l) => /^confirm: No project\.godot in '.*nowhere' \(the setting godot_project\)\.\n\nOpen YATI: Configure… now\?$/.test(l)));
});

test("sync: the layer enums with flags (patched into the file), GodotNodeType, MapRef; then nothing to do", () => {
	const s = make({ properties: configured() });
	s.fake.trigger(ACTIONS.sync);
	const alert = s.fake.out.alerts[0];
	assert.match(alert, /^Synced with the Godot project 'Extension Fixture'/);
	assert.match(alert, /Created: GodotPhysicsLayers, GodotNavigationLayers, GodotRenderLayers, GodotNodeType, MapRef/);
	assert.match(alert, /Reopen the project now/);
	const types = s.projectJson().propertyTypes;
	assert.deepEqual(types.filter((t) => t.valuesAsFlags).map((t) => t.name), ["GodotPhysicsLayers", "GodotNavigationLayers", "GodotRenderLayers"]);
	assert.equal(s.property("yati_fixups"), "{\"flags\":[\"GodotNavigationLayers\",\"GodotPhysicsLayers\",\"GodotRenderLayers\"],\"floats\":{}}");
	assert.ok(s.fake.out.logs.some((l) => l.indexOf("YATI:   Created:") === 0));
	// A second sync changes nothing (the file already has what memory lacks).
	const before = fs.readFileSync(s.projectFile, "utf8");
	s.fake.trigger(ACTIONS.sync);
	assert.match(s.fake.out.alerts[1], /Unchanged: GodotPhysicsLayers/);
	assert.equal(fs.readFileSync(s.projectFile, "utf8"), before);
});

test("scenes: asks first, makes the classes, reports notes and skips", () => {
	const s = make({ properties: configured() });
	s.fake.answers.confirms.push(false);
	s.fake.trigger(ACTIONS.scenes);
	assert.deepEqual(s.fake.out.alerts, []);
	assert.equal(s.projectJson().propertyTypes.length, 0);
	s.fake.trigger(ACTIONS.scenes);
	const alert = s.fake.out.alerts[0];
	assert.match(alert, /^Imported 4 scene\(s\) as classes\./);
	assert.match(alert, /Created: BaseUnit\.Team, Enemy\.Mood, Item\.Kind, Enemy\.gait, Enemy\.heading, Vector2, Vector2i, AreaScene, Boss, Door, Enemy/);
	assert.match(alert, /Enemy: export 'extra' skipped \(a Dictionary has no Tiled member type\)/);
	assert.match(alert, /Enemy: export 'drops' skipped \(Tiled class members cannot be lists\)/);
	assert.match(alert, /Enemy: export 'path_points' skipped \(Tiled class members cannot be lists\)/);
	assert.match(alert, /res:\/\/scenes\/decor\.tscn: skipped, its root has no script/);
	const enemy = s.projectJson().propertyTypes.find((t) => t.name === "Enemy");
	assert.equal(enemy.members.find((m) => m.name === "godot_scene").value, "godot_project/scenes/enemy.tscn");
	assert.equal(enemy.members.find((m) => m.name === "speed").type, "float");
	assert.equal(enemy.members.find((m) => m.name === "jump").type, "float");	// 1.0, patched
	assert.equal(enemy.members.find((m) => m.name === "drops" || m.name === "path_points"), undefined);
	// No scenes with scripts: nothing asked.
	const t = make({ properties: configured([{ name: "godot_scenes_dir", type: "string", value: "res://data" }]) });
	t.fake.trigger(ACTIONS.scenes);
	assert.match(t.fake.out.alerts[0], /No scene under .*data has a script/);
});

test("scenes after sync: the physics flags export is typed GodotPhysicsLayers; both patches survive", () => {
	const s = make({ properties: configured() });
	s.fake.trigger(ACTIONS.sync);
	s.fake.trigger(ACTIONS.scenes);
	const types = s.projectJson().propertyTypes;
	const enemy = types.find((t) => t.name === "Enemy");
	assert.deepEqual(enemy.members.find((m) => m.name === "hit_layers"), { name: "hit_layers", propertyType: "GodotPhysicsLayers", type: "int", value: 5 });
	// The scenes action saved the project from memory (dropping the sync's flags) and wrote them back.
	assert.deepEqual(types.filter((t) => t.valuesAsFlags).map((t) => t.name), ["GodotPhysicsLayers", "GodotNavigationLayers", "GodotRenderLayers"]);
});

test("check: needs a map and the game's check script", () => {
	const s = make({ properties: configured() });
	s.fake.openFile(path.join(s.maps, "mistakes.tmx"));
	s.fake.trigger(ACTIONS.check);
	assert.match(s.fake.out.alerts[0], /has no addons\/yati_native\/check_map\.gd/);
	assert.equal(s.fake.out.processes.length, 0);
});

test("check: runs Godot, files each message as an Issue that selects its object", () => {
	const s = make({
		checkScript: true,
		properties: configured([{ name: "yati_import_options", type: "string", value: "warn_unknown_classes=false" }]),
		fakeOptions: { process: () => ({ code: 1, stdout: CHECK_OUTPUT, stderr: "" }) },
	});
	const mapFile = path.join(s.maps, "mistakes.tmx");
	s.fake.openFile(mapFile);
	s.fake.trigger(ACTIONS.check);
	assert.deepEqual(s.fake.out.processes, [{
		binary: "godot",
		args: ["--headless", "--path", s.godot, "-s", "res://addons/yati_native/check_map.gd", "--", mapFile, "--project=" + s.projectFile, "--warn_unknown_classes=false"],
	}]);
	const issues = s.fake.out.issues;
	assert.deepEqual(issues.map((i) => i.severity), ["warning", "warning", "error", "warning"]);
	assert.equal(issues[2].text, "YATI check: mistakes.tmx: Object of class 'instance': Mandatory file property 'res_path' not found or invalid. -> Skipped");
	assert.match(s.fake.out.alerts[0], /^Godot imported the map with 1 error\(s\) and 3 warning\(s\)/);
	// Activating the error selects object 4 (Spawner) and centres the view on it.
	issues[2].activate();
	const active = s.fake.api.tiled.activeAsset;
	assert.deepEqual(active.selectedObjects.map((o) => o.name), ["Spawner"]);
	assert.deepEqual(s.fake.mapView.centered, [[64, 16]]);
	// Object 21 is inside a group layer.
	issues[3].activate();
	assert.deepEqual(active.selectedObjects.map((o) => o.name), ["Pointer"]);
});

test("check: a Godot that does not start, a check that does not finish", () => {
	const s = make({ checkScript: true, properties: configured(), fakeOptions: { process: () => ({ code: -1 }) } });
	s.fake.openFile(path.join(s.maps, "clean.tmx"));
	s.fake.trigger(ACTIONS.check);
	assert.match(s.fake.out.alerts[0], /Could not start 'godot'/);
	const t = make({ checkScript: true, properties: configured(), fakeOptions: { process: () => ({ code: 134, stdout: "", stderr: "SCRIPT ERROR: Parse Error\n" }) } });
	t.fake.openFile(path.join(t.maps, "clean.tmx"));
	t.fake.trigger(ACTIONS.check);
	assert.match(t.fake.out.alerts[0], /did not finish .* exit code 134\)\. Last output: SCRIPT ERROR: Parse Error/);
	const u = make({ checkScript: true, properties: configured(), fakeOptions: { process: () => ({ code: 0, stdout: "YATI_CHECK result 0 0 3 0 1\n" }) } });
	u.fake.openFile(path.join(u.maps, "clean.tmx"));
	u.fake.trigger(ACTIONS.check);
	assert.equal(u.fake.out.alerts[0], "Godot imported the map with 0 error(s) and 0 warning(s): 3 nodes, 0 cells, 1 ms.");
});

test("check: a modified map is saved first when the user agrees (and validated on that save)", () => {
	const s = make({ inMaps: true, checkScript: true, properties: inMaps(), propertyTypes: gameProjectTypes(), fakeOptions: { process: () => ({ code: 0, stdout: "YATI_CHECK result 0 0 3 0 1\n" }) } });
	const map = s.fake.openFile(path.join(s.maps, "mistakes.tmx"));
	map._modified = true;
	s.fake.trigger(ACTIONS.check);
	assert.equal(map._saves, 1);
	assert.ok(s.fake.out.issues.length > 0);
	map._modified = true;
	s.fake.answers.confirms.push(false);
	s.fake.trigger(ACTIONS.check);
	assert.equal(map._saves, 1);
	assert.ok(s.fake.out.logs.indexOf("YATI: checking the map as last saved.") >= 0);
});

test("save hook: the validator's findings as Issues; activation selects the object or the layer", () => {
	const s = make({ inMaps: true, properties: inMaps(), propertyTypes: gameProjectTypes() });
	const map = s.fake.openFile(path.join(s.maps, "mistakes.tmx"));
	s.fake.emitAboutToBeSaved(map);
	const issues = s.fake.out.issues;
	assert.equal(issues.length, 13);
	assert.ok(issues.every((i) => i.text.indexOf("YATI: mistakes.tmx: ") === 0));
	assert.equal(issues.filter((i) => i.severity === "error").length, 4);
	const ghost = issues.find((i) => i.text.indexOf("'Gohst'") >= 0);
	ghost.activate();
	assert.deepEqual(s.fake.api.tiled.activeAsset.selectedObjects.map((o) => o.id), [2]);
	const layer = issues.find((i) => i.text.indexOf("Layer 'Units'") >= 0);
	layer.activate();
	assert.equal(s.fake.api.tiled.activeAsset.currentLayer.name, "Units");
	assert.ok(s.fake.out.logs.indexOf("YATI: mistakes.tmx: 13 finding(s), see the Issues view.") >= 0);
	// The tileset of the map, saved on its own.
	const ts = s.fake.openFile(path.join(s.maps, "mistakes.tsx"));
	s.fake.emitAboutToBeSaved(ts);
	assert.equal(issues.length, 15);
	assert.match(issues[14].text, /^YATI: mistakes\.tsx: Tileset 'mistake_tiles'/);
});

test("save hook: yati_validate = false turns it off; a clean map files nothing; no project still validates", () => {
	const s = make({ inMaps: true, properties: inMaps([{ name: "yati_validate", type: "bool", value: false }]), propertyTypes: gameProjectTypes() });
	s.fake.emitAboutToBeSaved(s.fake.openFile(path.join(s.maps, "mistakes.tmx")));
	assert.equal(s.fake.out.issues.length, 0);
	const t = make({ inMaps: true, properties: inMaps(), propertyTypes: gameProjectTypes() });
	t.fake.emitAboutToBeSaved(t.fake.openFile(path.join(t.maps, "clean.tmx")));
	assert.equal(t.fake.out.issues.length, 0);
	const u = make({ noProject: true });
	u.fake.emitAboutToBeSaved(u.fake.openFile(path.join(u.root, "importer", "unknown_class.tmx")));
	assert.equal(u.fake.out.issues.length, 4);
});

test("an exception inside an action becomes an Issue, not a crash", () => {
	const s = make({ checkScript: true, properties: configured(), fakeOptions: { process: () => { throw new Error("boom"); } } });
	s.fake.openFile(path.join(s.maps, "clean.tmx"));
	s.fake.trigger(ACTIONS.check);
	assert.deepEqual(s.fake.out.errors, ["YATI: boom"]);
	assert.equal(typeof s.fake.out.issues[0].activate, "function");
	// A scan that finds nothing to read is no exception either.
	const t = make({ properties: configured() });
	fs.rmSync(path.join(t.godot, "scripts"), { recursive: true });
	t.fake.trigger(ACTIONS.scenes);
	assert.match(t.fake.out.alerts[0], /No scene under/);
});

// The glue's model from the API equals the test reader's from the file (same properties, values,
// classes), so the validator sees in Tiled what validate.test.mjs tests.
function normalize(model) {
	const props = (list) => list.map((p) => ({ name: p.name, type: p.type, value: p.type === "list" ? p.value.map((i) => ({ type: i.type, value: i.value })) : p.value, propertyType: p.propertyType || "", fromEnum: !!p.fromEnum }))
		.sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
	return {
		properties: props(model.properties),
		layers: model.layers.map((l) => ({ id: l.id, name: l.name, path: l.path, className: l.className, properties: props(l.properties) })),
		objects: model.objects.map((o) => ({ id: o.id, name: o.name, layerPath: o.layerPath, className: o.className, tileClassName: o.tileClassName, properties: props(o.properties) })),
	};
}

for (const [dir, file, project] of [
	["maps", "mistakes.tmx", "maps/game.tiled-project"],
	["maps", "clean.tmx", "maps/game.tiled-project"],
	["importer", "scenes.tmx", "importer/scenes.tiled-project"],
	["importer", "masks.tmx", "importer/masks.tiled-project"],
	["importer", "object_refs.tmx", null],
	["importer", "templates.tmx", null],
]) {
	test("the API model equals the file model: " + dir + "/" + file, () => {
		const types = project ? JSON.parse(fs.readFileSync(path.join(path.dirname(new URL(import.meta.url).pathname), "fixtures", project), "utf8")).propertyTypes : [];
		const s = make({ propertyTypes: types });
		const mapFile = path.join(s.root, dir, file);
		const asset = s.fake.openFile(mapFile);
		const api = mapModel(s.fake.api, s.fake.api.tiled.activeAsset);
		const enums = enumsOf(project ? projectTypes(path.join(s.root, project)) : []);
		void asset;
		assert.deepEqual(normalize(api), normalize(tmxModel(mapFile, enums)));
	});
}
