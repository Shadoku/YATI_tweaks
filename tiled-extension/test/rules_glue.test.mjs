// The project's rules in the glue, against the fake API on the rules sample (sample/rules/): the
// save hook's rule findings as Issues, and the four actions on the selected object (pick a map and
// an object, follow a reference, open or create a file, show a key's text).
import { test, after } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { ACTIONS } from "../yati-godot/yati-godot.mjs";
import { rulesScenario } from "./helpers/scenario.mjs";

const scenarios = [];
function make(options) {
	const s = rulesScenario(options);
	scenarios.push(s);
	return s;
}
after(() => { for (const s of scenarios) s.cleanup(); });

// An object's property as the fake holds it after the extension set it (a file: its local path).
function prop(map, id, name) {
	const v = map._findObject(id).resolvedProperties()[name];
	return v !== null && typeof v === "object" && v.localFile !== undefined ? v.localFile : v;
}

const setRules = (rules) => (json) => { json.properties.find((p) => p.name === "yati_rules").value = JSON.stringify(rules); };

test("save hook: the rules' findings as Issues; activation selects the object", () => {
	const s = make();
	const map = s.open("broken.tmx");
	s.fake.emitAboutToBeSaved(map);
	const issues = s.fake.out.issues;
	const expected = [
		/^YATI: broken\.tmx: Object 'nowhere' \(id 1, layer Objects\): to_map is not set/,
		/^YATI: broken\.tmx: Object 'lost' \(id 2, layer Objects\): to_map names .*attic\.tmx, which does not exist\.$/,
		/^YATI: broken\.tmx: Object 'typo' \(id 3, layer Objects\): to_spawn names 'from_twn'/,
		/^YATI: broken\.tmx: Object 'wrong_class' \(id 4, layer Objects\): to_spawn names 'up'/,
		/^YATI: broken\.tmx: Object 'empty' \(id 5, layer Objects\): sequence is not set/,
		/^YATI: broken\.tmx: Object 'half' \(id 6, layer Objects\): sequence names half_trigger\.xml, which has no section named on_leave\.$/,
		/^YATI: broken\.tmx: Object 'misspelt' \(id 7, layer Objects\): text is 'SIGN_WELCOM'/,
		/^YATI: broken\.tmx: Object 'silent' \(id 9, layer Objects\): sequence names well_trigger\.xml, which has no section named on_interact\.$/,
	];
	assert.equal(issues.length, expected.length, issues.map((i) => i.text).join("\n"));
	for (let i = 0; i < expected.length; i++) {
		assert.equal(issues[i].severity, "error");
		assert.match(issues[i].text, expected[i]);
	}
	issues.find((i) => i.text.indexOf("'typo'") >= 0).activate();
	assert.deepEqual(s.fake.api.tiled.activeAsset.selectedObjects.map((o) => o.id), [3]);
	assert.ok(s.fake.out.logs.indexOf("YATI: broken.tmx: 8 finding(s), see the Issues view.") >= 0);
	// The clean maps file nothing, the world's rules included (town.tmx is one of its maps).
	const t = make();
	t.fake.emitAboutToBeSaved(t.open("town.tmx"));
	t.fake.emitAboutToBeSaved(t.open("cellar.tmx"));
	assert.deepEqual(t.fake.out.issues, []);
});

test("save hook: a map open in Tiled is read as it is now, not as last saved", () => {
	const s = make();
	const cellar = s.open("cellar.tmx");
	cellar._findObject(1)._name = "renamed";			// renamed in Tiled, not saved yet
	s.fake.emitAboutToBeSaved(s.open("town.tmx"));
	assert.deepEqual(s.fake.out.issues.map((i) => i.text), [
		"YATI: town.tmx: Object 'to_cellar' (id 2, layer Objects): to_spawn names 'from_town', but cellar.tmx has no object of that name and class player_spawn.",
	]);
});

test("save hook: the world's rules when the saved map is one of its maps; the rules' own mistakes", () => {
	const s = make();
	const worldFile = path.join(s.maps, "game.world");
	const world = JSON.parse(fs.readFileSync(worldFile, "utf8"));
	world.properties[1].value = "gate";
	fs.writeFileSync(worldFile, JSON.stringify(world));
	s.fake.emitAboutToBeSaved(s.open("town.tmx"));
	assert.deepEqual(s.fake.out.issues.map((i) => i.text), [
		"YATI: town.tmx: World game.world: start_spawn names 'gate', but town.tmx has no object of that name and class player_spawn.",
	]);
	s.fake.out.issues.length = 0;
	s.fake.emitAboutToBeSaved(s.open("broken.tmx"));		// not one of the world's maps
	assert.equal(s.fake.out.issues.filter((i) => i.text.indexOf("World") >= 0).length, 0);

	const t = make({ edit: setRules([
		{ "class": "sign", member: "text", kind: "key", table: "text/strings.csv" },
		{ "class": "sign", member: "text", kind: "keys" },
		{ world: "maps/none.world", member: "start_map", kind: "map" },
	]) });
	t.fake.emitAboutToBeSaved(t.open("broken.tmx"));
	assert.deepEqual(t.fake.out.issues.map((i) => i.severity + " " + i.text.replace(t.root, "<root>")), [
		"warning YATI: broken.tmx: yati_rules, rule 2: \"kind\" is \"keys\"; it is one of map, object_in_map, object_in_project, unique_name, file, key, layer_in_map, counterpart.",
		"error YATI: broken.tmx: Object 'misspelt' (id 7, layer Objects): text is 'SIGN_WELCOM', shaped like a key, but strings.csv has no such key (did you mean SIGN_WELCOME, SIGN_WELL?).",
		"warning YATI: broken.tmx: yati_rules names the world <root>/maps/none.world, which does not exist.",
	]);
	// yati_validate = false turns the rules off with the rest.
	const u = make({ edit: (json) => json.properties.push({ name: "yati_validate", propertytype: "", type: "bool", value: false }) });
	u.fake.emitAboutToBeSaved(u.open("broken.tmx"));
	assert.deepEqual(u.fake.out.issues, []);
});

test("pick: a map from a file dialog, an object of the rule's class from a list, both set in one undo step", () => {
	const s = make();
	const map = s.open("broken.tmx");
	const cellar = path.join(s.maps, "cellar.tmx");
	// Nothing selected; then a sign, which has no map-and-object rule.
	s.fake.trigger(ACTIONS.pick);
	assert.match(s.fake.out.alerts[0], /^YATI: Pick map and object…: select exactly one object \(0 selected\)\.$/);
	s.fake.select(map, [7]);
	s.fake.trigger(ACTIONS.pick);
	assert.match(s.fake.out.alerts[1], /no rule gives class 'sign' a map member and an object member/);
	// The warp without a map: cellar.tmx chosen, its one player_spawn offered and taken.
	s.fake.select(map, [1]);
	s.fake.answers.files.push(cellar);
	let seen = null;
	s.fake.answers.dialogs.push((d) => { seen = d; d.button("OK").click(); });
	s.fake.trigger(ACTIONS.pick);
	assert.equal(seen.windowTitle, "YATI: Pick map and object…");
	assert.deepEqual(seen.field("Object").items, ["from_town"]);
	assert.match(seen.labels()[0].text, /^Object 'nowhere' \(id 1, class warp\): to_spawn in cellar\.tmx \(objects of class player_spawn\)$/);
	assert.equal(prop(map, 1, "to_map"), cellar);
	assert.equal(prop(map, 1, "to_spawn"), "from_town");
	assert.deepEqual(map._undo, ["YATI: Set to_map and to_spawn"]);
	assert.equal(map._modified, true);
	// The list starts at the current value; town.tmx offers its spawn.
	s.fake.select(map, [10]);
	s.fake.answers.files.push(path.join(s.maps, "town.tmx"));
	s.fake.answers.dialogs.push((d) => { seen = d; d.button("OK").click(); });
	s.fake.trigger(ACTIONS.pick);
	assert.deepEqual([seen.field("Object").items, seen.field("Object").currentIndex], [["from_cellar"], 0]);
	// Cancel in the file dialog, then in the list: nothing changes.
	s.fake.select(map, [3]);
	s.fake.trigger(ACTIONS.pick);
	s.fake.answers.files.push(cellar);
	s.fake.answers.dialogs.push((d) => { d.button("Cancel").click(); });
	s.fake.trigger(ACTIONS.pick);
	assert.equal(prop(map, 3, "to_spawn"), "from_twn");
	assert.equal(map._undo.length, 2);
	// A map with no object of the class; a file that is no map.
	s.fake.answers.files.push(path.join(s.maps, "broken.tmx"));
	s.fake.trigger(ACTIONS.pick);
	assert.match(s.fake.out.alerts[s.fake.out.alerts.length - 1], /broken\.tmx has no named object of class player_spawn\./);
	s.fake.answers.files.push(path.join(s.root, "text", "strings.csv"));
	s.fake.trigger(ACTIONS.pick);
	assert.match(s.fake.out.alerts[s.fake.out.alerts.length - 1], /strings\.csv is no Tiled map/);
});

test("pick: several map-and-object rules for a class are offered as a list first", () => {
	const s = make({ edit: setRules([
		{ "class": "warp", member: "to_spawn", kind: "object_in_map", map: "to_map", targetClass: "player_spawn" },
		{ "class": "warp", member: "back_spawn", kind: "object_in_map", map: "back_map" },
	]) });
	const map = s.open("broken.tmx");
	s.fake.select(map, [1]);
	s.fake.answers.files.push(path.join(s.maps, "town.tmx"));
	const seen = [];
	s.fake.answers.dialogs.push((d) => { seen.push(d); d.field("Member").currentIndex = 1; d.button("OK").click(); });
	s.fake.answers.dialogs.push((d) => { seen.push(d); d.field("Object").currentIndex = 2; d.button("OK").click(); });
	s.fake.trigger(ACTIONS.pick);
	assert.deepEqual(seen[0].field("Member").items, [
		"to_spawn (an object of class player_spawn in the map to_map names)", "back_spawn (an object in the map back_map names)",
	]);
	assert.deepEqual(seen[1].field("Object").items, ["from_cellar", "notice", "to_cellar", "welcome", "well"]);
	assert.equal(prop(map, 1, "back_map"), path.join(s.maps, "town.tmx"));
	assert.equal(prop(map, 1, "back_spawn"), "to_cellar");
	assert.deepEqual(map._undo, ["YATI: Set back_map and back_spawn"]);
});

test("follow: the target map opened, the object selected and centred; a missing object or map said plainly", () => {
	const s = make();
	const town = s.open("town.tmx");
	s.fake.select(town, [2]);
	s.fake.trigger(ACTIONS.follow);
	const active = s.fake.api.tiled.activeAsset;
	assert.equal(active.fileName, path.join(s.maps, "cellar.tmx"));
	assert.deepEqual(active.selectedObjects.map((o) => o.id), [1]);
	assert.deepEqual(s.fake.mapView.centered, [[32, 32]]);
	assert.deepEqual(s.fake.out.alerts, []);
	// Already open: brought to the front, not opened again.
	s.fake.api.tiled.activeAsset = town;
	s.fake.trigger(ACTIONS.follow);
	assert.equal(s.fake.api.tiled.openAssets.filter((a) => a.fileName.endsWith("cellar.tmx")).length, 1);

	const broken = s.open("broken.tmx");
	s.fake.select(broken, [3]);
	s.fake.trigger(ACTIONS.follow);
	assert.equal(s.fake.out.alerts.pop(), "YATI: Follow reference: cellar.tmx has no object named 'from_twn' of class player_spawn.");
	s.fake.api.tiled.activeAsset = broken;
	s.fake.select(broken, [2]);
	s.fake.trigger(ACTIONS.follow);
	assert.match(s.fake.out.alerts.pop(), /^YATI: Follow reference: to_map names .*attic\.tmx, which does not exist\.$/);
	s.fake.select(broken, [1]);
	s.fake.trigger(ACTIONS.follow);
	assert.match(s.fake.out.alerts.pop(), /^YATI: Follow reference: Object 'nowhere' \(id 1, class warp\) has no to_map\.$/);
	s.fake.select(broken, [7]);
	s.fake.trigger(ACTIONS.follow);
	assert.match(s.fake.out.alerts.pop(), /no rule of kind object_in_map, object_in_project, layer_in_map or map applies to class 'sign'/);
});

test("follow: an object_in_map rule without a map member looks in the same map; a map rule opens its map", () => {
	const s = make({ edit: setRules([
		{ "class": "sign", member: "text", kind: "object_in_map" },
		{ "class": "warp", member: "to_map", kind: "map" },
	]) });
	const map = s.open("broken.tmx");
	map._findObject(8)._props.text.value = "silent";
	s.fake.select(map, [8]);
	s.fake.trigger(ACTIONS.follow);
	assert.equal(s.fake.api.tiled.activeAsset.fileName, map.fileName);
	assert.deepEqual(s.fake.api.tiled.activeAsset.selectedObjects.map((o) => o.id), [9]);
	s.fake.select(map, [10]);
	s.fake.trigger(ACTIONS.follow);
	assert.equal(s.fake.api.tiled.activeAsset.fileName, path.join(s.maps, "town.tmx"));
	assert.deepEqual(s.fake.api.tiled.activeAsset.selectedObjects, []);
	assert.deepEqual(s.fake.out.alerts, []);
});

test("open or create: an existing file opened; an unset member made from the template and set; a missing one made", () => {
	const s = make({ fakeOptions: { process: (binary) => (binary === "xdg-open" ? { code: 0 } : { code: -1 }) } });
	const town = s.open("town.tmx");
	s.fake.select(town, [3]);
	s.fake.trigger(ACTIONS.openFile);
	assert.deepEqual(s.fake.out.processes, [{ binary: "xdg-open", args: [path.join(s.root, "sequences", "well_trigger.xml")] }]);
	assert.deepEqual(s.fake.out.alerts, []);
	// Unset: the template's copy in the rule's folder, named after the map and the object.
	const broken = s.open("broken.tmx");
	s.fake.select(broken, [5]);
	s.fake.trigger(ACTIONS.openFile);
	const made = path.join(s.root, "sequences", "broken_empty.xml");
	assert.equal(fs.readFileSync(made, "utf8"), fs.readFileSync(path.join(s.root, "templates", "trigger.xml"), "utf8"));
	assert.equal(prop(broken, 5, "sequence"), made);
	assert.deepEqual(broken._undo, ["YATI: Set sequence"]);
	assert.match(s.fake.out.logs.find((l) => l.indexOf("confirm: ") === 0), /^confirm: Make .*broken_empty\.xml from the template trigger\.xml and set sequence of Object 'empty' \(id 5, class trigger\) to it\?$/);
	assert.deepEqual(s.fake.out.processes[1], { binary: "xdg-open", args: [made] });
	// Set, but the file is gone: made again from the template, the member left as it is.
	fs.rmSync(path.join(s.root, "sequences", "half_trigger.xml"));
	s.fake.select(broken, [6]);
	s.fake.trigger(ACTIONS.openFile);
	assert.ok(fs.existsSync(path.join(s.root, "sequences", "half_trigger.xml")));
	assert.deepEqual(broken._undo, ["YATI: Set sequence"]);
	// The user says no: nothing made.
	s.fake.select(broken, [9]);
	fs.rmSync(path.join(s.root, "sequences", "well_trigger.xml"));
	s.fake.answers.confirms.push(false);
	s.fake.trigger(ACTIONS.openFile);
	assert.equal(fs.existsSync(path.join(s.root, "sequences", "well_trigger.xml")), false);
	// No file rule for the class.
	s.fake.select(broken, [7]);
	s.fake.trigger(ACTIONS.openFile);
	assert.match(s.fake.out.alerts.pop(), /no rule of kind file applies to class 'sign'/);
});

test("open or create: no launcher starts, one fails, yati_open_command; an existing copy reused; a missing folder", () => {
	const s = make({ fakeOptions: { process: () => ({ code: -1 }) } });
	const town = s.open("town.tmx");
	s.fake.select(town, [3]);
	s.fake.trigger(ACTIONS.openFile);
	assert.deepEqual(s.fake.out.processes.map((p) => p.binary), ["xdg-open", "open"]);
	assert.match(s.fake.out.alerts.pop(), /^Could not open .*well_trigger\.xml: xdg-open, open did not start\. Set the project property yati_open_command/);

	const t = make({
		edit: (json) => json.properties.push({ name: "yati_open_command", propertytype: "", type: "string", value: "code -r" }),
		fakeOptions: { process: () => ({ code: 2 }) },
	});
	const broken = t.open("broken.tmx");
	fs.writeFileSync(path.join(t.root, "sequences", "broken_empty.xml"), "<sequences/>");
	t.fake.select(broken, [5]);
	t.fake.trigger(ACTIONS.openFile);
	assert.match(t.fake.out.logs.find((l) => l.indexOf("confirm: ") === 0), /broken_empty\.xml exists already\. Set sequence of Object 'empty'/);
	assert.equal(fs.readFileSync(path.join(t.root, "sequences", "broken_empty.xml"), "utf8"), "<sequences/>");
	assert.deepEqual(t.fake.out.processes, [{ binary: "code", args: ["-r", path.join(t.root, "sequences", "broken_empty.xml")] }]);
	assert.match(t.fake.out.alerts.pop(), /'code' exited with 2/);

	fs.rmSync(path.join(t.root, "sequences"), { recursive: true });
	t.fake.select(broken, [9]);
	t.fake.trigger(ACTIONS.openFile);
	assert.match(t.fake.out.alerts.pop(), /the folder .*sequences does not exist/);
});

test("show text: the table's text for a key in every language, an unknown key, plain text", () => {
	const s = make();
	const town = s.open("town.tmx");
	s.fake.select(town, [4]);
	s.fake.trigger(ACTIONS.text);
	assert.equal(s.fake.out.alerts.pop(), "Object 'welcome' (id 4, class sign):\ntext: SIGN_WELCOME\n" +
		"  en: \"Welcome to town, traveller!\"\n  de: \"Willkommen in der Stadt, Reisender!\"");
	assert.ok(s.fake.out.logs.indexOf("YATI: text: SIGN_WELCOME") >= 0);
	s.fake.select(town, [5]);
	s.fake.trigger(ACTIONS.text);
	assert.equal(s.fake.out.alerts.pop(), "Object 'notice' (id 5, class interactable):\ntext: plain text (not shaped like a key): \"Plain words, no key.\"");
	const broken = s.open("broken.tmx");
	s.fake.select(broken, [7]);
	s.fake.trigger(ACTIONS.text);
	assert.equal(s.fake.out.alerts.pop(), "Object 'misspelt' (id 7, class sign):\ntext: SIGN_WELCOM is no key of strings.csv.\n  Did you mean SIGN_WELCOME, SIGN_WELL?");
	s.fake.select(broken, [1]);
	s.fake.trigger(ACTIONS.text);
	assert.match(s.fake.out.alerts.pop(), /no rule of kind key applies to class 'warp'/);
});

test("the rule actions without a project, without rules, without a map", () => {
	const s = make({ edit: (json) => { json.properties = []; } });
	for (const a of [ACTIONS.pick, ACTIONS.follow, ACTIONS.openFile, ACTIONS.text])
		s.fake.trigger(a);
	assert.equal(s.fake.out.alerts.length, 4);
	for (const text of s.fake.out.alerts)
		assert.match(text, /the project has no rules\. Set the project property yati_rules/);
	const t = make();
	t.fake.trigger(ACTIONS.follow);
	assert.match(t.fake.out.alerts[0], /open a map and select one object first/);
	assert.deepEqual(t.fake.out.errors, []);
});
