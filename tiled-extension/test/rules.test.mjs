// lib/rules.mjs: the project's rules (yati_rules) parsed, checked on the rules sample's maps and
// world, maps read from their files (.tmx and .tmj, templates and tile classes), the CSV table and
// the key preview, and the helpers the actions use.
import { test, after } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import {
	parseRules, rulesForClass, worldRules, ruleLabel, parseCsv, readTable, keyPreview, parseMapObjects, parseWorld,
	worldContains, checkRules, checkWorld, objectNames, newFilePath, openCommands, DEFAULT_KEY_PATTERN, findMember, memberPath,
} from "../yati-godot/lib/rules.mjs";
import { contextFromTypes } from "../yati-godot/lib/validate.mjs";
import { tmxModel, projectTypes, enumsOf } from "./helpers/tmx_model.mjs";
import { SAMPLE_DIR, io, tempDir } from "./helpers/node_io.mjs";

const RULES_DIR = path.join(SAMPLE_DIR, "rules");
const PROJECT = path.join(RULES_DIR, "rules.tiled-project");
const dirs = [];
after(() => { for (const d of dirs) fs.rmSync(d, { recursive: true, force: true }); });

function sampleRules() {
	const json = JSON.parse(fs.readFileSync(PROJECT, "utf8"));
	return json.properties.find((p) => p.name === "yati_rules").value;
}

function context(projectFile) {
	const types = projectTypes(projectFile);
	const base = contextFromTypes(types, projectFile, "", (p) => fs.existsSync(p));
	return Object.assign(base, {
		projectFolders: [path.dirname(projectFile)],
		listDir: io.listDir,
		readText: io.readText,
		mapObjects: (p) => {
			const text = io.readText(p);
			return text === null ? { objects: [], error: "cannot read" } : parseMapObjects(text, p, io.readText);
		},
		enums: enumsOf(types),
	});
}

function checkSample(mapName) {
	const parsed = parseRules(sampleRules());
	assert.deepEqual(parsed.problems, []);
	const ctx = context(PROJECT);
	return checkRules(tmxModel(path.join(RULES_DIR, "maps", mapName), ctx.enums), parsed.rules, ctx);
}

const brief = (f) => f.severity + " " + f.code + " #" + f.objectId;

test("the sample's rules parse into normalized rules", () => {
	const parsed = parseRules(sampleRules());
	assert.deepEqual(parsed.problems, []);
	assert.equal(parsed.rules.length, 9);		// the last two the portals' (rules_project_glue.test.mjs)
	const spawn = parsed.rules[1];
	assert.deepEqual([spawn.index, spawn.kind, spawn.member, spawn.classes, spawn.map, spawn.targetClass, spawn.required, spawn.severity],
		[2, "object_in_map", "to_spawn", ["warp"], "to_map", "player_spawn", true, "error"]);
	const seq = parsed.rules[2];
	assert.deepEqual(seq.contains.map((c) => c.label), ["section named on_enter", "section named on_leave"]);
	assert.ok(seq.contains[0].regex.test("<sequence name=\"on_enter\">"));
	assert.ok(seq.contains[0].regex.test("<sequence  name='on_enter'>"));
	assert.equal(seq.template, "templates/trigger.xml");
	const text = parsed.rules[4];
	assert.deepEqual(text.classes, ["sign", "interactable"]);
	assert.equal(text.keyPattern.source, DEFAULT_KEY_PATTERN);
	assert.deepEqual(rulesForClass(parsed.rules, "interactable").map((r) => r.index), [4, 5]);
	assert.deepEqual(rulesForClass(parsed.rules, "warp", ["object_in_map"]).map((r) => r.member), ["to_spawn"]);
	assert.deepEqual(rulesForClass(parsed.rules, "").map((r) => r.index), []);
	assert.deepEqual(worldRules(parsed.rules, RULES_DIR).map((w) => [w.file, w.rules.map((r) => r.member)]),
		[[path.join(RULES_DIR, "maps", "game.world"), ["start_map", "start_spawn"]]]);
	assert.equal(ruleLabel(spawn), "to_spawn (an object of class player_spawn in the map to_map names)");
	assert.equal(ruleLabel(parsed.rules[0]), "to_map (a map)");
	// "*" matches every class, an unclassed object too.
	assert.deepEqual(rulesForClass(parseRules("[{\"class\": \"*\", \"member\": \"note\", \"kind\": \"key\", \"table\": \"t.csv\"}]").rules, "").length, 1);
	// Unset or empty: no rules and no problems.
	assert.deepEqual(parseRules(undefined), { rules: [], problems: [] });
	assert.deepEqual(parseRules("  "), { rules: [], problems: [] });
});

test("a rule with a mistake is left out and named", () => {
	const problems = (text) => parseRules(text).problems;
	assert.match(problems("[{")[0], /^yati_rules is not valid JSON/);
	assert.match(problems("{}")[0], /must be a JSON array/);
	const cases = [
		["{\"class\": \"a\", \"member\": \"m\"}", /rule 1: "kind" is missing; it is one of map, object_in_map, object_in_project, unique_name, file, key\./],
		["{\"class\": \"a\", \"member\": \"m\", \"kind\": \"maps\"}", /"kind" is "maps"/],
		["{\"class\": \"a\", \"member\": \"m\", \"kind\": \"map\", \"requird\": true}", /unknown field "requird" for kind map/],
		["{\"class\": \"a\", \"member\": \"m\", \"kind\": \"map\", \"template\": \"x\"}", /unknown field "template" for kind map/],
		["{\"class\": \"a\", \"kind\": \"map\"}", /"member" \(the member's name\) is missing/],
		["{\"member\": \"m\", \"kind\": \"map\"}", /give either "class" .* or "world"/],
		["{\"class\": \"a\", \"world\": \"w.world\", \"member\": \"m\", \"kind\": \"map\"}", /give either "class"/],
		["{\"class\": [], \"member\": \"m\", \"kind\": \"map\"}", /"class" is a class name, a list of class names, or "\*"/],
		["{\"class\": \"a\", \"member\": \"m\", \"kind\": \"map\", \"required\": \"yes\"}", /"required" is true or false/],
		["{\"class\": \"a\", \"member\": \"m\", \"kind\": \"map\", \"severity\": \"info\"}", /"severity" is "error" or "warning"/],
		["{\"class\": \"a\", \"member\": \"m\", \"kind\": \"key\"}", /"table" .* is missing/],
		["{\"class\": \"a\", \"member\": \"m\", \"kind\": \"key\", \"table\": \"t.csv\", \"keyPattern\": \"(\"}", /"keyPattern" is not a regular expression/],
		["{\"class\": \"a\", \"member\": \"m\", \"kind\": \"file\", \"contains\": [{\"pattern\": \"[\"}]}", /"contains" item 1: '\[' is not a regular expression/],
		["{\"class\": \"a\", \"member\": \"m\", \"kind\": \"file\", \"contains\": [{\"label\": \"x\"}]}", /"contains" item 1 needs a "pattern"/],
		["{\"class\": \"a\", \"member\": \"m\", \"kind\": \"file\", \"folder\": \"seq\"}", /"folder" is where the template's copies go; give "template" too/],
		["{\"world\": \"w.world\", \"member\": \"s\", \"kind\": \"object_in_map\"}", /a world rule of kind object_in_map needs "map"/],
		["{\"class\": \"a\", \"member\": \"m\", \"kind\": \"object_in_map\", \"map\": \"m\"}", /"map" names the member the rule checks \(m\)/],
		["{\"class\": \"a\", \"member\": \"m\", \"kind\": \"object_in_map\", \"map\": 3}", /"map" is a string/],
		["7", /a rule is an object/],
	];
	for (const [rule, re] of cases) {
		const p = parseRules("[" + rule + "]");
		assert.equal(p.problems.length, 1, rule + ": " + JSON.stringify(p.problems));
		assert.match(p.problems[0], re, rule);
	}
	// The good rules of a list stay.
	const mixed = parseRules("[{\"class\": \"a\", \"member\": \"m\", \"kind\": \"map\"}, {\"kind\": \"map\"}]");
	assert.equal(mixed.rules.length, 1);
	assert.match(mixed.problems[0], /^yati_rules, rule 2: /);
	// A contains item may be a bare pattern; its label then shows the pattern.
	assert.equal(parseRules("[{\"class\": \"a\", \"member\": \"m\", \"kind\": \"file\", \"contains\": [\"on_x\"]}]").rules[0].contains[0].label, "a match for /on_x/");
});

test("broken.tmx: one finding per mistake, each naming the object, the member and why", () => {
	const findings = checkSample("broken.tmx");
	assert.deepEqual(findings.map(brief), [
		"error rule-required #1",			// warp without to_map (its to_spawn is not checked then)
		"error rule-missing-map #2",			// attic.tmx (to_spawn not checked: the map rule says it)
		"error rule-missing-object #3",		// from_twn
		"error rule-missing-object #4",		// "up" is a warp, not a player_spawn
		"error rule-required #5",			// trigger without sequence (its class default is empty)
		"error rule-missing-content #6",		// no on_leave
		"error rule-unknown-key #7",			// SIGN_WELCOM
		"error rule-missing-content #9",		// an interactable whose file has no on_interact
	]);
	const text = (id) => findings.find((f) => f.objectId === id).text;
	assert.equal(text(1), "Object 'nowhere' (id 1, layer Objects): to_map is not set; the project's rules (yati_rules) require a map.");
	assert.match(text(2), /^Object 'lost' \(id 2, layer Objects\): to_map names \/.*\/maps\/attic\.tmx, which does not exist\.$/);
	assert.equal(text(3), "Object 'typo' (id 3, layer Objects): to_spawn names 'from_twn', but cellar.tmx has no object of that name and class player_spawn.");
	assert.equal(text(4), "Object 'wrong_class' (id 4, layer Objects): to_spawn names 'up', but cellar.tmx has no object of that name and class player_spawn (the object 'up' there has class 'warp').");
	assert.equal(text(5), "Object 'empty' (id 5, layer Objects): sequence is not set; the project's rules (yati_rules) require a file.");
	assert.equal(text(6), "Object 'half' (id 6, layer Objects): sequence names half_trigger.xml, which has no section named on_leave.");
	assert.equal(text(7), "Object 'misspelt' (id 7, layer Objects): text is 'SIGN_WELCOM', shaped like a key, but strings.csv has no such key (did you mean SIGN_WELCOME, SIGN_WELL?).");
	assert.equal(text(9), "Object 'silent' (id 9, layer Objects): sequence names well_trigger.xml, which has no section named on_interact.");
});

test("town.tmx and cellar.tmx: no finding (the cellar's spawn takes its class from a template)", () => {
	assert.deepEqual(checkSample("town.tmx"), []);
	assert.deepEqual(checkSample("cellar.tmx"), []);
});

test("rule options: severity, object_in_map in the same map, a map member without a map rule, a missing file, ambiguity", () => {
	const ctx = context(PROJECT);
	const model = tmxModel(path.join(RULES_DIR, "maps", "broken.tmx"), ctx.enums);
	// Warnings only, and the map member checked by the object rule itself when no map rule covers it.
	let rules = parseRules(JSON.stringify([
		{ "class": "warp", member: "to_spawn", kind: "object_in_map", map: "to_map", targetClass: "player_spawn", severity: "warning" },
	])).rules;
	let f = checkRules(model, rules, ctx);
	assert.deepEqual(f.map(brief), ["warning rule-missing-map #2", "warning rule-missing-object #3", "warning rule-missing-object #4"]);
	assert.match(f[0].text, /to_spawn cannot be checked: to_map names .*attic\.tmx, which does not exist\.$/);
	// object_in_map without "map": the same map; without targetClass: any class.
	rules = parseRules(JSON.stringify([{ "class": "warp", member: "to_spawn", kind: "object_in_map" }])).rules;
	f = checkRules(model, rules, ctx);
	assert.deepEqual(f.map(brief), ["error rule-missing-object #1", "error rule-missing-object #2", "error rule-missing-object #3", "error rule-missing-object #4", "error rule-missing-object #10"]);
	assert.match(f[0].text, /to_spawn names 'from_town', but this map has no object of that name\.$/);
	// Two objects of the name and class: ambiguous.
	const twins = { kind: "map", fileName: model.fileName, properties: [], layers: [], templates: [], objects: [
		{ id: 1, name: "sign", layerPath: "L", className: "sign", tileClassName: "", properties: [{ name: "text", type: "string", value: "twin", propertyType: "" }] },
		{ id: 2, name: "twin", layerPath: "L", className: "sign", tileClassName: "", properties: [] },
		{ id: 3, name: "twin", layerPath: "L", className: "", tileClassName: "sign", properties: [] },
	] };
	rules = parseRules(JSON.stringify([{ "class": "sign", member: "text", kind: "object_in_map", targetClass: "sign" }])).rules;
	f = checkRules(twins, rules, ctx).filter((x) => x.objectId === 1);
	assert.deepEqual(f.map(brief), ["warning rule-ambiguous-object #1"]);
	assert.equal(f[0].text, "Object 'sign' (id 1, layer L): text names 'twin', and this map has 2 such objects (ids 2, 3); give them names of their own.");
	// A file rule whose file is missing mentions the template when the rule has one.
	rules = parseRules(JSON.stringify([{ "class": "warp", member: "to_map", kind: "file", template: "templates/trigger.xml" }])).rules;
	f = checkRules(model, rules, ctx);
	assert.deepEqual(f.map(brief), ["error rule-missing-file #2"]);
	assert.match(f[0].text, /attic\.tmx, which does not exist \(YATI: Open or create file… makes it from the rule's template\)\.$/);
	// A map rule on a file that is no map; a key rule whose table is missing warns.
	rules = parseRules(JSON.stringify([
		{ "class": "trigger", member: "sequence", kind: "map" },
		{ "class": "sign", member: "text", kind: "key", table: "text/nope.csv" },
	])).rules;
	f = checkRules(model, rules, ctx);
	assert.deepEqual(f.map(brief), ["error rule-not-a-map #6", "warning rule-unreadable #7"]);
	assert.match(f[1].text, /is 'SIGN_WELCOM', but the table .*text\/nope\.csv could not be read\.$/);
});

test("a key rule with its own pattern", () => {
	const ctx = context(PROJECT);
	const model = tmxModel(path.join(RULES_DIR, "maps", "broken.tmx"), ctx.enums);
	// Every value is a key: the plain text is flagged too.
	const rules = parseRules(JSON.stringify([{ "class": "sign", member: "text", kind: "key", table: "text/strings.csv", keyPattern: "." }])).rules;
	assert.deepEqual(checkRules(model, rules, ctx).map(brief), ["error rule-unknown-key #7", "error rule-unknown-key #8"]);
});

test("the world: its properties checked like an object's, when the saved map is one of its maps", () => {
	const parsed = parseRules(sampleRules());
	const ctx = context(PROJECT);
	const worldFile = path.join(RULES_DIR, "maps", "game.world");
	const world = parseWorld(fs.readFileSync(worldFile, "utf8"), worldFile);
	assert.equal(world.error, "");
	assert.deepEqual(world.maps, [path.join(RULES_DIR, "maps", "town.tmx"), path.join(RULES_DIR, "maps", "cellar.tmx")]);
	assert.deepEqual(world.properties.map((p) => [p.name, p.type, p.value]), [
		["start_map", "file", path.join(RULES_DIR, "maps", "town.tmx")], ["start_spawn", "string", "from_cellar"],
	]);
	assert.equal(worldContains(world, worldFile, path.join(RULES_DIR, "maps", "town.tmx")), true);
	assert.equal(worldContains(world, worldFile, path.join(RULES_DIR, "maps", "broken.tmx")), false);
	const rules = worldRules(parsed.rules, RULES_DIR)[0].rules;
	assert.deepEqual(checkWorld(worldFile, world, rules, ctx), []);
	// A spawn that is not there, and no start map.
	const bad = parseWorld(JSON.stringify({ maps: [], properties: [{ name: "start_map", type: "file", value: "cellar.tmx" }, { name: "start_spawn", type: "string", value: "gate" }] }), worldFile);
	assert.deepEqual(checkWorld(worldFile, bad, rules, ctx).map((f) => f.text), [
		"World game.world: start_spawn names 'gate', but cellar.tmx has no object of that name and class player_spawn.",
	]);
	const empty = parseWorld(JSON.stringify({ maps: [], start_spawn: "from_town" }), worldFile);
	assert.deepEqual(checkWorld(worldFile, empty, rules, ctx).map(brief), ["error rule-required #0"]);
	// Top-level keys stand in for a properties list; patterns match file names in the world's folder.
	const patterned = parseWorld(JSON.stringify({ patterns: [{ regexp: "^level_\\d+\\.tmx$" }], start_map: "level_1.tmx" }), worldFile);
	assert.deepEqual(patterned.properties, [{ name: "start_map", type: "string", value: "level_1.tmx", propertyType: "" }]);
	assert.equal(worldContains(patterned, worldFile, path.join(RULES_DIR, "maps", "level_12.tmx")), true);
	assert.equal(worldContains(patterned, worldFile, path.join(RULES_DIR, "level_12.tmx")), false);
	assert.equal(parseWorld("{", worldFile).error, "not valid JSON");
});

test("maps read from their files: .tmx and .tmj, templates (.tx, .tj), tile classes, no tile collision objects", () => {
	const cellar = path.join(RULES_DIR, "maps", "cellar.tmx");
	assert.deepEqual(parseMapObjects(fs.readFileSync(cellar, "utf8"), cellar, io.readText), {
		objects: [{ id: 1, name: "from_town", className: "player_spawn" }, { id: 2, name: "up", className: "warp" }, { id: 3, name: "barrel", className: "interactable" }],
		error: "",
	});
	const dir = tempDir();
	dirs.push(dir);
	const w = (name, text) => { fs.writeFileSync(path.join(dir, name), text); return path.join(dir, name); };
	w("props.tsx", "<tileset name=\"p\"><tile id=\"0\" type=\"door\"/><tile id=\"2\" class=\"player_spawn\"><objectgroup><object id=\"1\" name=\"shape\"/></objectgroup></tile></tileset>");
	w("props.tsj", JSON.stringify({ tiles: [{ id: 1, "class": "player_spawn" }] }));
	w("hero.tj", JSON.stringify({ type: "template", object: { name: "hero_start", gid: 2 }, tileset: { firstgid: 1, source: "props.tsj" } }));
	const tmx = w("a.tmx", "<map>\n<!-- <object id=\"99\" name=\"commented\"/> -->\n" +
		"<tileset firstgid=\"1\" source=\"props.tsx\"/>\n" +
		"<tileset firstgid=\"10\" name=\"inline\"><tile id=\"0\" class=\"chest\"><objectgroup><object id=\"1\" name=\"collision\"/></objectgroup></tile></tileset>\n" +
		"<objectgroup id=\"1\" name=\"o\"><object id=\"1\" name=\"a &amp; b\" gid=\"3\"/><object id=\"2\" name=\"box\" gid=\"2147483658\"/>" +
		"<object id=\"3\" class=\"own\" gid=\"1\"/><object id=\"4\" template=\"hero.tj\"/></objectgroup>\n" +
		"<group id=\"2\"><objectgroup id=\"3\"><object id=\"5\" name=\"deep\" type=\"old_style\"></object></objectgroup></group></map>");
	assert.deepEqual(parseMapObjects(fs.readFileSync(tmx, "utf8"), tmx, io.readText).objects, [
		{ id: 1, name: "a & b", className: "player_spawn" },		// the tile's class
		{ id: 2, name: "box", className: "chest" },				// a flipped gid of the embedded tileset
		{ id: 3, name: "", className: "own" },					// the object's class wins over its tile's
		{ id: 4, name: "hero_start", className: "player_spawn" },	// the template's name and its tile's class
		{ id: 5, name: "deep", className: "old_style" },			// a group's layer; "type" before Tiled 1.9
	]);
	const tmj = w("b.tmj", JSON.stringify({
		tilesets: [{ firstgid: 1, source: "props.tsj" }, { firstgid: 5, tiles: [{ id: 0, type: "door" }] }],
		layers: [{ type: "group", layers: [{ type: "objectgroup", objects: [
			{ id: 1, name: "s", gid: 2 }, { id: 2, name: "d", gid: 5 }, { id: 3, name: "t", template: "hero.tj" }, { id: 4, name: "c", "class": "warp" },
		] }] }],
	}));
	assert.deepEqual(parseMapObjects(fs.readFileSync(tmj, "utf8"), tmj, io.readText).objects, [
		{ id: 1, name: "s", className: "player_spawn" }, { id: 2, name: "d", className: "door" },
		{ id: 3, name: "t", className: "player_spawn" }, { id: 4, name: "c", className: "warp" },
	]);
	// A .tx template with a tile of an external tileset; missing templates and tilesets leave no class.
	w("spawn.tx", "<template><tileset firstgid=\"1\" source=\"props.tsx\"/><object name=\"spawned\" gid=\"3\"/></template>");
	const c = w("c.tmx", "<map><objectgroup><object id=\"1\" template=\"spawn.tx\"/><object id=\"2\" name=\"x\" template=\"gone.tx\"/><object id=\"3\" name=\"y\" gid=\"40\"/></objectgroup></map>");
	assert.deepEqual(parseMapObjects(fs.readFileSync(c, "utf8"), c, io.readText).objects, [
		{ id: 1, name: "spawned", className: "player_spawn" }, { id: 2, name: "x", className: "" }, { id: 3, name: "y", className: "" },
	]);
	assert.equal(parseMapObjects("{broken", tmj, io.readText).error, "not valid JSON");
	assert.equal(parseMapObjects("<tileset/>", tmx, io.readText).error, "not a Tiled map");
	assert.deepEqual(objectNames(parseMapObjects(fs.readFileSync(tmx, "utf8"), tmx, io.readText).objects, "player_spawn"), ["a & b", "hero_start"]);
	assert.deepEqual(objectNames([{ name: "b", className: "" }, { name: "a", className: "" }, { name: "b", className: "x" }, { name: "", className: "" }], ""), ["a", "b"]);
});

test("the CSV table and the key preview", () => {
	assert.deepEqual(parseCsv("keys,en\r\nA_B,\"x, \"\"y\"\"\nz\"\n\nC_D,w"), [["keys", "en"], ["A_B", "x, \"y\"\nz"], ["C_D", "w"]]);
	assert.deepEqual(parseCsv("\uFEFFa,b"), [["a", "b"]]);
	const table = readTable(fs.readFileSync(path.join(RULES_DIR, "text", "strings.csv"), "utf8"));
	assert.deepEqual(table.header, ["keys", "en", "de"]);
	assert.deepEqual(Object.keys(table.rows), ["SIGN_WELCOME", "SIGN_WELL", "NOTICE_READ"]);
	assert.equal(table.rows.NOTICE_READ[1], "\"Closed until spring,\" it says.");
	const rule = parseRules(sampleRules()).rules[4];
	assert.deepEqual(keyPreview(rule, "SIGN_WELL", table, "strings.csv"), {
		state: "key", lines: ["text: SIGN_WELL", "  en: \"The well is deep.\\nMind your step.\"", "  de: \"Der Brunnen ist tief.\""],
	});
	assert.deepEqual(keyPreview(rule, "SIGN_WELCOM", table, "strings.csv"), {
		state: "unknown", lines: ["text: SIGN_WELCOM is no key of strings.csv.", "  Did you mean SIGN_WELCOME, SIGN_WELL?"],
	});
	assert.deepEqual(keyPreview(rule, "Hello there", table, "strings.csv"), { state: "plain", lines: ["text: plain text (not shaped like a key): \"Hello there\""] });
	assert.deepEqual(keyPreview(rule, "OK", table, "strings.csv").state, "plain");
	assert.deepEqual(keyPreview(rule, "", table, "strings.csv"), { state: "unset", lines: ["text: (not set)"] });
	assert.deepEqual(keyPreview(rule, "SIGN_X", null, "strings.csv"), { state: "unknown", lines: ["text: SIGN_X: the table strings.csv could not be read."] });
});

test("the actions' helpers: where a template's copy goes, how a file is opened, a member's path", () => {
	const rules = parseRules(sampleRules()).rules;
	const map = path.join(RULES_DIR, "maps", "town.tmx");
	assert.equal(newFilePath(rules[2], { id: 3, name: "well", className: "trigger" }, map, RULES_DIR), path.join(RULES_DIR, "sequences", "town_well.xml"));
	assert.equal(newFilePath(rules[2], { id: 7, name: "", className: "trigger" }, map, RULES_DIR), path.join(RULES_DIR, "sequences", "town_trigger_7.xml"));
	const beside = Object.assign({}, rules[2], { folder: "" });
	assert.equal(newFilePath(beside, { id: 3, name: "old well/2", className: "trigger" }, map, RULES_DIR), path.join(RULES_DIR, "maps", "town_old_well_2.xml"));
	assert.deepEqual(openCommands("", "/home/me/s.xml"), [{ program: "xdg-open", args: ["/home/me/s.xml"] }, { program: "open", args: ["/home/me/s.xml"] }]);
	assert.deepEqual(openCommands("", "C:/game/s.xml"), [{ program: "cmd", args: ["/c", "start", "", "C:\\game\\s.xml"] }]);
	assert.deepEqual(openCommands("code --reuse-window", "/s.xml"), [{ program: "code", args: ["--reuse-window", "/s.xml"] }]);
	assert.deepEqual(openCommands("\"C:/Program Files/Editor/ed.exe\" -n", "C:/s.xml"), [{ program: "C:/Program Files/Editor/ed.exe", args: ["-n", "C:/s.xml"] }]);
	const props = [{ name: "a", type: "file", value: "x/y.xml" }, { name: "b", type: "file", value: "d.xml", fromClass: true }, { name: "c", type: "string", value: "res://s.xml" }];
	const ctx = { projectFile: "/p/game.tiled-project", godotProject: "/g" };
	assert.equal(memberPath(findMember(props, "a"), "/p/maps/m.tmx", ctx), "/p/maps/x/y.xml");
	assert.equal(memberPath(findMember(props, "b"), "/p/maps/m.tmx", ctx), "/p/d.xml");
	assert.equal(memberPath(findMember(props, "c"), "/p/maps/m.tmx", ctx), "/g/s.xml");
	assert.equal(memberPath(findMember(props, "zzz"), "/p/maps/m.tmx", ctx), "");
});

test("the README's example rules parse without a problem", () => {
	const readme = fs.readFileSync(path.join(path.dirname(SAMPLE_DIR), "README.md"), "utf8");
	const block = /## Rules[\s\S]*?```json\n([\s\S]*?)```/.exec(readme)[1];
	const parsed = parseRules(block);
	assert.deepEqual(parsed.problems, []);
	assert.deepEqual(parsed.rules.map((r) => r.kind), ["unique_name", "object_in_project", "file", "file", "file", "key", "object_in_project"]);
	assert.deepEqual(parsed.rules[0].classes, ["Warp", "PlayerSpawn"]);
	assert.equal(parsed.rules[0].required, true);
	assert.deepEqual([parsed.rules[1].member, parsed.rules[1].targetClasses], ["to", ["Warp", "PlayerSpawn"]]);
	assert.ok(parsed.rules[3].contains[0].regex.test("<sequence name=\"on_enter\">"));
	assert.ok(parsed.rules[3].contains[0].regex.test("<sequence name='on_leave'>"));
	assert.ok(!parsed.rules[3].contains[0].regex.test("<sequence name=\"main\">"));
	assert.deepEqual([parsed.rules[6].world, parsed.rules[6].member, parsed.rules[6].targetClasses], ["maps/overworld/overworld.world", "start_spawn", ["Warp", "PlayerSpawn"]]);
});
