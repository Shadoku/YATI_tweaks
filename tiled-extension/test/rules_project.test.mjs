// lib/rules.mjs: the rule kinds that look across every map of the project: object_in_project (the
// value names an object of a class in any map) and unique_name (objects of the classes have names no
// other such object in the project has); how the project's maps are found (its folders, or a rule's
// `maps` folder or glob), .tmx and .tmj, and the cost on a project of a few hundred maps.
import { test, after } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import {
	parseRules, rulesForClass, ruleLabel, parseMapObjects, checkRules, checkWorld, parseWorld, projectMapFiles, projectObjects,
	projectObjectChoices,
} from "../yati-godot/lib/rules.mjs";
import { io, tempDir } from "./helpers/node_io.mjs";

const dirs = [];
after(() => { for (const d of dirs) fs.rmSync(d, { recursive: true, force: true }); });

const brief = (f) => f.severity + " " + f.code + " #" + f.objectId;

// A map as Tiled writes it: [{id, name, cls}] on one object layer.
function tmx(objects) {
	return "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<map version=\"1.10\" orientation=\"orthogonal\" width=\"4\" height=\"4\" tilewidth=\"16\" tileheight=\"16\">\n" +
		" <objectgroup id=\"1\" name=\"Objects\">\n" +
		objects.map((o) => "  <object id=\"" + o.id + "\"" + (o.name ? " name=\"" + o.name + "\"" : "") + (o.cls ? " class=\"" + o.cls + "\"" : "") +
			" x=\"0\" y=\"0\" width=\"16\" height=\"16\">" +
			(o.to !== undefined ? "<properties><property name=\"to\" value=\"" + o.to + "\"/></properties>" : "") + "</object>\n").join("") +
		" </objectgroup>\n</map>\n";
}

function tmj(objects) {
	return JSON.stringify({ type: "map", layers: [{ type: "objectgroup", name: "Objects", objects: objects.map((o) => ({
		id: o.id, name: o.name || "", "class": o.cls || "", x: 0, y: 0,
		properties: o.to !== undefined ? [{ name: "to", type: "string", value: o.to }] : undefined,
	})) }] });
}

// A project in a temporary directory: files {relative path: text}; the project file lists `folders`.
function project(files, folders) {
	const root = tempDir("yati-rules-project-");
	dirs.push(root);
	fs.writeFileSync(path.join(root, "game.tiled-project"), JSON.stringify({ folders: folders || ["."], properties: [], propertyTypes: [] }));
	for (const rel of Object.keys(files)) {
		fs.mkdirSync(path.dirname(path.join(root, rel)), { recursive: true });
		fs.writeFileSync(path.join(root, rel), files[rel]);
	}
	return root;
}

// The context checkRules reads, as the glue fills it, with the reads counted.
function context(root, folders) {
	const counts = { mapObjects: 0, listDir: 0 };
	return {
		counts: counts,
		projectFile: path.join(root, "game.tiled-project"),
		projectFolders: (folders || ["."]).map((f) => path.join(root, f)),
		godotProject: "",
		classes: {},
		fileExists: (p) => fs.existsSync(p),
		readText: io.readText,
		listDir: (d) => { counts.listDir++; return io.listDir(d); },
		mapObjects: (p) => {
			counts.mapObjects++;
			const text = io.readText(p);
			return text === null ? { objects: [], error: "it does not exist" } : parseMapObjects(text, p, io.readText);
		},
	};
}

// A map model as the glue makes it from an open map (validate.mjs's model).
function model(file, objects) {
	return { kind: "map", fileName: file, properties: [], layers: [], templates: [], objects: objects.map((o) => ({
		id: o.id, name: o.name || "", layerPath: "Objects", className: o.cls || "", tileClassName: "",
		properties: o.to !== undefined ? [{ name: "to", type: "string", value: o.to, propertyType: "" }] : [],
	})) };
}

const GAME_RULES = [
	{ "class": ["Warp", "PlayerSpawn"], kind: "unique_name", required: true },
	{ "class": "Warp", member: "to", kind: "object_in_project", targetClass: ["Warp", "PlayerSpawn"] },
];

function threeMaps() {
	return project({
		"maps/town.tmx": tmx([{ id: 1, name: "town_start", cls: "PlayerSpawn" }, { id: 2, name: "to_cellar", cls: "Warp", to: "cellar_up" }]),
		"maps/cellar/cellar.tmj": tmj([{ id: 1, name: "cellar_up", cls: "Warp", to: "to_cellar" }, { id: 2, name: "crate", cls: "Box" }]),
		"maps/coast/coast.tmx": tmx([{ id: 4, name: "beach", cls: "PlayerSpawn" }, { id: 5, name: "crate", cls: "Sign" }]),
		".hidden/old.tmx": tmx([{ id: 1, name: "town_start", cls: "PlayerSpawn" }]),
		"maps/notes.txt": "not a map",
	});
}

test("object_in_project and unique_name parse; their mistakes are named", () => {
	const parsed = parseRules(JSON.stringify(GAME_RULES.concat([
		{ "class": "Door", member: "to", kind: "object_in_project", targetClass: "Door", maps: "maps/**/*.tmx", required: true, severity: "warning" },
		{ "class": "Door", kind: "unique_name", maps: "maps" },
	])));
	assert.deepEqual(parsed.problems, []);
	const [unique, to, door, doorUnique] = parsed.rules;
	assert.deepEqual([unique.kind, unique.member, unique.classes, unique.required, unique.maps], ["unique_name", "", ["Warp", "PlayerSpawn"], true, ""]);
	assert.deepEqual([to.kind, to.member, to.targetClasses, to.maps], ["object_in_project", "to", ["Warp", "PlayerSpawn"], ""]);
	assert.deepEqual([door.targetClasses, door.maps, door.required, door.severity], [["Door"], "maps/**/*.tmx", true, "warning"]);
	assert.equal(doorUnique.maps, "maps");
	assert.equal(ruleLabel(to), "to (an object of class Warp or PlayerSpawn in any map of the project)");
	assert.equal(ruleLabel(unique), "names (unique among Warp and PlayerSpawn objects in the project)");
	assert.deepEqual(rulesForClass(parsed.rules, "Warp").map((r) => r.index), [1, 2]);
	assert.deepEqual(rulesForClass(parsed.rules, "PlayerSpawn", ["unique_name"]).map((r) => r.index), [1]);
	// object_in_map keeps its targetClass as one name.
	assert.deepEqual(parseRules("[{\"class\": \"w\", \"member\": \"s\", \"kind\": \"object_in_map\", \"targetClass\": \"p\"}]").rules[0].targetClasses, ["p"]);
	const cases = [
		["{\"class\": \"a\", \"kind\": \"unique_name\", \"member\": \"m\"}", /unique_name checks the objects' own names; it takes no "member"/],
		["{\"world\": \"w.world\", \"kind\": \"unique_name\"}", /unique_name applies to objects: give "class"/],
		["{\"class\": \"a\", \"kind\": \"unique_name\", \"targetClass\": \"b\"}", /unknown field "targetClass" for kind unique_name/],
		["{\"class\": \"a\", \"member\": \"m\", \"kind\": \"object_in_project\", \"map\": \"x\"}", /unknown field "map" for kind object_in_project/],
		["{\"class\": \"a\", \"member\": \"m\", \"kind\": \"object_in_project\", \"targetClass\": []}", /"targetClass" is a class name or a list of class names/],
		["{\"class\": \"a\", \"member\": \"m\", \"kind\": \"object_in_project\", \"targetClass\": [3]}", /"targetClass" is a class name or a list of class names/],
		["{\"class\": \"a\", \"member\": \"m\", \"kind\": \"object_in_project\", \"maps\": 1}", /"maps" is a string/],
		["{\"class\": \"a\", \"member\": \"m\", \"kind\": \"object_in_map\", \"targetClass\": [\"b\"]}", /"targetClass" is a string/],
	];
	for (const [rule, re] of cases) {
		const p = parseRules("[" + rule + "]");
		assert.equal(p.problems.length, 1, rule + ": " + JSON.stringify(p.problems));
		assert.match(p.problems[0], re, rule);
	}
	assert.match(parseRules("[{\"class\": \"a\", \"kind\": \"map\"}]").problems[0], /"kind" is|"member" \(the member's name\) is missing/);
	assert.match(parseRules("[{\"class\": \"a\", \"member\": \"m\", \"kind\": \"nope\"}]").problems[0],
		/it is one of map, object_in_map, object_in_project, unique_name, file, key\./);
});

test("the project's maps: its folders, .tmx and .tmj, hidden folders and other files left out; a rule's maps folder or glob", () => {
	const root = threeMaps();
	const ctx = context(root);
	const rel = (list) => list.map((p) => path.relative(root, p).split(path.sep).join("/"));
	assert.deepEqual(rel(projectMapFiles("", root, ctx.projectFolders, ctx.listDir)), ["maps/cellar/cellar.tmj", "maps/coast/coast.tmx", "maps/town.tmx"]);
	assert.deepEqual(rel(projectMapFiles("maps/coast", root, ctx.projectFolders, ctx.listDir)), ["maps/coast/coast.tmx"]);
	assert.deepEqual(rel(projectMapFiles("maps/**/*.tmx", root, ctx.projectFolders, ctx.listDir)), ["maps/coast/coast.tmx", "maps/town.tmx"]);
	assert.deepEqual(rel(projectMapFiles("maps/*.tmx", root, ctx.projectFolders, ctx.listDir)), ["maps/town.tmx"]);
	assert.deepEqual(rel(projectMapFiles("maps/c*/*", root, ctx.projectFolders, ctx.listDir)), ["maps/cellar/cellar.tmj", "maps/coast/coast.tmx"]);
	assert.deepEqual(rel(projectMapFiles("maps/town.tmx", root, ctx.projectFolders, ctx.listDir)), ["maps/town.tmx"]);
	// Two folders that overlap give each map once; a folder that is not there gives none.
	assert.deepEqual(rel(projectMapFiles("", root, [path.join(root, "maps"), path.join(root, "maps", "coast"), path.join(root, "gone")], ctx.listDir)),
		["maps/cellar/cellar.tmj", "maps/coast/coast.tmx", "maps/town.tmx"]);
	// The objects by name, every map read once.
	const index = projectObjects(projectMapFiles("", root, ctx.projectFolders, ctx.listDir), ctx.mapObjects);
	assert.deepEqual(index.byName.crate.map((o) => [path.basename(o.file), o.id, o.className]), [["cellar.tmj", 2, "Box"], ["coast.tmx", 5, "Sign"]]);
	assert.deepEqual(index.errors, []);
	assert.equal(ctx.counts.mapObjects, 3);
});

test("object_in_project: an object of the target classes in any map; none, or more than one, is a finding naming the maps", () => {
	const root = threeMaps();
	const rules = parseRules(JSON.stringify(GAME_RULES)).rules;
	const town = path.join(root, "maps", "town.tmx");
	// The saved map as it is in Tiled (unsaved changes included): its own objects count, not its file's.
	const m = model(town, [
		{ id: 1, name: "town_start", cls: "PlayerSpawn" },
		{ id: 2, name: "to_cellar", cls: "Warp", to: "cellar_up" },		// in cellar.tmj (a .tmj)
		{ id: 3, name: "to_beach", cls: "Warp", to: "beach" },			// in coast.tmx
		{ id: 4, name: "to_start", cls: "Warp", to: "town_start" },		// in this map
		{ id: 5, name: "to_nowhere", cls: "Warp", to: "nowhere" },
		{ id: 6, name: "to_crate", cls: "Warp", to: "crate" },			// two objects, neither a Warp or PlayerSpawn
		{ id: 7, name: "loose", cls: "Warp" },							// to not set, not required
	]);
	const ctx = context(root);
	const f = checkRules(m, rules, ctx);
	assert.deepEqual(f.map(brief), ["error rule-missing-object #5", "error rule-missing-object #6"]);
	assert.equal(f[0].text, "Object 'to_nowhere' (id 5, layer Objects): to names 'nowhere', but no map of the project has an object of that name and class Warp or PlayerSpawn.");
	assert.equal(f[1].text, "Object 'to_crate' (id 6, layer Objects): to names 'crate', but no map of the project has an object of that name and class Warp or PlayerSpawn (maps/cellar/cellar.tmj has one of class 'Box').");
	assert.equal(ctx.counts.mapObjects, 2, "each other map read once per save; the saved map not at all");
	// Two targets of the name: in this map and in another one.
	const twice = model(town, [
		{ id: 1, name: "beach", cls: "PlayerSpawn" },
		{ id: 2, name: "to_beach", cls: "Warp", to: "beach" },
	]);
	const g = checkRules(twice, rules.slice(1), context(root));
	assert.deepEqual(g.map(brief), ["error rule-ambiguous-object #2"]);
	assert.equal(g[0].text, "Object 'to_beach' (id 2, layer Objects): to names 'beach', which 2 objects of the project have: this map (id 1), maps/coast/coast.tmx (id 4); give them names of their own.");
	// Without targetClass any class counts; required; severity; a map that cannot be read is a warning.
	const anyClass = parseRules(JSON.stringify([{ "class": "Warp", member: "to", kind: "object_in_project", required: true, severity: "warning" }])).rules;
	const h = checkRules(m, anyClass, context(root));
	assert.deepEqual(h.map(brief), ["warning rule-missing-object #5", "warning rule-ambiguous-object #6", "warning rule-required #7"]);
	assert.match(h[2].text, /to is not set; the project's rules \(yati_rules\) require an object's name\.$/);
	fs.writeFileSync(path.join(root, "maps", "coast", "broken.tmj"), "{oops");
	const k = checkRules(m, rules.slice(1), context(root));
	assert.deepEqual(k.map(brief), ["warning rule-unreadable #0", "error rule-missing-object #5", "error rule-missing-object #6"]);
	assert.match(k[0].text, /^The project's map maps\/coast\/broken\.tmj could not be read \(not valid JSON\); the rules that look across the project skip it\.$/);
	// A rule's maps limit where it looks: beach and the saved map are outside maps/cellar (whose crate
	// counts here: the rule names no class).
	const limited = parseRules(JSON.stringify([{ "class": "Warp", member: "to", kind: "object_in_project", maps: "maps/cellar" }])).rules;
	assert.deepEqual(checkRules(m, limited, context(root)).map(brief), ["error rule-missing-object #3", "error rule-missing-object #4", "error rule-missing-object #5"]);
});

test("object_in_project on a world's property", () => {
	const root = threeMaps();
	const worldFile = path.join(root, "maps", "game.world");
	const rules = parseRules(JSON.stringify([
		{ world: "maps/game.world", member: "start_spawn", kind: "object_in_project", targetClass: ["Warp", "PlayerSpawn"], required: true },
	])).rules;
	const good = parseWorld(JSON.stringify({ maps: [], properties: [{ name: "start_spawn", type: "string", value: "beach" }] }), worldFile);
	assert.deepEqual(checkWorld(worldFile, good, rules, context(root)), []);
	const bad = parseWorld(JSON.stringify({ maps: [], properties: [{ name: "start_spawn", type: "string", value: "crate" }] }), worldFile);
	assert.deepEqual(checkWorld(worldFile, bad, rules, context(root)).map((f) => f.text), [
		"World game.world: start_spawn names 'crate', but no map of the project has an object of that name and class Warp or PlayerSpawn (maps/cellar/cellar.tmj has one of class 'Box').",
	]);
});

test("unique_name: a name another object of the classes has, here or in another map, is a finding naming it; required names", () => {
	const root = threeMaps();
	const rules = parseRules(JSON.stringify(GAME_RULES.slice(0, 1))).rules;
	const town = path.join(root, "maps", "town.tmx");
	const m = model(town, [
		{ id: 1, name: "town_start", cls: "PlayerSpawn" },
		{ id: 2, name: "cellar_up", cls: "Warp", to: "x" },		// also in cellar.tmj
		{ id: 3, name: "twin", cls: "Warp" },
		{ id: 4, name: "twin", cls: "PlayerSpawn" },
		{ id: 5, cls: "Warp" },									// no name, required
		{ id: 6, name: "crate", cls: "Warp" },					// the other crates are no Warp or PlayerSpawn
		{ id: 7, name: "beach", cls: "Sign" },					// not of the classes
	]);
	const f = checkRules(m, rules, context(root));
	assert.deepEqual(f.map(brief), ["error rule-duplicate-name #2", "error rule-duplicate-name #3", "error rule-duplicate-name #4", "error rule-required #5"]);
	assert.equal(f[0].text, "Object 'cellar_up' (id 2, layer Objects): the name 'cellar_up' is also used by maps/cellar/cellar.tmj (id 1, class Warp); the project's rules (yati_rules) want the names of Warp and PlayerSpawn objects unique across the project.");
	assert.equal(f[1].text, "Object 'twin' (id 3, layer Objects): the name 'twin' is also used by this map (id 4, class PlayerSpawn); the project's rules (yati_rules) want the names of Warp and PlayerSpawn objects unique across the project.");
	assert.equal(f[3].text, "Object (unnamed) (id 5, layer Objects): has no name; the project's rules (yati_rules) require one for Warp and PlayerSpawn objects.");
	// The saved map's file is not read: a name it had when last saved is not a clash with itself.
	assert.deepEqual(checkRules(model(town, [{ id: 1, name: "town_start", cls: "PlayerSpawn" }, { id: 2, name: "to_cellar", cls: "Warp" }]), rules, context(root)), []);
	// Not required: an unnamed object is fine. The hidden folder's copy of town_start is not the project's.
	const optional = parseRules(JSON.stringify([{ "class": ["Warp", "PlayerSpawn"], kind: "unique_name", severity: "warning" }])).rules;
	assert.deepEqual(checkRules(m, optional, context(root)).map(brief), ["warning rule-duplicate-name #2", "warning rule-duplicate-name #3", "warning rule-duplicate-name #4"]);
	// A saved map outside the project's folders is still checked against them.
	const outside = model(path.join(root, "elsewhere.tmx"), [{ id: 9, name: "beach", cls: "PlayerSpawn" }]);
	assert.deepEqual(checkRules(outside, rules, context(root)).map((x) => x.text), [
		"Object 'beach' (id 9, layer Objects): the name 'beach' is also used by maps/coast/coast.tmx (id 4, class PlayerSpawn); the project's rules (yati_rules) want the names of Warp and PlayerSpawn objects unique across the project.",
	]);
});

test("several rules looking across the project read every map once per save", () => {
	const root = threeMaps();
	const rules = parseRules(JSON.stringify(GAME_RULES.concat([
		{ "class": "Warp", member: "to", kind: "object_in_project", targetClass: "Warp" },
	]))).rules;
	const ctx = context(root);
	checkRules(model(path.join(root, "maps", "town.tmx"), [{ id: 2, name: "to_cellar", cls: "Warp", to: "cellar_up" }]), rules, ctx);
	assert.equal(ctx.counts.mapObjects, 2);
	assert.ok(ctx.counts.listDir <= 6, "the folders listed once: " + ctx.counts.listDir);
});

test("the pick list: every named object of the target classes in the project, by map", () => {
	const root = threeMaps();
	const rule = parseRules(JSON.stringify(GAME_RULES)).rules[1];
	const ctx = context(root);
	const town = path.join(root, "maps", "town.tmx");
	const openTown = [{ id: 1, name: "town_start", className: "PlayerSpawn" }, { id: 2, name: "to_cellar", className: "Warp" }, { id: 3, name: "", className: "Warp" }];
	const choices = projectObjectChoices(rule, ctx, town, openTown);
	assert.deepEqual(choices.map((c) => c.label), [
		"maps/cellar/cellar.tmj: cellar_up (Warp)",
		"maps/coast/coast.tmx: beach (PlayerSpawn)",
		"maps/town.tmx: to_cellar (Warp)",
		"maps/town.tmx: town_start (PlayerSpawn)",
	]);
	assert.deepEqual([choices[0].name, path.basename(choices[0].file), choices[0].id], ["cellar_up", "cellar.tmj", 1]);
});

test("a project of 300 maps: a save's check stays fast, every map read once", () => {
	const files = {};
	for (let i = 0; i < 300; i++) {
		const objects = [{ id: 1, name: "spawn_" + i, cls: "PlayerSpawn" }];
		for (let k = 0; k < 20; k++)
			objects.push({ id: 2 + k, name: "warp_" + i + "_" + k, cls: "Warp", to: "spawn_" + ((i + k) % 300) });
		for (let k = 0; k < 30; k++)
			objects.push({ id: 30 + k, name: "prop_" + k, cls: "Prop" });
		files["maps/region_" + (i % 10) + "/map_" + i + (i % 3 === 0 ? ".tmj" : ".tmx")] = i % 3 === 0 ? tmj(objects) : tmx(objects);
	}
	const root = project(files);
	const rules = parseRules(JSON.stringify(GAME_RULES)).rules;
	const saved = path.join(root, "maps", "region_1", "map_1.tmx");
	const text = fs.readFileSync(saved, "utf8");
	const m = model(saved, parseMapObjects(text, saved, io.readText).objects.map((o) => ({ id: o.id, name: o.name, cls: o.className, to: o.className === "Warp" ? "spawn_2" : undefined })));
	const ctx = context(root);
	const t0 = process.hrtime.bigint();
	const f = checkRules(m, rules, ctx);
	const ms = Number(process.hrtime.bigint() - t0) / 1e6;
	assert.deepEqual(f, []);
	assert.equal(ctx.counts.mapObjects, 299);
	console.log("# a save in a project of 300 maps (15 300 objects): " + ms.toFixed(1) + " ms, reading and parsing every map");
	assert.ok(ms < 3000, ms + " ms");
});
