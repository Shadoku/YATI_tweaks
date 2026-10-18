// Runs the extension inside a real Tiled, headless: `tiled [--project <file>] --evaluate
// real_tiled_harness.mjs <step>...` (real_tiled.test.mjs does, through tools/install_tiled.sh's
// Tiled and xvfb-run). Under --evaluate there is no editor: no menus, no open documents, no
// dialogs to answer. So this imports the extension as a module and installs it a second time
// through install(api) with Tiled's own globals, except that the dialogs are stubbed (every alert
// and confirmation is logged, a confirmation is "yes", a text prompt answers "": keep) and the actions
// are kept in a table instead of registered again. The extension's own installation (the module's
// last lines) runs too, against the real globals: it must not throw.
//
// Steps: configure, sync, scenes, check (the actions), and save:<map or tileset file> (the save
// hook on that file, read through Tiled's own format: the real API values of its properties).
// Output: tiled.log lines, which Tiled prints to the console; the test reads the ones that start
// with "HARNESS ", "ALERT ", "CONFIRM ", "PROMPT ", "ISSUE " and "YATI".
//
// Under Node (node --test runs every file under test/) this file does nothing.
import { install, ACTIONS } from "../../yati-godot/yati-godot.mjs";

/* global tiled, TextFile, File, Process, ClassPropertyType, StorageType */
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
	install({
		tiled: t, TextFile: TextFile, File: File, Process: Process,
		ClassPropertyType: typeof ClassPropertyType !== "undefined" ? ClassPropertyType : null,
		StorageType: typeof StorageType !== "undefined" ? StorageType : null,
	});
	tiled.log("HARNESS installed: Tiled " + tiled.version + ", project " + (tiled.project === null ? "null" : "'" + tiled.project.fileName + "'") +
		", actions " + Object.keys(handlers).join(",") + ", save hooks " + saveHooks.length +
		", globals ClassPropertyType=" + typeof ClassPropertyType + " StorageType=" + typeof StorageType);
	const steps = tiled.scriptArguments;
	for (let i = 0; i < steps.length; i++) {
		const step = steps[i];
		if (step.indexOf("save:") === 0) {
			const file = step.substring(5);
			const asset = /\.tsx$/i.test(file) ? tiled.tilesetFormat("tsx").read(file) : tiled.mapFormat("tmx").read(file);
			tiled.log("HARNESS save hook on " + file + (asset ? "" : " (not read)"));
			for (let k = 0; k < saveHooks.length; k++)
				saveHooks[k](asset);
		} else if (handlers[ACTIONS[step]] !== undefined) {
			tiled.log("HARNESS action " + step);
			handlers[ACTIONS[step]]();
		} else {
			tiled.log("HARNESS unknown step '" + step + "'");
		}
	}
	tiled.log("HARNESS done: " + alerts + " alert(s)");
}
