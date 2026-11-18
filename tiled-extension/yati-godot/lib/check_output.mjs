// YATI → Check map in Godot: the command lines that run the importer's check script and the game's
// own check scene (the setting yati_check_scene), and their output read back. The script (addons/yati_native/check_map.gd, docs/tiled-extension-plan.md section 3)
// imports the map headless and prints one machine-readable line per message,
//
//   YATI_CHECK error|warning <object id or -> <text>
//
// and a final
//
//   YATI_CHECK result <errors> <warnings> <nodes> <cells> <ms>
//
// exiting with 0 when there was no error. Everything else it (or Godot) prints is kept as `other`:
// Godot's banner, the importer's own lines (its warnings in colour: ANSI escape sequences, which
// are removed), "Import of ... finished with ...".

export const CHECK_SCRIPT = "res://addons/yati_native/check_map.gd";
export const CHECK_SCRIPT_LOCAL = "addons/yati_native/check_map.gd";

// The arguments for the Godot binary.
export function checkArguments(godotProject, mapFile, projectFile, importOptions) {
	const args = ["--headless", "--path", godotProject, "-s", CHECK_SCRIPT, "--", mapFile];
	if (projectFile)
		args.push("--project=" + projectFile);
	for (let i = 0; i < importOptions.length; i++)
		args.push(importOptions[i].arg);
	return args;
}

// The arguments that run the game's own check (the setting yati_check_scene, a res:// scene) on a
// map: the scene is the main scene of a headless run, the map follows `--tiled` among the user
// arguments. The scene prints the same YATI_CHECK lines; parseCheckOutput reads them.
export function sceneCheckArguments(godotProject, scene, mapFile) {
	return ["--headless", "--path", godotProject, scene, "--", "--tiled", mapFile];
}

// {messages: [{severity, objectId (0: none), text}], result: {errors, warnings, nodes, cells, ms} |
// null, other: [lines]}.
export function parseCheckOutput(text) {
	const messages = [];
	const other = [];
	let result = null;
	const lines = String(text === undefined || text === null ? "" : text).split(/\r?\n/);
	for (let i = 0; i < lines.length; i++) {
		const line = stripAnsi(lines[i]).replace(/\s+$/, "");
		if (line === "")
			continue;
		const at = line.indexOf("YATI_CHECK ");
		if (at < 0) {
			other.push(line);
			continue;
		}
		const body = line.substring(at + "YATI_CHECK ".length);
		let m = /^result\s+(\d+)\s+(\d+)\s+(\d+)\s+(\d+)\s+(\d+(?:\.\d+)?)\s*$/.exec(body);
		if (m) {
			result = {
				errors: parseInt(m[1], 10),
				warnings: parseInt(m[2], 10),
				nodes: parseInt(m[3], 10),
				cells: parseInt(m[4], 10),
				ms: parseFloat(m[5]),
			};
			continue;
		}
		m = /^(error|warning)\s+(-|\d+)\s?(.*)$/.exec(body);
		if (m) {
			messages.push({ severity: m[1], objectId: m[2] === "-" ? 0 : parseInt(m[2], 10), text: m[3].trim() });
			continue;
		}
		other.push(line);
	}
	return { messages: messages, result: result, other: other };
}

// A line without its ANSI escape sequences (Godot prints rich text in colour on a terminal and in
// headless mode: the importer's warnings come as ESC[38;2;255;255;0m ... ESC[39mESC[0m).
export function stripAnsi(line) {
	return String(line).replace(/\u001b\[[0-9;?]*[ -\/]*[@-~]/g, "");
}

// One line for the log and the alert: what the check found, or why it did not finish.
export function summarize(parsed, exitCode, stderrText) {
	if (parsed.result === null)
		return unfinished("The check", parsed, exitCode, stderrText);
	const r = parsed.result;
	return "Godot imported the map with " + r.errors + " error(s) and " + r.warnings + " warning(s): " +
		r.nodes + " nodes, " + r.cells + " cells, " + r.ms + " ms.";
}

// The same for the game's check scene: it imports nothing, so its nodes and cells are not told.
export function summarizeScene(parsed, exitCode, stderrText, scene) {
	const name = String(scene).replace(/^.*\//, "");
	if (parsed.result === null)
		return unfinished(name, parsed, exitCode, stderrText);
	const r = parsed.result;
	return name + " found " + r.errors + " error(s) and " + r.warnings + " warning(s) in " + r.ms + " ms.";
}

// Why a run printed no result line, with the last line it printed (stderr's first).
function unfinished(who, parsed, exitCode, stderrText) {
	let why = who + " did not finish (no result line; exit code " + exitCode + ").";
	const tail = stripAnsi(stderrText || "").split(/\r?\n/).filter(function (l) { return l.trim() !== ""; });
	if (tail.length > 0)
		why += " Last output: " + tail[tail.length - 1].trim();
	else if (parsed.other.length > 0)
		why += " Last output: " + parsed.other[parsed.other.length - 1].trim();
	return why;
}
