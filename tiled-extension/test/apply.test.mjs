// lib/apply.mjs against the fake API: add, update, keep, remove; the file patch for what the API
// cannot set (flags, float members).
import { test, after } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { applyDefinitions, patchProjectText, mergeFixups, usageFlagsFor, storageTypeValue } from "../yati-godot/lib/apply.mjs";
import { syncDefinitions, vectorClassDefinition } from "../yati-godot/lib/types.mjs";
import { readGodotProject } from "../yati-godot/lib/godot_project.mjs";
import { createFakeTiled, tiledJson } from "./helpers/fake_tiled.mjs";
import { io, GODOT_FIXTURE, tempDir } from "./helpers/node_io.mjs";

const dirs = [];
after(() => { for (const d of dirs) fs.rmSync(d, { recursive: true, force: true }); });

function project(types, properties) {
	const dir = tempDir();
	dirs.push(dir);
	const file = path.join(dir, "game.tiled-project");
	fs.writeFileSync(file, tiledJson({
		automappingRulesFile: "", commands: [], compatibilityVersion: 1100, extensionsPath: "extensions",
		folders: ["."], properties: properties || [], propertyTypes: types || [],
	}));
	return { dir: dir, file: file, fake: createFakeTiled({ projectFile: file }) };
}

function typesIn(file) {
	return JSON.parse(fs.readFileSync(file, "utf8")).propertyTypes;
}

const SYNC = syncDefinitions(readGodotProject(io.readText(path.join(GODOT_FIXTURE, "project.godot")))).defs;

test("a new project: every type created; the flags are left to the file patch", () => {
	const p = project();
	const r = applyDefinitions(p.fake.api, SYNC, {}, p.dir);
	assert.deepEqual(r.created, ["GodotPhysicsLayers", "GodotNavigationLayers", "GodotRenderLayers", "GodotNodeType", "MapRef"]);
	assert.deepEqual(r.errors, []);
	assert.deepEqual(r.fixups.flagsEnums, ["GodotPhysicsLayers", "GodotNavigationLayers", "GodotRenderLayers"]);
	assert.deepEqual(r.record, { GodotPhysicsLayers: [], GodotNavigationLayers: [], GodotRenderLayers: [], GodotNodeType: [], MapRef: ["map", "object"] });
	const types = typesIn(p.file);
	assert.equal(types[0].valuesAsFlags, false);	// the API cannot set it
	assert.equal(types[0].storageType, "int");
	assert.deepEqual(types[4].useAs, ["property"]);
	// Applying again changes nothing and saves nothing.
	const saves = p.fake.project.saves;
	const again = applyDefinitions(p.fake.api, SYNC, r.record, p.dir);
	assert.deepEqual(again.unchanged, r.created);
	assert.equal(p.fake.project.saves, saves);
});

test("regeneration replaces an enum's values in place (same id)", () => {
	const p = project([{ id: 7, name: "GodotPhysicsLayers", storageType: "string", type: "enum", values: ["Old"], valuesAsFlags: true }]);
	const r = applyDefinitions(p.fake.api, SYNC.slice(0, 1), { GodotPhysicsLayers: [] }, p.dir);
	assert.deepEqual(r.updated, ["GodotPhysicsLayers"]);
	const t = typesIn(p.file)[0];
	assert.equal(t.id, 7);
	assert.deepEqual(t.values, ["World", "Player", "Layer 3", "Enemy"]);
	assert.equal(t.storageType, "int");
	assert.equal(t.valuesAsFlags, true);	// what the project file had stays
});

test("merge by name: hand-made members kept, generated ones updated and removed, colour kept", () => {
	const p = project([{
		color: "#ff112233", drawFill: false, id: 1, name: "Enemy", type: "class", useAs: ["object"],
		members: [
			{ name: "hp", type: "int", value: 99 },			// hand-made, also exported: kept
			{ name: "note", type: "string", value: "mine" },	// hand-made, not exported: kept
			{ name: "old", type: "int", value: 1 },			// generated before, gone now: removed
			{ name: "speed", type: "float", value: 1.5 },		// generated before: updated
		],
	}]);
	const def = {
		type: "class", name: "Enemy", useAs: ["object", "tile"], color: "#ffabcdef", drawFill: true,
		members: [
			{ name: "godot_scene", type: "file", value: "scenes/enemy.tscn" },
			{ name: "hp", type: "int", value: 1 },
			{ name: "speed", type: "float", value: 2.5 },
		],
	};
	const r = applyDefinitions(p.fake.api, [def], { Enemy: ["old", "speed"] }, p.dir);
	assert.deepEqual(r.updated, ["Enemy"]);
	assert.deepEqual(r.kept, [{ type: "Enemy", member: "hp" }]);
	assert.deepEqual(r.removed, [{ type: "Enemy", member: "old" }]);
	assert.deepEqual(r.record.Enemy, ["godot_scene", "speed"]);
	const t = typesIn(p.file)[0];
	assert.equal(t.color, "#ff112233");
	assert.equal(t.drawFill, false);
	assert.deepEqual(t.useAs, ["object", "tile"]);	// the missing usage added, none taken away
	assert.deepEqual(t.members, [
		{ name: "godot_scene", type: "file", value: "scenes/enemy.tscn" },
		{ name: "hp", type: "int", value: 99 },
		{ name: "note", type: "string", value: "mine" },
		{ name: "speed", type: "float", value: 2.5 },
	]);
});

test("a new class gets the definition's colour, fill and usage; whole floats become int until patched", () => {
	const p = project();
	const r = applyDefinitions(p.fake.api, [vectorClassDefinition("Vector2")], {}, p.dir);
	const t = typesIn(p.file)[0];
	assert.equal(t.color, vectorClassDefinition("Vector2").color);
	assert.deepEqual(t.useAs, ["property"]);
	assert.deepEqual(t.members.map((m) => m.type), ["int", "int"]);	// setMember(0) stores an int
	assert.deepEqual(r.fixups.memberTypes, [
		{ type: "Vector2", member: "x", memberType: "float" },
		{ type: "Vector2", member: "y", memberType: "float" },
	]);
	const patched = patchProjectText(fs.readFileSync(p.file, "utf8"), r.fixups);
	assert.deepEqual(patched.changes, ["Vector2.x: float", "Vector2.y: float"]);
	assert.deepEqual(JSON.parse(patched.text).propertyTypes[0].members.map((m) => m.type), ["float", "float"]);
});

test("enum and class members by value: enum by name, class by members, file, object, colour", () => {
	const p = project();
	const defs = [
		{ type: "enum", name: "Mood", storageType: "string", values: ["CALM", "ANGRY"], valuesAsFlags: false },
		vectorClassDefinition("Vector2"),
		{
			type: "class", name: "Thing", useAs: ["object", "tile"], color: "#ff000000", drawFill: true,
			members: [
				{ name: "mood", type: "string", value: "ANGRY", propertyType: "Mood" },
				{ name: "at", type: "class", value: { x: 1, y: 2.5 }, propertyType: "Vector2" },
				{ name: "scene", type: "file", value: "../x/a.tscn" },
				{ name: "none", type: "file", value: "" },
				{ name: "target", type: "object", value: 0 },
				{ name: "tint", type: "color", value: "#80ff0000" },
				{ name: "flag", type: "bool", value: true },
			],
		},
	];
	const r = applyDefinitions(p.fake.api, defs, {}, p.dir);
	const thing = typesIn(p.file)[2];
	assert.deepEqual(thing.members, [
		{ name: "at", propertyType: "Vector2", type: "class", value: { x: 1, y: 2.5 } },
		{ name: "flag", type: "bool", value: true },
		{ name: "mood", propertyType: "Mood", type: "string", value: "ANGRY" },
		{ name: "none", type: "file", value: "" },
		{ name: "scene", type: "file", value: "../x/a.tscn" },
		{ name: "target", type: "object", value: 0 },
		{ name: "tint", type: "color", value: "#80ff0000" },
	]);
	const saves = p.fake.project.saves;
	const again = applyDefinitions(p.fake.api, defs, r.record, p.dir);
	assert.deepEqual(again.unchanged, ["Mood", "Vector2", "Thing"]);
	assert.equal(p.fake.project.saves, saves);
});

test("a name taken by the other kind of type is an error, not a change", () => {
	const p = project([{ id: 1, name: "MapRef", storageType: "string", type: "enum", values: ["a"], valuesAsFlags: false }]);
	const r = applyDefinitions(p.fake.api, SYNC, {}, p.dir);
	assert.deepEqual(r.errors, ["'MapRef' is an enum in this project; the class was not generated."]);
	assert.equal(typesIn(p.file)[0].type, "enum");
});

test("patch: flags and member types, as Tiled writes the file; nothing to do keeps the text", () => {
	const text = tiledJson({
		propertyTypes: [
			{ id: 1, name: "L", storageType: "int", type: "enum", values: ["a"], valuesAsFlags: false },
			{ id: 2, name: "C", type: "class", color: "#ff000000", drawFill: true, useAs: ["object"],
				members: [{ name: "f", type: "int", value: 2 }, { name: "i", type: "float", value: 2.4 }, { name: "e", propertyType: "L", type: "int", value: 1 }] },
		],
	});
	const fix = { flagsEnums: ["L", "Missing"], memberTypes: [{ type: "C", member: "f", memberType: "float" }, { type: "C", member: "i", memberType: "int" }, { type: "C", member: "e", memberType: "float" }] };
	const r = patchProjectText(text, fix);
	assert.deepEqual(r.changes, ["L: values as flags", "C.f: float", "C.i: int"]);
	assert.deepEqual(r.missing, ["Missing"]);
	const j = JSON.parse(r.text);
	assert.equal(j.propertyTypes[0].valuesAsFlags, true);
	assert.deepEqual(j.propertyTypes[1].members, [{ name: "f", type: "float", value: 2 }, { name: "i", type: "int", value: 2 }, { name: "e", propertyType: "L", type: "int", value: 1 }]);
	assert.equal(r.text, tiledJson(j));
	const same = patchProjectText(r.text, fix);
	assert.equal(same.text, r.text);
	assert.deepEqual(same.changes, []);
	assert.match(patchProjectText("{nope", fix).error, /no JSON/);
});

test("kept fixups: merged with a run's, pruned to what is still generated", () => {
	const kept = { flags: ["A"], floats: { T: ["x", "gone"], Old: ["y"] } };
	const run = { flagsEnums: ["B"], memberTypes: [{ type: "T", member: "x", memberType: "int" }, { type: "U", member: "z", memberType: "float" }] };
	const record = { T: ["x", "w"], U: ["z"] };
	const m = mergeFixups(kept, run, record);
	assert.deepEqual(m.kept, { flags: ["A", "B"], floats: { T: [], Old: [], U: ["z"] } });
	assert.deepEqual(m.patch.flagsEnums, ["A", "B"]);
	assert.deepEqual(m.patch.memberTypes, [{ type: "T", member: "x", memberType: "int" }, { type: "U", member: "z", memberType: "float" }]);
});

test("usage flags and storage types: the globals when Tiled has them, else 1.12's values", () => {
	assert.equal(usageFlagsFor({ ClassPropertyType: null }, ["object", "tile"]), 0x004 | 0x020);
	assert.equal(usageFlagsFor({ ClassPropertyType: { MapObjectClass: 64, TileClass: 128 } }, ["object", "tile"]), 192);
	assert.equal(usageFlagsFor({ ClassPropertyType: null }, ["property", "nonsense"]), 1);
	assert.equal(storageTypeValue({ StorageType: null }, "int"), 1);
	assert.equal(storageTypeValue({ StorageType: null }, "string"), 0);
	assert.equal(storageTypeValue({ StorageType: { IntValue: 5 } }, "int"), 5);
});
