// The generated definitions (types.mjs) applied to the open project through Tiled's API, merged by
// name: a type is added when missing and updated in place when present (its id, and so every
// reference to it, stays); a class member is added or updated when the extension generated it
// (the project property yati_generated records which), kept when it was made by hand, and removed
// only when the extension generated it and the script no longer has it; a class's colour and fill
// are set only when the class is new.
//
// Then what the API cannot set is patched into the project file's JSON (patchProjectText): an
// enum's "Allow multiple values (flags)" (EnumPropertyType has no valuesAsFlags in Tiled 1.12) and
// a float member whose value is a whole number (setMember stores a whole JavaScript number as int).
//
// Pure: `api` is {tiled, ClassPropertyType, StorageType} (the glue passes Tiled's globals; the
// last two may be null).

import { fileUrl, joinPath, cleanPath } from "./settings.mjs";
import { sortKeys } from "./types.mjs";

// Tiled 1.12's values (src/tiled/scriptpropertytype.h, ClassUsageFlag), used when the global
// ClassPropertyType does not expose the constants the reference names.
const FALLBACK_USAGE = {
	PropertyValueType: 0x001, LayerClass: 0x002, MapObjectClass: 0x004, MapClass: 0x008,
	TilesetClass: 0x010, TileClass: 0x020, WangSetClass: 0x040, WangColorClass: 0x080,
	ProjectClass: 0x100,
};
const USE_AS_FLAG = {
	property: "PropertyValueType", map: "MapClass", layer: "LayerClass", object: "MapObjectClass",
	tile: "TileClass", tileset: "TilesetClass", wangcolor: "WangColorClass", wangset: "WangSetClass",
	project: "ProjectClass",
};

export function usageFlagsFor(api, useAs) {
	let flags = 0;
	for (let i = 0; i < useAs.length; i++) {
		const key = USE_AS_FLAG[useAs[i]];
		if (key === undefined)
			continue;
		let v = api.ClassPropertyType ? api.ClassPropertyType[key] : undefined;
		if (typeof v !== "number")
			v = FALLBACK_USAGE[key];
		flags |= v;
	}
	return flags;
}

// EnumPropertyType.storageType's value for "int" / "string": the reference's StorageType enum
// (StringValue, IntValue). Tiled 1.12's sources register no global StorageType (only the enum
// inside EnumPropertyType), so without one the enum's order gives 0 and 1.
export function storageTypeValue(api, storage) {
	const key = storage === "int" ? "IntValue" : "StringValue";
	const v = api.StorageType ? api.StorageType[key] : undefined;
	if (typeof v === "number")
		return v;
	return storage === "int" ? 1 : 0;
}

function arraysEqual(a, b) {
	if (!a || !b || a.length !== b.length)
		return false;
	for (let i = 0; i < a.length; i++) {
		if (String(a[i]) !== String(b[i]))
			return false;
	}
	return true;
}

// The value setMember takes for a member definition.
export function memberApiValue(api, member, projectDir) {
	const t = api.tiled;
	switch (member.type) {
	case "file":
		return t.filePath(member.value === "" ? "" : fileUrl(joinPath(projectDir, member.value)));
	case "object":
		return t.objectRef(member.value);
	case "color":
		return t.color(member.value);
	default:
		break;
	}
	if (member.propertyType !== undefined)
		return t.propertyValue(member.propertyType, member.value);
	return member.value;
}

// Whether a member's current value (ClassPropertyType.members[name]) already is the definition's.
// When in doubt, false: setting it again is harmless.
export function sameMemberValue(api, existing, member, projectDir, enumValues) {
	if (existing === undefined || existing === null)
		return false;
	const ex = existing;
	switch (member.type) {
	case "file": {
		if (typeof ex !== "object")
			return false;
		const local = ex.localFile !== undefined ? String(ex.localFile) : "";
		if (member.value === "")
			return local === "" && (ex.url === undefined || String(ex.url) === "");
		return local !== "" && cleanPath(local) === joinPath(projectDir, member.value);
	}
	case "object":
		return typeof ex === "object" && ex.id === member.value;
	case "color": {
		const s = String(ex).toLowerCase();
		const want = String(member.value).toLowerCase();
		if (want === "")
			return s === "";
		// #aarrggbb, or #rrggbb when opaque
		return s === want || (want.substring(1, 3) === "ff" && s === "#" + want.substring(3));
	}
	default:
		break;
	}
	if (member.propertyType !== undefined) {
		if (typeof ex !== "object" || ex.typeName !== member.propertyType)
			return false;
		if (member.type === "class") {
			const keys = Object.keys(member.value);
			const have = ex.value || {};
			for (let i = 0; i < keys.length; i++) {
				if (have[keys[i]] !== member.value[keys[i]])
					return false;
			}
			return Object.keys(have).length === keys.length;
		}
		if (typeof member.value === "number")
			return ex.value === member.value;
		// string storage: the value in memory is the index of the name
		return enumValues !== null && enumValues !== undefined && ex.value === enumValues.indexOf(member.value);
	}
	if (typeof member.value === "number")
		return typeof ex === "number" && ex === member.value;
	return ex === member.value;
}

// Applies `defs` (in order). `record` is the yati_generated record (read, not changed); the result
// carries the new one. Returns {created, updated, unchanged, kept: [{type, member}], removed:
// [{type, member}], errors, record, fixups: {flagsEnums: [names], memberTypes: [{type, member,
// memberType}]}}.
export function applyDefinitions(api, defs, record, projectDir) {
	const project = api.tiled.project;
	const report = {
		created: [], updated: [], unchanged: [], kept: [], removed: [], errors: [],
		record: {}, fixups: { flagsEnums: [], memberTypes: [] },
	};
	const keys = Object.keys(record);
	for (let i = 0; i < keys.length; i++)
		report.record[keys[i]] = record[keys[i]].slice();
	const enumValuesByName = {};
	for (let i = 0; i < defs.length; i++) {
		if (defs[i].type === "enum")
			enumValuesByName[defs[i].name] = defs[i].values;
	}

	for (let i = 0; i < defs.length; i++) {
		const def = defs[i];
		let t = project.findTypeByName(def.name);
		if (def.type === "enum") {
			if (t !== null && !t.isEnum) {
				report.errors.push("'" + def.name + "' is a class in this project; the enum was not generated.");
				continue;
			}
			let changed = false;
			const isNew = t === null;
			if (isNew)
				t = project.addEnumType(def.name);
			const storage = storageTypeValue(api, def.storageType);
			if (t.storageType !== storage) {
				t.storageType = storage;
				changed = true;
			}
			if (!arraysEqual(t.values, def.values)) {
				t.values = def.values.slice();
				changed = true;
			}
			if (def.valuesAsFlags)
				report.fixups.flagsEnums.push(def.name);
			report.record[def.name] = [];
			(isNew ? report.created : (changed ? report.updated : report.unchanged)).push(def.name);
			continue;
		}

		if (t !== null && !t.isClass) {
			report.errors.push("'" + def.name + "' is an enum in this project; the class was not generated.");
			continue;
		}
		const isNew = t === null;
		let changed = false;
		if (isNew) {
			t = project.addClassType(def.name);
			t.color = api.tiled.color(def.color);
			t.drawFill = def.drawFill;
			t.usageFlags = usageFlagsFor(api, def.useAs);
		} else {
			const want = usageFlagsFor(api, def.useAs);
			if ((t.usageFlags & want) !== want) {
				t.usageFlags = t.usageFlags | want;
				changed = true;
			}
		}
		const previous = record[def.name] || [];
		const existing = t.members || {};
		const generated = [];
		for (let m = 0; m < def.members.length; m++) {
			const member = def.members[m];
			const has = Object.prototype.hasOwnProperty.call(existing, member.name);
			if (has && !isNew && previous.indexOf(member.name) < 0) {
				report.kept.push({ type: def.name, member: member.name });
				continue;
			}
			generated.push(member.name);
			if (member.type === "int" || member.type === "float")
				report.fixups.memberTypes.push({ type: def.name, member: member.name, memberType: member.type });
			let enumValues = enumValuesByName[member.propertyType];
			if (enumValues === undefined && member.propertyType !== undefined) {
				const et = project.findTypeByName(member.propertyType);
				enumValues = et !== null && et.isEnum ? et.values : null;
			}
			if (has && sameMemberValue(api, existing[member.name], member, projectDir, enumValues))
				continue;
			t.setMember(member.name, memberApiValue(api, member, projectDir));
			changed = true;
		}
		for (let p = 0; p < previous.length; p++) {
			const name = previous[p];
			// A member generated before and still in the definition is in `generated`.
			if (generated.indexOf(name) >= 0)
				continue;
			if (Object.prototype.hasOwnProperty.call(existing, name)) {
				t.removeMember(name);
				report.removed.push({ type: def.name, member: name });
				changed = true;
			}
		}
		report.record[def.name] = generated;
		(isNew ? report.created : (changed ? report.updated : report.unchanged)).push(def.name);
	}
	return report;
}

// The kept fixups (settings.parseFixups) merged with a run's: this run's flags enums and float
// members added, members this run made int or no longer generates dropped. `record` is the run's
// yati_generated record. Returns {kept (for yati_fixups), patch (for patchProjectText)}.
export function mergeFixups(kept, run, record) {
	const flags = kept.flags.slice();
	for (let i = 0; i < run.flagsEnums.length; i++) {
		if (flags.indexOf(run.flagsEnums[i]) < 0)
			flags.push(run.flagsEnums[i]);
	}
	const floats = {};
	const keys = Object.keys(kept.floats);
	for (let i = 0; i < keys.length; i++)
		floats[keys[i]] = kept.floats[keys[i]].slice();
	for (let i = 0; i < run.memberTypes.length; i++) {
		const f = run.memberTypes[i];
		const list = floats[f.type] || [];
		const at = list.indexOf(f.member);
		if (f.memberType === "float" && at < 0)
			list.push(f.member);
		else if (f.memberType === "int" && at >= 0)
			list.splice(at, 1);
		floats[f.type] = list;
	}
	// Only members still generated.
	const types = Object.keys(floats);
	for (let i = 0; i < types.length; i++) {
		const generated = record[types[i]];
		floats[types[i]] = generated === undefined ? [] : floats[types[i]].filter(function (m) { return generated.indexOf(m) >= 0; });
	}
	const memberTypes = run.memberTypes.slice();
	for (let i = 0; i < types.length; i++) {
		for (let k = 0; k < floats[types[i]].length; k++) {
			const exists = memberTypes.some(function (x) { return x.type === types[i] && x.member === floats[types[i]][k]; });
			if (!exists)
				memberTypes.push({ type: types[i], member: floats[types[i]][k], memberType: "float" });
		}
	}
	return {
		kept: { flags: flags, floats: floats },
		patch: { flagsEnums: flags, memberTypes: memberTypes },
	};
}

// The project file's text with the fixups applied: flags on the named enums, int/float on the
// named members, and, when something changed, `properties` ({name: string value}) set as project
// properties of the file (added or replaced, as Tiled writes them). {text, changes:
// [descriptions]}; `text` is the input when nothing changed (then the properties are not written
// either: they ride only with a change). Written back as Tiled writes it (keys sorted, 4 spaces).
export function patchProjectText(text, fixups, properties) {
	let json;
	try {
		json = JSON.parse(text);
	} catch (e) {
		return { text: text, changes: [], error: "The project file is no JSON: " + e.message };
	}
	const types = Array.isArray(json.propertyTypes) ? json.propertyTypes : [];
	const byName = {};
	for (let i = 0; i < types.length; i++)
		byName[types[i].name] = types[i];
	const changes = [];
	const missing = [];
	for (let i = 0; i < fixups.flagsEnums.length; i++) {
		const t = byName[fixups.flagsEnums[i]];
		if (t === undefined || t.type !== "enum") {
			missing.push(fixups.flagsEnums[i]);
			continue;
		}
		if (t.valuesAsFlags !== true) {
			t.valuesAsFlags = true;
			changes.push(t.name + ": values as flags");
		}
	}
	for (let i = 0; i < fixups.memberTypes.length; i++) {
		const f = fixups.memberTypes[i];
		const t = byName[f.type];
		if (t === undefined || t.type !== "class" || !Array.isArray(t.members)) {
			missing.push(f.type);
			continue;
		}
		for (let m = 0; m < t.members.length; m++) {
			const mem = t.members[m];
			if (mem.name !== f.member || mem.propertyType !== undefined)
				continue;
			if ((mem.type === "int" || mem.type === "float") && mem.type !== f.memberType) {
				mem.type = f.memberType;
				if (f.memberType === "int" && typeof mem.value === "number")
					mem.value = Math.round(mem.value);
				changes.push(t.name + "." + f.member + ": " + f.memberType);
			}
		}
	}
	if (changes.length === 0)
		return { text: text, changes: changes, missing: missing };
	const names = Object.keys(properties || {});
	if (names.length > 0) {
		if (!Array.isArray(json.properties))
			json.properties = [];
		for (let i = 0; i < names.length; i++) {
			let entry = null;
			for (let k = 0; k < json.properties.length; k++) {
				if (json.properties[k].name === names[i])
					entry = json.properties[k];
			}
			if (entry === null) {
				entry = { name: names[i], propertytype: "", type: "string", value: "" };
				json.properties.push(entry);
			}
			entry.type = "string";
			entry.value = String(properties[names[i]]);
		}
		json.properties.sort(function (a, b) { return a.name < b.name ? -1 : a.name > b.name ? 1 : 0; });
	}
	return { text: JSON.stringify(sortKeys(json), null, 4) + "\n", changes: changes, missing: missing };
}
