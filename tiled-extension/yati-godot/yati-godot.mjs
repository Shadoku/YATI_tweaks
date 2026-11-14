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
//   YATI: Pick map and object...   sets an object's map member and object member (yati_rules) at once
//   YATI: Follow reference         opens the map an object's members name and selects the named object
//   YATI: Open or create file...   opens a file member's file, or makes it from the rule's template
//   YATI: Show text for selected object   the translation table's text for its key members
//   (on save)                      validation, with the project's rules (yati_rules); the project
//                                  property yati_validate = false turns it off
//
// Every Tiled API member this file uses is listed, with its reference page, in
// tiled-extension/test/helpers/api_surface.mjs; the tests fail when it uses another.

import {
	SETTING_NAMES, readSettings, settingsProblem, stringifyGenerated, stringifyFixups, joinPath, baseName, dirName, cleanPath, fileUrl,
} from "./lib/settings.mjs";
import {
	TITLE as CONFIGURE_TITLE, OPTIONS_TITLE, TEXTS, FIELDS, OPTION_FIELDS, initialValues, managedSummary,
	godotProjectStatus, godotVersionStatus, validateConfiguration, configuredLines, optionTooltip, optionChoices,
	optionsToValues, valuesToOptions, resolvedBinary,
} from "./lib/configure.mjs";
import { readGodotProject } from "./lib/godot_project.mjs";
import { indexClassNames, scanScenes, walkFiles } from "./lib/scene_exports.mjs";
import { syncDefinitions, sceneClassDefinitions } from "./lib/types.mjs";
import { applyDefinitions, patchProjectText, mergeFixups, storageTypeValue } from "./lib/apply.mjs";
import { validateMap, validateTileset, templateReferences, contextFromTypes, maskFamily, withClassMembers } from "./lib/validate.mjs";
import {
	parseRules, rulesForClass, worldRules, ruleLabel, findMember, memberText, memberPath, isMapFile, readTable, keyPreview,
	parseMapObjects, parseWorld, worldContains, checkRules, checkWorld, objectNames, newFilePath, openCommands, MAP_FILTER,
} from "./lib/rules.mjs";
import { CHECK_SCRIPT_LOCAL, checkArguments, parseCheckOutput, summarize } from "./lib/check_output.mjs";

export const ACTIONS = {
	configure: "YatiConfigure",
	sync: "YatiSyncGodotProject",
	scenes: "YatiImportScenes",
	check: "YatiCheckMap",
	pick: "YatiPickMapObject",
	follow: "YatiFollowReference",
	openFile: "YatiOpenOrCreateFile",
	text: "YatiShowText",
};

export const ACTION_TEXTS = {
	configure: "YATI: Configure…",
	sync: "YATI: Sync Godot project…",
	scenes: "YATI: Import scenes as classes…",
	check: "YATI: Check map in Godot",
	pick: "YATI: Pick map and object…",
	follow: "YATI: Follow reference",
	openFile: "YATI: Open or create file…",
	text: "YATI: Show text for selected object",
};

const TITLE = "YATI";

// ---------------------------------------------------------------------------------------------
// Installation

// Registers the actions, the menu entries and the save hook. `api` holds Tiled's globals:
// {tiled, TextFile, File, Process, Dialog, ClassPropertyType, StorageType} (the last two may be null).
export function install(api) {
	const t = api.tiled;
	const handlers = {
		configure: function () { guarded(api, function () { configure(api); }); },
		sync: function () { guarded(api, function () { syncGodotProject(api); }); },
		scenes: function () { guarded(api, function () { importScenes(api); }); },
		check: function () { guarded(api, function () { checkMap(api); }); },
		pick: function () { guarded(api, function () { pickMapAndObject(api); }); },
		follow: function () { guarded(api, function () { followReference(api); }); },
		openFile: function () { guarded(api, function () { openOrCreateFile(api); }); },
		text: function () { guarded(api, function () { showText(api); }); },
	};
	const keys = ["configure", "sync", "scenes", "check", "pick", "follow", "openFile", "text"];
	const items = [{ separator: true }];
	for (let i = 0; i < keys.length; i++) {
		const action = t.registerAction(ACTIONS[keys[i]], handlers[keys[i]]);
		action.text = ACTION_TEXTS[keys[i]];
		items.push({ action: ACTIONS[keys[i]] });
	}
	// Without the editor (tiled --evaluate, where a test can run the extension) there is no Map menu
	// and extendMenu throws "Unknown menu"; the actions are registered all the same.
	try {
		t.extendMenu("Map", items);
	} catch (e) {
		t.log("YATI: the Map menu was not extended (" + (e && e.message ? e.message : String(e)) + "): no editor; the actions are registered.");
	}
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

// The settings, from the Tiled project's properties. Without a project (Tiled starts without one
// unless the session restores it, and Project > Close Project closes it) tiled.project is null:
// Tiled 1.12's ProjectModel::editableProject() returns nullptr when it holds no project document
// (the reference's "the properties of the project will be blank" is no longer so). The settings
// are then empty, settingsProblem() names the cause, and the save hook validates without types.
function currentProject(api) {
	const project = api.tiled.project;
	return project === null || project === undefined ? null : project;
}

export function currentSettings(api) {
	const project = currentProject(api);
	if (project === null)
		return readSettings(function () { return undefined; }, "");
	return readSettings(function (name) { return project.property(name); }, project.fileName);
}

function godotProjectExists(api, s) {
	return s.godotProject !== "" && api.File.exists(joinPath(s.godotProject, "project.godot"));
}

// The settings, or null after telling the user what is missing. Without a Godot project the
// user is offered the Configure dialog at once, and the action goes on with what it saved.
function requireSettings(api) {
	let s = currentSettings(api);
	let problem = settingsProblem(s, godotProjectExists(api, s));
	if (problem === "")
		return s;
	if (s.projectFile === "") {
		api.tiled.alert(problem, TITLE);
		return null;
	}
	if (!api.tiled.confirm(problem + "\n\nOpen " + ACTION_TEXTS.configure + " now?", TITLE))
		return null;
	if (!configure(api))
		return null;
	s = currentSettings(api);
	problem = settingsProblem(s, godotProjectExists(api, s));
	return problem === "" ? s : null;
}

// ---------------------------------------------------------------------------------------------
// YATI: Configure...

// The dialog, shown until it is saved or cancelled. Returns whether it was saved.
function configure(api) {
	const t = api.tiled;
	const s = currentSettings(api);
	if (s.projectFile === "") {
		t.alert(settingsProblem(s, false), TITLE);
		return false;
	}
	const d = configureDialog(api, s);
	d.dialog.exec();
	return d.saved();
}

// Builds the dialog without showing it: {dialog, widgets, save(), cancel(), saved()}, so that a
// test (and the headless harness, where exec() cannot return) can fill the widgets and press Save.
export function configureDialog(api, settings) {
	const t = api.tiled;
	const D = api.Dialog;
	const values = initialValues(settings);
	const dialog = new D(CONFIGURE_TITLE);
	dialog.newRowMode = D.ManualRows;
	const widgets = {};
	const state = { saved: false, tested: null, warnedFor: null };

	dialog.addHeading(TEXTS.heading, true);
	dialog.addNewRow();
	dialog.addLabel(TEXTS.intro(settings.projectFile));
	dialog.addNewRow();
	dialog.addSeparator();
	dialog.addNewRow();

	const io = {
		exists: function (p) { return api.File.exists(p); },
		godotVersion: function (binary) { return runGodotVersion(api, binary); },
	};

	for (let i = 0; i < FIELDS.length; i++) {
		const f = FIELDS[i];
		if (f.kind === "directory") {
			const w = dialog.addFilePicker(f.label, values[f.key], f.tooltip);
			w.isDirectory = true;
			widgets[f.key] = w;
		} else if (f.kind === "bool") {
			widgets[f.key] = dialog.addCheckBox(f.label, values[f.key], f.tooltip);
		} else {
			const w = dialog.addTextInput(f.label, values[f.key], f.tooltip);
			w.placeholderText = f.placeholder;
			widgets[f.key] = w;
		}
		if (f.key === "godotBinary") {
			const browse = dialog.addButton(TEXTS.browse, TEXTS.browseTip);
			browse.clicked.connect(function () {
				const chosen = t.promptOpenFile(settings.projectDir, "", CONFIGURE_TITLE + ": the Godot binary");
				if (chosen !== "")
					widgets.godotBinary.text = chosen;
			});
			const test = dialog.addButton(TEXTS.test, TEXTS.testTip);
			test.clicked.connect(function () {
				const text = widgets.godotBinary.text.trim();
				const binary = resolvedBinary(text, settings.projectDir);
				const status = godotVersionStatus(binary, io.godotVersion(binary));
				state.tested = { binary: text, result: status };
				widgets.binaryStatus.text = status.text;
			});
		}
		if (f.key === "importOptions") {
			const options = dialog.addButton(TEXTS.options, TEXTS.optionsTip);
			options.clicked.connect(function () {
				const o = optionsDialog(api, widgets.importOptions.text, function (text) { widgets.importOptions.text = text; });
				o.dialog.exec();
			});
		}
		dialog.addNewRow();
		if (f.key === "godotProject" || f.key === "godotBinary" || f.key === "importOptions") {
			widgets[f.key + "Status"] = dialog.addLabel("");
			dialog.addNewRow();
		}
	}
	widgets.binaryStatus = widgets.godotBinaryStatus;

	// Live feedback on the fields whose mistakes are cheap to see.
	const projectStatus = function () {
		const status = godotProjectStatus(widgets.godotProject.fileName, settings, io.exists);
		widgets.godotProjectStatus.text = status.text;
	};
	widgets.godotProject.fileUrlChanged.connect(projectStatus);
	projectStatus();
	const optionsStatus = function () {
		const v = validateOptionsOnly(widgets.importOptions.text);
		widgets.importOptionsStatus.text = v.join("\n");
	};
	widgets.importOptions.textChanged.connect(optionsStatus);
	optionsStatus();

	dialog.addSeparator();
	dialog.addNewRow();
	widgets.managed = dialog.addLabel(managedSummary(settings), TEXTS.managedTip);
	dialog.addNewRow();
	widgets.status = dialog.addLabel("");
	dialog.addNewRow();
	widgets.save = dialog.addButton(TEXTS.save, TEXTS.saveTip);
	widgets.cancel = dialog.addButton(TEXTS.cancel, TEXTS.cancelTip);

	const readValues = function () {
		return {
			godotProject: widgets.godotProject.fileName,
			godotBinary: widgets.godotBinary.text,
			scenesDir: widgets.scenesDir.fileName,
			importOptions: widgets.importOptions.text,
			validate: widgets.validate.checked,
		};
	};
	// The warnings the dialog opened with (the binary is not run then): saving the settings as they
	// were never asks twice; a warning the user's change brings asks for a second Save.
	const initialWarnings = validateConfiguration(readValues(), settings, { exists: io.exists, godotVersion: null }).warnings;
	const showStatus = function (lines, isError) {
		widgets.status.text = lines.join("\n");
		widgets.status.styleSheet = isError ? "color: #c0392b" : "color: #b9770e";
	};
	const save = function () {
		const v = readValues();
		// --version runs once per binary text (the Test button's run, or the first Save's); its
		// result stays under the field and its warning is repeated on every Save.
		const binaryText = v.godotBinary.trim();
		const check = {
			exists: io.exists,
			godotVersion: function (binary) {
				if (state.tested === null || state.tested.binary !== binaryText) {
					state.tested = { binary: binaryText, result: godotVersionStatus(binary, io.godotVersion(binary)) };
					widgets.binaryStatus.text = state.tested.result.text;
				}
				return null;
			},
		};
		const result = validateConfiguration(v, settings, check);
		if (state.tested !== null && state.tested.binary === binaryText && state.tested.result.warning !== "")
			result.warnings.unshift("Godot binary: " + state.tested.result.warning);
		if (result.errors.length > 0) {
			showStatus(result.errors, true);
			widgets.save.text = TEXTS.save;
			state.warnedFor = null;
			return false;
		}
		const key = JSON.stringify(v);
		const newWarnings = result.warnings.filter(function (w) { return initialWarnings.indexOf(w) < 0; });
		if (newWarnings.length > 0 && state.warnedFor !== key) {
			showStatus(result.warnings.concat([TEXTS.anyway]), false);
			widgets.save.text = TEXTS.saveAnyway;
			state.warnedFor = key;
			return false;
		}
		const project = t.project;
		const names = Object.keys(result.stored);
		for (let i = 0; i < names.length; i++)
			setIfChanged(project, names[i], result.stored[names[i]]);
		state.saved = true;
		const lines = configuredLines(result);
		t.log("YATI: " + TEXTS.saved);
		for (let i = 0; i < lines.length; i++)
			t.log("YATI:   " + lines[i]);
		dialog.accept();
		return true;
	};
	widgets.save.clicked.connect(save);
	widgets.cancel.clicked.connect(function () { dialog.reject(); });

	return {
		dialog: dialog,
		widgets: widgets,
		save: save,
		cancel: function () { dialog.reject(); },
		saved: function () { return state.saved; },
		readValues: readValues,
	};
}

// The import options as fields: a combo box per boolean (default, true, false) and per option
// with listed values, a text input for the free ones, each with its meaning as its tooltip. OK
// hands the name=value text to `apply` (the Configure dialog puts it into its field, so the
// stored form stays the string). Built without exec(), like configureDialog.
export function optionsDialog(api, text, apply) {
	const D = api.Dialog;
	const dialog = new D(OPTIONS_TITLE);
	dialog.newRowMode = D.ManualRows;
	const parsed = optionsToValues(text);
	const widgets = {};
	const choices = {};
	dialog.addHeading(TEXTS.optionsHeading, true);
	dialog.addNewRow();
	dialog.addLabel(TEXTS.optionsIntro);
	dialog.addNewRow();
	for (let i = 0; i < OPTION_FIELDS.length; i++) {
		const f = OPTION_FIELDS[i];
		const current = parsed.values[f.name];
		if (f.kind === "text") {
			const w = dialog.addTextInput(f.name, current, optionTooltip(f));
			w.placeholderText = TEXTS.importerDefault(f.def);
			widgets[f.name] = w;
		} else {
			const c = optionChoices(f, current);
			choices[f.name] = c.values;
			widgets[f.name] = dialog.addComboBox(f.name, c.labels, c.index, optionTooltip(f));
		}
		dialog.addNewRow();
	}
	if (parsed.extra.length > 0) {
		widgets.extra = dialog.addLabel(TEXTS.optionsKept + parsed.extra.join(" "));
		dialog.addNewRow();
	}
	const readValues = function () {
		const values = {};
		for (let i = 0; i < OPTION_FIELDS.length; i++) {
			const f = OPTION_FIELDS[i];
			values[f.name] = f.kind === "text" ? widgets[f.name].text : choices[f.name][widgets[f.name].currentIndex];
		}
		return values;
	};
	const ok = function () {
		apply(valuesToOptions(readValues(), parsed.extra));
		dialog.accept();
	};
	widgets.ok = dialog.addButton(TEXTS.ok, TEXTS.okTip);
	widgets.cancel = dialog.addButton(TEXTS.cancel, TEXTS.cancelTip);
	widgets.ok.clicked.connect(ok);
	widgets.cancel.clicked.connect(function () { dialog.reject(); });
	return { dialog: dialog, widgets: widgets, ok: ok, readValues: readValues, text: function () { return valuesToOptions(readValues(), parsed.extra); } };
}

// The import options field's own messages (the validator's, without the other fields).
function validateOptionsOnly(text) {
	const v = validateConfiguration({ godotProject: "", godotBinary: "", scenesDir: "", importOptions: text, validate: true },
		{ projectDir: "" }, { exists: function () { return false; } });
	const out = [];
	const all = v.errors.concat(v.warnings);
	for (let i = 0; i < all.length; i++) {
		if (all[i].indexOf("Import options: ") === 0)
			out.push(all[i].substring("Import options: ".length));
	}
	return out;
}

// `<binary> --version`: {code, output}, code -1 when it did not start.
function runGodotVersion(api, binary) {
	const p = new api.Process();
	try {
		const code = p.exec(binary === "" ? "godot" : binary, ["--version"], false);
		return { code: code, output: p.readStdOut() + "\n" + p.readStdErr() };
	} catch (e) {
		return { code: -1, output: "" };
	} finally {
		p.close();
	}
}

// Writes a property only when its value changes (a write saves the project): an unset property
// stays unset for "" and for true (the defaults), so a fresh project gains only what is set.
function setIfChanged(project, name, value) {
	const old = project.property(name);
	if (old === undefined) {
		if (value !== "" && value !== true)
			project.setProperty(name, value);
		return;
	}
	if (typeof value === "boolean" ? (old !== value && String(old) !== String(value)) : String(old) !== String(value))
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
	const kept = { flags: fixups.kept.flags, floats: fixups.kept.floats, written: settings.fixups.written };
	const keptText = stringifyFixups(kept);

	// What the API cannot set goes into the file Tiled has just saved, all of it every time: a
	// save of the project from memory (any change above) has dropped what an earlier run wrote.
	// The record of the fixes goes into the file with them, with the write counted, and not
	// through the API: the project in memory keeps the old record until Tiled reads the file, so
	// the counter tells reloadProject whether the reload happened.
	const text = readText(api, project.fileName);
	let reload = null;
	if (text !== null) {
		const next = { flags: kept.flags, floats: kept.floats, written: kept.written + 1 };
		const nextText = stringifyFixups(next);
		const properties = {};
		properties[SETTING_NAMES.fixups] = nextText;
		const patched = patchProjectText(text, fixups.patch, properties);
		if (patched.error)
			lines.push("Error: " + patched.error);
		if (patched.changes.length > 0) {
			writeText(api, project.fileName, patched.text);
			reload = { expect: nextText };
			lines.push("Set in the project file (the scripting API cannot): " + patched.changes.join("; "));
		} else if (keptText !== stringifyFixups(settings.fixups)) {
			project.setProperty(SETTING_NAMES.fixups, keptText);
		}
	}
	return { lines: lines, reload: reload };
}

const RELOAD_TEXT = "The project is reloaded now, so that Tiled reads it. Open maps are reopened; one with " +
	"unsaved changes asks to be saved first (Cancel there keeps everything as it is: then reopen the project " +
	"yourself later).";
const REOPEN_TEXT = "The project was not reloaded. Reopen it yourself (File > Open File or Project, or Project > " +
	"Recent Projects) so that Tiled reads what was written into the project file. Until then Tiled's next save " +
	"of the project (any change to custom types or project properties) overwrites it; running the action again " +
	"repairs it.";

function report(api, headline, lines, reload) {
	const t = api.tiled;
	t.log("YATI: " + headline);
	for (let i = 0; i < lines.length; i++)
		t.log("YATI:   " + lines[i]);
	let text = headline;
	if (lines.length > 0)
		text += "\n\n" + lines.join("\n");
	if (reload)
		text += "\n\n" + RELOAD_TEXT;
	t.alert(text, TITLE);
}

// Reopens the project from its file: tiled.open on a .tiled-project goes through Tiled's
// MainWindow::openFile, which loads the project from disk and switches to it (the open files are
// recorded in the session, closed, and reopened by the session restore; an unsaved one asks).
// Nothing in the reference says so (read in Tiled 1.12's scriptmodule.cpp, documentmanager.cpp
// and mainwindow.cpp), and without the editor (tiled --evaluate) open() throws, so the result is
// read back: the project's record equals what the file was written with only after a reload.
// The action's last statement: the project object it held is replaced by the reload.
function reloadProject(api, reload) {
	const t = api.tiled;
	const before = currentProject(api);
	if (before === null)
		return;
	const fileName = before.fileName;
	try {
		t.open(fileName);
	} catch (e) {
		t.log("YATI: tiled.open(" + fileName + ") failed: " + (e && e.message ? e.message : String(e)));
	}
	const after = currentProject(api);
	const record = after === null ? undefined : after.property(SETTING_NAMES.fixups);
	if (after !== null && record !== undefined && String(record) === reload.expect) {
		t.log("YATI: the project was reloaded from " + fileName + ".");
		return;
	}
	t.log("YATI: " + REOPEN_TEXT);
	t.alert(REOPEN_TEXT, TITLE);
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
	report(api, "Synced with the Godot project " + (info.name !== "" ? "'" + info.name + "' " : "") + "(" + s.godotProject + ").", lines, done.reload !== null);
	if (done.reload !== null)
		reloadProject(api, done.reload);
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
	report(api, "Imported " + built.classes.length + " scene(s) as classes.", lines, done.reload !== null);
	if (done.reload !== null)
		reloadProject(api, done.reload);
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
		findings = validateMap(model, ctx).concat(ruleFindings(api, s, model, ctx));
	} else {
		findings = validateTileset(tilesetModel(api, asset), ctx);
	}
	const fileName = asset.fileName;
	const label = fileName !== "" ? baseName(fileName) : "unsaved " + (asset.isTileMap ? "map" : "tileset");
	for (let i = 0; i < findings.length; i++) {
		const f = findings[i];
		const text = "YATI: " + label + ": " + f.text;
		const activate = asset.isTileMap ? selector(api, fileName, f.objectId, f.layerId) : function () {};
		if (f.severity === "error")
			api.tiled.error(text, activate);
		else
			api.tiled.warn(text, activate);
	}
	if (findings.length > 0)
		api.tiled.log("YATI: " + label + ": " + findings.length + " finding(s), see the Issues view.");
}

// The project's types as the validator reads them (none without a project).
function projectTypes(api) {
	const out = [];
	const project = currentProject(api);
	if (project === null)
		return out;
	const types = project.propertyTypes;
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
			const project = currentProject(api);
			const et = project === null ? null : project.findTypeByName(v.typeName);
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
// The project's rules (yati_rules): on save, and the actions on the selected object

// The rule findings for a map being saved: its objects' class rules, the rules of each world that
// holds the map, and the rules' own mistakes (as warnings without an object).
function ruleFindings(api, s, model, vctx) {
	const parsed = parseRules(s.rulesRaw);
	const out = [];
	for (let i = 0; i < parsed.problems.length; i++)
		out.push({ severity: "warning", code: "rule-config", text: parsed.problems[i], objectId: 0, layerId: 0 });
	if (parsed.rules.length === 0)
		return out;
	const ctx = ruleContext(api, s, vctx.classes, model);
	const found = checkRules(model, parsed.rules, ctx);
	for (let i = 0; i < found.length; i++)
		out.push(found[i]);
	const worlds = worldRules(parsed.rules, s.projectDir);
	for (let i = 0; i < worlds.length; i++) {
		const w = worlds[i];
		const text = readText(api, w.file);
		if (text === null) {
			out.push({ severity: "warning", code: "rule-unreadable", text: "yati_rules names the world " + w.file + ", which does not exist.", objectId: 0, layerId: 0 });
			continue;
		}
		const world = parseWorld(text, w.file);
		if (world.error !== "") {
			out.push({ severity: "warning", code: "rule-unreadable", text: "The world " + w.file + " could not be read (" + world.error + ").", objectId: 0, layerId: 0 });
			continue;
		}
		if (model.fileName !== "" && worldContains(world, w.file, model.fileName)) {
			const wf = checkWorld(w.file, world, w.rules, ctx);
			for (let k = 0; k < wf.length; k++)
				out.push(wf[k]);
		}
	}
	return out;
}

// What lib/rules.mjs reads through the glue: files, and a map's objects from the map being saved,
// a map open in Tiled (with its unsaved changes), or else its file as last saved.
function ruleContext(api, s, classes, savingModel) {
	return {
		projectFile: s.projectFile,
		godotProject: s.godotProject,
		classes: classes,
		fileExists: function (p) { return api.File.exists(p); },
		readText: function (p) { return readText(api, p); },
		mapObjects: function (p) { return mapObjectsOf(api, p, savingModel); },
	};
}

function mapObjectsOf(api, file, savingModel) {
	const target = cleanPath(file);
	if (savingModel && savingModel.fileName !== "" && cleanPath(savingModel.fileName) === target) {
		return { objects: savingModel.objects.map(function (o) {
			return { id: o.id, name: o.name, className: o.className !== "" ? o.className : (o.tileClassName || "") };
		}), error: "" };
	}
	const open = openMap(api, target);
	if (open !== null)
		return { objects: objectList(open.layers), error: "" };
	const text = readText(api, file);
	if (text === null)
		return { objects: [], error: "it does not exist" };
	return parseMapObjects(text, file, function (p) { return readText(api, p); });
}

// The open map with this file name, or null.
function openMap(api, fileName) {
	const open = api.tiled.openAssets;
	for (let i = 0; i < open.length; i++) {
		if (open[i].isTileMap && open[i].fileName !== "" && cleanPath(open[i].fileName) === cleanPath(fileName))
			return open[i];
	}
	return null;
}

// A map's objects as [{id, name, className, object}], the class the object's or its tile's.
function objectList(layers) {
	const out = [];
	for (let i = 0; i < layers.length; i++) {
		const l = layers[i];
		if (l.isObjectLayer) {
			const objects = l.objects;
			for (let k = 0; k < objects.length; k++) {
				const o = objects[k];
				const tile = o.tile;
				out.push({ id: o.id, name: o.name, className: o.className !== "" ? o.className : (tile ? tile.className : ""), object: o });
			}
		}
		if (l.isGroupLayer) {
			const inner = objectList(l.layers);
			for (let k = 0; k < inner.length; k++)
				out.push(inner[k]);
		}
	}
	return out;
}

// The selected object and what the rules need of it, or null after telling the user why not:
// {s, rules, map, object, cls, props, ctx}. Rule mistakes are logged and shown once with the result.
function selectedForRules(api, title) {
	const t = api.tiled;
	const s = currentSettings(api);
	if (s.projectFile === "") {
		t.alert(settingsProblem(s, false), TITLE);
		return null;
	}
	const parsed = parseRules(s.rulesRaw);
	for (let i = 0; i < parsed.problems.length; i++)
		t.log("YATI: " + parsed.problems[i]);
	if (parsed.rules.length === 0) {
		t.alert(title + ": the project has no rules. Set the project property " + SETTING_NAMES.rules + " (Project > Project Properties; " +
			"the README's \"Rules\" section has the format)." + (parsed.problems.length > 0 ? "\n\n" + parsed.problems.join("\n") : ""), TITLE);
		return null;
	}
	const map = t.activeAsset;
	if (map === null || map === undefined || !map.isTileMap) {
		t.alert(title + ": open a map and select one object first.", TITLE);
		return null;
	}
	const selected = map.selectedObjects;
	if (selected.length !== 1) {
		t.alert(title + ": select exactly one object (" + selected.length + " selected).", TITLE);
		return null;
	}
	const o = selected[0];
	const tile = o.tile;
	const cls = o.className !== "" ? o.className : (tile ? tile.className : "");
	const vctx = contextFromTypes(projectTypes(api), s.projectFile, s.godotProject, function (p) { return api.File.exists(p); });
	const props = withClassMembers(propsOf(api, o.resolvedProperties()), vctx.classes[cls]);
	return {
		s: s, rules: parsed.rules, problems: parsed.problems, map: map, object: o, cls: cls, props: props,
		ctx: ruleContext(api, s, vctx.classes, null),
		label: "Object " + (o.name !== "" ? "'" + o.name + "'" : "(unnamed)") + " (id " + o.id + (cls !== "" ? ", class " + cls : "") + ")",
	};
}

// One of several rules, asked with a list when there is more than one; null when cancelled.
function chooseRule(api, title, text, rules) {
	if (rules.length === 1)
		return rules[0];
	let chosen = -1;
	const d = choiceDialog(api, title, text, "Member", rules.map(ruleLabel), 0, function (i) { chosen = i; });
	d.dialog.exec();
	return chosen < 0 ? null : rules[chosen];
}

// A dialog with one list and OK / Cancel, built without exec() (as configureDialog): OK hands the
// chosen index to `apply`.
export function choiceDialog(api, title, text, label, items, index, apply) {
	const D = api.Dialog;
	const dialog = new D(title);
	dialog.newRowMode = D.ManualRows;
	dialog.addLabel(text);
	dialog.addNewRow();
	const combo = dialog.addComboBox(label, items, index, "");
	dialog.addNewRow();
	const ok = dialog.addButton(TEXTS.ok, "Use the chosen entry.");
	const cancel = dialog.addButton(TEXTS.cancel, TEXTS.cancelTip);
	const accept = function () {
		apply(combo.currentIndex);
		dialog.accept();
	};
	ok.clicked.connect(accept);
	cancel.clicked.connect(function () { dialog.reject(); });
	return { dialog: dialog, widgets: { combo: combo, ok: ok, cancel: cancel }, ok: accept };
}

// The member's file as a local path, from the object's own value (relative to its map) or its
// class's default (relative to the project file).
function selectedPath(sel, member) {
	return memberPath(findMember(sel.props, member), sel.map.fileName, sel.ctx);
}

// YATI: Pick map and object... For an object_in_map rule with a map member: a map from a file
// dialog, then one of its objects of the rule's class from a list; both members set in one step.
function pickMapAndObject(api) {
	const t = api.tiled;
	const title = ACTION_TEXTS.pick;
	const sel = selectedForRules(api, title);
	if (sel === null)
		return;
	const pairs = rulesForClass(sel.rules, sel.cls, ["object_in_map"]).filter(function (r) { return r.map !== ""; });
	if (pairs.length === 0) {
		t.alert(title + ": no rule gives " + (sel.cls !== "" ? "class '" + sel.cls + "'" : "an object without a class") +
			" a map member and an object member (kind object_in_map with \"map\" in " + SETTING_NAMES.rules + ").", TITLE);
		return;
	}
	const rule = chooseRule(api, title, sel.label + ": which reference?", pairs);
	if (rule === null)
		return;
	const current = selectedPath(sel, rule.map);
	const startDir = current !== "" && api.File.exists(dirName(current)) ? dirName(current) : dirName(sel.map.fileName);
	const file = t.promptOpenFile(startDir, MAP_FILTER, title + " " + rule.map);
	if (file === "")
		return;
	if (!isMapFile(file)) {
		t.alert(title + ": " + file + " is no Tiled map (.tmx or .tmj).", TITLE);
		return;
	}
	const read = mapObjectsOf(api, file, null);
	if (read.error !== "") {
		t.alert(title + ": " + file + " could not be read (" + read.error + ").", TITLE);
		return;
	}
	const names = objectNames(read.objects, rule.targetClass);
	if (names.length === 0) {
		t.alert(title + ": " + baseName(file) + " has no named object" + (rule.targetClass !== "" ? " of class " + rule.targetClass : "") +
			". Give the object a name there first.", TITLE);
		return;
	}
	const currentName = memberText(findMember(sel.props, rule.member));
	let chosen = -1;
	const d = choiceDialog(api, title, sel.label + ": " + rule.member + " in " + baseName(file) +
		(rule.targetClass !== "" ? " (objects of class " + rule.targetClass + ")" : ""), "Object", names,
		Math.max(0, names.indexOf(currentName)), function (i) { chosen = i; });
	d.dialog.exec();
	if (chosen < 0)
		return;
	setMapAndObject(api, sel.map, sel.object, rule.map, file, rule.member, names[chosen]);
	t.log("YATI: " + sel.label + ": " + rule.map + " = " + file + ", " + rule.member + " = " + names[chosen] + ".");
}

// An object's map member (a file property) and object member (a string) set as one undo step.
export function setMapAndObject(api, map, object, mapMember, file, objectMember, name) {
	map.macro("YATI: Set " + mapMember + " and " + objectMember, function () {
		object.setProperty(mapMember, api.tiled.filePath(fileUrl(file)));
		object.setProperty(objectMember, name);
	});
}

// YATI: Follow reference. The map an object's object_in_map rule names (its map member, or the
// same map) opened, and the named object selected and centred; a map rule's map opened.
function followReference(api) {
	const t = api.tiled;
	const title = ACTION_TEXTS.follow;
	const sel = selectedForRules(api, title);
	if (sel === null)
		return;
	const withValue = function (r) { return memberText(findMember(sel.props, r.member)) !== ""; };
	const objectRules = rulesForClass(sel.rules, sel.cls, ["object_in_map"]);
	const mapRules = rulesForClass(sel.rules, sel.cls, ["map"]);
	if (objectRules.length + mapRules.length === 0) {
		t.alert(title + ": no rule of kind object_in_map or map applies to " + (sel.cls !== "" ? "class '" + sel.cls + "'" : "an object without a class") + ".", TITLE);
		return;
	}
	let candidates = objectRules.filter(withValue);
	if (candidates.length === 0)
		candidates = mapRules.filter(withValue);
	if (candidates.length === 0) {
		t.alert(title + ": " + sel.label + " has no value in " + objectRules.concat(mapRules).map(function (r) { return r.member; }).join(", ") + ".", TITLE);
		return;
	}
	const rule = chooseRule(api, title, sel.label + ": follow which reference?", candidates);
	if (rule === null)
		return;
	let file = sel.map.fileName;
	const mapMember = rule.kind === "map" ? rule.member : rule.map;
	if (mapMember !== "") {
		file = selectedPath(sel, mapMember);
		if (file === "") {
			t.alert(title + ": " + sel.label + " has no " + mapMember + ".", TITLE);
			return;
		}
		if (!api.File.exists(file)) {
			t.alert(title + ": " + mapMember + " names " + file + ", which does not exist.", TITLE);
			return;
		}
	}
	let target = cleanPath(file) === cleanPath(sel.map.fileName) ? sel.map : openMap(api, file);
	if (target === null)
		target = t.open(file);
	if (target === null || target === undefined || !target.isTileMap) {
		t.alert(title + ": " + file + " could not be opened as a map.", TITLE);
		return;
	}
	t.activeAsset = target;
	if (rule.kind === "map")
		return;
	const name = memberText(findMember(sel.props, rule.member));
	const objects = objectList(target.layers).filter(function (o) {
		return o.name === name && (rule.targetClass === "" || o.className === rule.targetClass);
	});
	if (objects.length === 0) {
		t.alert(title + ": " + baseName(file) + " has no object named '" + name + "'" +
			(rule.targetClass !== "" ? " of class " + rule.targetClass : "") + ".", TITLE);
		return;
	}
	const o = objects[0].object;
	target.selectedObjects = [o];
	const view = t.mapEditor.currentMapView;
	if (view) {
		const p = target.pixelToScreen(o.x, o.y);
		view.centerOn(p.x, p.y);
	}
	if (objects.length > 1)
		t.alert(title + ": " + baseName(file) + " has " + objects.length + " objects named '" + name + "'; the first is selected.", TITLE);
}

// YATI: Open or create file... A file rule's file opened with the system's program for it; when
// the member is unset (or names a file that is not there) and the rule has a template, a copy of
// the template is made first (and an unset member set to it, in one undo step).
function openOrCreateFile(api) {
	const t = api.tiled;
	const title = ACTION_TEXTS.openFile;
	const sel = selectedForRules(api, title);
	if (sel === null)
		return;
	const fileRules = rulesForClass(sel.rules, sel.cls, ["file"]);
	if (fileRules.length === 0) {
		t.alert(title + ": no rule of kind file applies to " + (sel.cls !== "" ? "class '" + sel.cls + "'" : "an object without a class") + ".", TITLE);
		return;
	}
	const rule = chooseRule(api, title, sel.label + ": which file?", fileRules);
	if (rule === null)
		return;
	let file = selectedPath(sel, rule.member);
	const unset = memberText(findMember(sel.props, rule.member)) === "";
	if (!unset && file === "") {
		t.alert(title + ": " + rule.member + " (" + memberText(findMember(sel.props, rule.member)) + ") cannot be resolved to a file " +
			"(a res:// path needs the Godot project set in " + ACTION_TEXTS.configure + ").", TITLE);
		return;
	}
	if (unset || !api.File.exists(file)) {
		if (rule.template === "") {
			t.alert(title + ": " + (unset ? rule.member + " is not set" : rule.member + " names " + file + ", which does not exist") +
				", and its rule has no template to make it from. Set the member in the Properties view.", TITLE);
			return;
		}
		const template = joinPath(sel.s.projectDir, rule.template);
		const text = readText(api, template);
		if (text === null) {
			t.alert(title + ": the rule's template " + template + " does not exist.", TITLE);
			return;
		}
		if (unset)
			file = newFilePath(rule, { id: sel.object.id, name: sel.object.name, className: sel.cls }, sel.map.fileName, sel.s.projectDir);
		if (!api.File.exists(dirName(file))) {
			t.alert(title + ": the folder " + dirName(file) + " does not exist. Make it, or change the rule's \"folder\".", TITLE);
			return;
		}
		if (api.File.exists(file)) {
			if (!t.confirm(file + " exists already. Set " + rule.member + " of " + sel.label + " to it?", TITLE))
				return;
		} else {
			if (!t.confirm("Make " + file + " from the template " + baseName(template) +
				(unset ? " and set " + rule.member + " of " + sel.label + " to it?" : "?"), TITLE))
				return;
			writeText(api, file, text);
			t.log("YATI: made " + file + " from " + template + ".");
		}
		if (unset) {
			const o = sel.object;
			sel.map.macro("YATI: Set " + rule.member, function () { o.setProperty(rule.member, t.filePath(fileUrl(file))); });
		}
	}
	openExternally(api, sel.s, file);
}

// Opens a file with the program the system (or yati_open_command) gives it. Process.exec waits for
// the command, so it must be a launcher that returns at once (xdg-open, open, cmd's start, code).
function openExternally(api, s, file) {
	const t = api.tiled;
	const commands = openCommands(s.openCommand, file);
	const tried = [];
	for (let i = 0; i < commands.length; i++) {
		const c = commands[i];
		const p = new api.Process();
		let code;
		try {
			code = p.exec(c.program, c.args, false);
		} finally {
			p.close();
		}
		tried.push(c.program);
		if (code === -1)
			continue;
		if (code !== 0) {
			t.alert("Could not open " + file + ": '" + c.program + "' exited with " + code + ". Set the project property " +
				SETTING_NAMES.openCommand + " to the editor's command (for example code).", TITLE);
			return false;
		}
		t.log("YATI: opened " + file + " with " + c.program + ".");
		return true;
	}
	t.alert("Could not open " + file + ": " + tried.join(", ") + " did not start. Set the project property " +
		SETTING_NAMES.openCommand + " to the editor's command (for example code).", TITLE);
	return false;
}

// YATI: Show text for selected object. Each key rule's member: plain text, or the table's text
// for the key in every language, or that the table lacks the key.
function showText(api) {
	const t = api.tiled;
	const title = ACTION_TEXTS.text;
	const sel = selectedForRules(api, title);
	if (sel === null)
		return;
	const keyRules = rulesForClass(sel.rules, sel.cls, ["key"]);
	if (keyRules.length === 0) {
		t.alert(title + ": no rule of kind key applies to " + (sel.cls !== "" ? "class '" + sel.cls + "'" : "an object without a class") + ".", TITLE);
		return;
	}
	const lines = [sel.label + ":"];
	for (let i = 0; i < keyRules.length; i++) {
		const r = keyRules[i];
		const tablePath = joinPath(sel.s.projectDir, r.table);
		const text = readText(api, tablePath);
		const preview = keyPreview(r, memberText(findMember(sel.props, r.member)), text === null ? null : readTable(text), baseName(tablePath));
		for (let k = 0; k < preview.lines.length; k++)
			lines.push(preview.lines[k]);
	}
	for (let i = 0; i < lines.length; i++)
		t.log("YATI: " + lines[i]);
	t.alert(lines.join("\n"), TITLE);
}

// ---------------------------------------------------------------------------------------------
// In Tiled: install with Tiled's globals. (Under Node, the tests call install() with a fake.)

/* global tiled, TextFile, File, Process, Dialog, ClassPropertyType, StorageType */
if (typeof tiled !== "undefined" && typeof tiled.registerAction === "function") {
	install({
		tiled: tiled,
		TextFile: TextFile,
		File: File,
		Process: Process,
		Dialog: typeof Dialog !== "undefined" ? Dialog : null,
		ClassPropertyType: typeof ClassPropertyType !== "undefined" ? ClassPropertyType : null,
		StorageType: typeof StorageType !== "undefined" ? StorageType : null,
	});
}
