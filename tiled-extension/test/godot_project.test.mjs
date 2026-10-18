// lib/godot_project.mjs: project.godot's sections and layer names.
import { test } from "node:test";
import assert from "node:assert/strict";
import path from "node:path";
import { parseConfigFile, parseGodotString, readGodotProject, layerEnumValues } from "../yati-godot/lib/godot_project.mjs";
import { io, GODOT_FIXTURE } from "./helpers/node_io.mjs";

test("config file: sections, multi-line values, junk lines", () => {
	const cfg = parseConfigFile(io.readText(path.join(GODOT_FIXTURE, "project.godot")));
	assert.equal(cfg.sections[""].config_version, "5");
	assert.deepEqual(cfg.order, ["", "application", "input", "layer_names", "rendering"]);
	// The [input] value spans lines and holds a "[not a section]" string: still one value.
	assert.match(cfg.sections.input.jump, /^\{\n"deadzone": 0\.5,/);
	assert.match(cfg.sections.input.jump, /\[not a section\]"\]\n\}$/);
	assert.equal(cfg.sections.rendering["textures/canvas_textures/default_texture_filter"], "0");
	const junk = parseConfigFile("no equals\n=novalue\n[s]\n; comment\nk = \"v\"\n");
	assert.deepEqual(junk.sections.s, { k: "\"v\"" });
});

test("strings: Godot's escapes and prefixes", () => {
	assert.equal(parseGodotString("\"a \\\"b\\\" \\\\ c\\n\""), "a \"b\" \\ c\n");
	assert.equal(parseGodotString("&\"name\""), "name");
	assert.equal(parseGodotString("42"), "42");
});

test("layer names: the three families, others ignored", () => {
	const info = readGodotProject(io.readText(path.join(GODOT_FIXTURE, "project.godot")));
	assert.equal(info.name, "Extension Fixture");
	assert.deepEqual(info.layers["2d_physics"], { 1: "World", 2: "Player", 4: "Enemy" });
	assert.deepEqual(info.layers["2d_navigation"], { 1: "Ground" });
	assert.deepEqual(info.layers["2d_render"], { 2: "Lights" });
	assert.equal(info.layers["3d_physics"], undefined);
	// The real game's project.godot has one physics layer name (goo_pachinko: walls).
	const game = readGodotProject("[layer_names]\n\n2d_physics/layer_1=\"walls\"\n");
	assert.deepEqual(game.layers["2d_physics"], { 1: "walls" });
	assert.deepEqual(readGodotProject("").layers["2d_render"], {});
});

test("layer enum values: index = bit, gaps named, duplicates told apart, the family's limit", () => {
	assert.deepEqual(layerEnumValues({ 1: "World", 2: "Player", 4: "Enemy" }, 32), ["World", "Player", "Layer 3", "Enemy"]);
	assert.deepEqual(layerEnumValues({ 2: "Lights" }, 20), ["Layer 1", "Lights"]);
	assert.deepEqual(layerEnumValues({ 1: "A", 2: "A", 3: "Layer 1" }, 32), ["A", "A (2)", "Layer 1"]);
	assert.deepEqual(layerEnumValues({ 21: "Too far" }, 20), []);
	assert.deepEqual(layerEnumValues({}, 32), []);
});
