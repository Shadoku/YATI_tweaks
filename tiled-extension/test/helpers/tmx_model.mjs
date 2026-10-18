// A .tmx / .tsx file as the validator's model (lib/validate.mjs), read directly from the file: the
// model the glue builds from Tiled's API for an open map (yati-godot.mjs, mapModel), without
// Tiled. glue.test.mjs checks that the two agree on the fixture maps.
import fs from "node:fs";
import path from "node:path";
import { parseXml, child, childrenNamed } from "./xml.mjs";

// enums: {Name: {values, storageType, valuesAsFlags}} of the project, to tell which values Tiled
// would hold as enum values (`fromEnum`).
function readProps(el, dir, enums) {
	const out = [];
	const props = child(el, "properties");
	if (!props)
		return out;
	for (const p of childrenNamed(props, "property"))
		out.push(readProp(p, dir, enums));
	return out;
}

function readProp(p, dir, enums) {
	const type = p.attrs.type || "string";
	const propertyType = p.attrs.propertytype || "";
	const name = p.attrs.name || "";
	if (type === "list")
		return { name: name, type: "list", value: childrenNamed(p, "item").map((it) => readProp(it, dir, enums)), propertyType: "" };
	if (type === "class") {
		const value = {};
		for (const m of readProps(p, dir, enums))
			value[m.name] = m.value;
		return { name: name, type: "class", value: value, propertyType: propertyType };
	}
	const raw = p.attrs.value !== undefined ? p.attrs.value : p.text;
	let value = raw;
	if (type === "int" || type === "float")
		value = Number(raw);
	else if (type === "bool")
		value = raw === "true";
	else if (type === "object")
		value = Number(raw);
	else if (type === "file")
		value = raw === "" || /^[a-z]+:\/\//.test(raw) ? raw : path.resolve(dir, raw);	// a URL (res://) as written
	const prop = { name: name, type: type, value: value, propertyType: "" };
	const e = propertyType ? enums[propertyType] : undefined;
	if (e !== undefined) {
		if (type === "int") {
			prop.propertyType = propertyType;
		} else if (type === "string") {
			const names = String(raw).split(",").map((s) => s.trim()).filter((s) => s !== "");
			const valid = e.valuesAsFlags ? names.every((n) => e.values.indexOf(n) >= 0) : e.values.indexOf(String(raw)) >= 0;
			if (valid) {
				prop.propertyType = propertyType;
				prop.fromEnum = true;
			}
		}
	}
	return prop;
}

function tileClass(tilesets, gid, dir) {
	let best = null;
	for (const ts of tilesets) {
		if (ts.firstgid <= gid && (best === null || ts.firstgid > best.firstgid))
			best = ts;
	}
	if (best === null)
		return { className: "", props: [] };
	return tileOf(best.el, gid - best.firstgid, best.dir);
}

function tileOf(tsEl, localId, dir) {
	let el = tsEl;
	let d = dir;
	if (tsEl.attrs.source !== undefined) {
		const p = path.resolve(dir, tsEl.attrs.source);
		if (!fs.existsSync(p))
			return { className: "", props: [] };
		el = parseXml(fs.readFileSync(p, "utf8"));
		d = path.dirname(p);
	}
	for (const t of childrenNamed(el, "tile")) {
		if (Number(t.attrs.id) === localId)
			return { className: t.attrs.class || t.attrs.type || "", el: t, dir: d };
	}
	return { className: "", el: null, dir: d };
}

// Properties by name, later ones winning (tile < template < object), in the order Tiled's
// resolvedProperties() would list them is not defined: sorted by name.
function merge(lists) {
	const byName = {};
	for (const list of lists) {
		for (const p of list)
			byName[p.name] = p;
	}
	return Object.keys(byName).sort().map((k) => byName[k]);
}

export function tmxModel(fileName, enums) {
	const dir = path.dirname(fileName);
	const root = parseXml(fs.readFileSync(fileName, "utf8"));
	const tilesets = childrenNamed(root, "tileset").map((ts) => ({ firstgid: Number(ts.attrs.firstgid || 1), el: ts, dir: dir }));
	const model = { kind: "map", fileName: fileName, properties: readProps(root, dir, enums), layers: [], objects: [], templates: [] };
	const walk = (el, prefix) => {
		for (const c of el.children) {
			if (c.name !== "objectgroup" && c.name !== "group" && c.name !== "layer" && c.name !== "imagelayer")
				continue;
			const p = prefix + (c.attrs.name || "");
			model.layers.push({ id: Number(c.attrs.id || 0), name: c.attrs.name || "", path: p, className: c.attrs.class || "", properties: readProps(c, dir, enums) });
			if (c.name === "objectgroup") {
				for (const o of childrenNamed(c, "object")) {
					let cls = o.attrs.class || o.attrs.type || "";
					let name = o.attrs.name || "";
					let gid = o.attrs.gid !== undefined ? Number(o.attrs.gid) & 0x0fffffff : 0;
					let tplProps = [];
					let tile = null;
					if (o.attrs.template !== undefined) {
						model.templates.push({ objectId: Number(o.attrs.id), path: o.attrs.template });
						const tp = path.resolve(dir, o.attrs.template);
						if (fs.existsSync(tp)) {
							const tpl = parseXml(fs.readFileSync(tp, "utf8"));
							const to = child(tpl, "object");
							if (to) {
								if (cls === "")
									cls = to.attrs.class || to.attrs.type || "";
								if (name === "")
									name = to.attrs.name || "";
								tplProps = readProps(to, path.dirname(tp), enums);
								const ts = child(tpl, "tileset");
								if (gid === 0 && to.attrs.gid !== undefined && ts)
									tile = tileOf(ts, (Number(to.attrs.gid) & 0x0fffffff) - Number(ts.attrs.firstgid || 1), path.dirname(tp));
							}
						}
					}
					if (gid > 0)
						tile = tileClass(tilesets, gid, dir);
					const tileProps = tile && tile.el ? readProps(tile.el, tile.dir, enums) : [];
					model.objects.push({
						id: Number(o.attrs.id || 0),
						name: name,
						layerPath: p,
						className: cls,
						tileClassName: tile ? tile.className : "",
						properties: merge([tileProps, tplProps, readProps(o, dir, enums)]),
					});
				}
			}
			if (c.name === "group")
				walk(c, p + "/");
		}
	};
	walk(root, "");
	return model;
}

export function tsxModel(fileName, enums) {
	const root = parseXml(fs.readFileSync(fileName, "utf8"));
	return { kind: "tileset", fileName: fileName, name: root.attrs.name || "", properties: readProps(root, path.dirname(fileName), enums) };
}

// A .tiled-project's types as contextFromTypes takes them (lib/validate.mjs).
export function projectTypes(projectFile) {
	const json = JSON.parse(fs.readFileSync(projectFile, "utf8"));
	const dir = path.dirname(projectFile);
	return (json.propertyTypes || []).map((t) => {
		if (t.type === "enum")
			return { name: t.name, isEnum: true, isClass: false, values: t.values, storageType: t.storageType === "int" ? 1 : 0, valuesAsFlags: t.valuesAsFlags };
		return {
			name: t.name, isEnum: false, isClass: true, useAs: t.useAs,
			members: (t.members || []).map((m) => ({
				name: m.name, type: m.type, propertyType: m.propertyType || "",
				value: m.type === "file" && m.value !== "" ? path.resolve(dir, m.value) : m.value,
			})),
		};
	});
}

export function enumsOf(types) {
	const out = {};
	for (const t of types) {
		if (t.isEnum)
			out[t.name] = { values: t.values, storageType: t.storageType, valuesAsFlags: t.valuesAsFlags };
	}
	return out;
}
