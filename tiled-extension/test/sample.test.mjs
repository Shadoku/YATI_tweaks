// tiled-extension/sample/: the .tiled-project the sync and the scenes action make from the sample
// Godot project equals the committed sample/tiled/sample.tiled-project (what the user opens in
// Tiled for the manual checklist), and the sample map's mistakes are the ones the README lists.
import { test, after } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { createFakeTiled } from "./helpers/fake_tiled.mjs";
import { install, ACTIONS } from "../yati-godot/yati-godot.mjs";
import { SAMPLE_DIR, FIXTURES, tempDir, copyDir } from "./helpers/node_io.mjs";

const EXPECTED_PROJECT = path.join(SAMPLE_DIR, "tiled", "sample.tiled-project");
const dirs = [];
after(() => { for (const d of dirs) fs.rmSync(d, { recursive: true, force: true }); });

function generate() {
	const root = tempDir();
	dirs.push(root);
	copyDir(path.join(SAMPLE_DIR, "godot"), path.join(root, "godot"));
	copyDir(path.join(SAMPLE_DIR, "tiled", "maps"), path.join(root, "tiled", "maps"));
	const projectFile = path.join(root, "tiled", "sample.tiled-project");
	fs.copyFileSync(path.join(FIXTURES, "sample_start.tiled-project"), projectFile);
	const fake = createFakeTiled({ projectFile: projectFile });
	install(fake.api);
	fake.trigger(ACTIONS.sync);
	fake.trigger(ACTIONS.scenes);
	return { root: root, projectFile: projectFile, fake: fake };
}

test("sync + scenes on the sample give the committed sample.tiled-project", () => {
	const g = generate();
	const text = fs.readFileSync(g.projectFile, "utf8");
	if (process.env.UPDATE_EXPECTED === "1")
		fs.writeFileSync(EXPECTED_PROJECT, text);
	assert.equal(text, fs.readFileSync(EXPECTED_PROJECT, "utf8"));
	assert.deepEqual(g.fake.out.errors, []);
	assert.match(g.fake.out.alerts[1], /Imported 2 scene\(s\) as classes\./);
	assert.match(g.fake.out.alerts[1], /res:\/\/scenes\/torch\.tscn: skipped, its root has no script/);
});

test("the sample reopened (Tiled reads the file): running both again changes nothing", () => {
	const g = generate();
	const before = fs.readFileSync(g.projectFile, "utf8");
	const again = createFakeTiled({ projectFile: g.projectFile });
	install(again.api);
	again.trigger(ACTIONS.sync);
	again.trigger(ACTIONS.scenes);
	assert.equal(again.project.saves, 0);
	assert.equal(fs.readFileSync(g.projectFile, "utf8"), before);
	assert.ok(again.out.alerts.every((a) => a.indexOf("Reopen the project") < 0));
});

test("the sample map's three mistakes, as the manual checklist says", () => {
	const g = generate();
	const fresh = createFakeTiled({ projectFile: g.projectFile });
	install(fresh.api);
	const map = fresh.openFile(path.join(g.root, "tiled", "maps", "demo.tmx"));
	fresh.emitAboutToBeSaved(map);
	assert.deepEqual(fresh.out.issues.map((i) => i.severity + ": " + i.text.replace(/ \(.*?\)/, "").split(":").slice(0, 3).join(":")), [
		"warning: YATI: demo.tmx: Object 'Typo'",
		"error: YATI: demo.tmx: Object 'Box'",
		"warning: YATI: demo.tmx: Object 'Wall'",
	]);
	assert.match(fresh.out.issues[0].text, /class 'Slme' is no Godot node type/);
	assert.match(fresh.out.issues[1].text, /class 'instance' needs a file property 'res_path'/);
	assert.match(fresh.out.issues[2].text, /mask 'collision_layer' names 'Ghosts', which GodotPhysicsLayers does not have/);
});
