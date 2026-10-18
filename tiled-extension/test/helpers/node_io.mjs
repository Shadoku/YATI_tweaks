// File access for the pure modules under Node, and the tests' paths.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const TEST_DIR = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
export const FIXTURES = path.join(TEST_DIR, "fixtures");
export const GODOT_FIXTURE = path.join(FIXTURES, "godot_project");
export const IMPORTER_FIXTURES = path.join(FIXTURES, "importer");
export const MAP_FIXTURES = path.join(FIXTURES, "maps");
export const EXPECTED = path.join(TEST_DIR, "expected");
export const EXTENSION_DIR = path.join(path.dirname(TEST_DIR), "yati-godot");
export const SAMPLE_DIR = path.join(path.dirname(TEST_DIR), "sample");

export const io = {
	listDir(dir) {
		let entries;
		try {
			entries = fs.readdirSync(dir, { withFileTypes: true });
		} catch (e) {
			return null;
		}
		return {
			dirs: entries.filter((e) => e.isDirectory()).map((e) => e.name),
			files: entries.filter((e) => e.isFile()).map((e) => e.name),
		};
	},
	readText(p) {
		try {
			return fs.readFileSync(p, "utf8");
		} catch (e) {
			return null;
		}
	},
};

export function readJson(p) {
	return JSON.parse(fs.readFileSync(p, "utf8"));
}

// A fresh directory under the system's temp directory, removed by the caller's `after`.
export function tempDir(prefix) {
	return fs.mkdtempSync(path.join(os.tmpdir(), prefix || "yati-godot-"));
}

// Copies a directory tree.
export function copyDir(from, to) {
	fs.cpSync(from, to, { recursive: true });
}

// When UPDATE_EXPECTED=1, writes `actual` to the expected file instead of comparing (for
// regenerating fragments after a deliberate change; review the diff).
export function expectedJson(name, actual) {
	const p = path.join(EXPECTED, name);
	if (process.env.UPDATE_EXPECTED === "1") {
		fs.writeFileSync(p, JSON.stringify(actual, null, 4) + "\n");
		return actual;
	}
	return readJson(p);
}
