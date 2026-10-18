// The custom types the extension generates, as pure data in the shape of a .tiled-project's
// "propertyTypes" entries (without ids): enums {type: "enum", name, storageType, values,
// valuesAsFlags} and classes {type: "class", name, useAs, color, drawFill, members: [{name, type,
// value, propertyType?}]}. Member values are in the file's form: a file path relative to the
// project file, a colour "#AARRGGBB", an enum value by name (string storage) or number (int), a
// class value as an object. apply.mjs turns them into Tiled API calls.

import { LAYER_FAMILIES, layerEnumValues } from "./godot_project.mjs";
import { parseLiteral, engineClassOf, pascalCase } from "./scene_exports.mjs";

export { pascalCase };
import { relativePath, godotToLocal, joinPath, dirName, stem } from "./settings.mjs";

// The mapping table's class names (Reference.md, "Mapping from Tiled elements to Godot elements";
// runtime/TilemapCreator.gd's GODOT_TYPE_MAP without ""), in that order.
export const MAPPING_TABLE_CLASSES = [
	"collision", "staticbody", "characterbody", "rigidbody", "animatablebody", "area",
	"navigation", "occluder", "line", "path", "polygon", "instance", "parallax",
];

export const NODE_TYPE_ENUM = "GodotNodeType";
export const MAP_REF_CLASS = "MapRef";
export const VECTOR2_CLASS = "Vector2";
export const VECTOR2I_CLASS = "Vector2i";

// Names the extension's own types take; a scene class with one of these names gets the suffix too.
export const RESERVED_TYPE_NAMES = [NODE_TYPE_ENUM, MAP_REF_CLASS, VECTOR2_CLASS, VECTOR2I_CLASS]
	.concat(LAYER_FAMILIES.map(function (f) { return f.enumName; }));

// ---------------------------------------------------------------------------------------------
// The sync's types (X1a, X1b, MapRef)

// The layer enums of a project.godot (readGodotProject's result): flags, storage int, one value per
// layer up to the highest named one. A family without names gets no enum (listed in `skipped`).
export function layerEnumDefinitions(godotInfo) {
	const defs = [];
	const skipped = [];
	for (let i = 0; i < LAYER_FAMILIES.length; i++) {
		const f = LAYER_FAMILIES[i];
		const values = layerEnumValues(godotInfo.layers[f.key] || {}, f.count);
		if (values.length === 0) {
			skipped.push(f.enumName + ": project.godot names no " + f.key + " layer");
			continue;
		}
		defs.push({ type: "enum", name: f.enumName, storageType: "int", values: values, valuesAsFlags: true });
	}
	return { defs: defs, skipped: skipped };
}

// GodotNodeType: the mapping table's names, for a godot_node_type property (storage string).
export function nodeTypeEnumDefinition() {
	return { type: "enum", name: NODE_TYPE_ENUM, storageType: "string", values: MAPPING_TABLE_CLASSES.slice(), valuesAsFlags: false };
}

// MapRef {map: file, object: int}: the Reference's convention for a reference into another map.
export function mapRefClassDefinition() {
	return {
		type: "class", name: MAP_REF_CLASS, useAs: ["property"], color: stableColor(MAP_REF_CLASS), drawFill: true,
		members: [
			{ name: "map", type: "file", value: "" },
			{ name: "object", type: "int", value: 0 },
		],
	};
}

// Vector2 {x, y: float} and Vector2i {x, y: int}: what a vector export becomes (section 3 of the
// plan: the importers turn such a class value into the vector).
export function vectorClassDefinition(name) {
	const t = name === VECTOR2I_CLASS ? "int" : "float";
	return {
		type: "class", name: name, useAs: ["property"], color: stableColor(name), drawFill: true,
		members: [
			{ name: "x", type: t, value: 0 },
			{ name: "y", type: t, value: 0 },
		],
	};
}

// Everything the sync generates, in the order it is applied (enums before the classes using them).
export function syncDefinitions(godotInfo) {
	const layers = layerEnumDefinitions(godotInfo);
	return {
		defs: layers.defs.concat([nodeTypeEnumDefinition(), mapRefClassDefinition()]),
		skipped: layers.skipped,
	};
}

// ---------------------------------------------------------------------------------------------
// Colours

// A stable colour for a type name ("#ffRRGGBB", Tiled's #AARRGGBB): FNV-1a of the name picks the hue.
export function stableColor(name) {
	let h = 0x811c9dc5;
	for (let i = 0; i < name.length; i++) {
		h ^= name.charCodeAt(i);
		h = Math.imul(h, 0x01000193) >>> 0;
	}
	const hue = h % 360;
	const rgb = hsvToRgb(hue, 0.5, 0.9);
	return "#ff" + hex2(rgb[0]) + hex2(rgb[1]) + hex2(rgb[2]);
}

function hsvToRgb(h, s, v) {
	const c = v * s;
	const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
	const m = v - c;
	let r = 0, g = 0, b = 0;
	if (h < 60) { r = c; g = x; } else if (h < 120) { r = x; g = c; } else if (h < 180) { g = c; b = x; }
	else if (h < 240) { g = x; b = c; } else if (h < 300) { r = x; b = c; } else { r = c; b = x; }
	return [Math.round((r + m) * 255), Math.round((g + m) * 255), Math.round((b + m) * 255)];
}

function hex2(n) {
	const v = Math.max(0, Math.min(255, Math.round(n)));
	return (v < 16 ? "0" : "") + v.toString(16);
}

// A Godot colour {r, g, b, a} (0..1) as Tiled saves a colour: "#AARRGGBB", alpha first.
export function tiledColor(c) {
	return "#" + hex2(c.a * 255) + hex2(c.r * 255) + hex2(c.g * 255) + hex2(c.b * 255);
}

// ---------------------------------------------------------------------------------------------
// Godot classes: Node or Resource

// Engine classes an Object-typed export may name. A name not listed is judged by its words
// (isNodeClass / isResourceClass); a script class by the engine class it extends.
const NODE_CLASSES = [
	"Node", "Node2D", "Node3D", "CanvasItem", "CanvasLayer", "Control", "Viewport", "SubViewport",
	"Window", "Timer", "Camera2D", "Sprite2D", "AnimatedSprite2D", "Area2D", "StaticBody2D",
	"CharacterBody2D", "RigidBody2D", "AnimatableBody2D", "PhysicsBody2D", "CollisionObject2D",
	"CollisionShape2D", "CollisionPolygon2D", "Marker2D", "Path2D", "PathFollow2D", "Line2D",
	"Polygon2D", "Label", "RichTextLabel", "Button", "TextureRect", "ColorRect", "NinePatchRect",
	"Panel", "Container", "AnimationPlayer", "AnimationTree", "AudioStreamPlayer",
	"AudioStreamPlayer2D", "TileMap", "TileMapLayer", "NavigationRegion2D", "NavigationAgent2D",
	"LightOccluder2D", "PointLight2D", "DirectionalLight2D", "GPUParticles2D", "CPUParticles2D",
	"Parallax2D", "ParallaxBackground", "ParallaxLayer", "RayCast2D", "ShapeCast2D",
	"RemoteTransform2D", "VisibleOnScreenNotifier2D", "VisibleOnScreenEnabler2D", "Skeleton2D",
	"Bone2D", "BackBufferCopy", "CanvasModulate", "MeshInstance2D", "MultiMeshInstance2D",
	"TouchScreenButton", "HTTPRequest", "ResourcePreloader", "MultiplayerSpawner",
	"MultiplayerSynchronizer", "WorldEnvironment", "Joint2D", "PinJoint2D", "GrooveJoint2D",
	"DampedSpringJoint2D", "TextureButton", "LineEdit", "TextEdit", "ProgressBar", "Range",
	"Slider", "HSlider", "VSlider", "SpinBox", "ItemList", "OptionButton", "CheckBox", "CheckButton",
	"VBoxContainer", "HBoxContainer", "GridContainer", "MarginContainer", "CenterContainer",
	"ScrollContainer", "PanelContainer", "TabContainer", "SubViewportContainer",
];

const RESOURCE_CLASSES = [
	"Resource", "Texture", "Texture2D", "ImageTexture", "AtlasTexture", "CompressedTexture2D",
	"GradientTexture1D", "GradientTexture2D", "NoiseTexture2D", "CanvasTexture", "AnimatedTexture",
	"PackedScene", "Script", "GDScript", "Shader", "Material", "ShaderMaterial",
	"CanvasItemMaterial", "ParticleProcessMaterial", "Mesh", "ArrayMesh", "AudioStream",
	"AudioStreamWAV", "AudioStreamOggVorbis", "AudioStreamMP3", "Font", "FontFile", "SystemFont",
	"FontVariation", "Theme", "StyleBox", "StyleBoxFlat", "StyleBoxTexture", "StyleBoxEmpty",
	"SpriteFrames", "Animation", "AnimationLibrary", "Curve", "Curve2D", "Gradient", "TileSet",
	"Shape2D", "RectangleShape2D", "CircleShape2D", "CapsuleShape2D", "SegmentShape2D",
	"ConvexPolygonShape2D", "ConcavePolygonShape2D", "WorldBoundaryShape2D", "SeparationRayShape2D",
	"PhysicsMaterial", "Environment", "LabelSettings", "Image", "JSON", "Translation",
	"NavigationPolygon", "OccluderPolygon2D", "BitMap", "InputEvent", "Shortcut", "Noise",
	"FastNoiseLite", "PolygonPathFinder", "World2D", "SkeletonModification2D",
];

export function isNodeClass(name) {
	if (NODE_CLASSES.indexOf(name) >= 0)
		return true;
	if (RESOURCE_CLASSES.indexOf(name) >= 0)
		return false;
	if (/(Texture|Material|Shape|Stream|Mesh|Font|Theme|StyleBox|Curve|Gradient|Resource|Scene|Script|Shader|Animation|Frames|Settings|TileSet|Image|Environment|Noise|Library|Translation|Polygon)(2D|3D)?$/.test(name))
		return false;
	return /(2D|3D|Node|Body|Container|Button|Label|Player|Timer|Camera|Light|Agent|Region|Joint|Sprite|Viewport|Layer)$/.test(name);
}

export function isResourceClass(name) {
	if (RESOURCE_CLASSES.indexOf(name) >= 0)
		return true;
	if (NODE_CLASSES.indexOf(name) >= 0)
		return false;
	return /(Texture|Material|Shape|Stream|Mesh|Font|Theme|StyleBox|Curve|Gradient|Resource|Scene|Script|Shader|Animation|Frames|Settings|TileSet|Image|Environment|Noise|Library|Translation|Polygon)(2D|3D)?$/.test(name);
}

// ---------------------------------------------------------------------------------------------
// Scenes to classes (X1c)

function enumValueNames(args) {
	// @export_enum("Walk", "Run:5") -> [{name: "Walk", value: 0}, {name: "Run", value: 5}]
	const out = [];
	let next = 0;
	for (let i = 0; i < args.length; i++) {
		const lit = parseLiteral(args[i]);
		if (lit.kind !== "string")
			continue;
		const parts = lit.value.split(",");
		for (let p = 0; p < parts.length; p++) {
			const kv = parts[p].split(":");
			const name = kv[0].trim();
			if (name === "")
				continue;
			let value = next;
			if (kv.length > 1 && /^-?\d+$/.test(kv[1].trim()))
				value = parseInt(kv[1].trim(), 10);
			out.push({ name: name, value: value });
			next = value + 1;
		}
	}
	return out;
}

function stringArgs(args) {
	const out = [];
	for (let i = 0; i < args.length; i++) {
		const lit = parseLiteral(args[i]);
		if (lit.kind === "string")
			out.push(lit.value);
	}
	return out;
}

// The type an untyped export takes from its default ("var x = 5" is an int).
function inferType(defaultText) {
	const lit = parseLiteral(defaultText);
	switch (lit.kind) {
	case "int": return "int";
	case "float": return "float";
	case "bool": return "bool";
	case "string": return lit.prefix === "&" ? "StringName" : (lit.prefix === "^" ? "NodePath" : "String");
	case "color": return "Color";
	case "vector2": return "Vector2";
	case "vector2i": return "Vector2i";
	case "array": return "Array";
	case "dictionary": return "Dictionary";
	default: return "";
	}
}

// A Godot path ("res://a.png" or relative to the script) as a file member's value, relative to
// the project file; "" when there is none.
function fileValue(godotPath, ctx, scriptPath) {
	if (godotPath === "" || godotPath === undefined)
		return "";
	if (/^(user|uid):\/\//.test(godotPath))
		return "";
	const local = godotPath.indexOf("res://") === 0
		? godotToLocal(ctx.godotProjectDir, godotPath)
		: joinPath(dirName(scriptPath), godotPath);
	return relativePath(ctx.tiledProjectDir, local);
}

function numberOr(lit, fallback) {
	if (lit.kind === "int" || lit.kind === "float")
		return lit.value;
	return fallback;
}

// One export as a member: {member: {name, type, value, propertyType?}, enumDef?, note?} or {skip}.
// ctx: {godotProjectDir, tiledProjectDir, classIndex, knownEnums: {name: true}, className}.
export function memberForExport(ex, resolved, ctx, scriptPath) {
	const a = ex.annotation;
	let type = ex.type;
	if (type === "" && ex.defaultText !== "")
		type = inferType(ex.defaultText);
	const lit = parseLiteral(ex.defaultText);
	const name = ex.name;

	// Annotations that decide the member's type whatever the variable's type is.
	if (a === "export_enum") {
		const values = enumValueNames(ex.args);
		if (values.length === 0)
			return { skip: "@export_enum without values" };
		const enumName = (ex.owner || ctx.className) + "." + name;
		let value = values[0].name;
		if (lit.kind === "string") {
			value = lit.value;
		} else if (lit.kind === "int") {
			for (let i = 0; i < values.length; i++) {
				if (values[i].value === lit.value)
					value = values[i].name;
			}
		}
		return {
			member: { name: name, type: "string", value: value, propertyType: enumName },
			enumDef: { type: "enum", name: enumName, storageType: "string", values: values.map(function (v) { return v.name; }), valuesAsFlags: false },
		};
	}
	if (a === "export_file" || a === "export_global_file" || a === "export_file_path") {
		return { member: { name: name, type: "file", value: lit.kind === "string" ? fileValue(lit.value, ctx, scriptPath) : "" } };
	}
	if (a === "export_dir" || a === "export_global_dir" || a === "export_multiline" || a === "export_placeholder") {
		return { member: { name: name, type: "string", value: lit.kind === "string" ? lit.value : "" } };
	}
	if (a === "export_node_path")
		return { member: { name: name, type: "object", value: 0 } };
	const layerEnum = { export_flags_2d_physics: "GodotPhysicsLayers", export_flags_2d_render: "GodotRenderLayers", export_flags_2d_navigation: "GodotNavigationLayers" }[a];
	if (layerEnum !== undefined) {
		const mask = lit.kind === "int" ? lit.value : 0;
		if (ctx.knownEnums[layerEnum])
			return { member: { name: name, type: "int", value: mask, propertyType: layerEnum } };
		return { member: { name: name, type: "int", value: mask }, note: "an int: run Sync Godot project first to type it " + layerEnum };
	}

	// By the variable's type. A list has no member type: a placeholder member would reach every
	// object of the class and make the importers warn that it does not fit the Array; an object
	// gives such a property as a list property of its own.
	if (/^(Array(\[.*\])?|Packed[A-Za-z0-9]*Array)$/.test(type))
		return { skip: "Tiled class members cannot be lists" };
	switch (type) {
	case "int":
		return { member: { name: name, type: "int", value: lit.kind === "int" ? lit.value : (lit.kind === "float" ? Math.round(lit.value) : 0) } };
	case "float":
		return { member: { name: name, type: "float", value: numberOr(lit, 0) } };
	case "bool":
		return { member: { name: name, type: "bool", value: lit.kind === "bool" ? lit.value : false } };
	case "String":
	case "StringName":
		return { member: { name: name, type: "string", value: lit.kind === "string" ? lit.value : "" } };
	case "NodePath":
		return { member: { name: name, type: "object", value: 0 } };
	case "Color": {
		let value = "";
		if (lit.kind === "color")
			value = tiledColor(lit.value);
		else if (lit.kind === "none")
			value = "#ff000000";	// Color(): opaque black
		return { member: { name: name, type: "color", value: value } };
	}
	case "Vector2":
	case "Vector2i": {
		const v = (lit.kind === "vector2" || lit.kind === "vector2i") ? lit.value : { x: 0, y: 0 };
		const x = type === "Vector2i" ? Math.round(v.x) : v.x;
		const y = type === "Vector2i" ? Math.round(v.y) : v.y;
		return { member: { name: name, type: "class", value: { x: x, y: y }, propertyType: type }, vector: type };
	}
	case "Dictionary":
		return { skip: "a Dictionary has no Tiled member type" };
	case "":
		return { skip: "untyped without a default to infer the type from" };
	default:
		break;
	}
	if (/^Dictionary\[/.test(type))
		return { skip: "a Dictionary has no Tiled member type" };

	// An enum: Mood, Enemy.Mood (a class_name's), or a script's own inherited one.
	const enumRef = enumFor(type, resolved, ctx);
	if (enumRef !== null) {
		let value = enumRef.values.length > 0 ? enumRef.values[0].name : "";
		if (lit.kind === "enumref") {
			const last = lit.path[lit.path.length - 1];
			for (let i = 0; i < enumRef.values.length; i++) {
				if (enumRef.values[i].name === last)
					value = last;
			}
		} else if (lit.kind === "int") {
			for (let i = 0; i < enumRef.values.length; i++) {
				if (enumRef.values[i].value === lit.value)
					value = enumRef.values[i].name;
			}
		}
		return {
			member: { name: name, type: "string", value: value, propertyType: enumRef.tiledName },
			enumDef: { type: "enum", name: enumRef.tiledName, storageType: "string", values: enumRef.values.map(function (v) { return v.name; }), valuesAsFlags: false },
		};
	}

	// An object type: a node (an object reference, F2) or a resource (a file, F1).
	if (/^[A-Za-z_][A-Za-z0-9_]*$/.test(type)) {
		const engine = engineClassOf(type, ctx.classIndex);
		if (isNodeClass(engine))
			return { member: { name: name, type: "object", value: 0 } };
		if (isResourceClass(engine) || engine === "Resource") {
			const path = lit.kind === "resource" || lit.kind === "string" ? lit.value : "";
			return { member: { name: name, type: "file", value: fileValue(path, ctx, scriptPath) } };
		}
		return { skip: "type " + type + (engine !== type ? " (" + engine + ")" : "") + " is neither a known Node nor a Resource class" };
	}
	return { skip: "type " + type + " has no Tiled member type" };
}

// The enum a type names, with the Tiled name it gets ("<declaring class>.<Enum>"), or null.
function enumFor(type, resolved, ctx) {
	const parts = type.split(".");
	if (parts.length === 1 && resolved.enums[type] !== undefined) {
		const owner = resolved.enumOwners[type] || ctx.className;	// the declaring script's name
		return { values: resolved.enums[type], tiledName: owner + "." + type };
	}
	if (parts.length === 2 && ctx.classIndex[parts[0]] !== undefined) {
		const s = ctx.classIndex[parts[0]].script;
		if (s.enums[parts[1]] !== undefined)
			return { values: s.enums[parts[1]], tiledName: parts[0] + "." + parts[1] };
	}
	return null;
}

// The class types for the scanned scenes (scanScenes' result). ctx: {godotProjectDir,
// tiledProjectDir, classIndex, knownEnums}. Returns {defs (enums first, then Vector2/Vector2i
// when used, then the classes), classes: [{name, scene, notes, skipped}], warnings}.
export function sceneClassDefinitions(scan, ctx) {
	const warnings = [];
	const classes = [];
	const enumDefs = {};
	const enumOrder = [];
	const vectors = {};
	const used = {};
	const reserved = {};
	for (let i = 0; i < MAPPING_TABLE_CLASSES.length; i++)
		reserved[MAPPING_TABLE_CLASSES[i]] = true;
	reserved.node2d = true;
	for (let i = 0; i < RESERVED_TYPE_NAMES.length; i++)
		reserved[RESERVED_TYPE_NAMES[i].toLowerCase()] = true;

	// A class_name shared by several scenes names none of them: they take their file names.
	const classNameCount = {};
	for (let i = 0; i < scan.scenes.length; i++) {
		const cn = scan.scenes[i].className;
		if (cn !== "")
			classNameCount[cn] = (classNameCount[cn] || 0) + 1;
	}

	const defs = [];
	for (let i = 0; i < scan.scenes.length; i++) {
		const sc = scan.scenes[i];
		let name = sc.className !== "" && classNameCount[sc.className] === 1 ? sc.className : pascalCase(stem(sc.scenePath));
		if (sc.className !== "" && classNameCount[sc.className] > 1)
			warnings.push(sc.sceneRes + ": class_name " + sc.className + " is the root script of " + classNameCount[sc.className] + " scenes; the class is named after the file, " + name);
		if (reserved[name.toLowerCase()]) {
			const renamed = name + "Scene";
			warnings.push(sc.sceneRes + ": '" + name + "' is a class the importer or the extension already gives a meaning; the class is named " + renamed);
			name = renamed;
		}
		if (used[name]) {
			warnings.push(sc.sceneRes + ": the class name " + name + " is taken by " + used[name] + "; the scene is skipped");
			continue;
		}
		used[name] = sc.sceneRes;
		const memberCtx = {
			godotProjectDir: ctx.godotProjectDir,
			tiledProjectDir: ctx.tiledProjectDir,
			classIndex: ctx.classIndex,
			knownEnums: ctx.knownEnums || {},
			className: name,
		};
		const members = [{ name: "godot_scene", type: "file", value: relativePath(ctx.tiledProjectDir, sc.scenePath) }];
		const notes = [];
		const skipped = [];
		const exportsList = sc.resolved.exports;
		for (let e = 0; e < exportsList.length; e++) {
			const ex = exportsList[e];
			if (ex.name === "godot_scene" || ex.name === "name" || ex.name === "script") {
				skipped.push({ name: ex.name, reason: "a name the importer never assigns" });
				continue;
			}
			const r = memberForExport(ex, sc.resolved, memberCtx, sc.scriptPath);
			if (r.skip !== undefined) {
				skipped.push({ name: ex.name, reason: r.skip });
				continue;
			}
			members.push(r.member);
			if (r.note !== undefined)
				notes.push(ex.name + ": " + r.note);
			if (r.vector !== undefined)
				vectors[r.vector] = true;
			if (r.enumDef !== undefined) {
				if (enumDefs[r.enumDef.name] === undefined) {
					enumDefs[r.enumDef.name] = r.enumDef;
					enumOrder.push(r.enumDef.name);
				}
			}
		}
		defs.push({
			type: "class",
			name: name,
			useAs: ["object", "tile"],
			color: stableColor(name),
			drawFill: true,
			members: members,
		});
		classes.push({ name: name, scene: sc.sceneRes, script: sc.scriptRes, notes: notes, skipped: skipped });
	}
	for (let i = 0; i < scan.skipped.length; i++)
		warnings.push(scan.skipped[i].sceneRes + ": skipped, " + scan.skipped[i].reason);
	const vectorDefs = [];
	if (vectors[VECTOR2_CLASS])
		vectorDefs.push(vectorClassDefinition(VECTOR2_CLASS));
	if (vectors[VECTOR2I_CLASS])
		vectorDefs.push(vectorClassDefinition(VECTOR2I_CLASS));
	return {
		defs: enumOrder.map(function (n) { return enumDefs[n]; }).concat(vectorDefs).concat(defs),
		classes: classes,
		warnings: warnings,
	};
}

// ---------------------------------------------------------------------------------------------
// As a .tiled-project fragment

// The definitions as Tiled writes them in "propertyTypes": keys sorted, members sorted by name,
// ids from `firstId` on. For comparing with expected fragments.
export function toPropertyTypesJson(defs, firstId) {
	const out = [];
	let id = firstId === undefined ? 1 : firstId;
	for (let i = 0; i < defs.length; i++) {
		const d = defs[i];
		if (d.type === "enum") {
			out.push(sortKeys({ id: id, name: d.name, storageType: d.storageType, type: "enum", values: d.values.slice(), valuesAsFlags: d.valuesAsFlags }));
		} else {
			const members = d.members.slice().sort(function (x, y) { return x.name < y.name ? -1 : (x.name > y.name ? 1 : 0); }).map(function (m) {
				const o = { name: m.name, type: m.type, value: m.value };
				if (m.propertyType !== undefined)
					o.propertyType = m.propertyType;
				return sortKeys(o);
			});
			out.push(sortKeys({ color: d.color, drawFill: d.drawFill, id: id, members: members, name: d.name, type: "class", useAs: d.useAs.slice() }));
		}
		id++;
	}
	return out;
}

// A copy of a JSON value with every object's keys sorted (how Qt writes JSON).
export function sortKeys(v) {
	if (Array.isArray(v))
		return v.map(sortKeys);
	if (v !== null && typeof v === "object") {
		const out = {};
		const keys = Object.keys(v).sort();
		for (let i = 0; i < keys.length; i++)
			out[keys[i]] = sortKeys(v[keys[i]]);
		return out;
	}
	return v;
}

// JSON text as Tiled writes a project file (QJsonDocument::Indented: 4 spaces, sorted keys).
export function tiledJsonText(value) {
	return JSON.stringify(sortKeys(value), null, 4) + "\n";
}
