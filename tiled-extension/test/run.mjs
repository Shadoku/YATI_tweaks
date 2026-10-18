// `node --test tiled-extension/test`: since Node 21 the test runner takes files and globs, not
// folders, and resolves this folder like a module (package.json's "main", this file). Loading
// every *.test.mjs here registers their tests in this process. Node 20 searches the folder for
// *.test.mjs itself and never runs this file. Either way: every test of this folder.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const dir = path.dirname(fileURLToPath(import.meta.url));
for (const f of fs.readdirSync(dir).filter((n) => n.endsWith(".test.mjs")).sort())
	await import(pathToFileURL(path.join(dir, f)).href);
