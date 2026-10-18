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

test("configure: refuses without a project, stops on cancel, checks for project.godot", () => {
	const none = make({ noProject: true });
	none.fake.trigger(ACTIONS.configure);
	assert.match(none.fake.out.alerts[0], /No Tiled project is open/);

	const s = make();
	s.fake.answers.directories.push("");
	s.fake.trigger(ACTIONS.configure);
	assert.deepEqual(s.fake.out.alerts, []);
	assert.equal(s.property("godot_project"), undefined);

	s.fake.answers.directories.push(s.root);
	s.fake.trigger(ACTIONS.configure);
	assert.match(s.fake.out.alerts[0], /holds no project\.godot/);
});

test("configure: stores the settings relative to the project file; empty keeps, '-' clears", () => {
	const s = make();
	const a = s.fake.answers;
	a.directories.push(s.godot, path.join(s.godot, "scenes"));
	a.prompts.push("/opt/godot/godot4", "warn_unknown_classes=false bogus=1");
	s.fake.trigger(ACTIONS.configure);
	assert.equal(s.property("godot_project"), "godot_project");
	assert.equal(s.property("godot_binary"), "/opt/godot/godot4");
	assert.equal(s.property("godot_scenes_dir"), "res://scenes");
	assert.equal(s.property("yati_import_options"), "warn_unknown_classes=false bogus=1");
	assert.match(s.fake.out.alerts[0], /^Configured\./);
	assert.match(s.fake.out.alerts[0], /Unknown import option 'bogus'/);
	// Again: empty answers keep the binary and the options, a cancelled scenes folder is the whole project.
	a.directories.push(s.godot, "");
	a.prompts.push("", "");
	s.fake.trigger(ACTIONS.configure);
	assert.equal(s.property("godot_binary"), "/opt/godot/godot4");
	assert.equal(s.property("godot_scenes_dir"), "");
	assert.equal(s.property("yati_import_options"), "warn_unknown_classes=false bogus=1");
	a.directories.push(s.godot, "");
	a.prompts.push("", "-");
	s.fake.trigger(ACTIONS.configure);
	assert.equal(s.property("yati_import_options"), "");
});

test("sync and scenes refuse until the Godot project is set", () => {
	const s = make();
	s.fake.trigger(ACTIONS.sync);
	s.fake.trigger(ACTIONS.scenes);
	s.fake.trigger(ACTIONS.check);
	assert.equal(s.fake.out.alerts.length, 3);
	assert.match(s.fake.out.alerts[0], /The Godot project is not set/);
	assert.match(s.fake.out.alerts[1], /The Godot project is not set/);
	assert.match(s.fake.out.alerts[2], /Open the map to check first/);
	const t = make({ properties: [{ name: "godot_project", type: "string", value: "nowhere" }] });
	t.fake.trigger(ACTIONS.sync);
	assert.match(t.fake.out.alerts[0], /No project\.godot in/);
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
