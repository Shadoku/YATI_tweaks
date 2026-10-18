// A fake of the part of Tiled 1.12's scripting API the extension uses (api_surface.mjs lists it,
// with the reference page of each member), backed by real files: the project is a .tiled-project
// that every type or property change saves, as Tiled does (src/tiled/editableproject.cpp,
// scriptpropertytype.cpp: applyPropertyChanges() saves the project after each change), and maps
// are read from .tmx files.
//
// Every API object is wrapped so that reading or writing a member the fake has records it under
// its documenting class (Project.property is TiledObject.property); a member that is not in
// API_SURFACE throws. Reading a member the fake does not have gives undefined, as in Tiled for a
// member an object lacks (the glue probes values that way).
//
// Behaviour copied from Tiled 1.12's sources where it matters:
// - setMember with a whole JavaScript number stores an int (QJSEngine turns it into an int);
// - the project file is written with sorted keys and 4 spaces (QJsonDocument::Indented), class
//   members sorted by name (a QMap), "useAs" in Tiled's order, a new type's id = highest + 1;
// - a new class type is gray with drawFill and every usage, a new enum stores strings, no flags.

import fs from "node:fs";
import path from "node:path";
import { API_SURFACE, CLASS_CHAINS } from "./api_surface.mjs";
import { parseXml, child, childrenNamed } from "./xml.mjs";

const USAGE = {
	PropertyValueType: 0x001, LayerClass: 0x002, MapObjectClass: 0x004, MapClass: 0x008,
	TilesetClass: 0x010, TileClass: 0x020, WangSetClass: 0x040, WangColorClass: 0x080,
	ProjectClass: 0x100, AnyUsage: 0xFFF, AnyObjectClass: 0xFFE,
};
const USE_AS_ORDER = [
	["property", 0x001], ["map", 0x008], ["layer", 0x002], ["object", 0x004], ["tile", 0x020],
	["tileset", 0x010], ["wangcolor", 0x080], ["wangset", 0x040], ["project", 0x100],
];

function sortKeys(v) {
	if (Array.isArray(v))
		return v.map(sortKeys);
	if (v !== null && typeof v === "object") {
		const out = {};
		for (const k of Object.keys(v).sort())
			out[k] = sortKeys(v[k]);
		return out;
	}
	return v;
}

export function tiledJson(value) {
	return JSON.stringify(sortKeys(value), null, 4) + "\n";
}

// ---------------------------------------------------------------------------------------------
// Values

class FakeColor {
	constructor(text) {
		const h = String(text || "").replace(/^#/, "");
		this.valid = /^([0-9a-fA-F]{6}|[0-9a-fA-F]{8})$/.test(h);
		this.argb = this.valid ? (h.length === 6 ? "ff" + h : h).toLowerCase() : "";
	}
	// As QML prints a colour: #rrggbb when opaque, else #aarrggbb.
	toString() {
		if (!this.valid)
			return "";
		return this.argb.substring(0, 2) === "ff" ? "#" + this.argb.substring(2) : "#" + this.argb;
	}
	json() {
		return this.valid ? "#" + this.argb : "";
	}
}

// An int or float value as Tiled holds it (the JS number the API hands out is the same).
class Num {
	constructor(value, isFloat) {
		this.value = value;
		this.isFloat = isFloat;
	}
}

function filePathValue(local) {
	return { url: local === "" ? "" : "file://" + local, localFile: local };
}

function localFromUrl(url) {
	const s = String(url);
	if (s.indexOf("file://") === 0)
		return s.substring(7);
	return "";
}

// ---------------------------------------------------------------------------------------------
// Tracking

export class Recorder {
	constructor() {
		this.used = new Set();
	}
	wrap(obj, className) {
		const chain = CLASS_CHAINS[className];
		if (!chain)
			throw new Error("fake: no class chain for " + className);
		const recorder = this;
		const keyFor = function (prop) {
			for (const c of chain) {
				const key = c + "." + prop;
				if (API_SURFACE[key] !== undefined)
					return key;
			}
			return null;
		};
		return new Proxy(obj, {
			get(target, prop, receiver) {
				if (typeof prop === "symbol" || prop === "__fake" || prop === "toJSON" || prop === "then")
					return prop === "__fake" ? target : Reflect.get(target, prop, receiver);
				if (!(prop in target))
					return undefined;
				const key = keyFor(prop);
				if (key === null)
					throw new Error("The extension used " + className + "." + String(prop) + ", which is not in the documented API surface (api_surface.mjs)");
				recorder.used.add(key);
				const v = Reflect.get(target, prop, target);
				return typeof v === "function" ? v.bind(target) : v;
			},
			set(target, prop, value) {
				const key = keyFor(prop);
				if (key === null || !(prop in target))
					throw new Error("The extension set " + className + "." + String(prop) + ", which is not in the documented API surface (api_surface.mjs)");
				recorder.used.add(key);
				target[prop] = value;
				return true;
			},
		});
	}
	// A constructor (TextFile, Process) whose statics and `new` are recorded.
	wrapClass(Cls, className) {
		const recorder = this;
		return new Proxy(Cls, {
			construct(target, args) {
				recorder.used.add(className + ".constructor");
				return recorder.wrap(new target(...args), className);
			},
			get(target, prop) {
				if (typeof prop === "symbol" || prop === "prototype" || prop === "name" || prop === "length")
					return target[prop];
				if (!(prop in target))
					return undefined;
				const key = className + "." + String(prop);
				if (API_SURFACE[key] === undefined)
					throw new Error("The extension used " + key + ", which is not in the documented API surface (api_surface.mjs)");
				recorder.used.add(key);
				return target[prop];
			},
		});
	}
}

// ---------------------------------------------------------------------------------------------
// Property types

class FakeEnumType {
	constructor(project, name) {
		this._project = project;
		this._name = name;
		this._values = [];
		this._storage = 0;
		this._flags = false;
		this._id = 0;
	}
	get name() { return this._name; }
	get isClass() { return false; }
	get isEnum() { return true; }
	get storageType() { return this._storage; }
	set storageType(v) { this._storage = v; this._project._changed(); }
	get values() { return this._values.slice(); }
	set values(v) { this._values = Array.from(v, String); this._project._changed(); }
	nameOf(value) {
		if (typeof value === "object" && value !== null)
			value = value.value;
		return value >= 0 && value < this._values.length ? this._values[value] : "";
	}
	_defaultValue() {
		return { value: 0, typeId: this._id, typeName: this._name };
	}
	_json() {
		return {
			id: this._id, name: this._name, storageType: this._storage === 1 ? "int" : "string",
			type: "enum", values: this._values.slice(), valuesAsFlags: this._flags,
		};
	}
	// A value as the file stores it (string storage: names; int: the number).
	_export(v) {
		if (this._storage === 1)
			return { type: "int", value: v };
		if (this._flags) {
			const names = [];
			for (let i = 0; i < this._values.length; i++) {
				if (v & (1 << i))
					names.push(this._values[i]);
			}
			return { type: "string", value: names.join(",") };
		}
		return { type: "string", value: v >= 0 && v < this._values.length ? this._values[v] : String(v) };
	}
	// A file's value (a name, names, or a number) as the index or mask held in memory; null if invalid.
	_import(value) {
		if (typeof value === "number")
			return value;
		const names = String(value).split(",").map((s) => s.trim()).filter((s) => s !== "");
		if (this._flags) {
			let mask = 0;
			for (const n of names) {
				const i = this._values.indexOf(n);
				if (i < 0)
					return null;
				mask |= 1 << i;
			}
			return mask;
		}
		const i = this._values.indexOf(String(value));
		return i < 0 ? null : i;
	}
}

class FakeClassType {
	constructor(project, name) {
		this._project = project;
		this._name = name;
		this._members = {};		// name -> internal value
		this._color = new FakeColor("#ffa0a0a4");
		this._drawFill = true;
		this._usage = USAGE.AnyUsage;
		this._id = 0;
	}
	get name() { return this._name; }
	get isClass() { return true; }
	get isEnum() { return false; }
	get members() {
		const out = {};
		for (const k of Object.keys(this._members))
			out[k] = this._project._apiValue(this._members[k]);
		return out;
	}
	setMember(name, value) {
		let v;
		if (value !== null && typeof value === "object" && value.__fake instanceof FakeEnumType)
			v = value.__fake._defaultValue();
		else if (value !== null && typeof value === "object" && value.__fake instanceof FakeClassType)
			v = { value: {}, typeId: value.__fake._id, typeName: value.__fake._name };
		else
			v = this._project._fromScript(value);
		this._members[name] = v;
		this._project._changed();
	}
	removeMember(name) {
		if (!(name in this._members))
			return;
		delete this._members[name];
		this._project._changed();
	}
	get color() { return this._color; }
	set color(c) { this._color = c instanceof FakeColor ? c : new FakeColor(String(c)); this._project._changed(); }
	get drawFill() { return this._drawFill; }
	set drawFill(v) { this._drawFill = !!v; this._project._changed(); }
	get usageFlags() { return this._usage; }
	set usageFlags(v) { this._usage = v & USAGE.AnyUsage; this._project._changed(); }
	_json(projectDir) {
		const members = Object.keys(this._members).sort().map((k) => {
			const e = this._project._exportValue(this._members[k], projectDir);
			const m = { name: k, type: e.type, value: e.value };
			if (e.propertyType !== undefined)
				m.propertyType = e.propertyType;
			return m;
		});
		const useAs = USE_AS_ORDER.filter((x) => (this._usage & x[1]) !== 0).map((x) => x[0]);
		return { color: this._color.json(), drawFill: this._drawFill, id: this._id, members: members, name: this._name, type: "class", useAs: useAs };
	}
}

// ---------------------------------------------------------------------------------------------
// The project

class FakeProject {
	constructor(fake, fileName) {
		this._fake = fake;
		this._fileName = fileName || "";
		this._types = [];
		this._props = {};		// name -> internal value
		this._other = {};		// the file's other keys, kept
		this._nextId = 0;
		this.saves = 0;
		if (this._fileName !== "")
			this._load(fs.readFileSync(this._fileName, "utf8"));
	}
	get fileName() { return this._fileName; }
	get propertyTypes() { return this._types.map((t) => this._fake._wrapType(t)); }
	findTypeByName(name) {
		const t = this._types.find((x) => x._name === name);
		return t ? this._fake._wrapType(t) : null;
	}
	addClassType(name) { return this._add(new FakeClassType(this, name)); }
	addEnumType(name) { return this._add(new FakeEnumType(this, name)); }
	property(name) {
		const v = this._props[name];
		return v === undefined ? undefined : this._apiValue(v);
	}
	setProperty(name, value) {
		this._props[name] = this._fromScript(value);
		this._changed();
	}

	_add(t) {
		if (this._types.find((x) => x._name === t._name))
			throw new Error("The name '" + t._name + "' is already in use.");
		t._id = ++this._nextId;
		this._types.push(t);
		this._changed();
		return this._fake._wrapType(t);
	}
	_dir() { return path.dirname(this._fileName); }
	_changed() {
		if (this._loading || this._fileName === "")
			return;
		this.save();
	}
	save() {
		this.saves++;
		fs.writeFileSync(this._fileName, tiledJson(this._json()));
	}
	_json() {
		const dir = this._dir();
		const props = Object.keys(this._props).sort().map((k) => {
			// Tiled writes "propertytype" for every project property, "" when it has none
			// (src/libtiled/properties.cpp, propertiesToJson); a class member gets it only when set.
			const e = this._exportValue(this._props[k], dir);
			return { name: k, propertytype: e.propertyType === undefined ? "" : e.propertyType, type: e.type, value: e.value };
		});
		return Object.assign({}, this._other, {
			properties: props,
			propertyTypes: this._types.map((t) => t._json(dir)),
		});
	}
	_load(text) {
		this._loading = true;
		const json = JSON.parse(text);
		for (const k of Object.keys(json)) {
			if (k !== "properties" && k !== "propertyTypes")
				this._other[k] = json[k];
		}
		const dir = this._dir();
		const types = json.propertyTypes || [];
		for (const j of types) {
			const t = j.type === "enum" ? new FakeEnumType(this, j.name) : new FakeClassType(this, j.name);
			t._id = j.id;
			this._nextId = Math.max(this._nextId, j.id);
			if (j.type === "enum") {
				t._values = (j.values || []).slice();
				t._storage = j.storageType === "int" ? 1 : 0;
				t._flags = !!j.valuesAsFlags;
			} else {
				t._color = new FakeColor(j.color);
				t._drawFill = j.drawFill !== false;
				let usage = 0;
				for (const [n, f] of USE_AS_ORDER) {
					if ((j.useAs || []).indexOf(n) >= 0)
						usage |= f;
				}
				t._usage = usage;
			}
			this._types.push(t);
		}
		// Members after all types exist (they may name one another).
		for (const j of types) {
			if (j.type !== "class")
				continue;
			const t = this._types.find((x) => x._name === j.name);
			for (const m of j.members || [])
				t._members[m.name] = this._importValue(m.type, m.value, m.propertyType, dir);
		}
		for (const p of json.properties || [])
			this._props[p.name] = this._importValue(p.type, p.value, p.propertytype, dir);
		this._loading = false;
	}

	// Internal values: Num, string, boolean, {file: abs}, {objectRef: id}, FakeColor,
	// {propertyValue: {typeName, value}} (an enum's index/mask, a class's {member: internal}).
	_importValue(type, value, propertyType, dir) {
		const t = propertyType ? this._types.find((x) => x._name === propertyType) : undefined;
		if (t instanceof FakeEnumType) {
			const v = t._import(value);
			if (v !== null)
				return { propertyValue: { typeName: t._name, value: v } };
		}
		if (t instanceof FakeClassType && type === "class") {
			const members = {};
			for (const k of Object.keys(value || {})) {
				const mt = t._members[k];
				const isFloat = mt instanceof Num ? mt.isFloat : !Number.isInteger(value[k]);
				members[k] = typeof value[k] === "number" ? new Num(value[k], isFloat) : value[k];
			}
			return { propertyValue: { typeName: t._name, value: members } };
		}
		switch (type) {
		case "int": return new Num(Number(value), false);
		case "float": return new Num(Number(value), true);
		case "bool": return value === true || value === "true";
		case "color": return new FakeColor(value);
		case "file": return { file: value === "" ? "" : path.resolve(dir, String(value)) };
		case "object": return { objectRef: Number(value) };
		default: return String(value);
		}
	}
	_exportValue(v, dir) {
		if (v instanceof Num)
			return { type: v.isFloat ? "float" : "int", value: v.value };
		if (typeof v === "boolean")
			return { type: "bool", value: v };
		if (typeof v === "string")
			return { type: "string", value: v };
		if (v instanceof FakeColor)
			return { type: "color", value: v.json() };
		if (v.file !== undefined)
			return { type: "file", value: v.file === "" ? "" : path.relative(dir, v.file).split(path.sep).join("/") };
		if (v.objectRef !== undefined)
			return { type: "object", value: v.objectRef };
		const pv = v.propertyValue;
		const t = this._types.find((x) => x._name === pv.typeName);
		if (t instanceof FakeEnumType) {
			const e = t._export(pv.value);
			return { type: e.type, value: e.value, propertyType: pv.typeName };
		}
		const value = {};
		for (const k of Object.keys(pv.value))
			value[k] = this._exportValue(pv.value[k], dir).value;
		return { type: "class", value: value, propertyType: pv.typeName };
	}
	// What the API hands out for an internal value.
	_apiValue(v) {
		if (v instanceof Num)
			return v.value;
		if (typeof v === "boolean" || typeof v === "string")
			return v;
		if (v instanceof FakeColor)
			return v;
		if (v.file !== undefined)
			return filePathValue(v.file);
		if (v.objectRef !== undefined)
			return { id: v.objectRef };
		const pv = v.propertyValue;
		const t = this._types.find((x) => x._name === pv.typeName);
		let value = pv.value;
		if (t instanceof FakeClassType) {
			value = {};
			for (const k of Object.keys(pv.value))
				value[k] = this._apiValue(pv.value[k]);
		}
		return { value: value, typeId: t ? t._id : 0, typeName: pv.typeName };
	}
	// What a script's value becomes (EditableObject::propertyValueFromScript and QJSEngine).
	_fromScript(value) {
		if (typeof value === "number")
			return new Num(value, !Number.isInteger(value));
		if (typeof value === "boolean" || typeof value === "string")
			return value;
		if (value instanceof FakeColor)
			return value;
		if (value && value.__filePath)
			return { file: value.localFile };
		if (value && value.__objectRef)
			return { objectRef: value.id };
		if (value && value.__propertyValue) {
			const t = this._types.find((x) => x._name === value.typeName);
			let inner = value.value;
			if (t instanceof FakeClassType) {
				inner = {};
				for (const k of Object.keys(value.value))
					inner[k] = this._fromScript(value.value[k]);
			}
			return { propertyValue: { typeName: value.typeName, value: inner } };
		}
		throw new Error("fake: cannot store " + JSON.stringify(value));
	}
}

// ---------------------------------------------------------------------------------------------
// Maps (read from .tmx)

class FakeMapObject {
	constructor(map, el, layer, fileDir) {
		this._map = map;
		this._layer = layer;
		this._id = Number(el.attrs.id || 0);
		this._name = el.attrs.name || "";
		this._class = el.attrs.class || el.attrs.type || "";
		this._x = Number(el.attrs.x || 0);
		this._y = Number(el.attrs.y || 0);
		this._props = map._readProps(el, fileDir);
		this._templateProps = {};
		this._tile = null;
		let gid = el.attrs.gid !== undefined ? Number(el.attrs.gid) : 0;
		if (el.attrs.template !== undefined) {
			const tplPath = path.resolve(fileDir, el.attrs.template);
			if (fs.existsSync(tplPath)) {
				const tpl = parseXml(fs.readFileSync(tplPath, "utf8"));
				const obj = child(tpl, "object");
				if (obj) {
					if (this._class === "")
						this._class = obj.attrs.class || obj.attrs.type || "";
					if (this._name === "")
						this._name = obj.attrs.name || "";
					this._templateProps = map._readProps(obj, path.dirname(tplPath));
					if (gid === 0 && obj.attrs.gid !== undefined) {
						const ts = child(tpl, "tileset");
						gid = 0;
						if (ts)
							this._tile = map._tileFromTilesetEl(ts, Number(obj.attrs.gid) - Number(ts.attrs.firstgid || 1), path.dirname(tplPath));
					}
				}
			}
		}
		if (gid > 0)
			this._tile = map._tileForGid(gid & 0x0fffffff);
	}
	get id() { return this._id; }
	get name() { return this._name; }
	get className() { return this._class; }
	get x() { return this._x; }
	get y() { return this._y; }
	get tile() { return this._tile === null ? null : this._map._fake._rec.wrap(this._tile, "Tile"); }
	properties() { return this._map._apiProps(this._props); }
	resolvedProperties() {
		const merged = Object.assign({}, this._tile ? this._tile._props : {}, this._templateProps, this._props);
		return this._map._apiProps(merged);
	}
}

class FakeTile {
	constructor(map, id, cls, props) {
		this._map = map;
		this._id = id;
		this._class = cls;
		this._props = props;
	}
	get className() { return this._class; }
}

class FakeLayer {
	constructor(map, el, kind, fileDir) {
		this._map = map;
		this._kind = kind;
		this._id = Number(el.attrs.id || 0);
		this._name = el.attrs.name || "";
		this._class = el.attrs.class || "";
		this._props = map._readProps(el, fileDir);
		this._objects = [];
		this._layers = [];
		if (kind === "objectgroup") {
			for (const o of childrenNamed(el, "object"))
				this._objects.push(new FakeMapObject(map, o, this, fileDir));
		} else if (kind === "group") {
			this._layers = map._readLayers(el, fileDir);
		}
	}
	get id() { return this._id; }
	get name() { return this._name; }
	get className() { return this._class; }
	get isObjectLayer() { return this._kind === "objectgroup"; }
	get isGroupLayer() { return this._kind === "group"; }
	get objects() { return this._objects.map((o) => this._map._fake._rec.wrap(o, "MapObject")); }
	get layers() { return this._layers.map((l) => this._map._wrapLayer(l)); }
	properties() { return this._map._apiProps(this._props); }
}

// Reading <properties> and handing them out as Tiled does; the base of FakeMap and used by FakeTileset.
class PropertyReader {
	constructor(fake) {
		this._fake = fake;
		this._layers = [];
		this._tilesets = [];
	}
}

class FakeMap extends PropertyReader {
	constructor(fake, fileName) {
		super(fake);
		this._fileName = fileName;
		this._modified = false;
		this._selected = [];
		this._currentLayer = null;
		this._saves = 0;
		const dir = path.dirname(fileName);
		const root = parseXml(fs.readFileSync(fileName, "utf8"));
		this._tilesets = [];
		for (const ts of childrenNamed(root, "tileset"))
			this._tilesets.push({ firstgid: Number(ts.attrs.firstgid || 1), el: ts, dir: dir });
		this._props = this._readProps(root, dir);
		this._layers = this._readLayers(root, dir);
	}
	get fileName() { return this._fileName; }
	get isTileMap() { return true; }
	get isTileset() { return false; }
	get modified() { return this._modified; }
	get layers() { return this._layers.map((l) => this._wrapLayer(l)); }
	get selectedObjects() { return this._selected; }
	set selectedObjects(v) { this._selected = v; }
	get currentLayer() { return this._currentLayer; }
	set currentLayer(v) { this._currentLayer = v; }
	pixelToScreen(x, y) { return { x: x, y: y }; }
	properties() { return this._apiProps(this._props); }
	save() {
		this._fake._emit("assetAboutToBeSaved", this._fake._wrapAsset(this));
		this._saves++;
		this._modified = false;
		return true;
	}

	_wrapLayer(l) {
		const cls = l._kind === "objectgroup" ? "ObjectGroup" : (l._kind === "group" ? "GroupLayer" : "TileLayer");
		return this._fake._rec.wrap(l, cls);
	}
	_readLayers(el, dir) {
		const out = [];
		for (const c of el.children) {
			if (c.name === "objectgroup" || c.name === "group" || c.name === "layer" || c.name === "imagelayer")
				out.push(new FakeLayer(this, c, c.name, dir));
		}
		return out;
	}
	_tileFromTilesetEl(ts, localId, dir) {
		let el = ts;
		let tsDir = dir;
		if (ts.attrs.source !== undefined) {
			const p = path.resolve(dir, ts.attrs.source);
			if (!fs.existsSync(p))
				return null;
			el = parseXml(fs.readFileSync(p, "utf8"));
			tsDir = path.dirname(p);
		}
		for (const t of childrenNamed(el, "tile")) {
			if (Number(t.attrs.id) === localId)
				return new FakeTile(this, localId, t.attrs.class || t.attrs.type || "", this._readProps(t, tsDir));
		}
		return new FakeTile(this, localId, "", {});
	}
	_tileForGid(gid) {
		let best = null;
		for (const ts of this._tilesets) {
			if (ts.firstgid <= gid && (best === null || ts.firstgid > best.firstgid))
				best = ts;
		}
		return best === null ? null : this._tileFromTilesetEl(best.el, gid - best.firstgid, best.dir);
	}
	// <properties> of an element as {name: {type, value, propertyType, dir}} (raw, resolved later).
	_readProps(el, dir) {
		const out = {};
		const props = child(el, "properties");
		if (!props)
			return out;
		for (const p of childrenNamed(props, "property"))
			out[p.attrs.name] = this._readProp(p, dir);
		return out;
	}
	_readProp(p, dir) {
		const type = p.attrs.type || "string";
		const raw = { type: type, propertyType: p.attrs.propertytype || "", dir: dir };
		if (type === "class") {
			raw.value = this._readProps(p, dir);
		} else if (type === "list") {
			raw.value = childrenNamed(p, "item").map((it) => this._readProp(it, dir));
		} else {
			raw.value = p.attrs.value !== undefined ? p.attrs.value : p.text;
		}
		return raw;
	}
	_apiProps(raw) {
		const out = {};
		for (const k of Object.keys(raw))
			out[k] = this._apiValue(raw[k]);
		return out;
	}
	// A raw property as Tiled hands it out (TiledObject.property: file as FilePath, object as
	// MapObject when found else ObjectRef, enum/class as PropertyValue).
	_apiValue(raw) {
		const project = this._fake._project;
		const t = raw.propertyType ? project._types.find((x) => x._name === raw.propertyType) : undefined;
		if (t instanceof FakeEnumType) {
			const v = t._import(raw.type === "int" ? Number(raw.value) : raw.value);
			if (v !== null)
				return { value: v, typeId: t._id, typeName: t._name };
		}
		switch (raw.type) {
		case "int":
		case "float": return Number(raw.value);
		case "bool": return raw.value === "true";
		case "color": return new FakeColor(raw.value);
		case "file": {
			// A URL other than a local file (res://...) has no local path, as with a QUrl.
			if (/^[a-z]+:\/\//.test(raw.value))
				return { url: raw.value, localFile: "" };
			const local = raw.value === "" ? "" : path.resolve(raw.dir, raw.value);
			return filePathValue(local);
		}
		case "object": {
			const id = Number(raw.value);
			const o = this._findObject(id);
			return o !== null ? this._fake._rec.wrap(o, "MapObject") : { id: id };
		}
		case "class": {
			const value = {};
			for (const k of Object.keys(raw.value))
				value[k] = this._apiValue(raw.value[k]);
			return { value: value, typeId: t ? t._id : 0, typeName: raw.propertyType };
		}
		case "list": return raw.value.map((it) => this._apiValue(it));
		default: return String(raw.value);
		}
	}
	_findObject(id) {
		const walk = (layers) => {
			for (const l of layers) {
				for (const o of l._objects) {
					if (o._id === id)
						return o;
				}
				const r = walk(l._layers);
				if (r !== null)
					return r;
			}
			return null;
		};
		return id > 0 ? walk(this._layers) : null;
	}
}

for (const name of ["_readProps", "_readProp", "_apiProps", "_apiValue", "_findObject", "_tileFromTilesetEl", "_tileForGid"])
	PropertyReader.prototype[name] = FakeMap.prototype[name];

class FakeTileset {
	constructor(fake, fileName) {
		this._fake = fake;
		this._fileName = fileName;
		const root = parseXml(fs.readFileSync(fileName, "utf8"));
		this._name = root.attrs.name || "";
		this._map = new PropertyReader(fake);
		this._props = this._map._readProps(root, path.dirname(fileName));
	}
	get fileName() { return this._fileName; }
	get isTileMap() { return false; }
	get isTileset() { return true; }
	get name() { return this._name; }
	properties() { return this._map._apiProps(this._props); }
	save() {
		this._fake._emit("assetAboutToBeSaved", this._fake._wrapAsset(this));
		return true;
	}
}

// ---------------------------------------------------------------------------------------------
// Files and processes

class FakeTextFile {
	constructor(filePath, mode) {
		this._path = filePath;
		this._mode = mode === undefined ? FakeTextFile.ReadOnly : mode;
		this._buffer = "";
		if (this._mode === FakeTextFile.ReadOnly && !fs.existsSync(filePath))
			throw new Error("Could not open '" + filePath + "'");
	}
	readAll() { return fs.readFileSync(this._path, "utf8"); }
	write(text) { this._buffer += text; }
	commit() { fs.writeFileSync(this._path, this._buffer); }
	close() {}
}
FakeTextFile.ReadOnly = 0;
FakeTextFile.WriteOnly = 1;

const FILE_FLAGS = { Dirs: 0x001, Files: 0x002, Hidden: 0x100, NoDotAndDotDot: 0x6000 };

function fakeFileModule() {
	return {
		Dirs: FILE_FLAGS.Dirs,
		Files: FILE_FLAGS.Files,
		Hidden: FILE_FLAGS.Hidden,
		NoDotAndDotDot: FILE_FLAGS.NoDotAndDotDot,
		exists(p) { return fs.existsSync(p); },
		directoryEntries(dir, filters) {
			let entries;
			try {
				entries = fs.readdirSync(dir, { withFileTypes: true });
			} catch (e) {
				return [];
			}
			const f = filters === undefined ? 0x007 : filters;
			return entries.filter((e) => {
				if (e.name.charAt(0) === "." && (f & FILE_FLAGS.Hidden) === 0)
					return false;
				if (e.isDirectory())
					return (f & FILE_FLAGS.Dirs) !== 0;
				return (f & FILE_FLAGS.Files) !== 0;
			}).map((e) => e.name).sort();
		},
	};
}

// ---------------------------------------------------------------------------------------------
// The fake

// options: {projectFile, prompts: [answers], directories: [answers], confirms: [answers],
//           process: function (binary, args) -> {code, stdout, stderr}}
export function createFakeTiled(options) {
	const opts = options || {};
	const rec = new Recorder();
	const out = {
		alerts: [], logs: [], warnings: [], errors: [], actions: {}, menus: {}, issues: [],
		processes: [],
	};
	const signals = { assetAboutToBeSaved: [] };
	const prompts = (opts.prompts || []).slice();
	const directories = (opts.directories || []).slice();
	const confirms = (opts.confirms || []).slice();

	const fake = {
		_rec: rec,
		_project: null,
		_emit(name, arg) {
			for (const cb of signals[name])
				cb(arg);
		},
		_wrapType(t) {
			return rec.wrap(t, t instanceof FakeEnumType ? "EnumPropertyType" : "ClassPropertyType");
		},
		_wrapAsset(a) {
			return rec.wrap(a, a instanceof FakeTileset ? "Tileset" : "TileMap");
		},
	};
	fake._project = new FakeProject(fake, opts.projectFile || "");
	let activeAsset = null;
	const openAssets = [];
	const mapView = { centered: [], centerOn(x, y) { this.centered.push([x, y]); } };
	const mapEditor = { get currentMapView() { return rec.wrap(mapView, "MapView"); } };

	const tiledModule = {
		// null without a project, as in Tiled 1.12 (ProjectModel::editableProject() is nullptr when
		// no project document is loaded: at startup without one, after Project > Close Project).
		get project() { return fake._project._fileName === "" ? null : rec.wrap(fake._project, "Project"); },
		get activeAsset() { return activeAsset === null ? null : fake._wrapAsset(activeAsset); },
		set activeAsset(a) { activeAsset = a === null ? null : (a.__fake || a); },
		get openAssets() { return openAssets.map((a) => fake._wrapAsset(a)); },
		get mapEditor() { return rec.wrap(mapEditor, "MapEditor"); },
		get assetAboutToBeSaved() {
			return rec.wrap({ connect(cb) { signals.assetAboutToBeSaved.push(cb); } }, "Signal");
		},
		registerAction(id, cb) {
			const action = { id: id, text: "", trigger() { cb(this); } };
			out.actions[id] = action;
			return rec.wrap(action, "Action");
		},
		extendMenu(menu, items) {
			if (["File", "Edit", "View", "Map", "Layer", "Tileset", "Help"].indexOf(menu) < 0)
				throw new Error("Unknown menu");
			for (const it of items) {
				if (it.action !== undefined && !out.actions[it.action])
					throw new Error("Unknown action: '" + it.action + "'");
				if (it.action === undefined && !it.separator)
					throw new Error("Non-separator item without action");
			}
			out.menus[menu] = (out.menus[menu] || []).concat(items);
		},
		open(fileName) {
			const a = fileName.endsWith(".tsx") ? new FakeTileset(fake, fileName) : new FakeMap(fake, fileName);
			openAssets.push(a);
			activeAsset = a;
			return fake._wrapAsset(a);
		},
		alert(text, title) { out.alerts.push(text); },
		confirm(text, title) {
			out.logs.push("confirm: " + text);
			return confirms.length > 0 ? confirms.shift() : true;
		},
		prompt(label, text, title) {
			return prompts.length > 0 ? prompts.shift() : "";
		},
		promptDirectory(defaultDir, title) {
			return directories.length > 0 ? directories.shift() : "";
		},
		log(text) { out.logs.push(text); },
		warn(text, activated) { out.warnings.push(text); out.issues.push({ severity: "warning", text: text, activate: activated }); },
		error(text, activated) { out.errors.push(text); out.issues.push({ severity: "error", text: text, activate: activated }); },
		color(name) { return new FakeColor(name); },
		filePath(url) { const local = localFromUrl(url); return { __filePath: true, url: url, localFile: local }; },
		objectRef(id) { return { __objectRef: true, id: id }; },
		propertyValue(typeName, value) {
			const t = fake._project._types.find((x) => x._name === typeName);
			if (!t)
				throw new Error("Unknown type: " + typeName);
			if (t instanceof FakeEnumType) {
				const v = t._import(value);
				return { __propertyValue: true, typeName: typeName, value: v === null ? 0 : v };
			}
			if (value === null || typeof value !== "object")
				throw new Error("Expected object to initialize class value");
			return { __propertyValue: true, typeName: typeName, value: value };
		},
	};

	class FakeProcess {
		constructor() {
			this._out = "";
			this._err = "";
		}
		exec(binary, args, throwOnError) {
			out.processes.push({ binary: binary, args: args.slice() });
			const r = opts.process ? opts.process(binary, args) : { code: -1, stdout: "", stderr: "" };
			this._out = r.stdout || "";
			this._err = r.stderr || "";
			return r.code;
		}
		readStdOut() { return this._out; }
		readStdErr() { return this._err; }
		close() {}
	}

	const api = {
		tiled: rec.wrap(tiledModule, "tiled"),
		TextFile: rec.wrapClass(FakeTextFile, "TextFile"),
		File: rec.wrap(fakeFileModule(), "File"),
		Process: rec.wrapClass(FakeProcess, "Process"),
		ClassPropertyType: rec.wrap({ PropertyValueType: USAGE.PropertyValueType, MapObjectClass: USAGE.MapObjectClass, TileClass: USAGE.TileClass }, "ClassPropertyType"),
		StorageType: rec.wrap({ StringValue: 0, IntValue: 1 }, "StorageType"),
	};

	return {
		api: api,
		out: out,
		used: rec.used,
		project: fake._project,
		mapView: mapView,
		// The answers the dialogs give, in order (push more after creating the fake).
		answers: { prompts: prompts, directories: directories, confirms: confirms },
		// Opens a map or tileset file as the active asset (as the user would), without recording.
		openFile(fileName) {
			const a = fileName.endsWith(".tsx") ? new FakeTileset(fake, fileName) : new FakeMap(fake, fileName);
			openAssets.push(a);
			activeAsset = a;
			return a;
		},
		trigger(actionId) { out.actions[actionId].trigger(); },
		// Closes every open asset (an Issue's activation then has to open its map).
		closeAll() { openAssets.length = 0; activeAsset = null; },
		emitAboutToBeSaved(asset) { fake._emit("assetAboutToBeSaved", fake._wrapAsset(asset)); },
	};
}
