// YATI: Configure... as data and rules: the dialog's fields with their labels, tooltips and
// placeholders, the values the dialog starts from, the validation of what the user entered, and
// the properties to store. The glue (yati-godot.mjs) builds Tiled's Dialog from FIELDS and calls
// validateConfiguration on Save; nothing here touches Tiled's globals, so the tests run it under
// Node and the glue has no prose of its own.
//
// Written for Qt's JavaScript engine (ECMAScript 2016) as well as Node.

import {
	SETTING_NAMES, DEFAULT_GODOT_BINARY, parseImportOptions, describeImportOptions, storedPath, localToGodot,
	godotToLocal, joinPath, cleanPath, isAbsolutePath, baseName,
} from "./settings.mjs";

export const TITLE = "YATI: Configure";

export const TEXTS = {
	heading: "The Godot side of this Tiled project",
	intro: function (projectFile) {
		return "These settings are custom properties of the project file (" + baseName(projectFile) + "): saved with it,\n" +
			"shared with everyone who opens it, and editable in Project > Project Properties as well.\n" +
			"Paths are stored relative to the project file when both are on the same drive, so the project\n" +
			"can move with the game. Hover a field for what it does.";
	},
	browse: "Browse…",
	browseTip: "Choose the Godot binary with a file dialog.",
	test: "Test",
	testTip: "Run the binary with --version to see whether it starts, and which Godot it is.",
	save: "Save",
	saveTip: "Check the settings and save them into the project. Nothing is saved while a field is wrong.",
	saveAnyway: "Save anyway",
	cancel: "Cancel",
	cancelTip: "Close without changing anything.",
	anyway: "Press Save anyway to keep these settings regardless.",
	saved: "Configured.",
	unset: "(not set)",
	wholeProject: "(the whole Godot project)",
	managedTip: "yati_generated records which types and members the extension made, so that Sync Godot project and\n" +
		"Import scenes as classes update and remove only those and keep what you made by hand. yati_fixups\n" +
		"records what they wrote into the project file past the scripting API. Both are the extension's\n" +
		"records, not settings: leave them alone.",
};

// The dialog's fields, top to bottom. kind: "directory" (a folder picker), "text", "bool".
// `key` is the name in the values object the glue reads from the widgets and the tests build.
export const FIELDS = [
	{
		key: "godotProject", property: SETTING_NAMES.godotProject, kind: "directory",
		label: "Godot project",
		placeholder: "the folder that holds project.godot",
		tooltip: "The folder that holds project.godot. Required by every action.\n\n" +
			"Sync Godot project reads the layer names from its project.godot, Import scenes as classes scans it for\n" +
			"scenes, Check map in Godot runs the import inside it, and the validation on save resolves res:// paths\n" +
			"against it.\n\n" +
			"Saved as " + SETTING_NAMES.godotProject + ".",
	},
	{
		key: "godotBinary", property: SETTING_NAMES.godotBinary, kind: "text",
		label: "Godot binary",
		placeholder: DEFAULT_GODOT_BINARY,
		tooltip: "The Godot 4 binary that Check map in Godot runs: a full path, or a command found on PATH\n" +
			"(empty: '" + DEFAULT_GODOT_BINARY + "'). A relative path is relative to the Tiled project file.\n\n" +
			"Only Check map in Godot uses it; the other actions work without Godot installed.\n\n" +
			"Saved as " + SETTING_NAMES.godotBinary + ".",
	},
	{
		key: "scenesDir", property: SETTING_NAMES.scenesDir, kind: "directory",
		label: "Scenes folder",
		placeholder: "(the whole Godot project)",
		tooltip: "Where Import scenes as classes looks for .tscn files whose root node has a script.\n" +
			"Empty: the whole Godot project, skipping its addons/. Point into addons/ to import an addon's scenes.\n\n" +
			"Saved as " + SETTING_NAMES.scenesDir + ", as res://... when the folder is inside the Godot project.",
	},
	{
		key: "importOptions", property: SETTING_NAMES.importOptions, kind: "text",
		label: "Import options",
		placeholder: "name=value name=value …  (empty: the importer's defaults)",
		tooltip: "Options for the import that Check map in Godot runs, as name=value pairs separated by spaces,\n" +
			"for example: warn_unknown_classes=false assign_node_properties=false\n" +
			"Give the same values the game gives the importer, so that the check sees what the game will.\n\n" +
			"The options (Runtime.md has each one's meaning):\n  " + describeImportOptions().join("\n  ") + "\n\n" +
			"Saved as " + SETTING_NAMES.importOptions + ".",
	},
	{
		key: "validate", property: SETTING_NAMES.validate, kind: "bool",
		label: "Validate maps and tilesets on save",
		placeholder: "",
		tooltip: "When a map or tileset is saved, check it for what the importers would only report at run time:\n" +
			"a class that is no Godot node type, an instance without res_path, a file that does not exist, an\n" +
			"object reference to an id the map does not have, a layer mask naming a layer the Godot project does\n" +
			"not have, a template file that is missing.\n\n" +
			"Each finding is an entry in the Issues view (View > Views and Toolbars > Issues); double-click one\n" +
			"to select its object. A map without findings costs nothing.\n\n" +
			"Saved as " + SETTING_NAMES.validate + ".",
	},
];

export function fieldByKey(key) {
	for (let i = 0; i < FIELDS.length; i++) {
		if (FIELDS[i].key === key)
			return FIELDS[i];
	}
	return null;
}

// What the widgets start with, from readSettings' result: resolved folders (so a picker shows the
// folder itself), the binary and the options as written, an empty scenes folder for "the whole
// project".
export function initialValues(settings) {
	return {
		godotProject: settings.godotProject,
		godotBinary: settings.godotBinaryRaw,
		scenesDir: settings.scenesDirRaw === "" ? "" : settings.scenesDir,
		importOptions: settings.importOptionsRaw,
		validate: settings.validate,
	};
}

// A line about the extension's records, for the dialog.
export function managedSummary(settings) {
	const types = Object.keys(settings.generated);
	let members = 0;
	for (let i = 0; i < types.length; i++)
		members += settings.generated[types[i]].length;
	const floats = Object.keys(settings.fixups.floats);
	let fixes = settings.fixups.flags.length;
	for (let i = 0; i < floats.length; i++)
		fixes += settings.fixups.floats[floats[i]].length;
	if (types.length === 0 && fixes === 0)
		return "No types made by the extension yet: run Sync Godot project and Import scenes as classes.";
	return "Made by the extension: " + types.length + " type(s) with " + members + " member(s) (" + SETTING_NAMES.generated +
		"), " + fixes + " project-file fix(es) (" + SETTING_NAMES.fixups + ").";
}

// ---------------------------------------------------------------------------------------------
// The Godot binary's --version

// Godot prints its version as the first line of --version: "4.7.1.stable.official.abcdef12".
// result: {code, output} from running it (code -1: it did not start), or null when it was not run.
export function godotVersionStatus(binary, result) {
	if (result === null || result === undefined)
		return { ok: false, text: "", warning: "" };
	const name = binary === "" ? DEFAULT_GODOT_BINARY : binary;
	if (result.code === -1) {
		return {
			ok: false, text: "Could not start '" + name + "'.",
			warning: "Could not start '" + name + "': Check map in Godot will not work until the Godot binary is a " +
				"full path or a command on PATH.",
		};
	}
	const lines = String(result.output || "").split(/\r?\n/);
	let version = "";
	for (let i = 0; i < lines.length && version === ""; i++) {
		const m = /^(\d+)\.(\d+)[\w.-]*/.exec(lines[i].trim());
		if (m !== null)
			version = m[0];
	}
	if (version === "") {
		const last = lastNonEmpty(lines);
		return {
			ok: false, text: "'" + name + "' started, but printed no Godot version" + (last === "" ? "." : ": " + last),
			warning: "'" + name + "' does not look like Godot (no version in its --version output" + (last === "" ? ")." : ": " + last + ")."),
		};
	}
	if (version.indexOf("4.") !== 0) {
		return {
			ok: false, text: "Godot " + version + ": YATI needs Godot 4.",
			warning: "'" + name + "' is Godot " + version + "; Check map in Godot needs Godot 4.",
		};
	}
	return { ok: true, text: "Godot " + version, warning: "" };
}

function lastNonEmpty(lines) {
	for (let i = lines.length - 1; i >= 0; i--) {
		if (lines[i].trim() !== "")
			return lines[i].trim();
	}
	return "";
}

// ---------------------------------------------------------------------------------------------
// Validation

// Resolves a folder the user typed or picked: against the Tiled project file's folder (as the
// settings are read), "" stays "".
function resolveAgainst(baseDir, p) {
	const s = String(p === undefined || p === null ? "" : p).trim();
	if (s === "")
		return "";
	return isAbsolutePath(s) ? cleanPath(s) : joinPath(baseDir, s);
}

// What the Godot project field says, for its status line: {ok, text, dir}.
export function godotProjectStatus(value, settings, exists) {
	const dir = resolveAgainst(settings.projectDir, value);
	if (dir === "")
		return { ok: false, text: "Choose the folder that holds project.godot.", dir: "" };
	if (!exists(dir))
		return { ok: false, text: "'" + dir + "' does not exist.", dir: dir };
	if (!exists(joinPath(dir, "project.godot")))
		return { ok: false, text: "'" + dir + "' holds no project.godot. Choose the Godot project's own folder.", dir: dir };
	return { ok: true, text: "project.godot found.", dir: dir };
}

// The values as the widgets hold them, checked. settings: readSettings' result (for projectDir).
// io: {exists(path), godotVersion(binary) -> {code, output} | null (not run: no check)}.
// Returns {errors, warnings, stored: {property: value} (only when there is no error), resolved}.
// Errors keep the dialog open; warnings are shown and the user may save regardless.
export function validateConfiguration(values, settings, io) {
	const errors = [];
	const warnings = [];
	const projectDir = settings.projectDir;

	// The Godot project.
	const project = godotProjectStatus(values.godotProject, settings, io.exists);
	if (!project.ok)
		errors.push("Godot project: " + project.text);
	const godotDir = project.dir;

	// The binary: a path is checked for existence; a command is left to PATH (and the --version run).
	const binaryText = String(values.godotBinary === undefined || values.godotBinary === null ? "" : values.godotBinary).trim();
	let binary = binaryText === "" ? DEFAULT_GODOT_BINARY : binaryText;
	let binaryMissing = false;
	if (binary.indexOf("/") >= 0 || binary.indexOf("\\") >= 0) {
		binary = resolveAgainst(projectDir, binary);
		binaryMissing = !io.exists(binary);
		if (binaryMissing)
			errors.push("Godot binary: '" + binary + "' does not exist.");
	}
	if (!binaryMissing) {
		const status = godotVersionStatus(binary, io.godotVersion ? io.godotVersion(binary) : null);
		if (status.warning !== "")
			warnings.push("Godot binary: " + status.warning);
	}

	// The scenes folder: a res:// path or a path relative to the Godot project, like the setting.
	const scenesText = String(values.scenesDir === undefined || values.scenesDir === null ? "" : values.scenesDir).trim();
	let scenesDir = "";
	let scenesStored = "";
	if (scenesText !== "" && godotDir !== "") {
		if (scenesText.indexOf("res://") === 0)
			scenesDir = godotToLocal(godotDir, scenesText);
		else
			scenesDir = isAbsolutePath(scenesText) ? cleanPath(scenesText) : joinPath(godotDir, scenesText);
		if (scenesDir === godotDir) {
			scenesDir = "";
		} else if (!io.exists(scenesDir)) {
			errors.push("Scenes folder: '" + scenesDir + "' does not exist.");
		} else {
			const res = localToGodot(godotDir, scenesDir);
			if (res === null) {
				warnings.push("Scenes folder: '" + scenesDir + "' is outside the Godot project; scenes found there get " +
					"absolute paths, which the game cannot load as res://.");
				scenesStored = scenesDir;
			} else {
				scenesStored = res;
			}
		}
	} else if (scenesText !== "" && godotDir === "") {
		scenesDir = scenesText;		// checked once the project is right
	}

	// The import options.
	const optionsText = String(values.importOptions === undefined || values.importOptions === null ? "" : values.importOptions).trim();
	const parsed = parseImportOptions(optionsText);
	for (let i = 0; i < parsed.warnings.length; i++) {
		if (parsed.blocking.indexOf(parsed.warnings[i]) >= 0)
			errors.push("Import options: " + parsed.warnings[i]);
		else
			warnings.push("Import options: " + parsed.warnings[i]);
	}

	const validate = values.validate !== false && String(values.validate).toLowerCase() !== "false";

	const out = {
		errors: errors,
		warnings: warnings,
		resolved: { godotProject: godotDir, godotBinary: binary, scenesDir: scenesDir === "" ? godotDir : scenesDir, validate: validate },
		stored: null,
	};
	if (errors.length === 0) {
		const stored = {};
		stored[SETTING_NAMES.godotProject] = storedPath(projectDir, godotDir);
		stored[SETTING_NAMES.godotBinary] = binaryText;
		stored[SETTING_NAMES.scenesDir] = scenesStored;
		stored[SETTING_NAMES.importOptions] = optionsText;
		stored[SETTING_NAMES.validate] = validate;
		out.stored = stored;
	}
	return out;
}

// The lines that report a saved configuration (the log, the status).
export function configuredLines(validation) {
	const r = validation.resolved;
	const lines = [
		"Godot project: " + r.godotProject,
		"Godot binary: " + r.godotBinary,
		"Scenes scanned under: " + r.scenesDir,
		"Import options: " + (validation.stored[SETTING_NAMES.importOptions] !== "" ? validation.stored[SETTING_NAMES.importOptions] : "(none)"),
		"Validate on save: " + (r.validate ? "yes" : "no"),
	];
	return lines.concat(validation.warnings);
}
