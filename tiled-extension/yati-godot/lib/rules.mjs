// The project's rules (the project property yati_rules): what class members refer to, checked on
// save and used by the actions that pick, follow, open and preview those references. Nothing here
// knows a game's classes: the rules name them. Pure: the glue passes file reads and the open maps
// in, so the tests run all of it under Node.
//
// yati_rules is a JSON array of rules (README, "Rules"). Each rule names the objects it applies to
// (`class`: a class name, a list of them, or "*"; or `world`: a .world file, whose own properties
// it checks), the `member` it checks, and its `kind`:
//
//   map            the value is a map file (.tmx or .tmj) that exists
//   object_in_map  the value is the name of an object in the map the member `map` names (in the
//                  same map without `map`), of class `targetClass` when given
//   object_in_project  the value is the name of an object in any map of the project (the maps under
//                  the project's folders, or under the rule's `maps` folder or glob), of one of the
//                  classes `targetClass` (a name or a list) when given; exactly one such object
//   unique_name    (no `member`) an object of the rule's classes has a name no other object of those
//                  classes has, in this map or any other map of the project (`maps` as above);
//                  `required`: every such object has a name
//   file           the value is a file that exists; each of `contains` ([{pattern, label}], a regular
//                  expression and how to call it) must match its text. `template` and `folder` are
//                  what "Open or create file" copies and where it puts the copy
//   key            when the value matches `keyPattern` (default UPPER_SNAKE_CASE with an underscore),
//                  it must be a key of the CSV file `table` (its first column); else it is plain text
//
// and optionally `required` (an empty value is a finding) and `severity` ("error", the default, or
// "warning"). Paths in a rule are relative to the project file; values relative to the map (as
// Tiled stores file properties), a class's default relative to the project file, res:// to the
// Godot project.
//
// Written for Qt's JavaScript engine (ECMAScript 2016) as well as Node.

import { joinPath, dirName, baseName, stem, cleanPath, relativePath } from "./settings.mjs";
import { resolveFileValue, withClassMembers, describeObject } from "./validate.mjs";
import { walkFiles } from "./scene_exports.mjs";

export const RULE_KINDS = ["map", "object_in_map", "object_in_project", "unique_name", "file", "key"];

// UPPER_SNAKE_CASE with at least one underscore (SIGN_WELCOME): "OK" or "HELP" stay plain text.
export const DEFAULT_KEY_PATTERN = "^[A-Z][A-Z0-9]*(_[A-Z0-9]+)+$";

export const MAP_FILTER = "Tiled maps (*.tmx *.tmj)";

const COMMON_FIELDS = ["kind", "member", "class", "world", "required", "severity"];
const KIND_FIELDS = {
	map: [],
	object_in_map: ["map", "targetClass"],
	object_in_project: ["targetClass", "maps"],
	unique_name: ["maps"],
	file: ["contains", "template", "folder"],
	key: ["table", "keyPattern"],
};

// ---------------------------------------------------------------------------------------------
// The rules

// yati_rules's text as {rules, problems}: each rule normalized ({index (1-based), kind, member ("" for
// unique_name), classes ([] for a world rule; ["*"] for any class), world, required, severity, map,
// targetClass (object_in_map's one name), targetClasses ([] for any class), maps, contains:
// [{regex, label}], template, folder, table, keyPattern: RegExp}); a rule with a problem is left out
// and the problem names it. Unset or empty: no rules, no problems.
export function parseRules(raw) {
	const out = { rules: [], problems: [] };
	if (raw === undefined || raw === null || String(raw).trim() === "")
		return out;
	let list;
	try {
		list = JSON.parse(String(raw));
	} catch (e) {
		out.problems.push("yati_rules is not valid JSON (" + (e && e.message ? e.message : String(e)) + ").");
		return out;
	}
	if (!Array.isArray(list)) {
		out.problems.push("yati_rules must be a JSON array of rules ([{\"class\": ..., \"member\": ..., \"kind\": ...}, ...]).");
		return out;
	}
	for (let i = 0; i < list.length; i++) {
		const r = parseRule(list[i], i + 1);
		if (r.problem !== "")
			out.problems.push("yati_rules, rule " + (i + 1) + ": " + r.problem);
		else
			out.rules.push(r.rule);
	}
	// A member `map` names: another member of the same objects, not this one.
	for (let i = 0; i < out.rules.length; i++) {
		const r = out.rules[i];
		if (r.kind === "object_in_map" && r.map === r.member)
			out.problems.push("yati_rules, rule " + r.index + ": \"map\" names the member the rule checks (" + r.member + ").");
	}
	return out;
}

function parseRule(v, index) {
	const fail = function (text) { return { rule: null, problem: text }; };
	if (v === null || typeof v !== "object" || Array.isArray(v))
		return fail("a rule is an object ({\"class\": ..., \"member\": ..., \"kind\": ...}).");
	const kind = v.kind;
	if (RULE_KINDS.indexOf(kind) < 0)
		return fail("\"kind\" is " + (kind === undefined ? "missing" : JSON.stringify(kind)) + "; it is one of " + RULE_KINDS.join(", ") + ".");
	const allowed = COMMON_FIELDS.concat(KIND_FIELDS[kind]);
	const keys = Object.keys(v);
	for (let i = 0; i < keys.length; i++) {
		if (allowed.indexOf(keys[i]) < 0)
			return fail("unknown field \"" + keys[i] + "\" for kind " + kind + " (it takes " + allowed.join(", ") + ").");
	}
	if (kind === "unique_name") {
		if (v.member !== undefined)
			return fail("unique_name checks the objects' own names; it takes no \"member\".");
		if (v.world !== undefined)
			return fail("unique_name applies to objects: give \"class\" (a class name or a list), not \"world\".");
	} else if (typeof v.member !== "string" || v.member === "") {
		return fail("\"member\" (the member's name) is missing.");
	}
	const rule = {
		index: index, kind: kind, member: kind === "unique_name" ? "" : v.member, classes: [], world: "", required: v.required === true,
		severity: "error", map: "", targetClass: "", targetClasses: [], maps: "", contains: [], template: "", folder: "", table: "", keyPattern: null,
	};
	if (v.required !== undefined && typeof v.required !== "boolean")
		return fail("\"required\" is true or false.");
	if (v.severity !== undefined) {
		if (v.severity !== "error" && v.severity !== "warning")
			return fail("\"severity\" is \"error\" or \"warning\".");
		rule.severity = v.severity;
	}
	const hasClass = v["class"] !== undefined;
	const hasWorld = v.world !== undefined;
	if (hasClass === hasWorld)
		return fail("give either \"class\" (the objects' class, a list of classes, or \"*\") or \"world\" (a .world file).");
	if (hasClass) {
		const classes = typeof v["class"] === "string" ? [v["class"]] : v["class"];
		if (!Array.isArray(classes) || classes.length === 0 || classes.some(function (c) { return typeof c !== "string" || c === ""; }))
			return fail("\"class\" is a class name, a list of class names, or \"*\".");
		rule.classes = classes.slice();
	} else {
		if (typeof v.world !== "string" || v.world === "")
			return fail("\"world\" is the path of a .world file, relative to the project file.");
		rule.world = v.world;
	}
	const str = function (name) {
		if (v[name] === undefined)
			return "";
		return typeof v[name] === "string" ? v[name] : null;
	};
	const fields = KIND_FIELDS[kind];
	for (let i = 0; i < fields.length; i++) {
		const name = fields[i];
		if (name === "contains" || name === "keyPattern" || (name === "targetClass" && kind === "object_in_project"))
			continue;
		const s = str(name);
		if (s === null)
			return fail("\"" + name + "\" is a string.");
		rule[name] = s;
	}
	if (kind === "object_in_map" && rule.targetClass !== "")
		rule.targetClasses = [rule.targetClass];
	if (kind === "object_in_project" && v.targetClass !== undefined) {
		const targets = typeof v.targetClass === "string" ? [v.targetClass] : v.targetClass;
		if (!Array.isArray(targets) || targets.length === 0 || targets.some(function (c) { return typeof c !== "string" || c === ""; }))
			return fail("\"targetClass\" is a class name or a list of class names.");
		rule.targetClasses = targets.slice();
	}
	if (kind === "object_in_map" && rule.world !== "" && rule.map === "")
		return fail("a world rule of kind object_in_map needs \"map\" (the world property that names the map).");
	if (kind === "file" && v.contains !== undefined) {
		if (!Array.isArray(v.contains))
			return fail("\"contains\" is a list of {\"pattern\": ..., \"label\": ...}.");
		for (let i = 0; i < v.contains.length; i++) {
			const c = typeof v.contains[i] === "string" ? { pattern: v.contains[i] } : v.contains[i];
			if (c === null || typeof c !== "object" || typeof c.pattern !== "string" || c.pattern === "")
				return fail("\"contains\" item " + (i + 1) + " needs a \"pattern\" (a regular expression).");
			if (c.label !== undefined && typeof c.label !== "string")
				return fail("\"contains\" item " + (i + 1) + ": \"label\" is a string.");
			const re = compile(c.pattern);
			if (re === null)
				return fail("\"contains\" item " + (i + 1) + ": '" + c.pattern + "' is not a regular expression.");
			rule.contains.push({ regex: re, label: c.label !== undefined && c.label !== "" ? c.label : "a match for /" + c.pattern + "/" });
		}
	}
	if (kind === "key") {
		if (rule.table === "")
			return fail("\"table\" (the CSV file of keys, relative to the project file) is missing.");
		const pattern = v.keyPattern === undefined ? DEFAULT_KEY_PATTERN : v.keyPattern;
		rule.keyPattern = typeof pattern === "string" ? compile(pattern) : null;
		if (rule.keyPattern === null)
			return fail("\"keyPattern\" is not a regular expression.");
	}
	if (kind === "file" && rule.folder !== "" && rule.template === "")
		return fail("\"folder\" is where the template's copies go; give \"template\" too.");
	return { rule: rule, problem: "" };
}

function compile(pattern) {
	try {
		return new RegExp(pattern);
	} catch (e) {
		return null;
	}
}

// The rules for objects of class `cls` ("" matches only "*"), of one of `kinds` (all when omitted).
export function rulesForClass(rules, cls, kinds) {
	return rules.filter(function (r) {
		if (r.world !== "")
			return false;
		if (kinds && kinds.indexOf(r.kind) < 0)
			return false;
		return r.classes.indexOf("*") >= 0 || (cls !== "" && r.classes.indexOf(cls) >= 0);
	});
}

// The rules that check a world file, by its absolute path: [{file, rules}] in rule order.
export function worldRules(rules, projectDir) {
	const out = [];
	for (let i = 0; i < rules.length; i++) {
		const r = rules[i];
		if (r.world === "")
			continue;
		const file = joinPath(projectDir, r.world);
		let entry = null;
		for (let k = 0; k < out.length; k++) {
			if (out[k].file === file)
				entry = out[k];
		}
		if (entry === null) {
			entry = { file: file, rules: [] };
			out.push(entry);
		}
		entry.rules.push(r);
	}
	return out;
}

// A rule in a list to choose from: "to_spawn (object in the map to_map names)".
export function ruleLabel(r) {
	if (r.kind === "object_in_project")
		return r.member + " (an object" + (r.targetClasses.length > 0 ? " of class " + r.targetClasses.join(" or ") : "") + " in any map of the project)";
	if (r.kind === "unique_name")
		return "names (unique among " + classList(r.classes, " and ") + " objects in the project)";
	if (r.kind === "object_in_map")
		return r.member + " (an object" + (r.targetClass !== "" ? " of class " + r.targetClass : "") + " in " + (r.map !== "" ? "the map " + r.map + " names" : "this map") + ")";
	if (r.kind === "map")
		return r.member + " (a map)";
	if (r.kind === "file")
		return r.member + " (a file)";
	return r.member + " (a text key)";
}

function classList(classes, joiner) {
	return classes.indexOf("*") >= 0 ? "all" : classes.join(joiner);
}

// ---------------------------------------------------------------------------------------------
// Values

// The member's prop in a props list, or null.
export function findMember(props, name) {
	for (let i = 0; i < props.length; i++) {
		if (props[i].name === name)
			return props[i];
	}
	return null;
}

// The member's value as text, "" when unset.
export function memberText(prop) {
	if (prop === null || prop === undefined || prop.value === undefined || prop.value === null)
		return "";
	if (typeof prop.value === "object")
		return "";
	return String(prop.value);
}

// A file-like member as a local path ("" when unset or unresolvable). An object's own value is
// relative to the file that holds it, a class's default to the project file.
export function memberPath(prop, holderFile, ctx) {
	const text = memberText(prop);
	if (text === "")
		return "";
	return resolveFileValue(text, prop.fromClass ? (ctx.projectFile || "") : holderFile, ctx.godotProject || "");
}

export function isMapFile(p) {
	return /\.(tmx|tmj)$/i.test(String(p));
}

// ---------------------------------------------------------------------------------------------
// CSV translation tables

// RFC 4180 CSV (quoted fields with commas, quotes doubled, line breaks inside quotes; \r\n or \n).
export function parseCsv(text) {
	const rows = [];
	let row = [];
	let field = "";
	let quoted = false;
	const s = String(text).replace(/^﻿/, "");
	for (let i = 0; i < s.length; i++) {
		const c = s.charAt(i);
		if (quoted) {
			if (c === "\"") {
				if (s.charAt(i + 1) === "\"") {
					field += "\"";
					i++;
				} else {
					quoted = false;
				}
			} else {
				field += c;
			}
		} else if (c === "\"" && field === "") {
			quoted = true;
		} else if (c === ",") {
			row.push(field);
			field = "";
		} else if (c === "\n" || c === "\r") {
			if (c === "\r" && s.charAt(i + 1) === "\n")
				i++;
			row.push(field);
			rows.push(row);
			row = [];
			field = "";
		} else {
			field += c;
		}
	}
	if (field !== "" || row.length > 0) {
		row.push(field);
		rows.push(row);
	}
	return rows.filter(function (r) { return !(r.length === 1 && r[0] === ""); });
}

// A translation table (Godot's CSV: the first row the header, `keys` then one column per locale)
// as {header: [...], rows: {KEY: [cells]}}.
export function readTable(text) {
	const rows = parseCsv(text);
	const out = { header: rows.length > 0 ? rows[0] : [], rows: {} };
	for (let i = 1; i < rows.length; i++) {
		const key = rows[i][0];
		if (key !== undefined && key !== "")
			out.rows[key] = rows[i];
	}
	return out;
}

// What a key rule makes of a value: {state: "unset" | "plain" | "key" | "unknown", lines} where the
// lines say it for a person ("SIGN_WELCOME", "  en: Welcome!"). `table` is readTable's, or null
// when it could not be read.
export function keyPreview(rule, value, table, tableName) {
	const member = rule.member;
	if (value === "")
		return { state: "unset", lines: [member + ": (not set)"] };
	if (!rule.keyPattern.test(value))
		return { state: "plain", lines: [member + ": plain text (not shaped like a key): " + quoteText(value)] };
	if (table === null)
		return { state: "unknown", lines: [member + ": " + value + ": the table " + tableName + " could not be read."] };
	const row = table.rows[value];
	if (row === undefined) {
		const lines = [member + ": " + value + " is no key of " + tableName + "."];
		const near = nearKeys(table, value);
		if (near.length > 0)
			lines.push("  Did you mean " + near.join(", ") + "?");
		return { state: "unknown", lines: lines };
	}
	const lines = [member + ": " + value];
	for (let i = 1; i < table.header.length; i++)
		lines.push("  " + (table.header[i] || "column " + (i + 1)) + ": " + quoteText(row[i] === undefined ? "" : row[i]));
	if (table.header.length < 2)
		lines.push("  (the table has no text column)");
	return { state: "key", lines: lines };
}

function quoteText(s) {
	const one = String(s).replace(/\r?\n/g, "\\n");
	return "\"" + (one.length > 120 ? one.substring(0, 117) + "..." : one) + "\"";
}

// Keys that differ only in case, or share all but the last part ("SIGN_WELCOM" -> "SIGN_WELCOME").
function nearKeys(table, value) {
	const keys = Object.keys(table.rows);
	const low = value.toLowerCase();
	const prefix = value.indexOf("_") > 0 ? value.substring(0, value.lastIndexOf("_") + 1) : "";
	const out = [];
	for (let i = 0; i < keys.length && out.length < 3; i++) {
		if (keys[i].toLowerCase() === low || (prefix !== "" && keys[i].indexOf(prefix) === 0))
			out.push(keys[i]);
	}
	return out;
}

// ---------------------------------------------------------------------------------------------
// Maps and worlds read from their files (a map that is not open in Tiled)

const ENTITIES = { amp: "&", lt: "<", gt: ">", quot: "\"", apos: "'" };

function decodeEntities(s) {
	return s.replace(/&(#x[0-9a-fA-F]+|#\d+|[a-z]+);/g, function (all, e) {
		if (e.charAt(0) === "#")
			return String.fromCharCode(e.charAt(1) === "x" ? parseInt(e.substring(2), 16) : parseInt(e.substring(1), 10));
		return ENTITIES[e] !== undefined ? ENTITIES[e] : all;
	});
}

function xmlAttrs(text) {
	const out = {};
	const re = /([\w.:-]+)\s*=\s*("([^"]*)"|'([^']*)')/g;
	let m;
	while ((m = re.exec(text)) !== null)
		out[m[1]] = decodeEntities(m[3] !== undefined ? m[3] : m[4]);
	return out;
}

function stripXmlComments(text) {
	return String(text).replace(/<!--[\s\S]*?-->/g, "");
}

function isJson(text) {
	return /^\s*\{/.test(String(text));
}

// A tileset's tile classes, {localId: class}, from a .tsx / embedded <tileset> or a .tsj / embedded JSON tileset.
function tileClassesXml(text) {
	const out = {};
	const re = /<tile\b([^>]*)>/g;
	let m;
	while ((m = re.exec(text)) !== null) {
		const a = xmlAttrs(m[1]);
		const cls = a["class"] !== undefined ? a["class"] : (a.type !== undefined ? a.type : "");
		if (a.id !== undefined && cls !== "")
			out[Number(a.id)] = cls;
	}
	return out;
}

function tileClassesJson(tileset) {
	const out = {};
	const tiles = tileset && Array.isArray(tileset.tiles) ? tileset.tiles : [];
	for (let i = 0; i < tiles.length; i++) {
		const cls = typeof tiles[i]["class"] === "string" ? tiles[i]["class"] : (typeof tiles[i].type === "string" ? tiles[i].type : "");
		if (cls !== "")
			out[Number(tiles[i].id)] = cls;
	}
	return out;
}

// An external tileset file's tile classes ({} when it cannot be read).
function externalTileClasses(file, readText) {
	const text = readText(file);
	if (text === null)
		return {};
	if (isJson(text)) {
		try {
			return tileClassesJson(JSON.parse(text));
		} catch (e) {
			return {};
		}
	}
	return tileClassesXml(stripXmlComments(text));
}

// The class of the tile a gid shows, given [{firstgid, classes}].
function tileClassFor(tilesets, gid) {
	const g = gid & 0x0fffffff;
	let best = null;
	for (let i = 0; i < tilesets.length; i++) {
		if (tilesets[i].firstgid <= g && (best === null || tilesets[i].firstgid > best.firstgid))
			best = tilesets[i];
	}
	if (best === null || g === 0)
		return "";
	const cls = best.classes[g - best.firstgid];
	return cls === undefined ? "" : cls;
}

// A template file (.tx or .tj): {className, name, tileClass}.
function readTemplate(file, readText) {
	const out = { className: "", name: "", tileClass: "" };
	const text = readText(file);
	if (text === null)
		return out;
	const dir = dirName(file);
	if (isJson(text)) {
		let json;
		try {
			json = JSON.parse(text);
		} catch (e) {
			return out;
		}
		const o = json.object || {};
		out.className = typeof o["class"] === "string" ? o["class"] : (typeof o.type === "string" ? o.type : "");
		out.name = typeof o.name === "string" ? o.name : "";
		if (o.gid && json.tileset) {
			const ts = json.tileset;
			const classes = typeof ts.source === "string" ? externalTileClasses(joinPath(dir, ts.source), readText) : tileClassesJson(ts);
			out.tileClass = tileClassFor([{ firstgid: Number(ts.firstgid || 1), classes: classes }], Number(o.gid));
		}
		return out;
	}
	const s = stripXmlComments(text);
	const tilesets = [];
	const tsRe = /<tileset\b([^>]*?)(\/>|>([\s\S]*?)<\/tileset>)/g;
	let m;
	while ((m = tsRe.exec(s)) !== null) {
		const a = xmlAttrs(m[1]);
		const classes = a.source !== undefined ? externalTileClasses(joinPath(dir, a.source), readText) : tileClassesXml(m[3] || "");
		tilesets.push({ firstgid: Number(a.firstgid || 1), classes: classes });
	}
	const om = /<object\b([^>]*)>/.exec(s.replace(tsRe, ""));
	if (om) {
		const a = xmlAttrs(om[1]);
		out.className = a["class"] !== undefined ? a["class"] : (a.type !== undefined ? a.type : "");
		out.name = a.name !== undefined ? a.name : "";
		if (a.gid !== undefined)
			out.tileClass = tileClassFor(tilesets, Number(a.gid));
	}
	return out;
}

// The objects of a map file (.tmx or .tmj) as [{id, name, className}]: the class is the object's
// own, else its template's, else its tile's (as Tiled resolves it); the name the object's, else
// its template's. Objects of tile collision shapes are not the map's. {objects, error}.
export function parseMapObjects(text, mapFile, readText) {
	const dir = dirName(mapFile);
	const templates = {};
	const template = function (path) {
		const file = joinPath(dir, path);
		if (templates[file] === undefined)
			templates[file] = readTemplate(file, readText);
		return templates[file];
	};
	const resolve = function (id, own, ownName, tplPath, gid, tilesets) {
		let cls = own;
		let name = ownName;
		let tileClass = gid ? tileClassFor(tilesets, gid) : "";
		if (tplPath !== "") {
			const t = template(tplPath);
			if (cls === "")
				cls = t.className;
			if (name === "")
				name = t.name;
			if (!gid)
				tileClass = t.tileClass;
		}
		return { id: id, name: name, className: cls !== "" ? cls : tileClass };
	};
	if (isJson(text)) {
		let json;
		try {
			json = JSON.parse(text);
		} catch (e) {
			return { objects: [], error: "not valid JSON" };
		}
		const tilesets = (json.tilesets || []).map(function (ts) {
			return { firstgid: Number(ts.firstgid || 1), classes: typeof ts.source === "string" ? externalTileClasses(joinPath(dir, ts.source), readText) : tileClassesJson(ts) };
		});
		const objects = [];
		const walk = function (layers) {
			for (let i = 0; i < (layers || []).length; i++) {
				const l = layers[i];
				const list = Array.isArray(l.objects) ? l.objects : [];
				for (let k = 0; k < list.length; k++) {
					const o = list[k];
					const own = typeof o["class"] === "string" ? o["class"] : (typeof o.type === "string" ? o.type : "");
					objects.push(resolve(Number(o.id || 0), own, typeof o.name === "string" ? o.name : "",
						typeof o.template === "string" ? o.template : "", Number(o.gid || 0), tilesets));
				}
				if (Array.isArray(l.layers))
					walk(l.layers);
			}
		};
		walk(json.layers);
		return { objects: objects, error: "" };
	}
	const s = stripXmlComments(text);
	if (!/<map\b/.test(s))
		return { objects: [], error: "not a Tiled map" };
	const tilesets = [];
	const tsRe = /<tileset\b([^>]*?)(\/>|>([\s\S]*?)<\/tileset>)/g;
	let m;
	while ((m = tsRe.exec(s)) !== null) {
		const a = xmlAttrs(m[1]);
		const classes = a.source !== undefined ? externalTileClasses(joinPath(dir, a.source), readText) : tileClassesXml(m[3] || "");
		tilesets.push({ firstgid: Number(a.firstgid || 1), classes: classes });
	}
	const body = s.replace(tsRe, "");
	const objects = [];
	const re = /<object\b([^>]*)>/g;
	while ((m = re.exec(body)) !== null) {
		const a = xmlAttrs(m[1]);
		const own = a["class"] !== undefined ? a["class"] : (a.type !== undefined ? a.type : "");
		objects.push(resolve(Number(a.id || 0), own, a.name !== undefined ? a.name : "",
			a.template !== undefined ? a.template : "", a.gid !== undefined ? Number(a.gid) : 0, tilesets));
	}
	return { objects: objects, error: "" };
}

// A .world file: {maps: [absolute paths], patterns: [regexp strings], properties: [prop], error}.
// Its properties are read from a Tiled properties list ("properties": [{name, type, value}]) or,
// failing that, from the file's own top-level string keys; a file value is relative to the world.
export function parseWorld(text, worldFile) {
	const out = { maps: [], patterns: [], properties: [], error: "" };
	let json;
	try {
		json = JSON.parse(String(text));
	} catch (e) {
		out.error = "not valid JSON";
		return out;
	}
	if (json === null || typeof json !== "object" || Array.isArray(json)) {
		out.error = "not a world file";
		return out;
	}
	const dir = dirName(worldFile);
	const maps = Array.isArray(json.maps) ? json.maps : [];
	for (let i = 0; i < maps.length; i++) {
		if (maps[i] && typeof maps[i].fileName === "string")
			out.maps.push(joinPath(dir, maps[i].fileName));
	}
	const patterns = Array.isArray(json.patterns) ? json.patterns : [];
	for (let i = 0; i < patterns.length; i++) {
		if (patterns[i] && typeof patterns[i].regexp === "string")
			out.patterns.push(patterns[i].regexp);
	}
	const seen = {};
	const props = Array.isArray(json.properties) ? json.properties : [];
	for (let i = 0; i < props.length; i++) {
		const p = props[i];
		if (!p || typeof p.name !== "string")
			continue;
		const type = typeof p.type === "string" ? p.type : "string";
		let value = p.value;
		if (type === "file" && typeof value === "string" && value !== "" && !/^[a-z]+:\/\//.test(value))
			value = joinPath(dir, value);
		out.properties.push({ name: p.name, type: type, value: value, propertyType: p.propertytype || "" });
		seen[p.name] = true;
	}
	const keys = Object.keys(json);
	for (let i = 0; i < keys.length; i++) {
		if (!seen[keys[i]] && typeof json[keys[i]] === "string" && keys[i] !== "type")
			out.properties.push({ name: keys[i], type: "string", value: json[keys[i]], propertyType: "" });
	}
	return out;
}

// Whether a world holds the map: listed in its maps, or (in the world's folder) matched by one of
// its patterns, as Tiled matches them against the file names there.
export function worldContains(world, worldFile, mapFile) {
	const m = cleanPath(mapFile);
	for (let i = 0; i < world.maps.length; i++) {
		if (cleanPath(world.maps[i]) === m)
			return true;
	}
	if (dirName(m) !== dirName(cleanPath(worldFile)))
		return false;
	for (let i = 0; i < world.patterns.length; i++) {
		const re = compile(world.patterns[i]);
		if (re !== null && re.test(baseName(m)))
			return true;
	}
	return false;
}

// ---------------------------------------------------------------------------------------------
// Checking

// The findings of the class rules for a map model (validate.mjs's: objects with resolved
// properties), as validate.mjs's findings ({severity, code, text, objectId, layerId}).
//
// ctx: {projectFile, godotProject, classes (validate's context: class defaults), fileExists(p),
//       readText(p) -> text or null, mapObjects(path) -> {objects: [{id, name, className}], error}
//       (the glue answers from an open map, or the file)}
export function checkRules(map, rules, ctx) {
	const findings = [];
	const env = environment(ctx);
	const mapFile = map.fileName || "";
	env.skipFile = mapFile !== "" ? cleanPath(mapFile) : "";	// the saved map: its model, not its file
	const sameMap = map.objects.map(function (o) {
		return { id: o.id, name: o.name, className: o.className !== "" ? o.className : (o.tileClassName || "") };
	});
	for (let i = 0; i < map.objects.length; i++) {
		const o = map.objects[i];
		const cls = o.className !== "" ? o.className : (o.tileClassName || "");
		const applying = rulesForClass(rules, cls);
		if (applying.length === 0)
			continue;
		const props = withClassMembers(o.properties || [], ctx.classes ? ctx.classes[cls] : undefined);
		const holder = { where: describeObject(o), file: mapFile, props: props, sameMap: sameMap, mapRules: applying };
		const report = function (severity, code, text) {
			findings.push({ severity: severity, code: code, text: text, objectId: o.id, layerId: 0 });
		};
		for (let k = 0; k < applying.length; k++) {
			if (applying[k].kind === "unique_name")
				checkUniqueName(applying[k], o, cls, holder, env, report);
			else
				checkMember(applying[k], holder, env, report);
		}
	}
	return env.unreadable.concat(findings);
}

// The findings of a world's rules (objectId 0: a world has no object to select).
export function checkWorld(worldFile, world, rules, ctx) {
	const findings = [];
	const env = environment(ctx);
	const holder = { where: "World " + baseName(worldFile), file: worldFile, props: world.properties, sameMap: null, mapRules: rules };
	for (let i = 0; i < rules.length; i++) {
		checkMember(rules[i], holder, env, function (severity, code, text) {
			findings.push({ severity: severity, code: code, text: text, objectId: 0, layerId: 0 });
		});
	}
	return env.unreadable.concat(findings);
}

// The context with caches: a file read once per check.
function environment(ctx) {
	const texts = {};
	const maps = {};
	const tables = {};
	const readText = function (p) {
		if (texts[p] === undefined)
			texts[p] = ctx.readText(p);
		return texts[p];
	};
	const projectDir = ctx.projectFile ? dirName(ctx.projectFile) : "";
	const mapObjects = function (p) {
		if (maps[p] === undefined)
			maps[p] = ctx.mapObjects(p);
		return maps[p];
	};
	const indexes = {};
	const env = {
		ctx: ctx,
		projectDir: projectDir,
		readText: readText,
		mapObjects: mapObjects,
		unreadable: [],
		skipFile: "",
		// The objects of the project's maps (the rule's `maps`, else the project's folders) by name,
		// built once per check and `maps` value; the maps that could not be read reported once.
		projectIndex: function (rule) {
			const key = rule.maps;
			if (indexes[key] === undefined) {
				const all = projectMapFiles(key, projectDir, ctx.projectFolders || [], ctx.listDir || function () { return null; });
				const files = all.filter(function (f) { return f !== env.skipFile; });
				indexes[key] = projectObjects(files, mapObjects);
				indexes[key].holdsSkipped = files.length < all.length;
				for (let i = 0; i < indexes[key].errors.length; i++) {
					const e = indexes[key].errors[i];
					const seen = env.unreadable.some(function (f) { return f.file === e.file; });
					if (!seen)
						env.unreadable.push({ severity: "warning", code: "rule-unreadable", file: e.file, objectId: 0, layerId: 0,
							text: "The project's map " + projectLabel(e.file, projectDir) + " could not be read (" + e.error + "); the rules that look across the project skip it." });
				}
			}
			return indexes[key];
		},
		table: function (p) {
			if (tables[p] === undefined) {
				const text = readText(p);
				tables[p] = text === null ? null : readTable(text);
			}
			return tables[p];
		},
	};
	return env;
}

function checkMember(rule, holder, env, report) {
	const ctx = env.ctx;
	const prop = findMember(holder.props, rule.member);
	const text = memberText(prop);
	const where = holder.where + ": " + rule.member;
	const push = function (severity, code, message) {
		report(rule.severity === "warning" ? "warning" : severity, code, where + " " + message);
	};
	if (text === "") {
		if (rule.required)
			push("error", "rule-required", "is not set; the project's rules (yati_rules) require " + kindNoun(rule) + ".");
		return;
	}
	if (rule.kind === "map") {
		const path = memberPath(prop, holder.file, ctx);
		if (path === "")
			return;
		if (!ctx.fileExists(path))
			push("error", "rule-missing-map", "names " + path + ", which does not exist.");
		else if (!isMapFile(path))
			push("error", "rule-not-a-map", "names " + path + ", which is no Tiled map (.tmx or .tmj).");
		return;
	}
	if (rule.kind === "object_in_map") {
		checkObjectInMap(rule, text, holder, env, push);
		return;
	}
	if (rule.kind === "object_in_project") {
		checkObjectInProject(rule, text, holder, env, push);
		return;
	}
	if (rule.kind === "file") {
		const path = memberPath(prop, holder.file, ctx);
		if (path === "")
			return;
		if (!ctx.fileExists(path)) {
			push("error", "rule-missing-file", "names " + path + ", which does not exist" +
				(rule.template !== "" ? " (YATI: Open or create file… makes it from the rule's template)." : "."));
			return;
		}
		if (rule.contains.length === 0)
			return;
		const content = env.readText(path);
		if (content === null) {
			push("warning", "rule-unreadable", "names " + path + ", which could not be read.");
			return;
		}
		for (let i = 0; i < rule.contains.length; i++) {
			if (!rule.contains[i].regex.test(content))
				push("error", "rule-missing-content", "names " + baseName(path) + ", which has no " + rule.contains[i].label + ".");
		}
		return;
	}
	// key
	if (!rule.keyPattern.test(text))
		return;
	const tablePath = joinPath(env.projectDir, rule.table);
	const table = env.table(tablePath);
	if (table === null) {
		push("warning", "rule-unreadable", "is '" + text + "', but the table " + tablePath + " could not be read.");
		return;
	}
	if (table.rows[text] === undefined) {
		const near = nearKeys(table, text);
		push("error", "rule-unknown-key", "is '" + text + "', shaped like a key, but " + baseName(tablePath) + " has no such key" +
			(near.length > 0 ? " (did you mean " + near.join(", ") + "?)." : "."));
	}
}

function kindNoun(rule) {
	if (rule.kind === "map")
		return "a map";
	if (rule.kind === "object_in_map" || rule.kind === "object_in_project")
		return "an object's name";
	if (rule.kind === "file")
		return "a file";
	return "a text or key";
}

function checkObjectInMap(rule, name, holder, env, push) {
	const ctx = env.ctx;
	let mapPath = holder.file;
	let objects;
	if (rule.map !== "") {
		const mapProp = findMember(holder.props, rule.map);
		mapPath = memberPath(mapProp, holder.file, ctx);
		if (mapPath === "")
			return;		// unset: the map member's own rule says so when it is required
		if (!ctx.fileExists(mapPath) || !isMapFile(mapPath)) {
			// The map member's own rule reports this; without one, this rule does.
			const covered = holder.mapRules.some(function (r) { return r.kind === "map" && r.member === rule.map; });
			if (!covered)
				push("error", "rule-missing-map", "cannot be checked: " + rule.map + " names " + mapPath + ", which " +
					(ctx.fileExists(mapPath) ? "is no Tiled map (.tmx or .tmj)." : "does not exist."));
			return;
		}
	}
	if (rule.map === "" && holder.sameMap !== null) {
		objects = holder.sameMap;
	} else {
		const read = env.mapObjects(mapPath);
		if (read.error !== "") {
			push("warning", "rule-unreadable", "cannot be checked: " + mapPath + " could not be read (" + read.error + ").");
			return;
		}
		objects = read.objects;
	}
	const named = objects.filter(function (o) { return o.name === name; });
	const matching = rule.targetClass === "" ? named : named.filter(function (o) { return o.className === rule.targetClass; });
	const inMap = rule.map === "" ? "this map" : baseName(mapPath);
	if (matching.length === 0) {
		let text = "names '" + name + "', but " + inMap + " has no object of that name" + (rule.targetClass !== "" ? " and class " + rule.targetClass : "");
		if (named.length > 0)
			text += " (the object '" + name + "' there has class " + (named[0].className === "" ? "none" : "'" + named[0].className + "'") + ")";
		push("error", "rule-missing-object", text + ".");
	} else if (matching.length > 1) {
		push("warning", "rule-ambiguous-object", "names '" + name + "', and " + inMap + " has " + matching.length + " such objects " +
			"(ids " + matching.map(function (o) { return o.id; }).join(", ") + "); give them names of their own.");
	}
}

// The objects of the project named `name`, with the holder's own map as it is now (an open map's
// unsaved objects) in place of its file: [{file, id, name, className, here}].
function projectNamed(rule, name, holder, env) {
	const index = env.projectIndex(rule);
	const here = holder.sameMap !== null ? cleanPath(holder.file) : "";
	const out = [];
	// The saved map counts when the rule looks across the whole project, or its maps hold it.
	if (holder.sameMap !== null && (rule.maps === "" || index.holdsSkipped)) {
		for (let i = 0; i < holder.sameMap.length; i++) {
			const o = holder.sameMap[i];
			if (o.name === name)
				out.push({ file: holder.file, id: o.id, name: name, className: o.className, here: true });
		}
	}
	const listed = index.byName[name] || [];
	for (let i = 0; i < listed.length; i++) {
		if (cleanPath(listed[i].file) !== here || here === "")
			out.push({ file: listed[i].file, id: listed[i].id, name: name, className: listed[i].className, here: false });
	}
	return out;
}

function ofClasses(o, classes) {
	return classes.length === 0 || classes.indexOf("*") >= 0 || classes.indexOf(o.className) >= 0;
}

// "this map (id 3)" or "maps/town.tmx (id 3)".
function placeLabel(o, env, withClass) {
	return (o.here ? "this map" : projectLabel(o.file, env.projectDir)) + " (id " + o.id + (withClass ? ", class " + (o.className === "" ? "none" : o.className) : "") + ")";
}

function projectLabel(file, projectDir) {
	return projectDir !== "" ? relativePath(projectDir, file) : cleanPath(file);
}

function checkObjectInProject(rule, name, holder, env, push) {
	const named = projectNamed(rule, name, holder, env);
	const matching = named.filter(function (o) { return ofClasses(o, rule.targetClasses); });
	const classes = rule.targetClasses.length > 0 ? " and class " + rule.targetClasses.join(" or ") : "";
	if (matching.length === 0) {
		let text = "names '" + name + "', but no map of the project has an object of that name" + classes;
		if (named.length > 0)
			text += " (" + (named[0].here ? "this map" : projectLabel(named[0].file, env.projectDir)) + " has one of class " +
				(named[0].className === "" ? "none" : "'" + named[0].className + "'") + ")";
		push("error", "rule-missing-object", text + ".");
	} else if (matching.length > 1) {
		push("error", "rule-ambiguous-object", "names '" + name + "', which " + matching.length + " objects of the project have: " +
			matching.map(function (o) { return placeLabel(o, env, false); }).join(", ") + "; give them names of their own.");
	}
}

function checkUniqueName(rule, o, cls, holder, env, report) {
	const severity = function (s) { return rule.severity === "warning" ? "warning" : s; };
	const kinds = classList(rule.classes, " and ");
	if (o.name === "") {
		if (rule.required)
			report(severity("error"), "rule-required", holder.where + ": has no name; the project's rules (yati_rules) require one for " + kinds + " objects.");
		return;
	}
	const others = projectNamed(rule, o.name, holder, env).filter(function (x) {
		return ofClasses(x, rule.classes) && !(x.here && x.id === o.id);
	});
	if (others.length === 0)
		return;
	report(severity("error"), "rule-duplicate-name", holder.where + ": the name '" + o.name + "' is also used by " +
		others.map(function (x) { return placeLabel(x, env, true); }).join(", ") +
		"; the project's rules (yati_rules) want the names of " + kinds + " objects unique across the project.");
}

// ---------------------------------------------------------------------------------------------
// The project's maps

// The map files (.tmx, .tmj) a rule looks across, sorted, each once: under each of the project's
// folders (absolute paths, as Tiled's Project.folders gives them) when `spec` is empty; else `spec`,
// relative to the project's directory: a map file, a folder (every map under it), or a glob where
// `*` and `?` stay within a folder and `**` spans folders. Hidden folders are skipped.
// listDir(dir) -> {dirs: [names], files: [names]} or null.
export function projectMapFiles(spec, projectDir, folders, listDir) {
	const isMap = function (name) { return isMapFile(name); };
	const seen = {};
	const out = [];
	const add = function (list) {
		for (let i = 0; i < list.length; i++) {
			const p = cleanPath(list[i]);
			if (!seen[p]) {
				seen[p] = true;
				out.push(p);
			}
		}
	};
	if (spec === "") {
		for (let i = 0; i < folders.length; i++)
			add(walkFiles(folders[i], listDir, isMap));
	} else if (/[*?]/.test(spec)) {
		const pattern = joinPath(projectDir, spec);
		const parts = pattern.split("/");
		let fixed = 0;
		while (fixed < parts.length && !/[*?]/.test(parts[fixed]))
			fixed++;
		const root = parts.slice(0, fixed).join("/") || "/";
		const re = globRegex(pattern);
		add(walkFiles(root, listDir, isMap).filter(function (p) { return re.test(cleanPath(p)); }));
	} else {
		const target = joinPath(projectDir, spec);
		if (isMapFile(target))
			add([target]);		// one map (one that is not there is reported as unreadable)
		else
			add(walkFiles(target, listDir, isMap));
	}
	return out.sort();
}

function globRegex(pattern) {
	let re = "";
	for (let i = 0; i < pattern.length; i++) {
		const c = pattern.charAt(i);
		if (c === "*" && pattern.charAt(i + 1) === "*") {
			if (pattern.charAt(i + 2) === "/") {
				re += "(?:[^/]*/)*";
				i += 2;
			} else {
				re += ".*";
				i += 1;
			}
		} else if (c === "*") {
			re += "[^/]*";
		} else if (c === "?") {
			re += "[^/]";
		} else {
			re += c.replace(/[.+^${}()|[\]\\]/g, "\\$&");
		}
	}
	return new RegExp("^" + re + "$");
}

// The named objects of the maps by name: {byName: {name: [{file, id, className}]}, errors: [{file, error}]}.
// mapObjects(path) -> {objects: [{id, name, className}], error}, as checkRules' context gives it.
export function projectObjects(files, mapObjects) {
	const byName = {};
	const errors = [];
	for (let i = 0; i < files.length; i++) {
		const read = mapObjects(files[i]);
		if (read.error !== "") {
			errors.push({ file: files[i], error: read.error });
			continue;
		}
		for (let k = 0; k < read.objects.length; k++) {
			const o = read.objects[k];
			if (o.name === "")
				continue;
			if (!Object.prototype.hasOwnProperty.call(byName, o.name))
				byName[o.name] = [];
			byName[o.name].push({ file: files[i], id: o.id, className: o.className });
		}
	}
	return { byName: byName, errors: errors };
}

// What "Pick map and object" offers for an object_in_project rule: every named object of the rule's
// classes in the project's maps, the map `currentFile` as `currentObjects` hold it ([{id, name,
// className}], an open map's objects), sorted by map then name: [{label: "maps/town.tmx: gate
// (Warp)", name, file, id}].
export function projectObjectChoices(rule, ctx, currentFile, currentObjects) {
	const projectDir = ctx.projectFile ? dirName(ctx.projectFile) : "";
	const files = projectMapFiles(rule.maps, projectDir, ctx.projectFolders || [], ctx.listDir);
	const here = currentFile ? cleanPath(currentFile) : "";
	const out = [];
	const push = function (file, o) {
		if (o.name !== "" && ofClasses(o, rule.targetClasses))
			out.push({ label: projectLabel(file, projectDir) + ": " + o.name + (o.className !== "" ? " (" + o.className + ")" : ""), name: o.name, file: file, id: o.id });
	};
	for (let i = 0; i < files.length; i++) {
		if (files[i] === here)
			continue;
		const read = ctx.mapObjects(files[i]);
		for (let k = 0; k < read.objects.length; k++)
			push(files[i], read.objects[k]);
	}
	if (here !== "") {
		for (let k = 0; k < currentObjects.length; k++)
			push(here, currentObjects[k]);
	}
	return out.sort(function (a, b) {
		const fa = projectLabel(a.file, projectDir);
		const fb = projectLabel(b.file, projectDir);
		if (fa !== fb)
			return fa < fb ? -1 : 1;
		return a.name < b.name ? -1 : (a.name > b.name ? 1 : a.id - b.id);
	});
}

// Where an object_in_project rule's value points: the objects of that name and the rule's classes,
// the open map `currentFile` as `currentObjects` hold it: [{file, id}].
export function findProjectObject(rule, name, ctx, currentFile, currentObjects) {
	return projectObjectChoices(rule, ctx, currentFile, currentObjects).filter(function (c) { return c.name === name; });
}

// The names to offer for an object_in_map rule from a map's objects: those of the rule's class
// with a name, sorted, each once.
export function objectNames(objects, targetClass) {
	const seen = {};
	const out = [];
	for (let i = 0; i < objects.length; i++) {
		const o = objects[i];
		if (o.name === "" || (targetClass !== "" && o.className !== targetClass) || seen[o.name])
			continue;
		seen[o.name] = true;
		out.push(o.name);
	}
	return out.sort();
}

// ---------------------------------------------------------------------------------------------
// Files: making one from a template, opening one

// Where "Open or create file" puts a new copy of the rule's template for an object: in the rule's
// folder (relative to the project file), else beside the map; named after the map and the object
// ("town_well.xml", or "town_trigger_7.xml" for an unnamed object), with the template's suffix.
export function newFilePath(rule, object, mapFile, projectDir) {
	const folder = rule.folder !== "" ? joinPath(projectDir, rule.folder) : dirName(mapFile);
	const tpl = baseName(rule.template);
	const dot = tpl.lastIndexOf(".");
	const suffix = dot > 0 ? tpl.substring(dot) : "";
	const own = object.name !== "" ? object.name : (object.className !== "" ? object.className : "object") + "_" + object.id;
	const name = (stem(mapFile) + "_" + own).replace(/[^A-Za-z0-9_.-]+/g, "_");
	return joinPath(folder, name + suffix);
}

// The commands that open a file with the program the system gives its type, tried in order until
// one starts: `custom` (the project property yati_open_command: a program and its arguments,
// split at spaces, "quoted" parts kept whole) with the file after them; else on Windows (a path
// with a drive letter) cmd's start, else xdg-open (Linux and the BSDs), then open (macOS).
export function openCommands(custom, file) {
	const c = String(custom === undefined || custom === null ? "" : custom).trim();
	if (c !== "") {
		const words = splitCommand(c);
		return [{ program: words[0], args: words.slice(1).concat([file]) }];
	}
	if (/^[A-Za-z]:\//.test(file) || isWindowsPath(file))
		return [{ program: "cmd", args: ["/c", "start", "", file.replace(/\//g, "\\")] }];
	return [{ program: "xdg-open", args: [file] }, { program: "open", args: [file] }];
}

function isWindowsPath(p) {
	return /^[A-Za-z]:\\/.test(p) || /^\\\\/.test(p);
}

function splitCommand(text) {
	const out = [];
	const re = /"([^"]*)"|(\S+)/g;
	let m;
	while ((m = re.exec(text)) !== null)
		out.push(m[1] !== undefined ? m[1] : m[2]);
	return out;
}
