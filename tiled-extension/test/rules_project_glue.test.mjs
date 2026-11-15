// The rules that look across the project's maps in the glue, against the fake API on the rules
// sample (sample/rules/: its portals in maps/portals/, rules object_in_project and unique_name):
// the save hook's findings, the maps found through Project.folders, a map open in Tiled read as it
// is now, the parsed maps kept from one save to the next, and Pick and Follow on such a member.
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

const issueTexts = (s) => s.fake.out.issues.map((i) => i.severity + " " + i.text);

test("save hook: object_in_project and unique_name findings across the project's folders", () => {
	const s = make();
	const clash = s.open("portals/clash.tmx");
	s.fake.emitAboutToBeSaved(clash);
	assert.deepEqual(issueTexts(s), [
		"error YATI: clash.tmx: Object 'east_gate' (id 1, layer Objects): the name 'east_gate' is also used by maps/portals/east.tmx (id 1, class portal); the project's rules (yati_rules) want the names of portal and player_spawn objects unique across the project.",
		"error YATI: clash.tmx: Object 'east_gate' (id 1, layer Objects): to names 'nowhere_gate', but no map of the project has an object of that name and class portal or player_spawn.",
		"error YATI: clash.tmx: Object (unnamed) (id 2, layer Objects): has no name; the project's rules (yati_rules) require one for portal and player_spawn objects.",
		"error YATI: clash.tmx: Object 'twin' (id 3, layer Objects): the name 'twin' is also used by this map (id 4, class player_spawn); the project's rules (yati_rules) want the names of portal and player_spawn objects unique across the project.",
		"error YATI: clash.tmx: Object 'twin' (id 4, layer Objects): the name 'twin' is also used by this map (id 3, class player_spawn); the project's rules (yati_rules) want the names of portal and player_spawn objects unique across the project.",
		"error YATI: clash.tmx: Object 'to_twin' (id 5, layer Objects): to names 'twin', which 2 objects of the project have: this map (id 3), this map (id 4); give them names of their own.",
	]);
	s.fake.out.issues.find((i) => i.text.indexOf("'nowhere_gate'") >= 0).activate();
	assert.deepEqual(s.fake.api.tiled.activeAsset.selectedObjects.map((o) => o.id), [1]);
	// west.tmx's portal leads into another folder (maps/cellar.tmx's spawn): no finding. east.tmx
	// is named by clash.tmx's portal too; the existing maps file nothing new.
	const t = make();
	t.fake.emitAboutToBeSaved(t.open("portals/west.tmx"));
	t.fake.emitAboutToBeSaved(t.open("town.tmx"));
	t.fake.emitAboutToBeSaved(t.open("cellar.tmx"));
	assert.deepEqual(issueTexts(t), []);
	t.fake.emitAboutToBeSaved(t.open("portals/east.tmx"));
	assert.deepEqual(issueTexts(t), [
		"error YATI: east.tmx: Object 'east_gate' (id 1, layer Objects): the name 'east_gate' is also used by maps/portals/clash.tmx (id 1, class portal); the project's rules (yati_rules) want the names of portal and player_spawn objects unique across the project.",
	]);
});

test("save hook: an open map counts as it is in Tiled; a renamed object is seen at once", () => {
	const s = make();
	const clash = s.open("portals/clash.tmx");
	clash._findObject(1)._name = "clash_gate";		// renamed in Tiled, not saved
	s.fake.emitAboutToBeSaved(s.open("portals/east.tmx"));
	assert.deepEqual(issueTexts(s), []);
	// The spawn west.tmx's portal names, renamed in its open map: west.tmx's save says so.
	const cellar = s.open("cellar.tmx");
	cellar._findObject(1)._name = "from_somewhere";
	s.fake.emitAboutToBeSaved(s.open("portals/west.tmx"));
	assert.deepEqual(issueTexts(s), [
		"error YATI: west.tmx: Object 'west_gate' (id 1, layer Objects): to names 'from_town', but no map of the project has an object of that name and class portal or player_spawn.",
	]);
});

test("save hook: the project's folders from Project.folders; a map changed on disk between saves is read again", () => {
	// Only maps/portals is a folder of the project: town and cellar are not, so from_town is unknown.
	const s = make({ edit: (json) => { json.folders = ["maps/portals"]; } });
	s.fake.emitAboutToBeSaved(s.open("portals/west.tmx"));
	assert.deepEqual(issueTexts(s), [
		"error YATI: west.tmx: Object 'west_gate' (id 1, layer Objects): to names 'from_town', but no map of the project has an object of that name and class portal or player_spawn.",
	]);
	// Saves in a row: a map's file changed in between (not open in Tiled) is read again.
	const t = make();
	const west = t.open("portals/west.tmx");
	t.fake.emitAboutToBeSaved(west);
	assert.deepEqual(issueTexts(t), []);
	const cellarFile = path.join(t.maps, "cellar.tmx");
	fs.writeFileSync(cellarFile, fs.readFileSync(cellarFile, "utf8").replace("name=\"from_town\"", "name=\"from_elsewhere\""));
	t.fake.emitAboutToBeSaved(west);
	assert.equal(t.fake.out.issues.length, 1);
	assert.match(t.fake.out.issues[0].text, /to names 'from_town', but no map of the project has an object of that name/);
});

test("pick: an object_in_project member offers every named target of the project by map and sets the one member", () => {
	const s = make();
	const east = s.open("portals/east.tmx");
	s.fake.select(east, [1]);
	let seen = null;
	s.fake.answers.dialogs.push((d) => { seen = d; d.field("Object").currentIndex = 0; d.button("OK").click(); });
	s.fake.trigger(ACTIONS.pick);
	assert.deepEqual(s.fake.out.alerts, []);
	assert.equal(seen.windowTitle, "YATI: Pick map and object…");
	assert.deepEqual(seen.field("Object").items, [
		"maps/cellar.tmx: from_town (player_spawn)",
		"maps/portals/clash.tmx: east_gate (portal)",
		"maps/portals/clash.tmx: to_twin (portal)",
		"maps/portals/clash.tmx: twin (player_spawn)",
		"maps/portals/clash.tmx: twin (player_spawn)",
		"maps/portals/east.tmx: east_gate (portal)",
		"maps/portals/east.tmx: east_start (player_spawn)",
		"maps/portals/west.tmx: west_gate (portal)",
		"maps/town.tmx: from_cellar (player_spawn)",
	]);
	assert.match(seen.labels()[0].text, /^Object 'east_gate' \(id 1, class portal\): to, an object of class portal or player_spawn in any map of the project$/);
	assert.equal(east._findObject(1).resolvedProperties().to, "from_town");
	assert.deepEqual(east._undo, ["YATI: Set to"]);
	// The list starts at the current value.
	s.fake.select(east, [1]);
	s.fake.answers.dialogs.push((d) => { seen = d; d.button("Cancel").click(); });
	s.fake.trigger(ACTIONS.pick);
	assert.equal(seen.field("Object").currentIndex, 0);
	assert.equal(east._undo.length, 1);
	// A warp has both kinds of rule (object_in_map from the sample): the rule is asked first.
	const t = make({ edit: (json) => {
		const p = json.properties.find((x) => x.name === "yati_rules");
		const rules = JSON.parse(p.value);
		rules.push({ "class": "warp", member: "back", kind: "object_in_project", targetClass: "warp" });
		p.value = JSON.stringify(rules);
	} });
	const broken = t.open("broken.tmx");
	t.fake.select(broken, [10]);
	const asked = [];
	t.fake.answers.dialogs.push((d) => { asked.push(d); d.field("Member").currentIndex = 1; d.button("OK").click(); });
	t.fake.answers.dialogs.push((d) => { asked.push(d); d.field("Object").currentIndex = 1; d.button("OK").click(); });
	t.fake.trigger(ACTIONS.pick);
	assert.deepEqual(asked[0].field("Member").items, [
		"to_spawn (an object of class player_spawn in the map to_map names)", "back (an object of class warp in any map of the project)",
	]);
	assert.deepEqual(asked[1].field("Object").items, ["maps/broken.tmx: back (warp)", "maps/broken.tmx: lost (warp)", "maps/broken.tmx: nowhere (warp)",
		"maps/broken.tmx: typo (warp)", "maps/broken.tmx: wrong_class (warp)", "maps/cellar.tmx: up (warp)", "maps/town.tmx: to_cellar (warp)"]);
	assert.equal(broken._findObject(10).resolvedProperties().back, "lost");
});

test("follow: the map that holds the named object opened and the object selected; none, or several, said plainly", () => {
	const s = make();
	const west = s.open("portals/west.tmx");
	s.fake.select(west, [1]);
	s.fake.trigger(ACTIONS.follow);
	const active = s.fake.api.tiled.activeAsset;
	assert.equal(active.fileName, path.join(s.maps, "cellar.tmx"));
	assert.deepEqual(active.selectedObjects.map((o) => o.id), [1]);
	assert.deepEqual(s.fake.out.alerts, []);
	const clash = s.open("portals/clash.tmx");
	s.fake.select(clash, [1]);
	s.fake.trigger(ACTIONS.follow);
	assert.equal(s.fake.out.alerts.pop(), "YATI: Follow reference: no map of the project has an object named 'nowhere_gate' of class portal or player_spawn.");
	// Two targets in this map: the first selected, and said.
	s.fake.api.tiled.activeAsset = clash;
	s.fake.select(clash, [5]);
	s.fake.trigger(ACTIONS.follow);
	assert.equal(s.fake.api.tiled.activeAsset.fileName, clash.fileName);
	assert.deepEqual(s.fake.api.tiled.activeAsset.selectedObjects.map((o) => o.id), [3]);
	assert.equal(s.fake.out.alerts.pop(), "YATI: Follow reference: 2 objects of the project are named 'twin' (maps/portals/clash.tmx id 3, maps/portals/clash.tmx id 4); the first is selected.");
	// An unset member.
	const east = s.open("portals/east.tmx");
	east._findObject(1)._props.to.value = "";
	s.fake.select(east, [1]);
	s.fake.trigger(ACTIONS.follow);
	assert.match(s.fake.out.alerts.pop(), /has no value in to\.$/);
});
