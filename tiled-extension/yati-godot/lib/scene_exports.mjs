// Godot scenes and scripts as text: a .tscn's root node and its script, a .gd file's class_name,
// extends, enums and @export declarations, GDScript literals. Tolerant: what it does not understand
// is skipped (and, for exports, reported), never fatal. Pure: file access comes in as functions.

import { cleanPath, joinPath, dirName, baseName, stem, godotToLocal } from "./settings.mjs";

// "big_slime" -> "BigSlime", "hud-panel 2" -> "HudPanel2": a scene's or script's file name as a
// type name.
export function pascalCase(name) {
	const parts = String(name).split(/[^A-Za-z0-9]+/);
	let out = "";
	for (let i = 0; i < parts.length; i++) {
		if (parts[i] === "")
			continue;
		out += parts[i].charAt(0).toUpperCase() + parts[i].substring(1);
	}
	if (out === "" || /^[0-9]/.test(out))
		out = "Scene" + out;
	return out;
}

// ---------------------------------------------------------------------------------------------
// .tscn

// The attributes of a section header: [ext_resource type="Script" path="res://a.gd" id="1_x"] ->
// {type: "Script", path: "res://a.gd", id: "1_x"}; unquoted values (id=1, Godot 3) as written.
export function parseHeaderAttributes(header) {
	const attrs = {};
	const re = /([A-Za-z_][A-Za-z0-9_]*)\s*=\s*("((?:[^"\\]|\\.)*)"|[A-Za-z_][A-Za-z0-9_]*\([^)]*\)|[^\s\]]+)/g;
	let m;
	while ((m = re.exec(header)) !== null)
		attrs[m[1]] = m[3] !== undefined ? m[3] : m[2];
	return attrs;
}

// The id inside ExtResource("1_x") / ExtResource( 1 ) / SubResource("GDScript_x").
function resourceRef(value) {
	const m = /^(Ext|Sub)Resource\(\s*"?([^")\s]*)"?\s*\)$/.exec(String(value).trim());
	return m ? { kind: m[1] === "Ext" ? "ext" : "sub", id: m[2] } : null;
}

// {extResources: {id: {type, path, uid}}, root: {name, type, script, instance} | null,
//  rootScript: "res://..." | null, rootInstance: "res://..." | null, builtinScript: bool}
export function parseTscn(text) {
	const ext = {};
	let root = null;
	let inRoot = false;
	const lines = String(text).split(/\r?\n/);
	for (let i = 0; i < lines.length; i++) {
		const line = lines[i].trim();
		if (line.charAt(0) === "[") {
			inRoot = false;
			const head = /^\[([A-Za-z_]+)\s*(.*)\]$/.exec(line);
			if (!head)
				continue;
			const attrs = parseHeaderAttributes(head[2]);
			if (head[1] === "ext_resource" && attrs.id !== undefined) {
				ext[attrs.id] = { type: attrs.type || "", path: attrs.path || "", uid: attrs.uid || "" };
			} else if (head[1] === "node" && root === null && attrs.parent === undefined) {
				root = { name: attrs.name || "", type: attrs.type || "", script: null, instance: null };
				const inst = attrs.instance !== undefined ? resourceRef(attrs.instance) : null;
				if (inst && inst.kind === "ext")
					root.instance = inst.id;
				inRoot = true;
			}
			continue;
		}
		if (!inRoot)
			continue;
		const prop = /^script\s*=\s*(.+)$/.exec(line);
		if (prop) {
			const ref = resourceRef(prop[1]);
			root.script = ref;
		}
	}
	let rootScript = null;
	let rootInstance = null;
	let builtin = false;
	if (root !== null) {
		if (root.script && root.script.kind === "ext" && ext[root.script.id])
			rootScript = ext[root.script.id].path;
		else if (root.script && root.script.kind === "sub")
			builtin = true;
		if (root.instance !== null && ext[root.instance])
			rootInstance = ext[root.instance].path;
	}
	return { extResources: ext, root: root, rootScript: rootScript, rootInstance: rootInstance, builtinScript: builtin };
}

// ---------------------------------------------------------------------------------------------
// .gd

// The line without its comment (a "#" outside strings).
function stripComment(line) {
	let quote = "";
	for (let i = 0; i < line.length; i++) {
		const c = line.charAt(i);
		if (quote !== "") {
			if (c === "\\")
				i++;
			else if (c === quote)
				quote = "";
			continue;
		}
		if (c === "\"" || c === "'")
			quote = c;
		else if (c === "#")
			return line.substring(0, i);
	}
	return line;
}

function depthOf(text) {
	let depth = 0;
	let quote = "";
	for (let i = 0; i < text.length; i++) {
		const c = text.charAt(i);
		if (quote !== "") {
			if (c === "\\")
				i++;
			else if (c === quote)
				quote = "";
			continue;
		}
		if (c === "\"" || c === "'")
			quote = c;
		else if (c === "(" || c === "[" || c === "{")
			depth++;
		else if (c === ")" || c === "]" || c === "}")
			depth--;
	}
	return depth;
}

// The top-level logical lines of a script: comments stripped, continued lines (open brackets, a
// trailing backslash) joined; each {text, indented, line} (line: 1-based number of its first line).
export function logicalLines(text) {
	const raw = String(text).split(/\r?\n/);
	const out = [];
	let i = 0;
	while (i < raw.length) {
		const start = i;
		const indented = /^[ \t]/.test(raw[i]) && raw[i].trim() !== "";
		let s = stripComment(raw[i]);
		i++;
		while (i < raw.length && (depthOf(s) > 0 || /\\\s*$/.test(s))) {
			s = s.replace(/\\\s*$/, "").replace(/\s+$/, "") + " " + stripComment(raw[i]).trim();
			i++;
		}
		s = s.trim();
		if (s !== "")
			out.push({ text: s, indented: indented, line: start + 1 });
	}
	return out;
}

// Splits "a, b(c, d), "e,f"" at top-level commas.
export function splitTopLevel(text, sep) {
	const parts = [];
	let depth = 0;
	let quote = "";
	let cur = "";
	for (let i = 0; i < text.length; i++) {
		const c = text.charAt(i);
		if (quote !== "") {
			cur += c;
			if (c === "\\" && i + 1 < text.length) {
				cur += text.charAt(i + 1);
				i++;
			} else if (c === quote) {
				quote = "";
			}
			continue;
		}
		if (c === "\"" || c === "'") {
			quote = c;
			cur += c;
			continue;
		}
		if (c === "(" || c === "[" || c === "{")
			depth++;
		else if (c === ")" || c === "]" || c === "}")
			depth--;
		if (c === sep && depth === 0) {
			parts.push(cur.trim());
			cur = "";
			continue;
		}
		cur += c;
	}
	if (cur.trim() !== "")
		parts.push(cur.trim());
	return parts;
}

// The index of the first `ch` at bracket depth 0 outside strings, from `from`, or -1.
function indexTopLevel(text, ch, from) {
	let depth = 0;
	let quote = "";
	for (let i = from || 0; i < text.length; i++) {
		const c = text.charAt(i);
		if (quote !== "") {
			if (c === "\\")
				i++;
			else if (c === quote)
				quote = "";
			continue;
		}
		if (c === "\"" || c === "'")
			quote = c;
		else if (c === "(" || c === "[" || c === "{")
			depth++;
		else if (c === ")" || c === "]" || c === "}")
			depth--;
		else if (c === ch && depth === 0)
			return i;
	}
	return -1;
}

// Leading annotations of a line: "@export_range(0, 10) @onready var x" ->
// {annotations: [{name: "export_range", args: ["0", "10"]}, {name: "onready", args: []}], rest: "var x"}.
export function takeAnnotations(text) {
	const annotations = [];
	let s = text.trim();
	while (s.charAt(0) === "@") {
		const m = /^@([A-Za-z_][A-Za-z0-9_]*)/.exec(s);
		if (!m)
			break;
		let rest = s.substring(m[0].length);
		let args = [];
		if (rest.charAt(0) === "(") {
			let depth = 0;
			let quote = "";
			let end = -1;
			for (let i = 0; i < rest.length; i++) {
				const c = rest.charAt(i);
				if (quote !== "") {
					if (c === "\\")
						i++;
					else if (c === quote)
						quote = "";
					continue;
				}
				if (c === "\"" || c === "'")
					quote = c;
				else if (c === "(")
					depth++;
				else if (c === ")") {
					depth--;
					if (depth === 0) {
						end = i;
						break;
					}
				}
			}
			if (end < 0)
				break;
			args = splitTopLevel(rest.substring(1, end), ",");
			rest = rest.substring(end + 1);
		}
		annotations.push({ name: m[1], args: args });
		s = rest.trim();
	}
	return { annotations: annotations, rest: s };
}

// "var x: int = 3: set = _set" / "var x := 3" / "var x" -> {name, type, inferred, defaultText}.
export function parseVarDeclaration(text) {
	const m = /^(?:static\s+)?var\s+([A-Za-z_][A-Za-z0-9_]*)\s*(.*)$/.exec(text.trim());
	if (!m)
		return null;
	let rest = m[2].trim();
	// A property block or inline setter/getter after the declaration.
	const setter = /:\s*(set|get)\b/.exec(rest);
	if (setter) {
		const cut = indexTopLevel(rest, ":", setter.index);
		if (cut === setter.index)
			rest = rest.substring(0, setter.index).trim();
	}
	if (rest.charAt(rest.length - 1) === ":" && rest.charAt(rest.length - 2) !== ":")
		rest = rest.substring(0, rest.length - 1).trim();
	let type = "";
	let inferred = false;
	let defaultText = "";
	if (rest.indexOf(":=") === 0) {
		inferred = true;
		defaultText = rest.substring(2).trim();
	} else if (rest.charAt(0) === ":") {
		const eq = indexTopLevel(rest, "=", 1);
		if (eq < 0) {
			type = rest.substring(1).trim();
		} else {
			type = rest.substring(1, eq).trim();
			defaultText = rest.substring(eq + 1).trim();
		}
	} else if (rest.charAt(0) === "=") {
		defaultText = rest.substring(1).trim();
	}
	return { name: m[1], type: type, inferred: inferred, defaultText: defaultText };
}

// The integer an enum value expression denotes: 5, -2, 0x10, 0b101, 1_000, 1 << 3; or null.
export function parseIntExpression(text) {
	const s = String(text).replace(/_/g, "").trim();
	let m = /^(-?)(0x[0-9a-fA-F]+|0b[01]+|\d+)$/.exec(s);
	if (m) {
		let v;
		if (m[2].indexOf("0x") === 0)
			v = parseInt(m[2].substring(2), 16);
		else if (m[2].indexOf("0b") === 0)
			v = parseInt(m[2].substring(2), 2);
		else
			v = parseInt(m[2], 10);
		return m[1] === "-" ? -v : v;
	}
	m = /^(\d+)\s*<<\s*(\d+)$/.exec(s);
	if (m)
		return parseInt(m[1], 10) * Math.pow(2, parseInt(m[2], 10));
	return null;
}

// "enum Mood { CALM, ANGRY = 5 }" -> {name: "Mood", values: [{name: "CALM", value: 0}, ...]};
// an anonymous enum has name "".
export function parseEnum(text) {
	const m = /^enum\s*([A-Za-z_][A-Za-z0-9_]*)?\s*\{([\s\S]*)\}\s*$/.exec(text.trim());
	if (!m)
		return null;
	const values = [];
	let next = 0;
	const parts = splitTopLevel(m[2], ",");
	for (let i = 0; i < parts.length; i++) {
		const kv = /^([A-Za-z_][A-Za-z0-9_]*)\s*(?:=\s*(.+))?$/.exec(parts[i].trim());
		if (!kv)
			continue;
		let value = next;
		if (kv[2] !== undefined) {
			const v = parseIntExpression(kv[2]);
			if (v !== null)
				value = v;
		}
		values.push({ name: kv[1], value: value });
		next = value + 1;
	}
	return { name: m[1] || "", values: values };
}

// Export annotations that do not make an inspector property a map can set.
const SKIPPED_EXPORTS = { export_storage: true, export_tool_button: true };
// Annotations that only group the inspector.
const GROUPING = { export_group: true, export_subgroup: true, export_category: true };

// {className, extendsName, extendsPath, enums: {Name: [{name, value}]}, exports: [...], skipped: [...]}
// Each export: {name, type (as written, "" when untyped), inferred, defaultText, annotation (the
// export annotation's name), args, line}. `skipped`: [{name, line, reason}].
export function parseGdScript(text) {
	const result = {
		className: "",
		extendsName: "",
		extendsPath: "",
		enums: {},
		exports: [],
		skipped: [],
	};
	let pending = [];
	const lines = logicalLines(text);
	for (let i = 0; i < lines.length; i++) {
		const ll = lines[i];
		if (ll.indented) {
			pending = [];
			continue;
		}
		const taken = takeAnnotations(ll.text);
		let annotations = taken.annotations;
		const rest = taken.rest;
		let grouping = false;
		for (let a = 0; a < annotations.length; a++) {
			if (GROUPING[annotations[a].name])
				grouping = true;
		}
		if (grouping) {
			pending = [];
			continue;
		}
		if (rest === "") {
			pending = pending.concat(annotations);
			continue;
		}
		annotations = pending.concat(annotations);
		pending = [];
		let m = /^class_name\s+([A-Za-z_][A-Za-z0-9_]*)(?:\s+extends\s+(.+))?$/.exec(rest);
		if (m) {
			result.className = m[1];
			if (m[2] !== undefined)
				setExtends(result, m[2]);
			continue;
		}
		m = /^extends\s+(.+?)(?:\s+class_name\s+([A-Za-z_][A-Za-z0-9_]*))?$/.exec(rest);
		if (m) {
			setExtends(result, m[1]);
			if (m[2] !== undefined)
				result.className = m[2];
			continue;
		}
		if (/^enum\b/.test(rest)) {
			const e = parseEnum(rest);
			if (e !== null && e.name !== "")
				result.enums[e.name] = e.values;
			continue;
		}
		if (/^(static\s+)?var\s/.test(rest)) {
			let exp = null;
			for (let a = 0; a < annotations.length; a++) {
				if (annotations[a].name.indexOf("export") === 0)
					exp = annotations[a];
			}
			if (exp === null)
				continue;
			const decl = parseVarDeclaration(rest);
			if (decl === null)
				continue;
			if (SKIPPED_EXPORTS[exp.name]) {
				result.skipped.push({ name: decl.name, line: ll.line, reason: "@" + exp.name + " is no inspector property" });
				continue;
			}
			result.exports.push({
				name: decl.name,
				type: decl.type,
				inferred: decl.inferred,
				defaultText: decl.defaultText,
				annotation: exp.name,
				args: exp.args,
				line: ll.line,
			});
		}
	}
	return result;
}

function setExtends(result, text) {
	const t = text.trim();
	const s = /^"([^"]*)"|^'([^']*)'/.exec(t);
	if (s) {
		result.extendsPath = s[1] !== undefined ? s[1] : s[2];
		// extends "res://base.gd".Inner is not followed.
	} else {
		result.extendsName = t.split(/\s/)[0];
	}
}

// ---------------------------------------------------------------------------------------------
// GDScript literals (an export's default)

const NAMED_COLORS = {
	WHITE: [1, 1, 1, 1], BLACK: [0, 0, 0, 1], TRANSPARENT: [1, 1, 1, 0], RED: [1, 0, 0, 1],
	GREEN: [0, 1, 0, 1], BLUE: [0, 0, 1, 1], YELLOW: [1, 1, 0, 1], CYAN: [0, 1, 1, 1],
	MAGENTA: [1, 0, 1, 1], GRAY: [0.745098, 0.745098, 0.745098, 1], ORANGE: [1, 0.647059, 0, 1],
	PURPLE: [0.627451, 0.12549, 0.941176, 1], PINK: [1, 0.752941, 0.796078, 1],
	BROWN: [0.647059, 0.164706, 0.164706, 1], DARK_GRAY: [0.662745, 0.662745, 0.662745, 1],
	LIGHT_GRAY: [0.827451, 0.827451, 0.827451, 1], NAVY_BLUE: [0, 0, 0.501961, 1],
	LIME: [0, 1, 0, 1], SILVER: [0.752941, 0.752941, 0.752941, 1], GOLD: [1, 0.843137, 0, 1],
	TEAL: [0, 0.501961, 0.501961, 1],
};

const VECTOR_CONSTANTS = {
	ZERO: [0, 0], ONE: [1, 1], LEFT: [-1, 0], RIGHT: [1, 0], UP: [0, -1], DOWN: [0, 1],
};

function parseNumber(text) {
	const s = String(text).replace(/_/g, "").trim();
	if (/^[-+]?(\d+\.?\d*|\.\d+)([eE][-+]?\d+)?$/.test(s))
		return parseFloat(s);
	const i = parseIntExpression(s);
	return i;
}

// A GDScript literal as {kind, value}: int, float, bool, string (also &"name" and ^"path"), null,
// color ({r, g, b, a} 0..1), vector2 / vector2i ({x, y}), resource (a preload/load path),
// enumref ({path: ["Mood", "CALM"]}), array, dictionary, or unknown (an expression).
export function parseLiteral(text) {
	const s = String(text === undefined ? "" : text).trim();
	if (s === "")
		return { kind: "none" };
	if (s === "true" || s === "false")
		return { kind: "bool", value: s === "true" };
	if (s === "null")
		return { kind: "null" };
	let m = /^([&^]?)("((?:[^"\\]|\\.)*)"|'((?:[^'\\]|\\.)*)')$/.exec(s);
	if (m) {
		const body = m[3] !== undefined ? m[3] : m[4];
		const value = body.replace(/\\(.)/g, function (all, c) {
			if (c === "n") return "\n";
			if (c === "t") return "\t";
			return c;
		});
		return { kind: "string", value: value, prefix: m[1] };
	}
	if (/^[-+]?[\d_]*\.[\d_]*([eE][-+]?\d+)?$/.test(s) || /^[-+]?[\d_]+[eE][-+]?\d+$/.test(s) ||
		s === "INF" || s === "-INF" || s === "NAN") {
		const f = parseNumber(s);
		if (f !== null && !isNaN(f))
			return { kind: "float", value: f };
	}
	if (/^[-+]?(0x[0-9a-fA-F_]+|0b[01_]+|[\d_]+)$/.test(s)) {
		const v = parseIntExpression(s.replace(/^\+/, ""));
		if (v !== null)
			return { kind: "int", value: v };
	}
	m = /^Color\((.*)\)$/.exec(s);
	if (m) {
		const args = splitTopLevel(m[1], ",");
		if (args.length === 1) {
			const str = parseLiteral(args[0]);
			if (str.kind === "string") {
				const c = colorFromHtml(str.value);
				if (c !== null)
					return { kind: "color", value: c };
			}
			return { kind: "unknown", text: s };
		}
		if (args.length === 0)
			return { kind: "color", value: { r: 0, g: 0, b: 0, a: 1 } };
		const nums = args.map(parseNumber);
		if ((nums.length === 3 || nums.length === 4) && nums.every(function (n) { return n !== null && !isNaN(n); }))
			return { kind: "color", value: { r: nums[0], g: nums[1], b: nums[2], a: nums.length === 4 ? nums[3] : 1 } };
		return { kind: "unknown", text: s };
	}
	m = /^Color8\((.*)\)$/.exec(s);
	if (m) {
		const nums = splitTopLevel(m[1], ",").map(parseNumber);
		if ((nums.length === 3 || nums.length === 4) && nums.every(function (n) { return n !== null; }))
			return { kind: "color", value: { r: nums[0] / 255, g: nums[1] / 255, b: nums[2] / 255, a: nums.length === 4 ? nums[3] / 255 : 1 } };
		return { kind: "unknown", text: s };
	}
	m = /^Color\.([A-Z_]+)$/.exec(s);
	if (m && NAMED_COLORS[m[1]]) {
		const c = NAMED_COLORS[m[1]];
		return { kind: "color", value: { r: c[0], g: c[1], b: c[2], a: c[3] } };
	}
	m = /^(Vector2i?)\((.*)\)$/.exec(s);
	if (m) {
		const args = splitTopLevel(m[2], ",").map(parseNumber);
		const kind = m[1] === "Vector2" ? "vector2" : "vector2i";
		if (args.length === 0)
			return { kind: kind, value: { x: 0, y: 0 } };
		if (args.length === 2 && args[0] !== null && args[1] !== null && !isNaN(args[0]) && !isNaN(args[1]))
			return { kind: kind, value: { x: args[0], y: args[1] } };
		return { kind: "unknown", text: s };
	}
	m = /^(Vector2i?)\.([A-Z_]+)$/.exec(s);
	if (m && VECTOR_CONSTANTS[m[2]]) {
		const v = VECTOR_CONSTANTS[m[2]];
		return { kind: m[1] === "Vector2" ? "vector2" : "vector2i", value: { x: v[0], y: v[1] } };
	}
	m = /^(preload|load)\(\s*("([^"]*)"|'([^']*)')\s*\)$/.exec(s);
	if (m)
		return { kind: "resource", value: m[3] !== undefined ? m[3] : m[4] };
	if (/^[A-Za-z_][A-Za-z0-9_]*(\.[A-Za-z_][A-Za-z0-9_]*)*$/.test(s))
		return { kind: "enumref", path: s.split(".") };
	if (/^\[.*\]$/.test(s) || /^Array(\[.*\])?\(.*\)$/.test(s) || /^Packed[A-Za-z0-9]*Array\(.*\)$/.test(s))
		return { kind: "array", text: s };
	if (/^\{.*\}$/.test(s) || /^Dictionary(\[.*\])?\(.*\)$/.test(s))
		return { kind: "dictionary", text: s };
	return { kind: "unknown", text: s };
}

// "#rgb", "#rrggbb", "#rrggbbaa" (Godot's html form, alpha last), without or with "#"; or null.
export function colorFromHtml(text) {
	let h = String(text).trim().replace(/^#/, "");
	if (!/^[0-9a-fA-F]+$/.test(h))
		return null;
	if (h.length === 3 || h.length === 4)
		h = h.split("").map(function (c) { return c + c; }).join("");
	if (h.length !== 6 && h.length !== 8)
		return null;
	const r = parseInt(h.substring(0, 2), 16) / 255;
	const g = parseInt(h.substring(2, 4), 16) / 255;
	const b = parseInt(h.substring(4, 6), 16) / 255;
	const a = h.length === 8 ? parseInt(h.substring(6, 8), 16) / 255 : 1;
	return { r: r, g: g, b: b, a: a };
}

// ---------------------------------------------------------------------------------------------
// Scanning a Godot project

// Every file under `root` whose name passes `accept`, recursively. `listDir(dir)` returns
// {dirs: [names], files: [names]}; `skipDir(path, name)` prunes a directory. Sorted, for stable output.
export function walkFiles(root, listDir, accept, skipDir) {
	const out = [];
	const stack = [cleanPath(root)];
	const seen = {};
	while (stack.length > 0) {
		const dir = stack.pop();
		if (seen[dir])
			continue;
		seen[dir] = true;
		const entries = listDir(dir);
		if (!entries)
			continue;
		const files = entries.files || [];
		if (files.indexOf(".gdignore") >= 0)
			continue;
		for (let i = 0; i < files.length; i++) {
			if (accept(files[i]))
				out.push(joinPath(dir, files[i]));
		}
		const dirs = (entries.dirs || []).slice().sort().reverse();
		for (let i = 0; i < dirs.length; i++) {
			const name = dirs[i];
			if (name === "." || name === ".." || name.charAt(0) === ".")
				continue;
			const path = joinPath(dir, name);
			if (skipDir && skipDir(path, name))
				continue;
			stack.push(path);
		}
	}
	return out.sort();
}

// The scripts of the project indexed by class_name: {Name: {path (local), script (parsed)}}.
// `scripts`: [{path, text}].
export function indexClassNames(scripts) {
	const index = {};
	for (let i = 0; i < scripts.length; i++) {
		const parsed = parseGdScript(scripts[i].text);
		if (parsed.className !== "" && index[parsed.className] === undefined)
			index[parsed.className] = { path: scripts[i].path, script: parsed };
	}
	return index;
}

// A script with what it inherits: {script, path, chain: [paths, base first], exports (base first,
// a redeclared name keeps the derived one), enums (own over inherited), engineBase (the first
// non-script ancestor's name, "" when unknown)}. `loadScript(localPath)` -> parsed script or null.
export function resolveScript(path, godotProjectDir, loadScript, classIndex) {
	const chain = [];
	const scripts = [];
	let current = { path: path, script: loadScript(path) };
	let engineBase = "";
	let guard = 0;
	while (current !== null && current.script !== null && guard < 16) {
		guard++;
		chain.unshift(current.path);
		scripts.unshift(current.script);
		const s = current.script;
		if (s.extendsPath !== "") {
			const p = s.extendsPath.indexOf("res://") === 0
				? godotToLocal(godotProjectDir, s.extendsPath)
				: joinPath(dirName(current.path), s.extendsPath);
			current = { path: p, script: loadScript(p) };
		} else if (s.extendsName !== "" && classIndex[s.extendsName]) {
			const entry = classIndex[s.extendsName];
			current = { path: entry.path, script: entry.script };
		} else {
			engineBase = s.extendsName !== "" ? s.extendsName : (s.extendsPath === "" ? "RefCounted" : "");
			current = null;
		}
	}
	const exportsByName = {};
	const order = [];
	const enums = {};
	const enumOwners = {};
	for (let i = 0; i < scripts.length; i++) {
		// The name a script's enums are qualified with: its class_name, else its file's name.
		const owner = scripts[i].className !== "" ? scripts[i].className : pascalCase(stem(chain[i]));
		for (let e = 0; e < scripts[i].exports.length; e++) {
			const ex = scripts[i].exports[e];
			if (exportsByName[ex.name] === undefined)
				order.push(ex.name);
			const copy = {};
			const keys = Object.keys(ex);
			for (let k = 0; k < keys.length; k++)
				copy[keys[k]] = ex[keys[k]];
			copy.owner = owner;
			exportsByName[ex.name] = copy;
		}
		const names = Object.keys(scripts[i].enums);
		for (let n = 0; n < names.length; n++) {
			enums[names[n]] = scripts[i].enums[names[n]];
			enumOwners[names[n]] = owner;
		}
	}
	return {
		path: path,
		script: scripts.length > 0 ? scripts[scripts.length - 1] : null,
		chain: chain,
		exports: order.map(function (n) { return exportsByName[n]; }),
		enums: enums,
		enumOwners: enumOwners,
		engineBase: engineBase,
	};
}

// The engine class at the root of a class_name's inheritance ("Arena" -> "StaticBody2D"), or the
// name itself when it is no script class.
export function engineClassOf(name, classIndex) {
	let n = name;
	let guard = 0;
	while (classIndex[n] && guard < 16) {
		guard++;
		const s = classIndex[n].script;
		if (s.extendsName === "")
			return s.extendsPath === "" ? "RefCounted" : "";
		n = s.extendsName;
	}
	return n;
}

// The scenes under `scenesDir` with a script on their root, resolved:
// {scenes: [{scenePath, sceneRes, scriptPath, scriptRes, className, resolved}], skipped: [{scenePath, reason}]}.
// io: {listDir(dir), readText(path) -> string | null}. A scene that inherits another (root
// `instance=`) without a script of its own takes the inherited scene's root script.
export function scanScenes(godotProjectDir, scenesDir, io, classIndex) {
	const projectRoot = cleanPath(godotProjectDir);
	const scanRoot = cleanPath(scenesDir);
	const files = walkFiles(scanRoot, io.listDir,
		function (name) { return /\.tscn$/i.test(name); },
		function (path, name) {
			// The project's addons are other people's scenes (editor docks...): scanned only when
			// godot_scenes_dir points into them.
			return name === "addons" && dirName(path) === projectRoot && scanRoot === projectRoot;
		});
	const cache = {};
	const loadScript = function (p) {
		if (cache[p] !== undefined)
			return cache[p];
		const text = io.readText(p);
		cache[p] = text === null ? null : parseGdScript(text);
		return cache[p];
	};
	const scenes = [];
	const skipped = [];
	for (let i = 0; i < files.length; i++) {
		const scenePath = files[i];
		let tscn = parseTscn(io.readText(scenePath) || "");
		let scriptRes = tscn.rootScript;
		let builtin = tscn.builtinScript;
		let hops = 0;
		while (scriptRes === null && !builtin && tscn.rootInstance !== null && hops < 8) {
			hops++;
			const base = godotToLocal(projectRoot, tscn.rootInstance);
			const text = io.readText(base);
			if (text === null)
				break;
			tscn = parseTscn(text);
			scriptRes = tscn.rootScript;
			builtin = tscn.builtinScript;
		}
		const sceneRes = "res://" + scenePath.substring(projectRoot.length + 1);
		if (tscn.root === null) {
			skipped.push({ scenePath: scenePath, sceneRes: sceneRes, reason: "no root node" });
			continue;
		}
		if (builtin) {
			skipped.push({ scenePath: scenePath, sceneRes: sceneRes, reason: "its root's script is built into the scene" });
			continue;
		}
		if (scriptRes === null) {
			skipped.push({ scenePath: scenePath, sceneRes: sceneRes, reason: "its root has no script" });
			continue;
		}
		const scriptPath = scriptRes.indexOf("res://") === 0
			? godotToLocal(projectRoot, scriptRes)
			: joinPath(dirName(scenePath), scriptRes);
		if (loadScript(scriptPath) === null) {
			skipped.push({ scenePath: scenePath, sceneRes: sceneRes, reason: "its script " + scriptRes + " cannot be read" });
			continue;
		}
		const resolved = resolveScript(scriptPath, projectRoot, loadScript, classIndex);
		scenes.push({
			scenePath: scenePath,
			sceneRes: sceneRes,
			sceneName: baseName(scenePath),
			rootType: tscn.root.type,
			scriptPath: scriptPath,
			scriptRes: scriptRes,
			className: resolved.script.className,
			resolved: resolved,
		});
	}
	return { scenes: scenes, skipped: skipped };
}
