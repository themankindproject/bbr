//! Pathspec matching for `bbr pr diff -- PATH…` filters.

/// Return true if `path` matches any of the pathspecs (OR semantics).
/// Empty `pathspecs` matches everything.
pub fn matches_any(pathspecs: &[String], path: &str) -> bool {
    if pathspecs.is_empty() {
        return true;
    }
    if path.is_empty() {
        return false;
    }
    pathspecs.iter().any(|spec| matches_one(spec, path))
}

/// Match a single pathspec against a repo-relative path.
///
/// Supported forms:
/// - exact: `src/foo.rs`
/// - directory prefix: `src/` or `src`
/// - basename: `foo.rs` matches `src/foo.rs`
/// - wildcards: `*` (any run) and `?` (one char), including across `/`
pub fn matches_one(spec: &str, path: &str) -> bool {
    let spec = normalize(spec);
    let path = normalize(path);
    if spec.is_empty() {
        return false;
    }
    if spec.contains('*') || spec.contains('?') {
        return wildcard_match(&spec, &path);
    }
    if path == spec {
        return true;
    }
    // Directory prefix: "src" or "src/" → "src/…"
    let dir = spec.trim_end_matches('/');
    if let Some(rest) = path.strip_prefix(dir) {
        if rest.is_empty() || rest.starts_with('/') {
            return true;
        }
    }
    // Basename / suffix: "foo.rs" → "…/foo.rs"
    if let Some(idx) = path.rfind('/') {
        if path[idx + 1..] == *spec {
            return true;
        }
    }
    false
}

fn normalize(s: &str) -> String {
    let s = s.trim().trim_start_matches("./");
    s.replace('\\', "/")
}

/// Simple glob: `*` = any chars, `?` = one char. No brace expansion.
fn wildcard_match(pattern: &str, text: &str) -> bool {
    wildcard_match_chars(
        &pattern.chars().collect::<Vec<_>>(),
        &text.chars().collect::<Vec<_>>(),
    )
}

fn wildcard_match_chars(pat: &[char], text: &[char]) -> bool {
    let mut pi = 0;
    let mut ti = 0;
    let mut star_pi: Option<usize> = None;
    let mut star_ti: usize = 0;

    while ti < text.len() {
        if pi < pat.len() && (pat[pi] == '?' || pat[pi] == text[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < pat.len() && pat[pi] == '*' {
            star_pi = Some(pi);
            star_ti = ti;
            pi += 1;
        } else if let Some(sp) = star_pi {
            pi = sp + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }
    while pi < pat.len() && pat[pi] == '*' {
        pi += 1;
    }
    pi == pat.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_and_prefix() {
        assert!(matches_one("src/foo.rs", "src/foo.rs"));
        assert!(matches_one("src/", "src/foo.rs"));
        assert!(matches_one("src", "src/foo.rs"));
        assert!(!matches_one("src", "src2/foo.rs"));
    }

    #[test]
    fn basename() {
        assert!(matches_one("foo.rs", "src/foo.rs"));
        assert!(!matches_one("foo.rs", "src/bar.rs"));
    }

    #[test]
    fn wildcards() {
        assert!(matches_one("src/*.rs", "src/foo.rs"));
        assert!(matches_one("*foo.rs", "a/b/foo.rs"));
        assert!(matches_one("src/f?o.rs", "src/foo.rs"));
        assert!(!matches_one("src/*.rs", "src/foo.toml"));
    }

    #[test]
    fn matches_any_or() {
        let specs = vec!["a.rs".into(), "b.rs".into()];
        assert!(matches_any(&specs, "lib/a.rs"));
        assert!(!matches_any(&specs, "lib/c.rs"));
        assert!(matches_any(&[], "anything"));
    }
}
