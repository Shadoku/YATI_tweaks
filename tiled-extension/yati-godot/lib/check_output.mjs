// YATI → Check map in Godot: the command line that runs the game's check script, and its output
// read back. The script (addons/yati_native/check_map.gd, docs/tiled-extension-plan.md section 3)
// imports the map headless and prints one machine-readable line per message,
//
//   YATI_CHECK error|warning <object id or -> <text>
//
// and a final
//
//   YATI_CHECK result <errors> <warnings> <nodes> <cells> <ms>
//
// exiting with 0 when there was no error. Everything else it (or Godot) prints is kept as `other`.

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

// {messages: [{severity, objectId (0: none), text}], result: {errors, warnings, nodes, cells, ms} |
// null, other: [lines]}.
export function parseCheckOutput(text) {
	const messages = [];
	const other = [];
	let result = null;
	const lines = String(text === undefined || text === null ? "" : text).split(/\r?\n/);
	for (let i = 0; i < lines.length; i++) {
		const line = lines[i].replace(/\s+$/, "");
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

// One line for the log and the alert: what the check found, or why it did not finish.
export function summarize(parsed, exitCode, stderrText) {
	if (parsed.result === null) {
		let why = "The check did not finish (no result line; exit code " + exitCode + ").";
		const tail = String(stderrText || "").split(/\r?\n/).filter(function (l) { return l.trim() !== ""; });
		if (tail.length > 0)
			why += " Last output: " + tail[tail.length - 1].trim();
		else if (parsed.other.length > 0)
			why += " Last output: " + parsed.other[parsed.other.length - 1].trim();
		return why;
	}
	const r = parsed.result;
	return "Godot imported the map with " + r.errors + " error(s) and " + r.warnings + " warning(s): " +
		r.nodes + " nodes, " + r.cells + " cells, " + r.ms + " ms.";
}
