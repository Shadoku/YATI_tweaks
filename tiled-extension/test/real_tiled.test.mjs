// The extension inside a real Tiled, headless. tools/install_tiled.sh's Tiled (or $TILED) runs
// helpers/real_tiled_harness.mjs with --project and --evaluate, under xvfb-run when there is no
// display, with its own config directory (no user extensions, no session). This covers the
// README's manual checklist steps 1 to 3 and 5 on the sample (the module loads with its imports,
// the sync, the scenes, "Unchanged" on a second run, the project file equal to the committed one,
// the save hook's three findings with Tiled's own property values) and the no-project case.
// Skipped when no Tiled is found; CI installs one.
import { test, after } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { SAMPLE_DIR, FIXTURES, TEST_DIR, tempDir, copyDir } from "./helpers/node_io.mjs";

const ROOT = path.dirname(path.dirname(TEST_DIR));
const HARNESS = path.join(TEST_DIR, "helpers", "real_tiled_harness.mjs");
const EXPECTED_PROJECT = path.join(SAMPLE_DIR, "tiled", "sample.tiled-project");

function findTiled() {
	if (process.env.TILED)
		return process.env.TILED;
	let version = "1.12.0";
	try {
		version = fs.readFileSync(path.join(ROOT, ".tiled-version"), "utf8").trim();
	} catch (e) {
		// the default
	}
	const cached = path.join(process.env.TILED_CACHE_DIR || path.join(os.homedir(), ".cache", "tiled"), version, "tiled");
	return fs.existsSync(cached) ? cached : null;
}

function onPath(name) {
	return spawnSync("sh", ["-c", "command -v " + name], { encoding: "utf8" }).status === 0;
}

const TILED = findTiled();
const SKIP = TILED === null ? "no Tiled: bash tools/install_tiled.sh (Linux x86_64), or TILED=<tiled binary>" : false;
const dirs = [];
after(() => { for (const d of dirs) fs.rmSync(d, { recursive: true, force: true }); });

// One Tiled process running the harness's steps; the harness's log lines.
function runTiled(projectFile, steps) {
	const config = tempDir("yati-tiled-config-");
	dirs.push(config);
	let cmd = TILED;
	let args = [];
	if (!process.env.DISPLAY && onPath("xvfb-run")) {
		cmd = "xvfb-run";
		args = ["-a", TILED];
	}
	if (projectFile)
		args.push("--project", projectFile);
	args = args.concat(["--evaluate", HARNESS], steps);
	const r = spawnSync(cmd, args, {
		encoding: "utf8",
		timeout: 180000,
		env: Object.assign({}, process.env, { XDG_CONFIG_HOME: config, LANG: "C.UTF-8", LC_ALL: "C.UTF-8" }),
	});
	const text = (r.stdout || "") + (r.stderr || "");
	assert.equal(r.status, 0, "tiled exited with " + r.status + (r.error ? " (" + r.error.message + ")" : "") + ":\n" + text);
	assert.match(text, /^HARNESS done: \d+ alert\(s\)$/m, "the harness did not finish (an exception in the module?):\n" + text);
	return text.split(/\r?\n/).filter((l) => /^(HARNESS |ALERT |CONFIRM |PROMPT |ISSUE |YATI)/.test(l));
}

// The sample as the manual checklist's step 1 sets it up: the Godot project, the maps, and the
// project file with only the godot_project setting.
function sampleCopy() {
	const root = tempDir("yati-real-tiled-");
	dirs.push(root);
	copyDir(path.join(SAMPLE_DIR, "godot"), path.join(root, "godot"));
	copyDir(path.join(SAMPLE_DIR, "tiled", "maps"), path.join(root, "tiled", "maps"));
	const projectFile = path.join(root, "tiled", "sample.tiled-project");
	fs.copyFileSync(path.join(FIXTURES, "sample_start.tiled-project"), projectFile);
	return { root: root, projectFile: projectFile };
}

const has = (lines, line) => assert.ok(lines.indexOf(line) >= 0, "missing '" + line + "' in:\n" + lines.join("\n"));
const some = (lines, re) => assert.ok(lines.some((l) => re.test(l)), "nothing matches " + re + " in:\n" + lines.join("\n"));
const none = (lines, re) => assert.ok(!lines.some((l) => re.test(l)), "something matches " + re + " in:\n" + lines.join("\n"));

test("real Tiled: sync and scenes on the sample give the committed project; a second run changes nothing", { skip: SKIP }, () => {
	const s = sampleCopy();
	let lines = runTiled(s.projectFile, ["sync"]);
	some(lines, /^HARNESS installed: Tiled 1\.1[2-9]/);
	some(lines, /^HARNESS installed: .*globals ClassPropertyType=function/);
	has(lines, "YATI:   Created: GodotPhysicsLayers, GodotRenderLayers, GodotNodeType, MapRef");
	// The extension reloads the project itself; without the editor tiled.open throws ("Editor not
	// available"), which it catches, reads back that nothing was reloaded, and says what to do.
	some(lines, /^ALERT \[YATI\]: Synced with the Godot project 'YATI Sample' .*The project is reloaded now, so that Tiled reads it\./);
	has(lines, "YATI: tiled.open(" + s.projectFile + ") failed: Editor not available");
	some(lines, /^ALERT \[YATI\]: The project was not reloaded\. Reopen it yourself \(File > Open File or Project, or Project > Recent Projects\)/);
	lines = runTiled(s.projectFile, ["sync"]);
	has(lines, "YATI:   Unchanged: GodotPhysicsLayers, GodotRenderLayers, GodotNodeType, MapRef");
	none(lines, /reloaded/);
	lines = runTiled(s.projectFile, ["scenes"]);
	some(lines, /^CONFIRM \(yes\): Make or update 2 class type\(s\)/);
	has(lines, "YATI:   Created: Chest.material, Slime.Mood, Vector2, Chest, Slime");
	has(lines, "YATI:   Slime: export 'drops' skipped (Tiled class members cannot be lists)");
	some(lines, /^ALERT \[YATI\]: The project was not reloaded\./);
	lines = runTiled(s.projectFile, ["scenes"]);
	has(lines, "YATI:   Unchanged: Chest.material, Slime.Mood, Vector2, Chest, Slime");
	none(lines, /reloaded/);
	assert.equal(fs.readFileSync(s.projectFile, "utf8"), fs.readFileSync(EXPECTED_PROJECT, "utf8"));
	// The Configure dialog, built with the real Dialog and its widgets, saved with nothing changed:
	// the file stays as it is. godot is not on PATH here, so the first Save warns (through Process).
	lines = runTiled(s.projectFile, ["configure"]);
	some(lines, /^HARNESS installed: .*Dialog=function/);
	some(lines, /^HARNESS dialog 'YATI: Configure': project '.*\/godot' \(project\.godot found\.\), binary '' \(placeholder 'godot'\), scenes '', options '', validate true, tooltips true,true,true,true,true,true$/);
	some(lines, /^HARNESS dialog after Save: saved false, button 'Save anyway', status 'Godot binary: Could not start 'godot': .* \| Press Save anyway to keep these settings regardless\.'$/);
	some(lines, /^HARNESS dialog after Save anyway: saved true, status '/);
	has(lines, "YATI: Configured.");
	some(lines, /^YATI:   Scenes scanned under: .*\/godot$/);
	none(lines, /^ALERT/);
	assert.equal(fs.readFileSync(s.projectFile, "utf8"), fs.readFileSync(EXPECTED_PROJECT, "utf8"));
	// Saved with the import options set and the validation off: the properties reach the file.
	lines = runTiled(s.projectFile, ["configure:warn_unknown_classes=false bogus=1"]);
	some(lines, /^HARNESS dialog options status: 'Unknown import option 'bogus' \(passed on as it is\)\.'$/);
	some(lines, /^HARNESS dialog after Save anyway: saved true/);
	const saved = JSON.parse(fs.readFileSync(s.projectFile, "utf8"));
	assert.deepEqual(saved.properties.filter((p) => p.name.indexOf("yati_g") !== 0 && p.name !== "yati_fixups").map((p) => p.name + "=" + p.value),
		["godot_project=../godot", "yati_import_options=warn_unknown_classes=false bogus=1", "yati_validate=false"]);
	assert.deepEqual(saved.propertyTypes, JSON.parse(fs.readFileSync(EXPECTED_PROJECT, "utf8")).propertyTypes);
});

test("real Tiled: the save hook on the sample map files its three mistakes", { skip: SKIP }, () => {
	const s = sampleCopy();
	fs.copyFileSync(EXPECTED_PROJECT, s.projectFile);
	const lines = runTiled(s.projectFile, ["save:" + path.join(s.root, "tiled", "maps", "demo.tmx")]);
	// A map read through a format has no file name (the editor's would), hence "unsaved map".
	const issues = lines.filter((l) => l.indexOf("ISSUE ") === 0);
	assert.equal(issues.length, 3, lines.join("\n"));
	assert.match(issues[0], /^ISSUE warning: YATI: unsaved map: Object 'Typo' \(id 3, layer Things\): class 'Slme' is no Godot node type/);
	assert.match(issues[1], /^ISSUE error: YATI: unsaved map: Object 'Box' \(id 4, layer Things\): class 'instance' needs a file property 'res_path'/);
	assert.match(issues[2], /^ISSUE warning: YATI: unsaved map: Object 'Wall' \(id 5, layer Things\): mask 'collision_layer' names 'Ghosts', which GodotPhysicsLayers does not have/);
	has(lines, "YATI: unsaved map: 3 finding(s), see the Issues view.");
});

test("real Tiled: without a project (tiled.project is null) the actions say so and nothing throws", { skip: SKIP }, () => {
	const lines = runTiled(null, ["sync", "scenes", "check", "configure"]);
	some(lines, /^HARNESS installed: Tiled .*, project null,/);
	const alerts = lines.filter((l) => l.indexOf("ALERT ") === 0);
	assert.equal(alerts.length, 4, lines.join("\n"));
	assert.match(alerts[0], /No Tiled project is open\. Open or create one/);
	assert.match(alerts[1], /No Tiled project is open\. Open or create one/);
	assert.match(alerts[2], /Open the map to check first/);
	assert.match(alerts[3], /No Tiled project is open\. Open or create one/);
	none(lines, /^ISSUE error/);
	none(lines, /Cannot read property/);
});
