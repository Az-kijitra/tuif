//! POSIX path helpers for remote (SFTP) locations.
//!
//! Remote paths are always `/`-separated and case-sensitive, so they must never
//! go through the Windows-oriented `normalize_tree_key` / `PathBuf` logic.

/// Collapses duplicate slashes, resolves `.` / `..` segments lexically, and strips
/// a trailing slash (except for the root `/`). Relative input is treated as rooted.
pub fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    if parts.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", parts.join("/"))
    }
}

/// Joins a directory and a single child name.
pub fn join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

/// Parent directory, or `None` for the root.
pub fn parent(path: &str) -> Option<String> {
    let p = normalize(path);
    if p == "/" {
        return None;
    }
    match p.rfind('/') {
        Some(0) => Some("/".to_string()),
        Some(i) => Some(p[..i].to_string()),
        None => None,
    }
}

/// Last path segment (`""` for the root).
pub fn file_name(path: &str) -> String {
    let p = normalize(path);
    p.rsplit('/').next().unwrap_or("").to_string()
}

/// All ancestors from the root down to `path` itself: `/a/b` -> `["/", "/a", "/a/b"]`.
pub fn ancestors_root_first(path: &str) -> Vec<String> {
    let p = normalize(path);
    let mut out = vec!["/".to_string()];
    let mut acc = String::new();
    for seg in p.split('/').filter(|s| !s.is_empty()) {
        acc.push('/');
        acc.push_str(seg);
        out.push(acc.clone());
    }
    out
}

/// `true` when `path` equals `ancestor` or lies below it (case-sensitive).
pub fn is_same_or_descendant(path: &str, ancestor: &str) -> bool {
    let p = normalize(path);
    let a = normalize(ancestor);
    p == a || a == "/" || p.starts_with(&format!("{a}/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_handles_slashes_dots_and_root() {
        assert_eq!(normalize("/"), "/");
        assert_eq!(normalize(""), "/");
        assert_eq!(normalize("//home//user/"), "/home/user");
        assert_eq!(normalize("/home/./user/../toshi"), "/home/toshi");
        assert_eq!(normalize("/.."), "/");
    }

    #[test]
    fn join_parent_and_file_name() {
        assert_eq!(join("/", "etc"), "/etc");
        assert_eq!(join("/home", "a.txt"), "/home/a.txt");
        assert_eq!(parent("/home/user"), Some("/home".to_string()));
        assert_eq!(parent("/home"), Some("/".to_string()));
        assert_eq!(parent("/"), None);
        assert_eq!(file_name("/home/user/a.txt"), "a.txt");
        assert_eq!(file_name("/"), "");
    }

    #[test]
    fn ancestors_and_descendant_checks_are_case_sensitive() {
        assert_eq!(ancestors_root_first("/a/b"), vec!["/", "/a", "/a/b"]);
        assert!(is_same_or_descendant("/a/b/c", "/a/b"));
        assert!(is_same_or_descendant("/a/b", "/a/b"));
        assert!(!is_same_or_descendant("/a/bc", "/a/b"));
        assert!(!is_same_or_descendant("/A/b", "/a"));
        assert!(is_same_or_descendant("/x", "/"));
    }
}
