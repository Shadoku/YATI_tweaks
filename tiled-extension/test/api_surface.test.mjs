// The Tiled API the extension uses is exactly the documented surface of api_surface.mjs: every
// action, the save hook and the Issues' activation are run against the fake, which throws on any
// member outside the list; then every listed member must have been used (so the list is the
// extension's real surface, each entry with the reference page that documents it).
import { test, after } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { API_SURFACE, REFERENCE } from "./helpers/api_surface.mjs";
import { ACTIONS } from "../yati-godot/yati-godot.mjs";
import { scenario, CHECK_OUTPUT, gameProjectTypes } from "./helpers/scenario.mjs";

const scenarios = [];
after(() => { for (const s of scenarios) s.cleanup(); });

test("every member the extension touches is documented, and every documented one is used", () => {
	const used = new Set();
	const run = (s) => { scenarios.push(s); return s; };

	// Configure, sync, scenes twice (the second after an export was removed), on a fresh project.
	const a = run(scenario({ checkScript: true, fakeOptions: { process: () => ({ code: 1, stdout: CHECK_OUTPUT, stderr: "" }) } }));
	a.fake.answers.directories.push(a.godot, "");
	a.fake.answers.prompts.push("godot", "warn_unknown_classes=false");
	a.fake.trigger(ACTIONS.configure);
	a.fake.trigger(ACTIONS.sync);
	a.fake.trigger(ACTIONS.scenes);
	const door = path.join(a.godot, "scripts", "door.gd");
	fs.writeFileSync(door, fs.readFileSync(door, "utf8").replace("@export var width: float = 32.0\n", ""));
	a.fake.trigger(ACTIONS.scenes);
	assert.ok(a.fake.out.alerts[3].indexOf("Removed Door.width") >= 0);
	// The check on a modified map (saved first), and the activation of its Issues.
	const map = a.fake.openFile(path.join(a.maps, "mistakes.tmx"));
	map._modified = true;
	a.fake.trigger(ACTIONS.check);
	for (const issue of a.fake.out.issues)
		issue.activate();
	for (const k of a.fake.used)
		used.add(k);

	// The save hook with the game's types, on a map and a tileset; Issues activated with the map closed.
	const b = run(scenario({ inMaps: true, properties: [{ name: "godot_project", type: "string", value: "../godot_project" }], propertyTypes: gameProjectTypes() }));
	b.fake.emitAboutToBeSaved(b.fake.openFile(path.join(b.maps, "mistakes.tmx")));
	b.fake.emitAboutToBeSaved(b.fake.openFile(path.join(b.maps, "mistakes.tsx")));
	b.fake.closeAll();
	for (const issue of b.fake.out.issues)
		issue.activate();
	for (const k of b.fake.used)
		used.add(k);

	const documented = Object.keys(API_SURFACE).sort();
	const touched = Array.from(used).sort();
	for (const k of touched)
		assert.ok(API_SURFACE[k] !== undefined, k + " is not documented in api_surface.mjs");
	assert.deepEqual(documented.filter((k) => !used.has(k)), [], "listed but not used: the list should be exactly what the extension uses");
	assert.equal(touched.length, documented.length);
	assert.equal(REFERENCE, "https://www.mapeditor.org/docs/scripting/");
	for (const k of documented)
		assert.match(API_SURFACE[k], /^(modules|classes|interfaces|enums)\/[A-Za-z]+\.html#[A-Za-z]+/, k);
});

test("the fake implements nothing beyond the documented surface", async () => {
	// Every member a fake object exposes to the extension is listed (the wrapper throws on the rest,
	// so this only guards the fake itself against growing past the reference).
	const { createFakeTiled } = await import("./helpers/fake_tiled.mjs");
	const f = createFakeTiled({});
	const tiledMembers = Object.keys(Object.getOwnPropertyDescriptors(Object.getPrototypeOf(f.api.tiled.__fake) === Object.prototype ? f.api.tiled.__fake : {}));
	for (const m of tiledMembers)
		assert.ok(API_SURFACE["tiled." + m] !== undefined, "tiled." + m);
	const fileMembers = Object.keys(f.api.File.__fake);
	for (const m of fileMembers)
		assert.ok(API_SURFACE["File." + m] !== undefined, "File." + m);
});
