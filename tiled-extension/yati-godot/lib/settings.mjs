// The extension's settings (custom properties of the Tiled project, saved in the .tiled-project) and
// the path helpers every module shares. Pure: no Tiled globals; the glue passes the values in.
//
// Written for Qt's JavaScript engine (ECMAScript 2016, as Tiled 1.12 runs extensions) as well as
// Node: no optional chaining, no object spread, no async, no Object.entries/values.

// The project properties the extension reads and writes.
export const SETTING_NAMES = {
	godotProject: "godot_project",
	godotBinary: "godot_binary",
	scenesDir: "godot_scenes_dir",
	importOptions: "yati_import_options",
	validate: "yati_validate",
	generated: "yati_generated",
	fixups: "yati_fixups",
};

export const DEFAULT_GODOT_BINARY = "godot";

// The importers' options (Runtime.md's table) that a check run may set as name=value.
export const IMPORT_OPTIONS = {
	use_default_filter: "bool",
	add_class_as_metadata: "bool",
	add_id_as_metadata: "bool",
	no_alternative_tiles: "bool",
	map_wangset_to_terrain: "bool",
	custom_data_prefix: "string",
	save_tileset_to: "string",
	parallax_mode: "string",
	animation_mode: "string",
	always_wrap_root: "bool",
	unknown_class_fallback: "string",
	warn_unknown_classes: "bool",
	class_scenes: "dictionary",
	read_godot_scene_from_custom_types: "bool",
	assign_node_properties: "bool",
	object_refs: "string",
};

// The values a string option takes (Runtime.md's table); an option not listed is free text.
// unknown_class_fallback also takes any mapping-table class (types.mjs lists them; the validator
// warns, not refuses, about a value outside this list).
export const IMPORT_OPTION_VALUES = {
	parallax_mode: ["parallax2d", "legacy"],
	animation_mode: ["strip", "atlas"],
	object_refs: ["nodepath", "geometry", "both"],
	unknown_class_fallback: ["staticbody", "collision", "characterbody", "rigidbody", "animatablebody", "area",
		"navigation", "occluder", "line", "path", "polygon", "instance", "parallax", "node2d"],
};

// One line per option, for a tooltip or a report: "name=true|false", "name=a|b", "name=<text>".
export function describeImportOptions() {
	const names = Object.keys(IMPORT_OPTIONS);
	const lines = [];
	for (let i = 0; i < names.length; i++) {
		const kind = IMPORT_OPTIONS[names[i]];
		if (kind === "dictionary")
			continue;
		let values = "<text>";
		if (kind === "bool")
			values = "true|false";
		else if (IMPORT_OPTION_VALUES[names[i]] !== undefined)
			values = IMPORT_OPTION_VALUES[names[i]].join("|");
		lines.push(names[i] + "=" + values);
	}
	return lines;
}

// ---------------------------------------------------------------------------------------------
// Paths. Tiled hands out paths with "/" on every platform (Qt), Windows ones with a drive letter
// ("C:/games/x"). These helpers work on that form; backslashes are turned into "/" first.

export function normalizeSlashes(p) {
	return String(p === undefined || p === null ? "" : p).replace(/\\/g, "/");
}

export function isAbsolutePath(p) {
	const s = normalizeSlashes(p);
	return s.charAt(0) === "/" || /^[A-Za-z]:\//.test(s);
}

// "a/./b/../c/" -> "a/c"; keeps a leading "/" or drive, and leading ".." of a relative path.
export function cleanPath(p) {
	const s = normalizeSlashes(p);
	let prefix = "";
	let rest = s;
	const drive = /^([A-Za-z]:)\//.exec(s);
	if (drive) {
		prefix = drive[1] + "/";
		rest = s.substring(3);
	} else if (s.charAt(0) === "/") {
		prefix = "/";
		rest = s.substring(1);
	}
	const out = [];
	const parts = rest.split("/");
	for (let i = 0; i < parts.length; i++) {
		const part = parts[i];
		if (part === "" || part === ".")
			continue;
		if (part === "..") {
			if (out.length > 0 && out[out.length - 1] !== "..")
				out.pop();
			else if (prefix === "")
				out.push("..");
			continue;
		}
		out.push(part);
	}
	const joined = prefix + out.join("/");
	if (joined === "")
		return ".";
	return joined;
}

export function joinPath(base, rel) {
	const r = normalizeSlashes(rel);
	if (isAbsolutePath(r))
		return cleanPath(r);
	return cleanPath(normalizeSlashes(base) + "/" + r);
}

export function dirName(p) {
	const s = cleanPath(p);
	const i = s.lastIndexOf("/");
	if (i < 0)
		return ".";
	if (i === 0)
		return "/";
	if (i === 2 && /^[A-Za-z]:/.test(s))
		return s.substring(0, 3);
	return s.substring(0, i);
}

export function baseName(p) {
	const s = cleanPath(p);
	const i = s.lastIndexOf("/");
	return i < 0 ? s : s.substring(i + 1);
}

// The file name without its last suffix: "big_slime.tscn" -> "big_slime".
export function stem(p) {
	const b = baseName(p);
	const i = b.lastIndexOf(".");
	return i > 0 ? b.substring(0, i) : b;
}

// The path of `target` relative to the directory `fromDir` (both absolute), with "/".
export function relativePath(fromDir, target) {
	const a = cleanPath(fromDir).split("/");
	const b = cleanPath(target).split("/");
	if (a[0] !== b[0])
		return cleanPath(target);		// another drive: no relative form
	let i = 0;
	while (i < a.length && i < b.length && a[i] === b[i])
		i++;
	const up = [];
	for (let k = i; k < a.length; k++) {
		if (a[k] !== "")
			up.push("..");
	}
	const rest = b.slice(i).filter(function (x) { return x !== ""; });
	const rel = up.concat(rest).join("/");
	return rel === "" ? "." : rel;
}

// A local path as the file URL Tiled's FilePath takes ("file:///C:/x" on Windows).
export function fileUrl(absPath) {
	const s = cleanPath(absPath);
	return s.charAt(0) === "/" ? "file://" + s : "file:///" + s;
}

// A Godot path ("res://scenes/a.tscn", or a path relative to the Godot project) as a local path.
export function godotToLocal(godotProjectDir, godotPath) {
	const s = String(godotPath);
	if (s.indexOf("res://") === 0)
		return joinPath(godotProjectDir, s.substring(6));
	return joinPath(godotProjectDir, s);
}

// A local path inside the Godot project as "res://...", or null when it is outside.
export function localToGodot(godotProjectDir, localPath) {
	const rel = relativePath(godotProjectDir, localPath);
	if (rel === ".." || rel.indexOf("../") === 0 || isAbsolutePath(rel))
		return null;
	return "res://" + (rel === "." ? "" : rel);
}

// ---------------------------------------------------------------------------------------------
// Settings.

// The settings from the project's properties. `getProperty(name)` returns the raw property value
// (undefined when unset); `projectFile` is the .tiled-project's path. Relative paths are relative
// to the project file's directory (so a project can be shared). `missing` lists what an action
// that needs the Godot project lacks.
export function readSettings(getProperty, projectFile) {
	const projectDir = projectFile ? dirName(projectFile) : "";
	const str = function (name) {
		const v = getProperty(name);
		return v === undefined || v === null ? "" : String(v).trim();
	};
	const godotProjectRaw = str(SETTING_NAMES.godotProject);
	const godotProject = godotProjectRaw === "" ? "" : joinPath(projectDir, godotProjectRaw);
	const scenesRaw = str(SETTING_NAMES.scenesDir);
	let scenesDir = godotProject;
	if (scenesRaw !== "" && godotProject !== "") {
		scenesDir = scenesRaw.indexOf("res://") === 0
			? godotToLocal(godotProject, scenesRaw)
			: joinPath(godotProject, scenesRaw);
	}
	const binaryRaw = str(SETTING_NAMES.godotBinary);
	let godotBinary = binaryRaw === "" ? DEFAULT_GODOT_BINARY : binaryRaw;
	// A binary given as a relative path (with a slash) is relative to the project, like the others;
	// a bare name ("godot", "godot4") is looked up on PATH by the process launcher.
	if (godotBinary.indexOf("/") >= 0 || godotBinary.indexOf("\\") >= 0)
		godotBinary = joinPath(projectDir, godotBinary);
	const validateRaw = getProperty(SETTING_NAMES.validate);
	const validate = !(validateRaw === false || String(validateRaw).toLowerCase() === "false");
	const options = parseImportOptions(str(SETTING_NAMES.importOptions));
	return {
		projectFile: projectFile || "",
		projectDir: projectDir,
		godotProjectRaw: godotProjectRaw,
		godotProject: godotProject,
		scenesDirRaw: scenesRaw,
		scenesDir: scenesDir,
		godotBinaryRaw: binaryRaw,
		godotBinary: godotBinary,
		importOptionsRaw: str(SETTING_NAMES.importOptions),
		importOptions: options.options,
		importOptionWarnings: options.warnings,
		validate: validate,
		generated: parseGenerated(getProperty(SETTING_NAMES.generated)),
		fixups: parseFixups(getProperty(SETTING_NAMES.fixups)),
		missing: godotProject === "" ? [SETTING_NAMES.godotProject] : [],
	};
}

// Why an action that needs the Godot project cannot run, or "" when it can. `projectDirExists`
// and `projectGodotExists` are the glue's file checks.
export function settingsProblem(settings, projectGodotExists) {
	if (settings.projectFile === "")
		return "No Tiled project is open. Open or create one (Project > Open Project...) first: " +
			"the extension keeps its settings in the project.";
	if (settings.missing.length > 0)
		return "The Godot project is not set: YATI needs the folder that holds project.godot.";
	if (!projectGodotExists)
		return "No project.godot in '" + settings.godotProject + "' (the setting " +
			SETTING_NAMES.godotProject + ").";
	return "";
}

// "assign_node_properties=false  warn_unknown_classes=false" -> options as {name, value, arg} with
// arg "--name=value" for check_map.gd; unknown names and bad values become warnings (still passed:
// a newer importer may know them). A value may be quoted: object_refs="both". `blocking` is the
// part of `warnings` the Configure dialog refuses to save: a boolean that is not true or false,
// a Dictionary option, text that is no name=value pair. An unknown name or an unlisted value of
// a string option only warns. `leftover` is the text that was no name=value pair (kept as it is
// by the options dialog).
export function parseImportOptions(text) {
	const options = [];
	const warnings = [];
	const blocking = [];
	const re = /([A-Za-z_][A-Za-z0-9_]*)\s*=\s*("([^"]*)"|'([^']*)'|(\S*))/g;
	let rest = String(text || "");
	let m;
	let consumed = "";
	while ((m = re.exec(rest)) !== null) {
		const name = m[1];
		let value = m[3] !== undefined ? m[3] : (m[4] !== undefined ? m[4] : m[5]);
		consumed += m[0];
		const kind = IMPORT_OPTIONS[name];
		if (kind === undefined) {
			warnings.push("Unknown import option '" + name + "' (passed on as it is).");
		} else if (kind === "bool") {
			const low = value.toLowerCase();
			if (low !== "true" && low !== "false") {
				warnings.push("Option '" + name + "' takes true or false, not '" + value + "'.");
				blocking.push(warnings[warnings.length - 1]);
			} else {
				value = low;
			}
		} else if (kind === "dictionary") {
			warnings.push("Option '" + name + "' is a Dictionary and cannot be given here; set it in the game's code.");
			blocking.push(warnings[warnings.length - 1]);
			continue;
		} else if (IMPORT_OPTION_VALUES[name] !== undefined && IMPORT_OPTION_VALUES[name].indexOf(value) < 0) {
			warnings.push("Option '" + name + "' takes " + IMPORT_OPTION_VALUES[name].join(", ") + "; '" + value + "' is passed on as it is.");
		}
		options.push({ name: name, value: value, arg: "--" + name + "=" + value });
	}
	const leftover = rest.replace(re, "").trim();
	if (leftover !== "") {
		warnings.push("Ignored in the import options: '" + leftover + "' (expected name=value pairs).");
		blocking.push(warnings[warnings.length - 1]);
	}
	return { options: options, warnings: warnings, blocking: blocking, leftover: leftover };
}

// The record of what the extension generated: {typeName: [member names]} (JSON in the project
// property yati_generated). Anything unreadable is an empty record.
export function parseGenerated(raw) {
	if (raw === undefined || raw === null || raw === "")
		return {};
	try {
		const v = JSON.parse(String(raw));
		if (v === null || typeof v !== "object" || Array.isArray(v))
			return {};
		const out = {};
		const keys = Object.keys(v);
		for (let i = 0; i < keys.length; i++) {
			if (Array.isArray(v[keys[i]]))
				out[keys[i]] = v[keys[i]].map(String);
		}
		return out;
	} catch (e) {
		return {};
	}
}

// The record as the property's JSON: keys and member names sorted, so a re-run without changes
// writes the same text.
export function stringifyGenerated(record) {
	const keys = Object.keys(record).sort();
	const out = {};
	for (let i = 0; i < keys.length; i++)
		out[keys[i]] = record[keys[i]].slice().sort();
	return JSON.stringify(out);
}

// The fixes the project file needs beyond what the API sets (apply.mjs, patchProjectText), kept
// so that every action re-applies all of them after Tiled has saved the project from memory:
// {flags: [enum names], floats: {type: [member names]}, written: N} (JSON in the project property
// yati_fixups). `written` counts the extension's writes of the project file; the patch writes the
// record with the next count into the file only, so after the reload the extension asks for, the
// project's value tells whether Tiled read the file (yati-godot.mjs, reloadProject).
export function parseFixups(raw) {
	const out = { flags: [], floats: {}, written: 0 };
	if (raw === undefined || raw === null || raw === "")
		return out;
	try {
		const v = JSON.parse(String(raw));
		if (v && typeof v.written === "number" && v.written >= 0)
			out.written = Math.floor(v.written);
		if (v && Array.isArray(v.flags))
			out.flags = v.flags.map(String);
		if (v && v.floats && typeof v.floats === "object" && !Array.isArray(v.floats)) {
			const keys = Object.keys(v.floats);
			for (let i = 0; i < keys.length; i++) {
				if (Array.isArray(v.floats[keys[i]]))
					out.floats[keys[i]] = v.floats[keys[i]].map(String);
			}
		}
	} catch (e) {
		// unreadable: start over
	}
	return out;
}

export function stringifyFixups(fixups) {
	const floats = {};
	const keys = Object.keys(fixups.floats).sort();
	for (let i = 0; i < keys.length; i++) {
		if (fixups.floats[keys[i]].length > 0)
			floats[keys[i]] = fixups.floats[keys[i]].slice().sort();
	}
	return JSON.stringify({ flags: fixups.flags.slice().sort(), floats: floats, written: fixups.written === undefined ? 0 : fixups.written });
}

// What Configure stores for a chosen directory: relative to the project file when the two share
// a directory below the file system's root (so the project can move with the game), else absolute.
export function storedPath(projectDir, chosen) {
	if (chosen === "")
		return "";
	const a = cleanPath(projectDir).split("/");
	const b = cleanPath(chosen).split("/");
	const shared = a.length > 1 && b.length > 1 && a[0] === b[0] && a[1] === b[1] && a[1] !== "";
	if (!shared)
		return cleanPath(chosen);
	return relativePath(projectDir, chosen);
}
