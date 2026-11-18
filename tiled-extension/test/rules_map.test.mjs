// lib/rules.mjs: the rules about a map's own layers and properties. layer_in_map (the value names
// a layer of the same map, of a class and kind), the map scope ("map": true: the rule checks the
// map's own properties) and counterpart (an object has a twin on the layer group a member names,
// with members swapped), on the rules sample's maps/house.tmx (every reference fine) and
// maps/tower.tmx (one mistake of each), and the helpers the actions use.
import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import {
	parseRules, rulesForClass, mapRulesOf, ruleLabel, checkRules, layerChoices, findLayers, counterpartPlan, enclosingGroup,
	newFilePath, parseMapObjects,
} from "../yati-godot/lib/rules.mjs";
import { contextFromTypes } from "../yati-godot/lib/validate.mjs";
import { tmxModel, projectTypes, enumsOf } from "./helpers/tmx_model.mjs";
import { SAMPLE_DIR, io } from "./helpers/node_io.mjs";

const RULES_DIR = path.join(SAMPLE_DIR, "rules");
const PROJECT = path.join(RULES_DIR, "rules.tiled-project");

function sampleRules() {
	return JSON.parse(fs.readFileSync(PROJECT, "utf8")).properties.find((p) => p.name === "yati_rules").value;
}

function context() {
	const types = projectTypes(PROJECT);
	const base = contextFromTypes(types, PROJECT, "", (p) => fs.existsSync(p));
	return Object.assign(base, {
		projectFolders: [RULES_DIR],
		listDir: io.listDir,
		readText: io.readText,
		mapObjects: (p) => {
			const text = io.readText(p);
			return text === null ? { objects: [], error: "cannot read" } : parseMapObjects(text, p, io.readText);
		},
		enums: enumsOf(types),
	});
}

function model(name) {
	return tmxModel(path.join(RULES_DIR, "maps", name), context().enums);
}

const rulesOf = (list) => {
	const parsed = parseRules(JSON.stringify(list));
	assert.deepEqual(parsed.problems, []);
	return parsed.rules;
};

const SWAP = { direction: { UP: "DOWN", DOWN: "UP", LEFT: "RIGHT", RIGHT: "LEFT" } };

test("the sample's level and map rules parse: layer_in_map, counterpart, and two rules of the map's own properties", () => {
	const parsed = parseRules(sampleRules());
	assert.deepEqual(parsed.problems, []);
	assert.equal(parsed.rules.length, 13);
	const [layer, twin, sequence, name] = parsed.rules.slice(9);
	assert.deepEqual([layer.kind, layer.member, layer.classes, layer.targetClass, layer.layerType, layer.required, layer.scope],
		["layer_in_map", "target", ["stairs"], "level", "group", true, "class"]);
	assert.deepEqual([twin.kind, twin.targetClass, twin.swap], ["counterpart", "level", SWAP]);
	assert.deepEqual([sequence.scope, sequence.classes, sequence.world, sequence.member, sequence.kind, sequence.map], ["map", [], "", "sequence", "file", ""]);
	assert.equal(name.scope, "map");
	// The map's rules are no class's, and the class rules no map's.
	assert.deepEqual(rulesForClass(parsed.rules, "stairs").map((r) => r.index), [10, 11]);
	assert.deepEqual(rulesForClass(parsed.rules, "*").map((r) => r.index), []);
	assert.deepEqual(mapRulesOf(parsed.rules).map((r) => r.index), [12, 13]);
	assert.deepEqual(mapRulesOf(parsed.rules, ["key"]).map((r) => r.member), ["display_name"]);
	assert.equal(ruleLabel(layer), "target (a group layer of class level in this map)");
	assert.equal(ruleLabel(twin), "target (an object of the same class at the same place on the layer target names)");
	assert.equal(ruleLabel(sequence), "sequence (a file; the map's own property)");
	assert.equal(ruleLabel(rulesOf([{ "class": "x", member: "l", kind: "layer_in_map" }])[0]), "l (a layer in this map)");
	// object_in_map with "map": true: the map's own property names an object of the same map.
	const own = rulesOf([{ map: true, member: "start", kind: "object_in_map", targetClass: "spawn" }])[0];
	assert.deepEqual([own.scope, own.map, own.targetClass], ["map", "", "spawn"]);
});

test("a level or map rule with a mistake is left out and named", () => {
	const problems = (rule) => parseRules(JSON.stringify([rule])).problems;
	assert.deepEqual(problems({ "class": "s", member: "t", kind: "layer_in_map", layerType: "folder" }),
		["yati_rules, rule 1: \"layerType\" is one of group, tile, object, image."]);
	assert.deepEqual(problems({ world: "w.world", member: "t", kind: "layer_in_map" }),
		["yati_rules, rule 1: layer_in_map names a layer of the map: give \"class\" or \"map\": true, not \"world\"."]);
	assert.deepEqual(problems({ "class": "s", map: true, member: "t", kind: "key", table: "t.csv" }),
		["yati_rules, rule 1: give one of \"class\" (the objects' class, a list of classes, or \"*\"), \"world\" (a .world file) or \"map\": true (the map's own properties)."]);
	assert.deepEqual(problems({ member: "t", kind: "key", table: "t.csv" }),
		["yati_rules, rule 1: give one of \"class\" (the objects' class, a list of classes, or \"*\"), \"world\" (a .world file) or \"map\": true (the map's own properties)."]);
	assert.deepEqual(problems({ map: "yes", member: "t", kind: "file" }),
		["yati_rules, rule 1: \"map\" is true (the rule checks the map's own properties); kind file has no member that names a map."]);
	assert.deepEqual(problems({ map: true, kind: "unique_name" }),
		["yati_rules, rule 1: unique_name applies to objects: give \"class\" (a class name or a list), not \"map\"."]);
	assert.deepEqual(problems({ map: true, member: "t", kind: "counterpart" }),
		["yati_rules, rule 1: counterpart applies to objects: give \"class\" (a class name or a list)."]);
	assert.deepEqual(problems({ "class": "s", member: "t", kind: "counterpart", swap: { direction: "DOWN" } }),
		["yati_rules, rule 1: \"swap\" is {\"member\": {\"value\": \"its counterpart's value\", ...}, ...}."]);
	assert.deepEqual(problems({ "class": "s", member: "t", kind: "counterpart", swap: { t: { a: "b" } } }),
		["yati_rules, rule 1: \"swap\" cannot name the member the rule checks (t): the counterpart's t names this object's layer."]);
	assert.deepEqual(problems({ "class": "s", member: "t", kind: "counterpart", layerType: "group" }),
		["yati_rules, rule 1: unknown field \"layerType\" for kind counterpart (it takes kind, member, class, world, map, required, severity, targetClass, swap)."]);
	// An object_in_map rule's "map" is still a member's name.
	assert.equal(rulesOf([{ "class": "w", member: "s", kind: "object_in_map", map: "m" }])[0].scope, "class");
});

test("house.tmx: the stairs, the levels and the map's own properties are fine", () => {
	const ctx = context();
	assert.deepEqual(checkRules(model("house.tmx"), parseRules(sampleRules()).rules, ctx), []);
});

test("tower.tmx: the map's own properties first, then a layer that is not there or of another kind, stairs without their other half", () => {
	const ctx = context();
	const found = checkRules(model("tower.tmx"), parseRules(sampleRules()).rules, ctx);
	assert.deepEqual(found.map((f) => f.severity + " " + f.code + " #" + f.objectId + " " + f.text), [
		"error rule-missing-content #0 Map properties: sequence names notice.xml, which has no section named on_enter or on_leave.",
		"error rule-unknown-key #0 Map properties: display_name is 'MAP_HOUS', shaped like a key, but strings.csv has no such key (did you mean MAP_HOUSE?).",
		"error rule-missing-counterpart #2 Object 'lone' (id 2, layer ground/Objects): target names the layer 'first', which has no object of class stairs overlapping this one with target 'ground' and direction 'DOWN' (YATI: Create counterpart makes one).",
		"error rule-missing-layer #3 Object 'roof' (id 3, layer ground/Objects): target names 'roof', but this map has no group layer of that name and class level (its group layers of class level: ground, first).",
		"error rule-missing-layer #5 Object 'floorish' (id 5, layer ground/Objects): target names 'Floor', but this map has no group layer of that name and class level (its group layers of class level: ground, first; the layer 'Floor' is a tile layer of class none).",
		"error rule-missing-counterpart #6 Object 'same' (id 6, layer ground/Objects): target names the layer 'first', which has no object of class stairs overlapping this one with target 'ground' and direction 'RIGHT' (YATI: Create counterpart makes one).",
		"error rule-missing-counterpart #7 Object 'same_up' (id 7, layer first/Objects): target names the layer 'ground', which has no object of class stairs overlapping this one with target 'first' and direction 'RIGHT' (YATI: Create counterpart makes one).",
	]);
});

test("layer_in_map: any kind or class when not given, required, a warning severity; the map scope", () => {
	const ctx = context();
	const tower = model("tower.tmx");
	const any = rulesOf([{ "class": "stairs", member: "target", kind: "layer_in_map" }]);
	// Floor and roof: only roof is missing now.
	assert.deepEqual(checkRules(tower, any, ctx).map((f) => f.objectId + " " + f.text), [
		"3 Object 'roof' (id 3, layer ground/Objects): target names 'roof', but this map has no layer of that name (its layers: ground, Objects, first, decor, Backdrop, Floor).",
	]);
	const tiles = rulesOf([{ "class": "stairs", member: "target", kind: "layer_in_map", layerType: "tile", severity: "warning" }]);
	assert.deepEqual(checkRules(tower, tiles, ctx).filter((f) => f.objectId === 1).map((f) => f.severity + " " + f.text), [
		"warning Object 'up' (id 1, layer ground/Objects): target names 'first', but this map has no tile layer of that name (its tile layers: Floor; the layer 'first' is a group layer of class level).",
	]);
	const required = rulesOf([{ "class": "stairs", member: "nothing", kind: "layer_in_map", required: true }]);
	assert.match(checkRules(tower, required, ctx)[0].text, /nothing is not set; the project's rules \(yati_rules\) require a layer's name\.$/);
	const mapScope = rulesOf([{ map: true, member: "display_name", kind: "layer_in_map", layerType: "group" }]);
	assert.deepEqual(checkRules(tower, mapScope, ctx).map((f) => f.objectId + " " + f.text), [
		"0 Map properties: display_name names 'MAP_HOUS', but this map has no group layer of that name (its group layers: ground, first, decor).",
	]);
});

test("the map scope: a class's defaults for the map's class, required, an unsaved map, excluded maps still checked", () => {
	const ctx = context();
	ctx.classes.house_map = { members: [{ name: "display_name", type: "string", value: "NO_SUCH_KEY", fromClass: true }] };
	const town = model("town.tmx");
	const rules = rulesOf([
		{ map: true, member: "display_name", kind: "key", table: "text/strings.csv" },
		{ map: true, member: "sequence", kind: "file", required: true },
	]);
	assert.deepEqual(checkRules(town, rules, ctx).map((f) => f.text), [
		"Map properties: sequence is not set; the project's rules (yati_rules) require a file.",
	]);
	town.className = "house_map";
	assert.deepEqual(checkRules(town, rules, ctx).map((f) => f.code), ["rule-unknown-key", "rule-required"]);
	// A map's file property is relative to the map; with yati_rules_exclude the map's rules still run.
	const tower = model("tower.tmx");
	const excluded = Object.assign({}, ctx, { exclude: ["maps"] });
	assert.deepEqual(checkRules(tower, rules, excluded).map((f) => f.code), ["rule-unknown-key"]);
});

test("counterpart: overlapping is enough, the layers' offsets count, a value outside swap must be equal, tile objects' rectangles", () => {
	const ctx = context();
	const rules = rulesOf([{ "class": "stairs", member: "target", kind: "counterpart", targetClass: "level", swap: SWAP }]);
	const tower = model("tower.tmx");
	// Moved by the upper level's offset, 'down' no longer overlaps 'up': both report.
	const first = tower.layers.find((l) => l.name === "first");
	first.offsetX = 40;
	assert.deepEqual(checkRules(tower, rules, ctx).map((f) => f.objectId), [1, 2, 6, 4, 7]);
	first.offsetX = 0;
	// A direction outside the swap table: the counterpart must have the same one.
	tower.objects.find((o) => o.id === 6).properties.find((p) => p.name === "direction").value = "NORTH";
	tower.objects.find((o) => o.id === 7).properties.find((p) => p.name === "direction").value = "NORTH";
	assert.deepEqual(checkRules(tower, rules, ctx).map((f) => f.objectId), [2]);
	// Without targetClass, the innermost group the object lies in is its own.
	const loose = rulesOf([{ "class": "stairs", member: "target", kind: "counterpart", swap: SWAP }]);
	assert.deepEqual(checkRules(model("tower.tmx"), loose, ctx).map((f) => f.objectId), [2, 6, 7]);
	// A tile object's rectangle is above its y (Tiled keeps a tile object's bottom-left corner).
	const house = model("house.tmx");
	const down = house.objects.find((o) => o.id === 2);
	down.isTile = true;
	down.y = 96;
	assert.deepEqual(checkRules(house, rules, ctx), []);
	down.y = 64;
	assert.deepEqual(checkRules(house, rules, ctx).map((f) => f.objectId), [1, 2]);
});

test("the actions' helpers: the layers Pick offers, the layer Follow selects, the counterpart Create makes", () => {
	const ctx = context();
	const tower = model("tower.tmx");
	const rules = parseRules(sampleRules()).rules;
	const layerRule = rules[9];
	const twinRule = rules[10];
	assert.deepEqual(layerChoices(tower.layers, layerRule), ["ground", "first"]);
	assert.deepEqual(layerChoices(tower.layers, rulesOf([{ "class": "s", member: "t", kind: "layer_in_map" }])[0]), ["ground", "Objects", "first", "decor", "Backdrop", "Floor"]);
	assert.deepEqual(findLayers(tower.layers, layerRule, "first").map((l) => l.id), [3]);
	assert.deepEqual(findLayers(tower.layers, layerRule, "Floor"), []);
	assert.equal(enclosingGroup(tower, tower.objects.find((o) => o.id === 7), "level").name, "first");
	assert.equal(enclosingGroup(tower, tower.objects.find((o) => o.id === 7), "nothing"), null);

	const lone = tower.objects.find((o) => o.id === 2);
	assert.deepEqual(counterpartPlan(twinRule, tower, lone, ctx.classes), {
		error: "", existing: null, groupId: 3, groupName: "first", layerId: 4, layerName: "first/Objects", x: 160, y: 64,
		values: [{ name: "target", value: "ground", propertyType: "" }, { name: "direction", value: "DOWN", propertyType: "facing" }],
	});
	const up = tower.objects.find((o) => o.id === 1);
	assert.deepEqual(counterpartPlan(twinRule, tower, up, ctx.classes).existing, { id: 4, name: "down", layerPath: "first/Objects" });
	assert.match(counterpartPlan(twinRule, tower, tower.objects.find((o) => o.id === 3), ctx.classes).error, /^target names 'roof', but this map has no group layer of that name and class level/);
	const outside = Object.assign({}, lone, { layerId: 99 });
	assert.equal(counterpartPlan(twinRule, tower, outside, ctx.classes).error, "the object lies in no group layer of class level, so its counterpart's target cannot name its layer.");
	const own = Object.assign({}, lone, { properties: [{ name: "target", type: "string", value: "ground", propertyType: "" }] });
	assert.equal(counterpartPlan(twinRule, tower, own, ctx.classes).error, "target names the object's own layer 'ground'.");
	// The target group with no object layer; the same name as the source's object layer first.
	const noLayer = model("tower.tmx");
	noLayer.layers = noLayer.layers.filter((l) => l.id !== 4);
	assert.equal(counterpartPlan(twinRule, noLayer, lone, ctx.classes).error, "the layer 'first' holds no object layer to put the counterpart in; add one.");
	const two = model("tower.tmx");
	two.layers.push({ id: 20, name: "Objects", path: "first/Objects", className: "", properties: [], type: "object", parentId: 3, offsetX: 0, offsetY: 0 });
	two.layers.find((l) => l.id === 4).name = "Decor";
	assert.equal(counterpartPlan(twinRule, two, lone, ctx.classes).layerId, 20);
	// An offset between the two layers: the counterpart lands where the object is shown.
	const shifted = model("tower.tmx");
	shifted.layers.find((l) => l.id === 3).offsetX = 16;
	assert.deepEqual([counterpartPlan(twinRule, shifted, lone, ctx.classes).x, counterpartPlan(twinRule, shifted, lone, ctx.classes).y], [144, 64]);

	// A map's new file from a template: named after the map alone.
	const fileRule = rules[11];
	assert.equal(newFilePath(fileRule, null, "/p/maps/town.tmx", "/p"), "/p/sequences/town.xml");
});
