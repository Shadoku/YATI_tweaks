// The rules about a map's own layers and properties in the glue, against the fake API on the rules
// sample's maps/tower.tmx and maps/house.tmx: the save hook, Pick and Follow on a layer_in_map rule,
// Open or create file and Show text on the map's own rules with nothing selected, and Create
// counterpart.
import { test, after } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { ACTIONS, mapModel } from "../yati-godot/yati-godot.mjs";
import { rulesScenario } from "./helpers/scenario.mjs";
import { tmxModel, projectTypes, enumsOf } from "./helpers/tmx_model.mjs";

const scenarios = [];
function make(options) {
	const s = rulesScenario(options);
	scenarios.push(s);
	return s;
}
after(() => { for (const s of scenarios) s.cleanup(); });

// A property as the fake holds it: a file's local path, an enum value's name.
function value(s, holder, name) {
	const v = holder.resolvedProperties ? holder.resolvedProperties()[name] : holder.properties()[name];
	if (v !== null && typeof v === "object" && v.localFile !== undefined)
		return v.localFile;
	if (v !== null && typeof v === "object" && v.typeName !== undefined)
		return s.fake.project._types.find((t) => t._name === v.typeName)._values[v.value];
	return v;
}

const addRules = (rules) => (json) => {
	const p = json.properties.find((x) => x.name === "yati_rules");
	p.value = JSON.stringify(JSON.parse(p.value).concat(rules));
};
const lastAlert = (s) => s.fake.out.alerts[s.fake.out.alerts.length - 1];

test("the API model of a map with levels equals the file model: kinds, groups, offsets, rectangles, the map's class", () => {
	const s = make();
	const file = path.join(s.maps, "tower.tmx");
	s.open("tower.tmx");
	const fromApi = mapModel(s.fake.api, s.fake.api.tiled.activeAsset);
	const fromFile = tmxModel(file, enumsOf(projectTypes(s.projectFile)));
	const strip = (m) => ({
		className: m.className,
		layers: m.layers.map((l) => [l.id, l.name, l.type, l.parentId, l.offsetX, l.offsetY, l.className]),
		objects: m.objects.map((o) => [o.id, o.layerId, o.x, o.y, o.width, o.height, o.isTile]),
	});
	assert.deepEqual(strip(fromApi), strip(fromFile));
	assert.deepEqual(fromApi.layers.map((l) => l.type), ["group", "object", "group", "object", "group", "image", "tile"]);
	assert.deepEqual([fromApi.layers[4].offsetX, fromApi.layers[4].offsetY], [8, 4]);
});

test("save hook: the map's own properties and the level rules as Issues; a map's Issue selects nothing", () => {
	const s = make();
	s.fake.emitAboutToBeSaved(s.open("house.tmx"));
	assert.deepEqual(s.fake.out.issues, []);
	const tower = s.open("tower.tmx");
	s.fake.emitAboutToBeSaved(tower);
	const issues = s.fake.out.issues;
	assert.deepEqual(issues.map((i) => i.text.replace(/\(YATI: Create.*$/, "")), [
		"YATI: tower.tmx: Map properties: sequence names notice.xml, which has no section named on_enter or on_leave.",
		"YATI: tower.tmx: Map properties: display_name is 'MAP_HOUS', shaped like a key, but strings.csv has no such key (did you mean MAP_HOUSE?).",
		"YATI: tower.tmx: Object 'lone' (id 2, layer ground/Objects): target names the layer 'first', which has no object of class stairs overlapping this one with target 'ground' and direction 'DOWN' ",
		"YATI: tower.tmx: Object 'roof' (id 3, layer ground/Objects): target names 'roof', but this map has no group layer of that name and class level (its group layers of class level: ground, first).",
		"YATI: tower.tmx: Object 'floorish' (id 5, layer ground/Objects): target names 'Floor', but this map has no group layer of that name and class level (its group layers of class level: ground, first; the layer 'Floor' is a tile layer of class none).",
		"YATI: tower.tmx: Object 'same' (id 6, layer ground/Objects): target names the layer 'first', which has no object of class stairs overlapping this one with target 'ground' and direction 'RIGHT' ",
		"YATI: tower.tmx: Object 'same_up' (id 7, layer first/Objects): target names the layer 'ground', which has no object of class stairs overlapping this one with target 'first' and direction 'RIGHT' ",
	]);
	issues[0].activate();
	assert.equal(s.fake.api.tiled.activeAsset.fileName, tower.fileName);
	assert.deepEqual(tower.selectedObjects, []);
	issues[3].activate();
	assert.deepEqual(tower.selectedObjects.map((o) => o.id), [3]);
});

test("pick: a layer_in_map member from the map's layers of the rule's class and kind, in one undo step; the map's own", () => {
	const s = make({ edit: addRules([{ map: true, member: "start_level", kind: "layer_in_map", layerType: "group", targetClass: "level" }]) });
	const tower = s.open("tower.tmx");
	s.fake.select(tower, [3]);
	let seen = null;
	s.fake.answers.dialogs.push((d) => { seen = d; d.field("Layer").currentIndex = 1; d.button("OK").click(); });
	s.fake.trigger(ACTIONS.pick);
	assert.deepEqual(seen.field("Layer").items, ["ground", "first"]);
	assert.equal(seen.labels()[0].text, "Object 'roof' (id 3, class stairs): target, a group layer of class level of this map");
	assert.equal(value(s, tower._findObject(3), "target"), "first");
	assert.deepEqual(tower._undo, ["YATI: Set target"]);
	// The list starts at the current value.
	s.fake.select(tower, [7]);
	s.fake.answers.dialogs.push((d) => { seen = d; d.button("Cancel").click(); });
	s.fake.trigger(ACTIONS.pick);
	assert.equal(seen.field("Layer").currentIndex, 0);
	// Nothing selected: the map's own layer_in_map rule.
	tower._selected = [];
	s.fake.answers.dialogs.push((d) => { seen = d; d.field("Layer").currentIndex = 1; d.button("OK").click(); });
	s.fake.trigger(ACTIONS.pick);
	assert.match(seen.labels()[0].text, /^Map tower\.tmx: start_level, a group layer of class level of this map$/);
	assert.equal(value(s, tower, "start_level"), "first");
	assert.deepEqual(tower._undo, ["YATI: Set target", "YATI: Set start_level"]);
});

test("follow: a layer_in_map member makes the layer current; a layer that is not there is said", () => {
	const s = make();
	const tower = s.open("tower.tmx");
	s.fake.select(tower, [1]);
	s.fake.trigger(ACTIONS.follow);
	assert.equal(tower.currentLayer.id, 3);
	assert.equal(tower.currentLayer.name, "first");
	assert.deepEqual(s.fake.out.alerts, []);
	s.fake.select(tower, [3]);
	s.fake.trigger(ACTIONS.follow);
	assert.equal(lastAlert(s), "YATI: Follow reference: this map has no group layer named 'roof' of class level.");
});

test("open or create and show text with nothing selected act on the map's own rules", () => {
	const s = make({ fakeOptions: { process: (binary) => (binary === "xdg-open" ? { code: 0 } : { code: -1 }) } });
	const tower = s.open("tower.tmx");
	s.fake.trigger(ACTIONS.openFile);
	assert.deepEqual(s.fake.out.processes, [{ binary: "xdg-open", args: [path.join(s.root, "sequences", "notice.xml")] }]);
	s.fake.trigger(ACTIONS.text);
	assert.equal(lastAlert(s), "Map tower.tmx:\ndisplay_name: MAP_HOUS is no key of strings.csv.\n  Did you mean MAP_HOUSE?");
	s.open("house.tmx");
	s.fake.trigger(ACTIONS.text);
	assert.equal(lastAlert(s), "Map house.tmx:\ndisplay_name: MAP_HOUSE\n  en: \"The house\"\n  de: \"Das Haus\"");
	// An unset map sequence: the template copied as sequences/<map>.xml and the property set.
	const town = s.open("town.tmx");
	s.fake.trigger(ACTIONS.openFile);
	const made = path.join(s.root, "sequences", "town.xml");
	assert.equal(fs.readFileSync(made, "utf8"), fs.readFileSync(path.join(s.root, "templates", "trigger.xml"), "utf8"));
	assert.equal(value(s, town, "sequence"), made);
	assert.deepEqual(town._undo, ["YATI: Set sequence"]);
	assert.match(s.fake.out.logs.find((l) => /^confirm: Make .*town\.xml/.test(l)), /and set sequence of Map town\.tmx to it\?$/);
	assert.deepEqual(tower._undo, []);
	// Without a rule of the map for the action, nothing selected is still "select one object".
	const t = make({ edit: (json) => {
		const p = json.properties.find((x) => x.name === "yati_rules");
		p.value = JSON.stringify(JSON.parse(p.value).filter((r) => r.map !== true));
	} });
	t.open("tower.tmx");
	t.fake.trigger(ACTIONS.text);
	assert.equal(lastAlert(t), "YATI: Show text for selected object: select exactly one object (0 selected).");
});

test("create counterpart: the twin on the named level in one undo step, selected; the check then quiet about it", () => {
	const s = make();
	const tower = s.open("tower.tmx");
	s.fake.select(tower, [2]);
	s.fake.trigger(ACTIONS.counterpart);
	assert.deepEqual(s.fake.out.alerts, []);
	const twin = tower._findObject(8);
	assert.ok(twin !== null);
	assert.deepEqual([twin._layer._name, twin._name, twin._class, twin._x, twin._y, twin._width, twin._height], ["Objects", "lone", "stairs", 160, 64, 32, 32]);
	assert.equal(twin._layer, tower._findObject(4)._layer);
	assert.equal(value(s, twin, "target"), "ground");
	assert.equal(value(s, twin, "direction"), "DOWN");
	assert.deepEqual(tower._undo, ["YATI: Create counterpart"]);
	assert.deepEqual(tower.selectedObjects.map((o) => o.id), [8]);
	assert.ok(s.fake.out.logs.indexOf("YATI: Object 'lone' (id 2, class stairs): made its counterpart on layer first/Objects (target ground, direction DOWN).") >= 0);
	s.fake.emitAboutToBeSaved(tower);
	assert.equal(s.fake.out.issues.filter((i) => i.text.indexOf("'lone'") >= 0).length, 0);
	assert.equal(s.fake.out.issues.length, 6);
	// An object that has one: it is selected and said.
	s.fake.select(tower, [1]);
	s.fake.trigger(ACTIONS.counterpart);
	assert.equal(lastAlert(s), "YATI: Create counterpart: Object 'up' (id 1, class stairs) already has its counterpart: Object 'down' (id 4, layer first/Objects), now selected.");
	assert.deepEqual(tower.selectedObjects.map((o) => o.id), [4]);
	// Its other properties are copied (here a note), a layer that is not there is said.
	tower._findObject(6).setProperty("note", "by the window");
	s.fake.select(tower, [3]);
	s.fake.trigger(ACTIONS.counterpart);
	assert.match(lastAlert(s), /^YATI: Create counterpart: Object 'roof' \(id 3, class stairs\): target names 'roof', but this map has no group layer/);
	s.fake.select(tower, [6]);
	s.fake.trigger(ACTIONS.counterpart);
	const copy = tower._findObject(9);
	assert.deepEqual([value(s, copy, "note"), value(s, copy, "direction"), value(s, copy, "target")], ["by the window", "RIGHT", "ground"]);
	// Nothing selected, no rule for the class.
	tower._selected = [];
	s.fake.trigger(ACTIONS.counterpart);
	assert.equal(lastAlert(s), "YATI: Create counterpart: select exactly one object (0 selected).");
	const broken = s.open("broken.tmx");
	s.fake.select(broken, [7]);
	s.fake.trigger(ACTIONS.counterpart);
	assert.equal(lastAlert(s), "YATI: Create counterpart: no rule of kind counterpart applies to class 'sign'.");
	assert.deepEqual(s.fake.out.errors.filter((e) => e.indexOf("YATI: tower.tmx") !== 0), []);
});
