// Save-time validation of a map or tileset (X1d): the mistakes the importers would only report at
// run time. Works on a plain model the glue builds from Tiled's API (or a test builds from a file):
//
//   map:     {kind: "map", fileName, properties: [prop], layers: [layer], objects: [object],
//             templates: [{objectId, path}]}
//   layer:   {id, name, path ("Group/Inner"), className, properties: [prop]}
//   object:  {id, name, layerPath, className, tileClassName, properties: [prop]}  (resolved:
//             template and tile values included)
//   tileset: {kind: "tileset", fileName, name, properties: [prop]}
//   prop:    {name, type: "int"|"float"|"bool"|"string"|"color"|"file"|"object"|"enum"|"class"|"list",
//             value, propertyType: "" | type name}; a file's value an absolute path or as written,
//             an object's the id, a list's an array of props; `fromEnum` marks a value Tiled
//             holds as an enum value (so it is one of the enum's)
//
// ctx: {classes: {Name: {members: [{name, type, value, propertyType}], useAs: [...]}},
//       enums: {Name: {values: [...], storageType, valuesAsFlags}},
//       projectFile: the .tiled-project (class members' file values are relative to it),
//       godotProject: "" | dir, fileExists(path) -> bool}
//
// Each finding: {severity: "error"|"warning", code, text, objectId (0: none), layerId (0: none)}.

import { MAPPING_TABLE_CLASSES, NODE_TYPE_ENUM } from "./types.mjs";
import { LAYER_FAMILIES } from "./godot_project.mjs";
import { dirName, joinPath, isAbsolutePath, godotToLocal } from "./settings.mjs";

// Mask properties (Reference.md's custom properties reference: the "string (*)" layer masks) and the
// layer family each one names. `layers` alone is the tileset's navigation layers (layers_n).
const MASK_FAMILIES = {
	collision_layer: "2d_physics",
	collision_mask: "2d_physics",
	platform_floor_layers: "2d_physics",
	platform_wall_layers: "2d_physics",
	parsed_collision_mask: "2d_physics",
	navigation_layers: "2d_navigation",
	light_mask: "2d_render",
	visibility_layer: "2d_render",
	occluder_light_mask: "2d_render",
};
const TILESET_MASK_FAMILIES = {
	collision_layer: "2d_physics",
	collision_mask: "2d_physics",
	layers: "2d_navigation",
	light_mask: "2d_render",
};

// The family a property name is a mask of ("collision_layer_1" -> "2d_physics"), or "".
export function maskFamily(name, isTileset) {
	const table = isTileset ? TILESET_MASK_FAMILIES : MASK_FAMILIES;
	const m = /^(.*?)(_\d+)?$/.exec(name);
	const base = m[1];
	if (table[base] !== undefined)
		return table[base];
	if (table[name] !== undefined)
		return table[name];
	return "";
}

function enumNameOfFamily(family) {
	for (let i = 0; i < LAYER_FAMILIES.length; i++) {
		if (LAYER_FAMILIES[i].key === family)
			return LAYER_FAMILIES[i].enumName;
	}
	return "";
}

const FILE_PROPERTIES = ["res_path", "godot_scene", "godot_script"];

function lowerTable() {
	const t = {};
	for (let i = 0; i < MAPPING_TABLE_CLASSES.length; i++)
		t[MAPPING_TABLE_CLASSES[i]] = true;
	return t;
}

function findProp(props, name) {
	for (let i = 0; i < props.length; i++) {
		if (props[i].name === name)
			return props[i];
	}
	return null;
}

// The object's properties with its class's members that it does not override (the importers add
// those with their defaults), each member marked `fromClass`.
function withClassMembers(props, cls) {
	if (!cls || !cls.members)
		return props;
	const out = props.slice();
	for (let i = 0; i < cls.members.length; i++) {
		const m = cls.members[i];
		if (findProp(props, m.name) !== null)
			continue;
		out.push({ name: m.name, type: m.type, value: m.value, propertyType: m.propertyType || "", fromClass: true });
	}
	return out;
}

function describeObject(o) {
	const name = o.name !== "" ? "'" + o.name + "'" : "(unnamed)";
	return "Object " + name + " (id " + o.id + (o.layerPath ? ", layer " + o.layerPath : "") + ")";
}

// A file property's value as a local path: res:// against the Godot project, relative against
// the file that holds it. "" when it cannot be resolved (res:// without a Godot project).
export function resolveFileValue(value, holderFile, godotProject) {
	const s = String(value === undefined || value === null ? "" : value);
	if (s === "")
		return "";
	if (s.indexOf("res://") === 0)
		return godotProject !== "" ? godotToLocal(godotProject, s) : "";
	if (/^[a-z]+:\/\//.test(s)) {
		if (s.indexOf("file://") === 0)
			return s.replace(/^file:\/\/\/?([A-Za-z]:)/, "$1").replace(/^file:\/\//, "");
		return "";
	}
	if (isAbsolutePath(s))
		return s;
	return holderFile ? joinPath(dirName(holderFile), s) : "";
}

// Layer numbers and ranges, the importers' "1,3,5-7" form (also empty).
function isLayerNumbers(s) {
	return /^\s*(\d+\s*(-\s*\d+\s*)?)?(,\s*\d+\s*(-\s*\d+\s*)?)*$/.test(s);
}

function checkMask(prop, family, ctx, where, push) {
	if (prop.type !== "string" || prop.fromEnum)
		return;		// int (the mask), or an enum value Tiled itself keeps valid
	const value = String(prop.value);
	const names = value.split(",").map(function (s) { return s.trim(); }).filter(function (s) { return s !== ""; });
	if (prop.propertyType) {
		const e = ctx.enums[prop.propertyType];
		if (e === undefined) {
			push("warning", "mask-unknown-enum", where + ": mask '" + prop.name + "' is of type " + prop.propertyType +
				", which the project has no enum of; the importer reads it as layer numbers.");
			return;
		}
		const unknown = names.filter(function (n) { return e.values.indexOf(n) < 0; });
		if (unknown.length > 0)
			push("warning", "mask-unknown-name", where + ": mask '" + prop.name + "' names " + quoteList(unknown) +
				", not values of " + prop.propertyType + "; the importer ignores them.");
		return;
	}
	if (isLayerNumbers(value))
		return;
	const enumName = enumNameOfFamily(family);
	const e = ctx.enums[enumName];
	const unknown = e === undefined ? names : names.filter(function (n) { return e.values.indexOf(n) < 0; });
	if (unknown.length > 0) {
		if (e === undefined)
			push("warning", "mask-unknown-name", where + ": mask '" + prop.name + "' names " + quoteList(unknown) +
				", but the project has no " + enumName + " enum (run YATI: Sync Godot project...); the importer reads layer numbers here.");
		else
			push("warning", "mask-unknown-name", where + ": mask '" + prop.name + "' names " + quoteList(unknown) +
				", which " + enumName + " does not have; the importer reads layer numbers, or the names of the property's enum.");
		return;
	}
	push("warning", "mask-untyped-names", where + ": mask '" + prop.name + "' names layers of " + enumName +
		" but is a plain string; give it the type " + enumName + " so the importer reads the names.");
}

function quoteList(names) {
	return names.map(function (n) { return "'" + n + "'"; }).join(", ");
}

function checkProperties(props, holder, ctx, where, push, isTileset, objectIds, holderFile) {
	for (let i = 0; i < props.length; i++) {
		const p = props[i];
		const family = maskFamily(p.name, isTileset);
		if (family !== "")
			checkMask(p, family, ctx, where, push);
		if (p.name.toLowerCase() === "godot_node_type" && p.type === "string") {
			if (MAPPING_TABLE_CLASSES.indexOf(String(p.value).toLowerCase()) < 0)
				push("warning", "unknown-node-type", where + ": godot_node_type '" + p.value + "' is not a " + NODE_TYPE_ENUM +
					" value (" + MAPPING_TABLE_CLASSES.join(", ") + ").");
		}
		if (FILE_PROPERTIES.indexOf(p.name) >= 0 && (p.type === "file" || p.type === "string") && String(p.value) !== "") {
			// A class member's default is relative to the project file (F1), the object's own value to the map.
			const local = resolveFileValue(p.value, p.fromClass ? (ctx.projectFile || "") : holderFile, ctx.godotProject);
			if (local !== "" && !ctx.fileExists(local))
				push("error", "missing-file", where + ": " + p.name + (p.fromClass ? " (its class's default)" : "") +
					" names " + local + ", which does not exist.");
		}
		checkObjectRefs(p, ctx, where, push, objectIds);
	}
}

function checkObjectRefs(p, ctx, where, push, objectIds) {
	if (p.type === "object") {
		const id = Number(p.value);
		if (!id) {
			push("warning", "unset-object-ref", where + ": object property '" + p.name + "' is not set" +
				(p.fromClass ? " (its class's default)" : "") + "; the importer gives an empty NodePath.");
		} else if (objectIds !== null && objectIds[id] !== true) {
			push("warning", "dangling-object-ref", where + ": object property '" + p.name + "' names object " + id +
				", which this map does not have; the importer gives an empty NodePath.");
		}
		return;
	}
	if (p.type === "list" && Array.isArray(p.value)) {
		for (let i = 0; i < p.value.length; i++) {
			const item = p.value[i];
			checkObjectRefs({ name: p.name + "[" + i + "]", type: item.type, value: item.value, propertyType: item.propertyType }, ctx, where, push, objectIds);
		}
	}
}

// The findings for a map model.
export function validateMap(map, ctx) {
	const findings = [];
	const table = lowerTable();
	const classNames = Object.keys(ctx.classes || {});
	const objectIds = {};
	for (let i = 0; i < map.objects.length; i++)
		objectIds[map.objects[i].id] = true;
	const fileName = map.fileName || "";

	// The map's own properties and the layers'.
	checkProperties(map.properties || [], null, ctx, "Map", function (sev, code, text) {
		findings.push({ severity: sev, code: code, text: text, objectId: 0, layerId: 0 });
	}, false, objectIds, fileName);
	for (let i = 0; i < map.layers.length; i++) {
		const l = map.layers[i];
		checkProperties(l.properties || [], l, ctx, "Layer '" + l.path + "'", function (sev, code, text) {
			findings.push({ severity: sev, code: code, text: text, objectId: 0, layerId: l.id });
		}, false, objectIds, fileName);
	}

	for (let i = 0; i < map.objects.length; i++) {
		const o = map.objects[i];
		const where = describeObject(o);
		const push = function (sev, code, text) {
			findings.push({ severity: sev, code: code, text: text, objectId: o.id, layerId: 0 });
		};
		const cls = o.className !== "" ? o.className : (o.tileClassName || "");
		const props = withClassMembers(o.properties || [], ctx.classes[cls]);
		const nodeType = findProp(props, "godot_node_type");
		const hasNodeType = nodeType !== null && nodeType.type === "string";
		if (cls !== "" && !hasNodeType && !table[cls.toLowerCase()] && ctx.classes[cls] === undefined) {
			const near = classNames.filter(function (n) { return n.toLowerCase() === cls.toLowerCase(); });
			push("warning", "unknown-class", where + ": class '" + cls + "' is no Godot node type (" + MAPPING_TABLE_CLASSES.join(", ") +
				") and no class of the project" + (near.length > 0 ? " (class names are case-sensitive: '" + near[0] + "'?)" : "") +
				"; the importer makes it its unknown_class_fallback (a StaticBody2D by default) unless the game's class_scenes maps it.");
		}
		const effective = hasNodeType ? String(nodeType.value).toLowerCase() : cls.toLowerCase();
		if (effective === "instance") {
			const rp = findProp(props, "res_path");
			if (rp === null || String(rp.value) === "")
				push("error", "instance-without-res-path", where + ": class 'instance' needs a file property 'res_path' (the scene to instantiate); the importer skips the object.");
		}
		checkProperties(props, o, ctx, where, push, false, objectIds, fileName);
	}

	const templates = map.templates || [];
	for (let i = 0; i < templates.length; i++) {
		const t = templates[i];
		const local = resolveFileValue(t.path, fileName, ctx.godotProject);
		if (local !== "" && !ctx.fileExists(local)) {
			let name = "";
			for (let k = 0; k < map.objects.length; k++) {
				if (map.objects[k].id === t.objectId)
					name = map.objects[k].name;
			}
			findings.push({
				severity: "error", code: "missing-template", objectId: t.objectId, layerId: 0,
				text: "Object " + (name !== "" ? "'" + name + "' " : "") + "(id " + t.objectId + "): template '" + t.path + "' does not exist (" + local + ").",
			});
		}
	}
	return findings;
}

// The findings for a tileset model: its mask properties.
export function validateTileset(tileset, ctx) {
	const findings = [];
	checkProperties(tileset.properties || [], null, ctx, "Tileset '" + (tileset.name || "") + "'", function (sev, code, text) {
		findings.push({ severity: sev, code: code, text: text, objectId: 0, layerId: 0 });
	}, true, null, tileset.fileName || "");
	return findings;
}

// The templates a saved map uses: [{objectId, path}] from a .tmx's <object ... template="..."> or a
// .tmj's "template" keys. Tiled's API does not expose an object's template, so the glue reads the
// map file as last saved.
export function templateReferences(text) {
	const s = String(text);
	const out = [];
	if (/^\s*\{/.test(s)) {
		let json;
		try {
			json = JSON.parse(s);
		} catch (e) {
			return out;
		}
		const walk = function (layers) {
			for (let i = 0; i < (layers || []).length; i++) {
				const l = layers[i];
				if (Array.isArray(l.objects)) {
					for (let k = 0; k < l.objects.length; k++) {
						if (typeof l.objects[k].template === "string")
							out.push({ objectId: l.objects[k].id, path: l.objects[k].template });
					}
				}
				if (Array.isArray(l.layers))
					walk(l.layers);
			}
		};
		walk(json.layers);
		return out;
	}
	const re = /<object\b([^>]*)>/g;
	let m;
	while ((m = re.exec(s)) !== null) {
		const tpl = /\btemplate="([^"]*)"/.exec(m[1]);
		if (!tpl)
			continue;
		const id = /\bid="(\d+)"/.exec(m[1]);
		out.push({ objectId: id ? parseInt(id[1], 10) : 0, path: tpl[1].replace(/&amp;/g, "&").replace(/&quot;/g, "\"").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&apos;/g, "'") });
	}
	return out;
}

// The validation context from the project's types as the glue reads them: [{name, isClass,
// isEnum, members (name -> prop), values, storageType, valuesAsFlags}].
export function contextFromTypes(types, projectFile, godotProject, fileExists) {
	const classes = {};
	const enums = {};
	for (let i = 0; i < types.length; i++) {
		const t = types[i];
		if (t.isEnum)
			enums[t.name] = { values: t.values || [], storageType: t.storageType, valuesAsFlags: t.valuesAsFlags };
		else if (t.isClass)
			classes[t.name] = { members: t.members || [], useAs: t.useAs || [] };
	}
	return { classes: classes, enums: enums, projectFile: projectFile || "", godotProject: godotProject || "", fileExists: fileExists };
}
