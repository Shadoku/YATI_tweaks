// lib/validate.mjs on fixture maps: one finding per mistake of mistakes.tmx, none on clean.tmx, the
// importer's fixtures as the importers would report them.
import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { validateMap, validateTileset, templateReferences, contextFromTypes, maskFamily, resolveFileValue } from "../yati-godot/lib/validate.mjs";
import { tmxModel, tsxModel, projectTypes, enumsOf } from "./helpers/tmx_model.mjs";
import { MAP_FIXTURES, IMPORTER_FIXTURES, GODOT_FIXTURE } from "./helpers/node_io.mjs";

function check(mapFile, projectFile, godotProject) {
	const types = projectFile ? projectTypes(projectFile) : [];
	const ctx = contextFromTypes(types, projectFile || "", godotProject || "", (p) => fs.existsSync(p));
	return validateMap(tmxModel(mapFile, enumsOf(types)), ctx);
}

const brief = (f) => f.severity + " " + f.code + " #" + f.objectId;

test("mistakes.tmx: each mistake found once, fine objects not", () => {
	const findings = check(path.join(MAP_FIXTURES, "mistakes.tmx"), path.join(MAP_FIXTURES, "game.tiled-project"), GODOT_FIXTURE);
	assert.deepEqual(findings.map(brief), [
		"warning unset-object-ref #0",		// the map's "start"
		"warning mask-unknown-name #0",		// layer Units: collision_layer "Bogus"
		"warning unknown-class #2",			// Gohst
		"warning unknown-class #3",			// enemy (case)
		"error instance-without-res-path #4",
		"error missing-file #5",				// res_path chest.tscn
		"warning unknown-node-type #7",		// staticbdy
		"error missing-file #11",			// godot_scene override
		"warning mask-unknown-name #20",		// Player,Ghosts of GodotPhysicsLayers
		"warning mask-untyped-names #20",	// collision_mask "Player" without the enum type
		"warning mask-unknown-name #20",		// visibility_layer Lights,Nope
		"warning dangling-object-ref #21",	// 99
		"warning dangling-object-ref #21",	// many[1] = 98
		"warning unset-object-ref #21",		// unset
		"warning unset-object-ref #22",		// Door's target, the class's default
		"error missing-template #10",
	]);
	const text = (code, id) => findings.find((f) => f.code === code && f.objectId === id).text;
	assert.match(text("unknown-class", 3), /case-sensitive: 'Enemy'\?/);
	assert.match(text("unknown-class", 2), /^Object 'Ghost' \(id 2, layer Units\): class 'Gohst'/);
	assert.match(text("missing-file", 5), /res_path names \/.*\/maps\/chest\.tscn, which does not exist/);
	assert.match(text("missing-template", 10), /template 'missing\.tx' does not exist/);
	assert.match(text("unset-object-ref", 22), /its class's default/);
	assert.match(findings[1].text, /^Layer 'Units': mask 'collision_layer' names 'Bogus', which GodotPhysicsLayers does not have/);
	assert.equal(findings[1].layerId, 1);
	const masks = findings.filter((f) => f.objectId === 20).map((f) => f.text);
	assert.match(masks[0], /names 'Ghosts', which GodotPhysicsLayers does not have/);
	assert.match(masks[1], /give it the type GodotPhysicsLayers/);
	assert.match(masks[2], /names 'Nope', which GodotRenderLayers does not have/);
});

test("without the Godot project, res:// paths are not checked", () => {
	const findings = check(path.join(MAP_FIXTURES, "mistakes.tmx"), path.join(MAP_FIXTURES, "game.tiled-project"), "");
	assert.equal(findings.filter((f) => f.objectId === 6).length, 0);
	assert.equal(resolveFileValue("res://a.tscn", "/m/map.tmx", ""), "");
	assert.equal(resolveFileValue("res://a.tscn", "/m/map.tmx", "/g"), "/g/a.tscn");
	assert.equal(resolveFileValue("a.tscn", "/m/map.tmx", ""), "/m/a.tscn");
	assert.equal(resolveFileValue("file:///x/a.tscn", "/m/map.tmx", ""), "/x/a.tscn");
	assert.equal(resolveFileValue("https://x", "/m/map.tmx", ""), "");
});

test("clean.tmx: no findings", () => {
	assert.deepEqual(check(path.join(MAP_FIXTURES, "clean.tmx"), path.join(MAP_FIXTURES, "game.tiled-project"), GODOT_FIXTURE), []);
});

test("the tileset's masks", () => {
	const types = projectTypes(path.join(MAP_FIXTURES, "game.tiled-project"));
	const ctx = contextFromTypes(types, "", "", (p) => fs.existsSync(p));
	const findings = validateTileset(tsxModel(path.join(MAP_FIXTURES, "mistakes.tsx"), enumsOf(types)), ctx);
	assert.deepEqual(findings.map((f) => f.code), ["mask-unknown-name", "mask-unknown-name"]);
	assert.match(findings[0].text, /^Tileset 'mistake_tiles': mask 'collision_layer_0' names 'Nope'/);
	assert.match(findings[1].text, /'layers_1' names 'Ground', but the project has no GodotNavigationLayers enum \(run YATI: Sync Godot project\.\.\.\)/);
});

test("the importer's fixtures: what the importers would warn about", () => {
	// unknown_class.tmx (B8): Door three times, Lever once; Wall has no class.
	const unknown = check(path.join(IMPORTER_FIXTURES, "unknown_class.tmx"));
	assert.deepEqual(unknown.map(brief), ["warning unknown-class #1", "warning unknown-class #2", "warning unknown-class #3", "warning unknown-class #4"]);
	// object_refs.tmx (F2): the dangling 999 and the unset 0 of Door.
	const refs = check(path.join(IMPORTER_FIXTURES, "object_refs.tmx"));
	assert.deepEqual(refs.map(brief), ["warning dangling-object-ref #1", "warning unset-object-ref #1"]);
	// masks.tmx (F5) with its project: every mask is fine (ints, names of its enums, numbers).
	assert.deepEqual(check(path.join(IMPORTER_FIXTURES, "masks.tmx"), path.join(IMPORTER_FIXTURES, "masks.tiled-project")), []);
	// scenes.tmx (F1) with its project: "enemy" is not the project's "Enemy"; Elite's godot_scene
	// override exists; the Typed object's godot_node_type wins over its class.
	const scenes = check(path.join(IMPORTER_FIXTURES, "scenes.tmx"), path.join(IMPORTER_FIXTURES, "scenes.tiled-project"));
	assert.deepEqual(scenes.map(brief), ["warning unknown-class #4"]);
	// Without the project, Enemy is unknown too (Typed has a godot_node_type).
	assert.deepEqual(check(path.join(IMPORTER_FIXTURES, "scenes.tmx")).filter((f) => f.code === "unknown-class").map((f) => f.objectId), [1, 2, 4, 6, 7]);
});

test("templates: from the saved .tmx and .tmj", () => {
	assert.deepEqual(templateReferences(fs.readFileSync(path.join(IMPORTER_FIXTURES, "templates.tmx"), "utf8")), [
		{ objectId: 1, path: "probe_tpl.tx" }, { objectId: 2, path: "probe_text.tx" }, { objectId: 4, path: "probe_tpl.tx" },
		{ objectId: 5, path: "probe_rect.tx" }, { objectId: 6, path: "probe_rect.tx" }, { objectId: 3, path: "probe_tpl.tx" },
	]);
	const tmj = JSON.stringify({ layers: [{ type: "group", layers: [{ type: "objectgroup", objects: [{ id: 9, template: "a&b.tx" }, { id: 10 }] }] }] });
	assert.deepEqual(templateReferences(tmj), [{ objectId: 9, path: "a&b.tx" }]);
	assert.deepEqual(templateReferences("<object id=\"3\" template=\"a&amp;b.tx\"/>"), [{ objectId: 3, path: "a&b.tx" }]);
	assert.deepEqual(templateReferences("{broken"), []);
	assert.deepEqual(check(path.join(IMPORTER_FIXTURES, "templates.tmx")), []);
});

test("mask families", () => {
	assert.equal(maskFamily("collision_layer", false), "2d_physics");
	assert.equal(maskFamily("collision_mask_3", true), "2d_physics");
	assert.equal(maskFamily("navigation_layers", false), "2d_navigation");
	assert.equal(maskFamily("layers_1", true), "2d_navigation");
	assert.equal(maskFamily("layers", false), "");
	assert.equal(maskFamily("light_mask", false), "2d_render");
	assert.equal(maskFamily("occluder_light_mask", false), "2d_render");
	assert.equal(maskFamily("hp", false), "");
});
