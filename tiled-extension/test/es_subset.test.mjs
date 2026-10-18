// The extension runs in Tiled's JavaScript engine (Qt's QJSEngine: ECMAScript 2016 with ES
// modules), so its modules keep to ES2016 and to library functions that edition has. The same
// files must also compile under Node (node --check).
import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { EXTENSION_DIR } from "./helpers/node_io.mjs";

const files = [path.join(EXTENSION_DIR, "yati-godot.mjs")].concat(
	fs.readdirSync(path.join(EXTENSION_DIR, "lib")).filter((f) => f.endsWith(".mjs")).map((f) => path.join(EXTENSION_DIR, "lib", f)));

test("the extension's modules: lib/ has the plan's seven, nothing else is shipped", () => {
	assert.deepEqual(fs.readdirSync(path.join(EXTENSION_DIR, "lib")).sort(), [
		"apply.mjs", "check_output.mjs", "godot_project.mjs", "scene_exports.mjs", "settings.mjs", "types.mjs", "validate.mjs",
	]);
	assert.deepEqual(fs.readdirSync(EXTENSION_DIR).sort(), ["lib", "yati-godot.mjs"]);
});

test("node --check on every module", () => {
	for (const f of files)
		execFileSync(process.execPath, ["--check", f]);
});

// Library members and syntax newer than ES2016 (a regular expression per item, on code without
// comments and strings).
const NEWER = [
	[/\?\./, "optional chaining"],
	[/\?\?/, "nullish coalescing"],
	[/\.\.\./, "spread / rest (object spread is ES2018; kept out entirely)"],
	[/\basync\b|\bawait\b/, "async functions (ES2017)"],
	[/Object\.(entries|values|fromEntries|getOwnPropertyDescriptors)\b/, "Object.entries/values/... (ES2017+)"],
	[/\.(padStart|padEnd|trimStart|trimEnd|matchAll|replaceAll|flatMap|flat|at)\(/, "ES2017+ string/array methods"],
	[/\bstructuredClone\b|\bglobalThis\b|\bBigInt\b/, "newer globals"],
	[/\(\?<[=!A-Za-z]/, "regular expression lookbehind / named groups (ES2018)"],
	[/\bimport\.meta\b|\bimport\(/, "import.meta / dynamic import"],
];

function stripCommentsAndStrings(src) {
	return src
		.replace(/\/\*[\s\S]*?\*\//g, "")
		.replace(/(^|[^:\\])\/\/.*$/gm, "$1")
		.replace(/"(?:[^"\\\n]|\\.)*"/g, "\"\"")
		.replace(/'(?:[^'\\\n]|\\.)*'/g, "''")
		.replace(/`(?:[^`\\]|\\.)*`/g, "``");
}

test("no syntax or library members newer than ES2016", () => {
	for (const f of files) {
		const code = stripCommentsAndStrings(fs.readFileSync(f, "utf8"));
		for (const [re, what] of NEWER)
			assert.ok(!re.test(code), path.basename(f) + ": " + what + " (" + (code.match(re) || [""])[0] + ")");
	}
});

test("parses as ECMAScript 2016 (acorn, when available)", async (t) => {
	let acorn = null;
	try {
		acorn = await import(process.env.YATI_ACORN || "acorn");
	} catch (e) {
		t.skip("acorn is not installed (npm i acorn, or YATI_ACORN=<path to acorn.mjs>)");
		return;
	}
	for (const f of files)
		acorn.parse(fs.readFileSync(f, "utf8"), { ecmaVersion: 2016, sourceType: "module" });
});
