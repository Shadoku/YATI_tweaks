// yati-godot: the Godot side of a Tiled project, inside Tiled (Tiled 1.12 or newer).
//
// Actions in the Map menu (Tiled's scripting API cannot add a submenu, so the entries carry the
// prefix "YATI:"), the save-time validation of maps and tilesets, and nothing else: the logic is
// in lib/, plain modules without Tiled's globals, tested under Node (tiled-extension/test/).
//
//   YATI: Configure...             the settings, kept as custom properties of the Tiled project
//   YATI: Sync Godot project...    GodotPhysicsLayers / GodotNavigationLayers / GodotRenderLayers
//                                  (flags enums of project.godot's layer names), GodotNodeType, MapRef
//   YATI: Import scenes as classes...  one class type per scene with a script (godot_scene + exports)
//   YATI: Check map in Godot       runs the game's addons/yati_native/check_map.gd on the map
//   (on save)                      validation; the project property yati_validate = false turns it off
//
// Every Tiled API member this file uses is listed, with its reference page, in
// tiled-extension/test/helpers/api_surface.mjs; the tests fail when it uses another.

import {
	SETTING_NAMES, readSettings, settingsProblem, stringifyGenerated, stringifyFixups, storedPath, joinPath,
	baseName, localToGodot, cleanPath, parseImportOptions,
} from "./lib/settings.mjs";
import { readGodotProject } from "./lib/godot_project.mjs";
import { indexClassNames, scanScenes, walkFiles } from "./lib/scene_exports.mjs";
import { syncDefinitions, sceneClassDefinitions } from "./lib/types.mjs";
import { applyDefinitions, patchProjectText, mergeFixups, storageTypeValue } from "./lib/apply.mjs";
import { validateMap, validateTileset, templateReferences, contextFromTypes, maskFamily } from "./lib/validate.mjs";
import { CHECK_SCRIPT_LOCAL, checkArguments, parseCheckOutput, summarize } from "./lib/check_output.mjs";

export const ACTIONS = {
	configure: "YatiConfigure",
	sync: "YatiSyncGodotProject",
	scenes: "YatiImportScenes",
	check: "YatiCheckMap",
};

export const ACTION_TEXTS = {
	configure: "YATI: Configure…",
	sync: "YATI: Sync Godot project…",
	scenes: "YATI: Import scenes as classes…",
	check: "YATI: Check map in Godot",
};

const TITLE = "YATI";

// ---------------------------------------------------------------------------------------------
// Installation

// Registers the actions, the menu entries and the save hook. `api` holds Tiled's globals:
// {tiled, TextFile, File, Process, ClassPropertyType, StorageType} (the last two may be null).
export function install(api) {
	const t = api.tiled;
	const handlers = {
		configure: function () { guarded(api, function () { configure(api); }); },
		sync: function () { guarded(api, function () { syncGodotProject(api); }); },
		scenes: function () { guarded(api, function () { importScenes(api); }); },
		check: function () { guarded(api, function () { checkMap(api); }); },
	};
	const keys = ["configure", "sync", "scenes", "check"];
	const items = [{ separator: true }];
	for (let i = 0; i < keys.length; i++) {
		const action = t.registerAction(ACTIONS[keys[i]], handlers[keys[i]]);
		action.text = ACTION_TEXTS[keys[i]];
		items.push({ action: ACTIONS[keys[i]] });
	}
	t.extendMenu("Map", items);
	t.assetAboutToBeSaved.connect(function (asset) {
		guarded(api, function () { validateOnSave(api, asset); });
	});
}

// An action never lets an exception escape into Tiled: it becomes an Issue.
function guarded(api, fn) {
	try {
		fn();
	} catch (e) {
		api.tiled.error("YATI: " + (e && e.message ? e.message : String(e)), function () {});
	}
}

// ---------------------------------------------------------------------------------------------
// Files

function readText(api, path) {
	if (!api.File.exists(path))
		return null;
	const f = new api.TextFile(path, api.TextFile.ReadOnly);
	try {
		return f.readAll();
	} finally {
		f.close();
	}
}

function writeText(api, path, text) {
	const f = new api.TextFile(path, api.TextFile.WriteOnly);
	f.write(text);
	f.commit();
}

function listDir(api, dir) {
	const F = api.File;
	return {
		dirs: F.directoryEntries(dir, F.Dirs | F.NoDotAndDotDot),
		files: F.directoryEntries(dir, F.Files | F.Hidden),
	};
}

function currentSettings(api) {
	const project = api.tiled.project;
	return readSettings(function (name) { return project.property(name); }, project.fileName);
}

// The settings, or null after telling the user what is missing.
function requireSettings(api) {
	const s = currentSettings(api);
	const problem = settingsProblem(s, s.godotProject !== "" && api.File.exists(joinPath(s.godotProject, "project.godot")));
	if (problem !== "") {
		api.tiled.alert(problem, TITLE);
		return null;
	}
	return s;
}

// ---------------------------------------------------------------------------------------------
// YATI: Configure...

function configure(api) {
	const t = api.tiled;
	const project = t.project;
	const s = currentSettings(api);
	if (s.projectFile === "") {
		t.alert(settingsProblem(s, false), TITLE);
		return;
	}
	const dir = t.promptDirectory(s.godotProject !== "" ? s.godotProject : s.projectDir,
		"YATI: the Godot project (the folder that holds project.godot)");
	if (dir === "")
		return;
	const godotDir = cleanPath(dir);
	if (!api.File.exists(joinPath(godotDir, "project.godot"))) {
		t.alert("'" + godotDir + "' holds no project.godot. Choose the Godot project's own folder.", TITLE);
		return;
	}
	const binary = t.prompt("The Godot binary: a path, or a command on PATH (empty: keep '" +
		(s.godotBinaryRaw !== "" ? s.godotBinaryRaw : "godot") + "').", s.godotBinaryRaw !== "" ? s.godotBinaryRaw : "godot", TITLE).trim();
	const scenes = t.promptDirectory(s.scenesDirRaw !== "" ? s.scenesDir : godotDir,
		"YATI: the folder to scan for scenes (Cancel: the whole Godot project)");
	const options = t.prompt("Import options for Check map in Godot, as name=value pairs " +
		"(e.g. warn_unknown_classes=false); empty: keep; '-': none.", s.importOptionsRaw, TITLE).trim();

	const lines = [];
	setIfChanged(project, SETTING_NAMES.godotProject, storedPath(s.projectDir, godotDir));
	if (binary !== "")
		setIfChanged(project, SETTING_NAMES.godotBinary, binary);
	let scenesValue = "";
	if (scenes !== "" && cleanPath(scenes) !== godotDir) {
		const res = localToGodot(godotDir, cleanPath(scenes));
		scenesValue = res !== null ? res : cleanPath(scenes);
	}
	setIfChanged(project, SETTING_NAMES.scenesDir, scenesValue);
	if (options === "-")
		setIfChanged(project, SETTING_NAMES.importOptions, "");
	else if (options !== "")
		setIfChanged(project, SETTING_NAMES.importOptions, options);
	const after = currentSettings(api);
	lines.push("Godot project: " + after.godotProject);
	lines.push("Godot binary: " + after.godotBinary);
	lines.push("Scenes scanned under: " + after.scenesDir);
	lines.push("Import options: " + (after.importOptionsRaw !== "" ? after.importOptionsRaw : "(none)"));
	const optionWarnings = parseImportOptions(after.importOptionsRaw).warnings;
	for (let i = 0; i < optionWarnings.length; i++)
		lines.push(optionWarnings[i]);
	report(api, "Configured.", lines);
}

function setIfChanged(project, name, value) {
	const old = project.property(name);
	if (old === undefined ? value !== "" : String(old) !== value)
		project.setProperty(name, value);
}

// ---------------------------------------------------------------------------------------------
// Applying types: shared by the sync and the scenes action

// After applyDefinitions: the record saved, the project file patched (flags, float members), and
// the lines that tell the user what happened.
function finishApply(api, settings, result) {
	const t = api.tiled;
	const project = t.project;
	const lines = [];
	if (result.created.length > 0)
		lines.push("Created: " + result.created.join(", "));
	if (result.updated.length > 0)
		lines.push("Updated: " + result.updated.join(", "));
	if (result.unchanged.length > 0)
		lines.push("Unchanged: " + result.unchanged.join(", "));
	for (let i = 0; i < result.kept.length; i++)
		lines.push("Kept " + result.kept[i].type + "." + result.kept[i].member + " (made by hand; not overwritten)");
	for (let i = 0; i < result.removed.length; i++)
		lines.push("Removed " + result.removed[i].type + "." + result.removed[i].member + " (the script no longer exports it)");
	for (let i = 0; i < result.errors.length; i++)
		lines.push("Error: " + result.errors[i]);

	const recordText = stringifyGenerated(result.record);
	if (recordText !== stringifyGenerated(settings.generated))
		project.setProperty(SETTING_NAMES.generated, recordText);
	const fixups = mergeFixups(settings.fixups, result.fixups, result.record);
	const fixupsText = stringifyFixups(fixups.kept);
	if (fixupsText !== stringifyFixups(settings.fixups))
		project.setProperty(SETTING_NAMES.fixups, fixupsText);

	// What the API cannot set goes into the file Tiled has just saved, all of it every time: a
	// save of the project from memory (any change above) has dropped what an earlier run wrote.
	const text = readText(api, project.fileName);
	let reopen = false;
	if (text !== null) {
		const patched = patchProjectText(text, fixups.patch);
		if (patched.error)
			lines.push("Error: " + patched.error);
		if (patched.changes.length > 0) {
			writeText(api, project.fileName, patched.text);
			reopen = true;
			lines.push("Set in the project file (the scripting API cannot): " + patched.changes.join("; "));
		}
	}
	return { lines: lines, reopen: reopen };
}

const REOPEN_TEXT = "Reopen the project now (File > Open File or Project, or Project > Recent Projects) so " +
	"Tiled reads what was written into the project file. Until then Tiled's next save of the project " +
	"(any change to custom types or project properties) overwrites it; running the action again repairs it.";

function report(api, headline, lines, reopen) {
	const t = api.tiled;
	t.log("YATI: " + headline);
	for (let i = 0; i < lines.length; i++)
		t.log("YATI:   " + lines[i]);
	let text = headline;
	if (lines.length > 0)
		text += "\n\n" + lines.join("\n");
	if (reopen)
		text += "\n\n" + REOPEN_TEXT;
	t.alert(text, TITLE);
}

// ---------------------------------------------------------------------------------------------
// YATI: Sync Godot project...

function syncGodotProject(api) {
	const s = requireSettings(api);
	if (s === null)
		return;
	const godotText = readText(api, joinPath(s.godotProject, "project.godot"));
	const info = readGodotProject(godotText === null ? "" : godotText);
	const sync = syncDefinitions(info);
	const result = applyDefinitions(api, sync.defs, s.generated, s.projectDir);
	const done = finishApply(api, s, result);
	const lines = done.lines.concat(sync.skipped.map(function (x) { return "Not generated: " + x; }));
	report(api, "Synced with the Godot project " + (info.name !== "" ? "'" + info.name + "' " : "") + "(" + s.godotProject + ").", lines, done.reopen);
}

// ---------------------------------------------------------------------------------------------
// YATI: Import scenes as classes...

function importScenes(api) {
	const t = api.tiled;
	const s = requireSettings(api);
	if (s === null)
		return;
	const io = {
		listDir: function (dir) { return listDir(api, dir); },
		readText: function (path) { return readText(api, path); },
	};
	const scriptFiles = walkFiles(s.godotProject, io.listDir,
		function (name) { return /\.gd$/i.test(name); }, null);
	const scripts = [];
	for (let i = 0; i < scriptFiles.length; i++) {
		const text = io.readText(scriptFiles[i]);
		if (text !== null)
			scripts.push({ path: scriptFiles[i], text: text });
	}
	const classIndex = indexClassNames(scripts);
	const scan = scanScenes(s.godotProject, s.scenesDir, io, classIndex);
	if (scan.scenes.length === 0) {
		t.alert("No scene under " + s.scenesDir + " has a script on its root node; nothing to import." +
			(scan.skipped.length > 0 ? "\n\n" + scan.skipped.map(function (x) { return x.sceneRes + ": " + x.reason; }).join("\n") : ""), TITLE);
		return;
	}
	if (!t.confirm("Make or update " + scan.scenes.length + " class type(s) from the scenes under " + s.scenesDir +
		"?\n\nMembers made by hand in Tiled are kept.", TITLE))
		return;
	const knownEnums = {};
	const types = t.project.propertyTypes;
	for (let i = 0; i < types.length; i++) {
		if (types[i].isEnum)
			knownEnums[types[i].name] = true;
	}
	const built = sceneClassDefinitions(scan, {
		godotProjectDir: s.godotProject,
		tiledProjectDir: s.projectDir,
		classIndex: classIndex,
		knownEnums: knownEnums,
	});
	const result = applyDefinitions(api, built.defs, s.generated, s.projectDir);
	const done = finishApply(api, s, result);
	const lines = done.lines.slice();
	for (let i = 0; i < built.classes.length; i++) {
		const c = built.classes[i];
		for (let k = 0; k < c.notes.length; k++)
			lines.push(c.name + "." + c.notes[k]);
		for (let k = 0; k < c.skipped.length; k++)
			lines.push(c.name + ": export '" + c.skipped[k].name + "' skipped (" + c.skipped[k].reason + ")");
	}
	for (let i = 0; i < built.warnings.length; i++)
		lines.push(built.warnings[i]);
	report(api, "Imported " + built.classes.length + " scene(s) as classes.", lines, done.reopen);
}

// ---------------------------------------------------------------------------------------------
// YATI: Check map in Godot

function checkMap(api) {
	const t = api.tiled;
	const map = t.activeAsset;
	if (map === null || map === undefined || !map.isTileMap) {
		t.alert("Open the map to check first (Check map in Godot runs on the active map).", TITLE);
		return;
	}
	if (map.fileName === "") {
		t.alert("Save the map first: Godot imports the file.", TITLE);
		return;
	}
	const s = requireSettings(api);
	if (s === null)
		return;
	if (!api.File.exists(joinPath(s.godotProject, CHECK_SCRIPT_LOCAL))) {
		t.alert("The Godot project has no " + CHECK_SCRIPT_LOCAL + " (the native YATI importer's check script, " +
			"in releases after YATI 3.1.0). Copy a current addons/yati_native/ into the game to use Check map in Godot.", TITLE);
		return;
	}
	if (map.modified) {
		if (!t.confirm("The map has unsaved changes. Save it before the check? (No: check the file as last saved.)", TITLE)) {
			t.log("YATI: checking the map as last saved.");
		} else {
			map.save();
		}
	}
	const args = checkArguments(s.godotProject, map.fileName, s.projectFile, s.importOptions);
	const p = new api.Process();
	let code;
	let out = "";
	let err = "";
	try {
		code = p.exec(s.godotBinary, args, false);
		out = p.readStdOut();
		err = p.readStdErr();
	} finally {
		p.close();
	}
	if (code === -1) {
		t.alert("Could not start '" + s.godotBinary + "'. Set the Godot binary with YATI: Configure... " +
			"(a full path, or a command on PATH).", TITLE);
		return;
	}
	const parsed = parseCheckOutput(out + "\n" + err);
	const fileName = map.fileName;
	for (let i = 0; i < parsed.messages.length; i++) {
		const m = parsed.messages[i];
		const text = "YATI check: " + baseName(fileName) + ": " + m.text;
		const activate = selector(api, fileName, m.objectId, 0);
		if (m.severity === "error")
			t.error(text, activate);
		else
			t.warn(text, activate);
	}
	const line = summarize(parsed, code, err);
	t.log("YATI: " + line);
	if (parsed.result === null) {
		t.alert(line, TITLE);
		return;
	}
	t.alert(parsed.messages.length === 0 ? line : line + "\n\nThe messages are in the Issues view; activating one selects its object.", TITLE);
}

// ---------------------------------------------------------------------------------------------
// Validation on save

function validateOnSave(api, asset) {
	if (asset === null || asset === undefined || !(asset.isTileMap || asset.isTileset))
		return;
	const s = currentSettings(api);
	if (!s.validate)
		return;
	const ctx = contextFromTypes(projectTypes(api), s.projectFile, s.godotProject,
		function (path) { return api.File.exists(path); });
	let findings;
	if (asset.isTileMap) {
		const model = mapModel(api, asset);
		if (asset.fileName !== "") {
			const text = readText(api, asset.fileName);
			if (text !== null)
				model.templates = templateReferences(text);
		}
		findings = validateMap(model, ctx);
	} else {
		findings = validateTileset(tilesetModel(api, asset), ctx);
	}
	const fileName = asset.fileName;
	for (let i = 0; i < findings.length; i++) {
		const f = findings[i];
		const text = "YATI: " + baseName(fileName) + ": " + f.text;
		const activate = asset.isTileMap ? selector(api, fileName, f.objectId, f.layerId) : function () {};
		if (f.severity === "error")
			api.tiled.error(text, activate);
		else
			api.tiled.warn(text, activate);
	}
	if (findings.length > 0)
		api.tiled.log("YATI: " + baseName(fileName) + ": " + findings.length + " finding(s), see the Issues view.");
}

// The project's types as the validator reads them.
function projectTypes(api) {
	const out = [];
	const types = api.tiled.project.propertyTypes;
	for (let i = 0; i < types.length; i++) {
		const t = types[i];
		if (t.isEnum) {
			out.push({ name: t.name, isEnum: true, isClass: false, values: t.values.slice(), storageType: t.storageType });
		} else if (t.isClass) {
			const members = [];
			const m = t.members;
			const names = Object.keys(m);
			for (let k = 0; k < names.length; k++) {
				const p = propFromValue(api, names[k], m[names[k]]);
				if (p !== null)
					members.push(p);
			}
			out.push({ name: t.name, isEnum: false, isClass: true, members: members });
		}
	}
	return out;
}

// A custom property's value from the API as the validator's {name, type, value, propertyType}.
export function propFromValue(api, name, v) {
	if (v === undefined || v === null)
		return null;
	if (typeof v === "boolean")
		return { name: name, type: "bool", value: v, propertyType: "" };
	if (typeof v === "number")
		return { name: name, type: Math.floor(v) === v ? "int" : "float", value: v, propertyType: "" };
	if (typeof v === "string")
		return { name: name, type: "string", value: v, propertyType: "" };
	if (Array.isArray(v)) {
		const items = [];
		for (let i = 0; i < v.length; i++) {
			const p = propFromValue(api, "", v[i]);
			if (p !== null)
				items.push(p);
		}
		return { name: name, type: "list", value: items, propertyType: "" };
	}
	if (typeof v === "object") {
		if (v.typeName !== undefined) {
			// A PropertyValue: an enum's value (an index, or a mask of flags) or a class value.
			const et = api.tiled.project.findTypeByName(v.typeName);
			if (et !== null && et.isEnum && typeof v.value === "number") {
				// Stored as a string (as the importers read it): the value's name, or the flags' names.
				if (et.storageType === storageTypeValue(api, "int"))
					return { name: name, type: "int", value: v.value, propertyType: v.typeName };
				// The API does not tell a flags enum: a mask property's value is taken as the flags
				// (its set bits' names), any other as one value.
				if (maskFamily(name, true) !== "" || maskFamily(name, false) !== "") {
					const values = et.values;
					const names = [];
					for (let b = 0; b < values.length; b++) {
						if ((v.value & (1 << b)) !== 0)
							names.push(values[b]);
					}
					return { name: name, type: "string", value: names.join(","), propertyType: v.typeName, fromEnum: true };
				}
				return { name: name, type: "string", value: et.nameOf(v.value), propertyType: v.typeName, fromEnum: true };
			}
			return { name: name, type: "class", value: v.value, propertyType: v.typeName };
		}
		if (v.localFile !== undefined) {
			// A FilePath: the local file, or the URL as written when it is none (res://...).
			const local = String(v.localFile);
			return { name: name, type: "file", value: local !== "" ? local : String(v.url === undefined || v.url === null ? "" : v.url), propertyType: "" };
		}
		if (v.id !== undefined)
			return { name: name, type: "object", value: v.id, propertyType: "" };	// ObjectRef or MapObject
		return { name: name, type: "color", value: String(v), propertyType: "" };
	}
	return { name: name, type: "string", value: String(v), propertyType: "" };
}

function propsOf(api, values) {
	const out = [];
	const names = Object.keys(values || {});
	for (let i = 0; i < names.length; i++) {
		const p = propFromValue(api, names[i], values[names[i]]);
		if (p !== null)
			out.push(p);
	}
	return out;
}

// The map as the validator's model.
export function mapModel(api, map) {
	const model = { kind: "map", fileName: map.fileName, properties: propsOf(api, map.properties()), layers: [], objects: [], templates: [] };
	const walk = function (layers, prefix) {
		for (let i = 0; i < layers.length; i++) {
			const l = layers[i];
			const path = prefix + l.name;
			model.layers.push({ id: l.id, name: l.name, path: path, className: l.className, properties: propsOf(api, l.properties()) });
			if (l.isObjectLayer) {
				const objects = l.objects;
				for (let k = 0; k < objects.length; k++) {
					const o = objects[k];
					model.objects.push({
						id: o.id,
						name: o.name,
						layerPath: path,
						className: o.className,
						tileClassName: o.tile ? o.tile.className : "",
						properties: propsOf(api, o.resolvedProperties()),
					});
				}
			}
			if (l.isGroupLayer)
				walk(l.layers, path + "/");
		}
	};
	walk(map.layers, "");
	return model;
}

export function tilesetModel(api, tileset) {
	return { kind: "tileset", fileName: tileset.fileName, name: tileset.name, properties: propsOf(api, tileset.properties()) };
}

// An Issue's activation: the map opened (or brought to the front) and the object (or layer) selected.
function selector(api, fileName, objectId, layerId) {
	return function () {
		const t = api.tiled;
		let map = null;
		const open = t.openAssets;
		for (let i = 0; i < open.length; i++) {
			if (open[i].fileName === fileName)
				map = open[i];
		}
		if (map === null)
			map = t.open(fileName);
		if (map === null || !map.isTileMap)
			return;
		t.activeAsset = map;
		if (objectId > 0) {
			const o = findObject(map.layers, objectId);
			if (o !== null) {
				map.selectedObjects = [o];
				const view = t.mapEditor.currentMapView;
				if (view) {
					const p = map.pixelToScreen(o.x, o.y);
					view.centerOn(p.x, p.y);
				}
			}
		} else if (layerId > 0) {
			const l = findLayer(map.layers, layerId);
			if (l !== null)
				map.currentLayer = l;
		}
	};
}

function findObject(layers, id) {
	for (let i = 0; i < layers.length; i++) {
		const l = layers[i];
		if (l.isObjectLayer) {
			const objects = l.objects;
			for (let k = 0; k < objects.length; k++) {
				if (objects[k].id === id)
					return objects[k];
			}
		}
		if (l.isGroupLayer) {
			const o = findObject(l.layers, id);
			if (o !== null)
				return o;
		}
	}
	return null;
}

function findLayer(layers, id) {
	for (let i = 0; i < layers.length; i++) {
		if (layers[i].id === id)
			return layers[i];
		if (layers[i].isGroupLayer) {
			const l = findLayer(layers[i].layers, id);
			if (l !== null)
				return l;
		}
	}
	return null;
}

// ---------------------------------------------------------------------------------------------
// In Tiled: install with Tiled's globals. (Under Node, the tests call install() with a fake.)

/* global tiled, TextFile, File, Process, ClassPropertyType, StorageType */
if (typeof tiled !== "undefined" && typeof tiled.registerAction === "function") {
	install({
		tiled: tiled,
		TextFile: TextFile,
		File: File,
		Process: Process,
		ClassPropertyType: typeof ClassPropertyType !== "undefined" ? ClassPropertyType : null,
		StorageType: typeof StorageType !== "undefined" ? StorageType : null,
	});
}
