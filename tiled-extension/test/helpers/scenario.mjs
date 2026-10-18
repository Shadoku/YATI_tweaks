// A Tiled project in a temporary directory next to a copy of the fixture Godot project, with the
// fake API and the extension installed: what glue.test.mjs and api_surface.test.mjs drive.
import fs from "node:fs";
import path from "node:path";
import { createFakeTiled, tiledJson } from "./fake_tiled.mjs";
import { install } from "../../yati-godot/yati-godot.mjs";
import { GODOT_FIXTURE, MAP_FIXTURES, IMPORTER_FIXTURES, FIXTURES, tempDir, copyDir } from "./node_io.mjs";

// options: {properties: [{name, type, value}], propertyTypes, checkScript: bool, fakeOptions,
//           noProject: bool, inMaps: bool (the project file in maps/, where game.tiled-project's
//           relative paths hold)}
export function scenario(options) {
	const o = options || {};
	const root = tempDir();
	const godot = path.join(root, "godot_project");
	copyDir(GODOT_FIXTURE, godot);
	if (o.checkScript) {
		fs.mkdirSync(path.join(godot, "addons", "yati_native"), { recursive: true });
		fs.writeFileSync(path.join(godot, "addons", "yati_native", "check_map.gd"), "extends SceneTree\n");
	}
	const maps = path.join(root, "maps");
	copyDir(MAP_FIXTURES, maps);
	copyDir(IMPORTER_FIXTURES, path.join(root, "importer"));
	let projectFile = "";
	if (!o.noProject) {
		projectFile = path.join(o.inMaps ? maps : root, "game.tiled-project");
		fs.writeFileSync(projectFile, tiledJson({
			automappingRulesFile: "", commands: [], compatibilityVersion: 1100, extensionsPath: "extensions",
			folders: ["."], properties: o.properties || [], propertyTypes: o.propertyTypes || [],
		}));
	}
	const fake = createFakeTiled(Object.assign({ projectFile: projectFile }, o.fakeOptions || {}));
	install(fake.api);
	return {
		root: root, godot: godot, maps: maps, projectFile: projectFile, fake: fake,
		cleanup() { fs.rmSync(root, { recursive: true, force: true }); },
		projectJson() { return JSON.parse(fs.readFileSync(projectFile, "utf8")); },
		property(name) {
			const p = this.projectJson().properties.find((x) => x.name === name);
			return p === undefined ? undefined : p.value;
		},
	};
}

export const CHECK_OUTPUT = fs.readFileSync(path.join(FIXTURES, "check_output.txt"), "utf8");

// The types of maps/game.tiled-project, for a scenario that validates the fixture maps.
export function gameProjectTypes() {
	return JSON.parse(fs.readFileSync(path.join(MAP_FIXTURES, "game.tiled-project"), "utf8")).propertyTypes;
}
