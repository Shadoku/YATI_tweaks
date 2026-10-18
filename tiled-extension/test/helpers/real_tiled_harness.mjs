// Runs the extension inside a real Tiled, headless: `tiled [--project <file>] --evaluate
// real_tiled_harness.mjs <step>...` (real_tiled.test.mjs does, through tools/install_tiled.sh's
// Tiled and xvfb-run). Under --evaluate there is no editor: no menus, no open documents, no
// dialogs to answer. So this imports the extension as a module and installs it a second time
// through install(api) with Tiled's own globals, except that the dialogs are stubbed (every alert
// and confirmation is logged, a confirmation is "yes", a text prompt answers "": keep) and the actions
// are kept in a table instead of registered again. The extension's own installation (the module's
// last lines) runs too, against the real globals: it must not throw.
//
// Steps: sync, scenes, check (the actions), configure (the Configure dialog built with the real
// Dialog, its widgets read and logged, Save pressed through QAbstractButton::click(); exec() would
// block, so it is never called) and configure:<options> (the same, with the import options set
// and the validation switched off before Save), and save:<map or tileset file> (the save hook on
// that file, read through Tiled's own format: the real API values of its properties), and
// options:<text> (the Import options dialog built from that text with the real combo boxes, two
// of them changed, OK pressed, the text it hands back logged).
// Output: tiled.log lines, which Tiled prints to the console; the test reads the ones that start
// with "HARNESS ", "ALERT ", "CONFIRM ", "PROMPT ", "ISSUE " and "YATI".
//
// Under Node (node --test runs every file under test/) this file does nothing.
import { install, ACTIONS, configureDialog, optionsDialog, currentSettings } from "../../yati-godot/yati-godot.mjs";

/* global tiled, TextFile, File, Process, Dialog, ClassPropertyType, StorageType */
if (typeof tiled !== "undefined" && typeof tiled.registerAction === "function")
	main();

function main() {
	const handlers = {};
	const saveHooks = [];
	let alerts = 0;
	const oneLine = function (text) { return String(text).replace(/\n+/g, " | "); };
	const t = {
		get project() { return tiled.project; },
		get activeAsset() { return tiled.activeAsset; },
		set activeAsset(a) { tiled.activeAsset = a; },
		get openAssets() { return tiled.openAssets; },
		get mapEditor() { return tiled.mapEditor; },
		get assetAboutToBeSaved() { return { connect: function (cb) { saveHooks.push(cb); } }; },
		registerAction: function (id, cb) { handlers[id] = cb; return { text: "" }; },
		extendMenu: function () {},
		alert: function (text, title) { alerts++; tiled.log("ALERT [" + title + "]: " + oneLine(text)); },
		confirm: function (text) { tiled.log("CONFIRM (yes): " + oneLine(text)); return true; },
		prompt: function (label, text) { tiled.log("PROMPT (empty: keep; default '" + text + "'): " + oneLine(label)); return ""; },
		promptDirectory: function (dir, title) { tiled.log("PROMPT DIRECTORY (" + dir + "): " + oneLine(title)); return dir; },
		log: function (text) { tiled.log(text); },
		warn: function (text, activated) { tiled.log("ISSUE warning: " + text); tiled.warn(text, activated); },
		error: function (text, activated) { tiled.log("ISSUE error: " + text); tiled.error(text, activated); },
		color: function (s) { return tiled.color(s); },
		filePath: function (s) { return tiled.filePath(s); },
		objectRef: function (id) { return tiled.objectRef(id); },
		propertyValue: function (name, value) { return tiled.propertyValue(name, value); },
		open: function (f) { return tiled.open(f); },
	};
	const api = {
		tiled: t, TextFile: TextFile, File: File, Process: Process,
		Dialog: typeof Dialog !== "undefined" ? Dialog : null,
		ClassPropertyType: typeof ClassPropertyType !== "undefined" ? ClassPropertyType : null,
		StorageType: typeof StorageType !== "undefined" ? StorageType : null,
	};
	install(api);
	tiled.log("HARNESS installed: Tiled " + tiled.version + ", project " + (tiled.project === null ? "null" : "'" + tiled.project.fileName + "'") +
		", actions " + Object.keys(handlers).join(",") + ", save hooks " + saveHooks.length +
		", globals ClassPropertyType=" + typeof ClassPropertyType + " StorageType=" + typeof StorageType + " Dialog=" + typeof Dialog);
	const steps = tiled.scriptArguments;
	for (let i = 0; i < steps.length; i++) {
		const step = steps[i];
		if (step.indexOf("save:") === 0) {
			const file = step.substring(5);
			const asset = /\.tsx$/i.test(file) ? tiled.tilesetFormat("tsx").read(file) : tiled.mapFormat("tmx").read(file);
			tiled.log("HARNESS save hook on " + file + (asset ? "" : " (not read)"));
			for (let k = 0; k < saveHooks.length; k++)
				saveHooks[k](asset);
		} else if (step.indexOf("options:") === 0) {
			optionsStep(api, step.substring(8));
		} else if ((step === "configure" || step.indexOf("configure:") === 0) && tiled.project !== null) {
			configureStep(api, step.indexOf("configure:") === 0 ? step.substring(10) : null);
		} else if (handlers[ACTIONS[step]] !== undefined) {
			tiled.log("HARNESS action " + step);
			handlers[ACTIONS[step]]();
		} else {
			tiled.log("HARNESS unknown step '" + step + "'");
		}
	}
	tiled.log("HARNESS done: " + alerts + " alert(s)");
}

// The Configure dialog without exec(): what its widgets hold (the real FileEdit.fileName,
// QLineEdit.text, QCheckBox.checked, the tooltips), then Save pressed; a second press when the
// first only warned (godot is not on PATH here, so the --version run says so).
function configureStep(api, options) {
	const s = currentSettings(api);
	const d = configureDialog(api, s);
	const w = d.widgets;
	tiled.log("HARNESS dialog '" + d.dialog.windowTitle + "': project '" + w.godotProject.fileName + "' (" + w.godotProjectStatus.text +
		"), binary '" + w.godotBinary.text + "' (placeholder '" + w.godotBinary.placeholderText + "'), scenes '" + w.scenesDir.fileName +
		"', options '" + w.importOptions.text + "', validate " + w.validate.checked + ", tooltips " +
		[w.godotProject, w.godotBinary, w.scenesDir, w.importOptions, w.validate, w.save].map(function (x) { return x.toolTip.length > 20; }).join(","));
	if (options !== null) {
		w.importOptions.text = options;
		w.validate.checked = false;
		tiled.log("HARNESS dialog options status: '" + w.importOptionsStatus.text + "'");
	}
	w.save.click();
	tiled.log("HARNESS dialog after Save: saved " + d.saved() + ", button '" + w.save.text + "', status '" + w.status.text.replace(/\n/g, " | ") + "'");
	if (!d.saved()) {
		w.save.click();
		tiled.log("HARNESS dialog after Save anyway: saved " + d.saved() + ", status '" + w.status.text.replace(/\n/g, " | ") + "'");
	}
}

// The Import options dialog without exec(): the combo boxes' indices from the text, two changed,
// OK pressed, and what it hands back.
function optionsStep(api, text) {
	let applied = null;
	const o = optionsDialog(api, text, function (t) { applied = t; });
	const w = o.widgets;
	tiled.log("HARNESS options '" + o.dialog.windowTitle + "': warn_unknown_classes " + w.warn_unknown_classes.currentIndex +
		", object_refs " + w.object_refs.currentIndex + ", add_id_as_metadata " + w.add_id_as_metadata.currentIndex +
		", custom_data_prefix '" + w.custom_data_prefix.text + "' (placeholder '" + w.custom_data_prefix.placeholderText + "')" +
		", tooltip " + (w.object_refs.toolTip.indexOf("object_refs\n\n") === 0));
	w.add_id_as_metadata.currentIndex = 1;
	w.object_refs.currentIndex = 3;
	w.custom_data_prefix.text = "d_";
	w.ok.click();
	tiled.log("HARNESS options applied: '" + applied + "'");
}
