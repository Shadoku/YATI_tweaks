// project.godot: Godot's INI-like ConfigFile text. Read for the layer names under [layer_names]
// (2d_physics/layer_N, 2d_navigation/layer_N, 2d_render/layer_N) and the project's name. Tolerant:
// a line it does not understand is skipped; a value that opens a bracket ({ [ () continues on the
// following lines until it closes (the [input] section's events are written that way).

// The layer families the importers read masks of, with Godot's number of layers for each.
export const LAYER_FAMILIES = [
	{ key: "2d_physics", enumName: "GodotPhysicsLayers", count: 32 },
	{ key: "2d_navigation", enumName: "GodotNavigationLayers", count: 32 },
	{ key: "2d_render", enumName: "GodotRenderLayers", count: 20 },
];

// {sections: {name: {key: rawValue}}, order: [section names]}; keys before any section header
// land in the section "" (config_version).
export function parseConfigFile(text) {
	const sections = { "": {} };
	const order = [""];
	let current = "";
	const lines = String(text).split(/\r?\n/);
	let i = 0;
	while (i < lines.length) {
		const line = lines[i].trim();
		i++;
		if (line === "" || line.charAt(0) === ";" || line.charAt(0) === "#")
			continue;
		const header = /^\[([^\]]*)\]$/.exec(line);
		if (header) {
			current = header[1].trim();
			if (sections[current] === undefined) {
				sections[current] = {};
				order.push(current);
			}
			continue;
		}
		const eq = line.indexOf("=");
		if (eq <= 0)
			continue;
		const key = line.substring(0, eq).trim();
		let value = line.substring(eq + 1).trim();
		let depth = bracketDepth(value);
		while (depth > 0 && i < lines.length) {
			value += "\n" + lines[i];
			depth = bracketDepth(value);
			i++;
		}
		sections[current][key] = value;
	}
	return { sections: sections, order: order };
}

// How many brackets the text leaves open, ignoring those inside "strings".
function bracketDepth(text) {
	let depth = 0;
	let inString = false;
	for (let i = 0; i < text.length; i++) {
		const c = text.charAt(i);
		if (inString) {
			if (c === "\\")
				i++;
			else if (c === "\"")
				inString = false;
			continue;
		}
		if (c === "\"")
			inString = true;
		else if (c === "{" || c === "[" || c === "(")
			depth++;
		else if (c === "}" || c === "]" || c === ")")
			depth--;
	}
	return depth;
}

// A Godot string literal ("a \"b\"" or &"name" / ^"path") as its text; other values as written.
export function parseGodotString(raw) {
	const s = String(raw).trim();
	const m = /^[&^]?"((?:[^"\\]|\\.)*)"$/.exec(s);
	if (!m)
		return s;
	return m[1].replace(/\\(.)/g, function (all, c) {
		if (c === "n") return "\n";
		if (c === "t") return "\t";
		return c;
	});
}

// The project's facts the extension uses: {name, layers: {2d_physics: {N: "Name"}, ...}}.
export function readGodotProject(text) {
	const cfg = parseConfigFile(text);
	const app = cfg.sections.application || {};
	const layers = {};
	for (let f = 0; f < LAYER_FAMILIES.length; f++)
		layers[LAYER_FAMILIES[f].key] = {};
	const names = cfg.sections.layer_names || {};
	const keys = Object.keys(names);
	for (let k = 0; k < keys.length; k++) {
		const m = /^([A-Za-z0-9_]+)\/layer_(\d+)$/.exec(keys[k]);
		if (!m || layers[m[1]] === undefined)
			continue;
		const n = parseInt(m[2], 10);
		const name = parseGodotString(names[keys[k]]).trim();
		if (n >= 1 && name !== "")
			layers[m[1]][n] = name;
	}
	return {
		name: app["config/name"] !== undefined ? parseGodotString(app["config/name"]) : "",
		layers: layers,
	};
}

// The values of a layer enum: one per layer from 1 to the highest named one (at most the family's
// count), an unnamed layer as "Layer N", so that a value's index is its bit (F5). A name used twice
// gets " (N)" on its later layers: Tiled's enum values must be distinct.
export function layerEnumValues(layerNames, count) {
	let highest = 0;
	const keys = Object.keys(layerNames);
	for (let i = 0; i < keys.length; i++) {
		const n = parseInt(keys[i], 10);
		if (n <= count && n > highest)
			highest = n;
	}
	const values = [];
	const seen = {};
	for (let n = 1; n <= highest; n++) {
		let v = layerNames[n] !== undefined ? layerNames[n] : "Layer " + n;
		if (seen[v])
			v = v + " (" + n + ")";
		seen[v] = true;
		values.push(v);
	}
	return values;
}
