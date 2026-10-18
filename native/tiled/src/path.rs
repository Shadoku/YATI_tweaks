//! Path handling of the file-request protocol.
//!
//! Paths are opaque strings: `res://`, `user://`, any other `scheme://`, an absolute `/...` or
//! a Windows drive `C:/...` prefix is kept as it is, and only the part after it is cleaned the
//! way the GDScript runtime's `CommonUtils.cleanup_path` cleans a path (`.` segments and empty
//! segments dropped, `x/..` pairs removed, a leading `..` kept).

/// Splits a path into its root prefix (`res://`, `/`, `C:/`, or empty) and the rest.
pub fn split_root(path: &str) -> (&str, &str) {
    if let Some(i) = path.find("://") {
        let scheme = &path[..i];
        if !scheme.is_empty()
            && scheme
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_')
        {
            return path.split_at(i + 3);
        }
    }
    let b = path.as_bytes();
    if b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'/' || b[2] == b'\\')
    {
        return path.split_at(3);
    }
    if b.first() == Some(&b'/') {
        return path.split_at(1);
    }
    ("", path)
}

/// True when the path has a root (scheme, `/` or drive), i.e. does not depend on a base.
pub fn is_absolute(path: &str) -> bool {
    !split_root(path).0.is_empty()
}

/// `CommonUtils.cleanup_path` on the part after the root, the root kept.
pub fn cleanup(path: &str) -> String {
    let (root, rest) = split_root(path);
    let mut out = String::with_capacity(path.len());
    out.push_str(root);
    out.push_str(&cleanup_relative(rest));
    out
}

/// The GDScript `cleanup_path` itself (on a path without root).
fn cleanup_relative(path: &str) -> String {
    let mut current = path.to_string();
    loop {
        let mut parts: Vec<&str> = current.split('/').collect();
        let mut clean = true;
        for i in 1..parts.len() {
            if parts[i] == ".." {
                parts[i] = "";
                parts[i - 1] = "";
                clean = false;
                break;
            }
            if parts[i] == "." {
                parts[i] = "";
                clean = false;
            }
        }
        let joined = parts
            .iter()
            .filter(|p| !p.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join("/");
        if clean {
            return joined;
        }
        current = joined;
    }
}

/// Godot's `String.path_join`.
pub fn join(base: &str, file: &str) -> String {
    if base.is_empty() {
        return file.to_string();
    }
    if base.ends_with('/') || file.starts_with('/') {
        format!("{base}{file}")
    } else {
        format!("{base}/{file}")
    }
}

/// Godot's `String.get_base_dir`: everything before the last `/`, the root kept whole.
pub fn base_dir(path: &str) -> String {
    let (root, rest) = split_root(path);
    match rest.rfind('/') {
        Some(i) => format!("{root}{}", &rest[..i]),
        None => root.to_string(),
    }
}

/// The path a file referenced as `reference` from a file in `base` resolves to: the reference
/// itself when it is absolute, else joined to the base; cleaned either way.
pub fn resolve(base: &str, reference: &str) -> String {
    if is_absolute(reference) {
        cleanup(reference)
    } else {
        cleanup(&join(base, reference))
    }
}

/// The lowercase extension of the file name, without the dot.
pub fn extension(path: &str) -> String {
    let file = path.rsplit('/').next().unwrap_or(path);
    match file.rfind('.') {
        Some(i) => file[i + 1..].to_ascii_lowercase(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_like_gdscript() {
        assert_eq!(cleanup("a/b/../c"), "a/c");
        assert_eq!(cleanup("a/./b//c"), "a/b/c");
        assert_eq!(cleanup("../x"), "../x");
        assert_eq!(cleanup("./a"), "./a");
        assert_eq!(cleanup("a/../../x"), "../x");
        assert_eq!(cleanup("res://test/fixtures/../x.tsx"), "res://test/x.tsx");
        assert_eq!(cleanup("/abs/./y"), "/abs/y");
        assert_eq!(cleanup("user://a/b/.."), "user://a");
    }

    #[test]
    fn base_and_join() {
        assert_eq!(base_dir("res://a/b.tmx"), "res://a");
        assert_eq!(base_dir("res://b.tmx"), "res://");
        assert_eq!(base_dir("a/b"), "a");
        assert_eq!(base_dir("b"), "");
        assert_eq!(base_dir("/x"), "/");
        assert_eq!(join("res://", "a"), "res://a");
        assert_eq!(join("", "a"), "a");
        assert_eq!(resolve("res://maps", "../tiles/t.tsx"), "res://tiles/t.tsx");
        assert_eq!(resolve("maps", "res://t.tsx"), "res://t.tsx");
        assert_eq!(resolve("maps", "t.tsx"), "maps/t.tsx");
    }
}
