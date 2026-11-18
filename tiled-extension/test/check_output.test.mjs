// lib/check_output.mjs: the check's command line and its output. check_output.txt is written by
// hand in the format of the plan's section 3; check_output_unknown_class.txt and
// check_output_scenes.txt are the real check_map.gd's stdout (Godot 4.7.2, the native importer,
// on the importers' fixtures test/fixtures/unknown_class.tmx --warn_unknown_classes=true and
// test/fixtures/scenes.tmx), saved as captured, colour codes included.
import { test } from "node:test";
import assert from "node:assert/strict";
import path from "node:path";
import { checkArguments, sceneCheckArguments, parseCheckOutput, summarize, summarizeScene, stripAnsi, CHECK_SCRIPT } from "../yati-godot/lib/check_output.mjs";
import { parseImportOptions } from "../yati-godot/lib/settings.mjs";
import { io, FIXTURES } from "./helpers/node_io.mjs";

test("arguments: headless, the project, the script, the map, the project file, the options", () => {
	const opts = parseImportOptions("warn_unknown_classes=false assign_node_properties=false").options;
	assert.equal(CHECK_SCRIPT, "res://addons/yati_native/check_map.gd");
	assert.deepEqual(checkArguments("/g", "/g/maps/a.tmx", "/g/maps/a.tiled-project", opts), [
		"--headless", "--path", "/g", "-s", "res://addons/yati_native/check_map.gd", "--", "/g/maps/a.tmx",
		"--project=/g/maps/a.tiled-project", "--warn_unknown_classes=false", "--assign_node_properties=false",
	]);
	assert.deepEqual(checkArguments("/g", "/m.tmx", "", []), ["--headless", "--path", "/g", "-s", CHECK_SCRIPT, "--", "/m.tmx"]);
});

test("output: messages with object ids, the result line, the rest kept", () => {
	const parsed = parseCheckOutput(io.readText(path.join(FIXTURES, "check_output.txt")));
	assert.deepEqual(parsed.messages, [
		{ severity: "warning", objectId: 1, text: "Unknown class 'Door' (3 objects). -> Using 'staticbody'" },
		{ severity: "warning", objectId: 0, text: "Option unknown_class_fallback 'instance' is not supported. -> Using 'staticbody'" },
		{ severity: "error", objectId: 4, text: "Object of class 'instance': Mandatory file property 'res_path' not found or invalid. -> Skipped" },
		{ severity: "warning", objectId: 21, text: "Property 'target' of 'Pointer': object 99 does not exist. -> Empty NodePath" },
	]);
	assert.deepEqual(parsed.result, { errors: 1, warnings: 3, nodes: 42, cells: 128, ms: 17.5 });
	assert.equal(parsed.other.length, 3);
	assert.equal(summarize(parsed, 1, ""), "Godot imported the map with 1 error(s) and 3 warning(s): 42 nodes, 128 cells, 17.5 ms.");
});

test("output: CRLF, prefixes before the marker, unknown YATI_CHECK lines, no result", () => {
	const parsed = parseCheckOutput("x\r\nlog: YATI_CHECK error 7 broken\r\nYATI_CHECK something else\r\n");
	assert.deepEqual(parsed.messages, [{ severity: "error", objectId: 7, text: "broken" }]);
	assert.equal(parsed.result, null);
	assert.deepEqual(parsed.other, ["x", "YATI_CHECK something else"]);
	assert.equal(summarize(parsed, 134, "SCRIPT ERROR: oops\n\n"), "The check did not finish (no result line; exit code 134). Last output: SCRIPT ERROR: oops");
	assert.equal(summarize(parseCheckOutput(""), 1, ""), "The check did not finish (no result line; exit code 1).");
	assert.deepEqual(parseCheckOutput(undefined).messages, []);
});

const CLASS_TAIL = "is not a Godot node type -> using 'staticbody'. Set 'godot_node_type', map the class to a scene " +
	"('class_scenes') or set the option 'unknown_class_fallback' (see Reference).";

test("real output, unknown_class.tmx: the class warnings with their first objects, the result, the rest without colour codes", () => {
	const text = io.readText(path.join(FIXTURES, "check_output_unknown_class.txt"));
	assert.ok(text.indexOf("\u001b[") >= 0, "the fixture keeps Godot's colour codes");
	const parsed = parseCheckOutput(text);
	assert.deepEqual(parsed.messages, [
		{ severity: "warning", objectId: 1, text: "Class 'Door' " + CLASS_TAIL },
		{ severity: "warning", objectId: 4, text: "Class 'Lever' " + CLASS_TAIL },
	]);
	assert.deepEqual(parsed.result, { errors: 0, warnings: 2, nodes: 11, cells: 0, ms: 2 });
	assert.equal(parsed.other.length, 5);
	assert.match(parsed.other[0], /^Initialize godot-rust/);
	assert.match(parsed.other[1], /^Godot Engine v4\.7/);
	assert.equal(parsed.other[2], " -- Class 'Door' " + CLASS_TAIL);
	assert.equal(parsed.other[4], "Import of 'res://test/fixtures/unknown_class.tmx' finished with 0 error(s) and 2 warning(s).");
	assert.ok(parsed.other.every((l) => l.indexOf("\u001b") < 0));
	assert.equal(summarize(parsed, 0, ""), "Godot imported the map with 0 error(s) and 2 warning(s): 11 nodes, 0 cells, 2 ms.");
});

test("real output, scenes.tmx: Enemy and enemy; cut short, the last line without colour codes", () => {
	const text = io.readText(path.join(FIXTURES, "check_output_scenes.txt"));
	const parsed = parseCheckOutput(text);
	assert.deepEqual(parsed.messages.map((m) => m.severity + " " + m.objectId + " " + m.text), [
		"warning 1 Class 'Enemy' " + CLASS_TAIL,
		"warning 4 Class 'enemy' " + CLASS_TAIL,
	]);
	assert.deepEqual(parsed.result, { errors: 0, warnings: 2, nodes: 15, cells: 0, ms: 3 });
	// Godot gone before the script's lines: the last line it printed, readable.
	const lines = text.split("\n");
	const cut = lines.slice(0, lines.findIndex((l) => l.indexOf("Import of") === 0)).join("\n");
	const partial = parseCheckOutput(cut);
	assert.equal(partial.result, null);
	assert.deepEqual(partial.messages, []);
	assert.equal(summarize(partial, 134, ""), "The check did not finish (no result line; exit code 134). Last output: -- Class 'enemy' " + CLASS_TAIL);
	assert.equal(summarize(partial, 134, "\u001b[1;31mERROR:\u001b[0m boom\n"), "The check did not finish (no result line; exit code 134). Last output: ERROR: boom");
	assert.equal(stripAnsi("\u001b[38;2;255;255;0m -- x\u001b[39m\u001b[0m"), " -- x");
});

// The game's own check (the setting yati_check_scene): a scene run headless after the importer's
// check, printing the same YATI_CHECK lines. check_output_scene.txt and
// check_output_scene_stairs.txt are a game's tools/check_maps.tscn's stdout (Godot 4.7.2,
// `-- --tiled <map>`) on two of its fixture maps, saved as captured.
test("the game's check: arguments, its real output read by the same parser, its own summary line", () => {
	assert.deepEqual(sceneCheckArguments("/g", "res://tools/check_maps.tscn", "/g/maps/a.tmx"),
		["--headless", "--path", "/g", "res://tools/check_maps.tscn", "--", "--tiled", "/g/maps/a.tmx"]);
	const parsed = parseCheckOutput(io.readText(path.join(FIXTURES, "check_output_scene.txt")));
	assert.deepEqual(parsed.messages.map((m) => m.severity + " " + m.objectId + " " + m.text), [
		"error 0 map: display_name MAP_GOOD is not a key of the text table",
		"error 5 object 5 \"sign\" (Sign): text SIGN_HELLO is not a key of the text table",
		"error 1 object 1 \"start\" (PlayerSpawn): the name 'start' is also object 318 in res://content/maps/overworld/coast/overworld_1x1.tmx; arrival names are unique across the maps",
		"error 2 object 2 \"door\" (Warp): to 'arrival' names no arrival point (a Warp or PlayerSpawn under the maps)",
	]);
	assert.equal(parsed.result.errors, 4);
	assert.deepEqual([parsed.result.nodes, parsed.result.cells], [0, 0]);
	assert.match(parsed.other[1], /^Godot Engine v4\.7/);
	assert.match(summarizeScene(parsed, 1, "", "res://tools/check_maps.tscn"), /^check_maps\.tscn found 4 error\(s\) and 0 warning\(s\) in \d+ ms\.$/);
	const stairs = parseCheckOutput(io.readText(path.join(FIXTURES, "check_output_scene_stairs.txt")));
	assert.deepEqual(stairs.messages, [{ severity: "error", objectId: 1,
		text: "object 1 \"up\" (Stairs): no other half on level 'upper': a Stairs there overlapping it, its direction DOWN, its target 'lower'" }]);
	// Not finished: the same words as the importer's check, naming the scene.
	assert.equal(summarizeScene(parseCheckOutput("SCRIPT ERROR: x\n"), 1, "", "res://tools/check_maps.tscn"),
		"check_maps.tscn did not finish (no result line; exit code 1). Last output: SCRIPT ERROR: x");
	assert.equal(summarizeScene(parseCheckOutput("YATI_CHECK result 0 0 0 0 12\n"), 0, "", "res://check.tscn"), "check.tscn found 0 error(s) and 0 warning(s) in 12 ms.");
});
