// A small XML reader for the tests' Tiled files (.tmx, .tsx, .tx): elements with attributes,
// children and text. Enough for Tiled's output; not a general parser.

const ENTITIES = { amp: "&", lt: "<", gt: ">", quot: "\"", apos: "'" };

export function decodeEntities(s) {
	return s.replace(/&(#x[0-9a-fA-F]+|#\d+|[a-z]+);/g, function (all, e) {
		if (e.charAt(0) === "#")
			return String.fromCharCode(e.charAt(1) === "x" ? parseInt(e.substring(2), 16) : parseInt(e.substring(1), 10));
		return ENTITIES[e] !== undefined ? ENTITIES[e] : all;
	});
}

// {name, attrs, children, text} for the document element.
export function parseXml(text) {
	const root = { name: "#document", attrs: {}, children: [], text: "" };
	const stack = [root];
	const re = /<!--[\s\S]*?-->|<\?[\s\S]*?\?>|<!\[CDATA\[([\s\S]*?)\]\]>|<\/([A-Za-z_][\w.-]*)\s*>|<([A-Za-z_][\w.-]*)((?:\s+[\w.:-]+\s*=\s*(?:"[^"]*"|'[^']*'))*)\s*(\/?)>|([^<]+)/g;
	let m;
	while ((m = re.exec(text)) !== null) {
		const top = stack[stack.length - 1];
		if (m[1] !== undefined) {
			top.text += m[1];
		} else if (m[2] !== undefined) {
			if (top.name !== m[2])
				throw new Error("XML: </" + m[2] + "> closes <" + top.name + ">");
			stack.pop();
		} else if (m[3] !== undefined) {
			const el = { name: m[3], attrs: {}, children: [], text: "" };
			const attrRe = /([\w.:-]+)\s*=\s*("([^"]*)"|'([^']*)')/g;
			let a;
			while ((a = attrRe.exec(m[4] || "")) !== null)
				el.attrs[a[1]] = decodeEntities(a[3] !== undefined ? a[3] : a[4]);
			top.children.push(el);
			if (m[5] !== "/")
				stack.push(el);
		} else if (m[6] !== undefined) {
			top.text += decodeEntities(m[6]);
		}
	}
	const el = root.children[0];
	if (!el)
		throw new Error("XML: no element");
	return el;
}

export function child(el, name) {
	for (let i = 0; i < el.children.length; i++) {
		if (el.children[i].name === name)
			return el.children[i];
	}
	return null;
}

export function childrenNamed(el, name) {
	return el.children.filter(function (c) { return c.name === name; });
}
