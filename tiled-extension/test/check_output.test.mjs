// lib/check_output.mjs: the check's command line and its output (the format of the plan's
// section 3; the fixture is written in that format, as stage 2's check_map.gd is to print it).
import { test } from "node:test";
import assert from "node:assert/strict";
import path from "node:path";
import { checkArguments, parseCheckOutput, summarize, CHECK_SCRIPT } from "../yati-godot/lib/check_output.mjs";
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
