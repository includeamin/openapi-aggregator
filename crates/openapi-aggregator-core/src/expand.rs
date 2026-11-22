//! Expand glob file sources (`path: specs/*.yaml`) into one ordinary source per matching file.
//!
//! Pure and I/O-free: the caller supplies the candidate file paths (the CLI lists a real
//! directory, the playground passes its workspace file names).

// The internal helpers are used by `expand_sources` in the next step.
#![allow(dead_code)]

/// Whether a source path is a glob pattern (contains `*`).
pub fn is_glob(path: &str) -> bool {
    path.contains('*')
}

/// The leading directory part of `pattern` before the first segment containing `*`, without a
/// trailing slash (`""` when the pattern starts with a glob segment). For a pattern with no glob
/// it is the parent directory.
pub fn glob_base(pattern: &str) -> &str {
    let mut offset = 0;
    let mut end = None;
    for segment in pattern.split('/') {
        if segment.contains('*') {
            end = Some(offset);
            break;
        }
        offset += segment.len() + 1;
    }
    let end = match end {
        // The base ends before the slash that precedes the first glob segment.
        Some(start) => start.saturating_sub(1),
        // No glob: the parent directory.
        None => pattern.rfind('/').unwrap_or(0),
    };
    let base = &pattern[..end.min(pattern.len())];
    if base.is_empty() && pattern.starts_with('/') && pattern.len() > 1 {
        "/"
    } else {
        base
    }
}

/// The part of `path` below `base` (which may be empty).
fn relative_to<'a>(base: &str, path: &'a str) -> &'a str {
    if base.is_empty() {
        return path;
    }
    path.strip_prefix(base)
        .map(|rest| rest.trim_start_matches('/'))
        .unwrap_or(path)
}

/// The source name for a file matched by `pattern`: its path relative to the pattern's base,
/// without the final extension, with `/` replaced by `_`.
pub(crate) fn source_name(pattern: &str, matched: &str) -> String {
    let relative = relative_to(glob_base(pattern), matched);
    let (dir, file) = match relative.rfind('/') {
        Some(i) => (&relative[..=i], &relative[i + 1..]),
        None => ("", relative),
    };
    let stem = match file.rfind('.') {
        Some(i) if i > 0 => &file[..i],
        _ => file,
    };
    format!("{dir}{stem}").replace('/', "_")
}

/// Whether `candidate` is hidden below the pattern's base: some segment starts with `.` and the
/// pattern's segment at the same position does not.
pub(crate) fn is_hidden_below(base: &str, pattern: &str, candidate: &str) -> bool {
    let pattern_segments: Vec<&str> = relative_to(base, pattern).split('/').collect();
    relative_to(base, candidate)
        .split('/')
        .enumerate()
        .any(|(i, segment)| {
            segment.starts_with('.')
                && segment != "."
                && !pattern_segments.get(i).is_some_and(|p| p.starts_with('.'))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_globs() {
        assert!(is_glob("specs/*.yaml"));
        assert!(is_glob("**/x.yaml"));
        assert!(!is_glob("specs/a.yaml"));
    }

    #[test]
    fn glob_bases() {
        assert_eq!(glob_base("specs/*.yaml"), "specs");
        assert_eq!(
            glob_base("/abs/cfg/services/*/api.yaml"),
            "/abs/cfg/services"
        );
        assert_eq!(glob_base("*.yaml"), "");
        assert_eq!(glob_base("**/x.yaml"), "");
        assert_eq!(glob_base("specs/a*/b.yaml"), "specs");
        assert_eq!(glob_base("a/b/c.yaml"), "a/b");
        assert_eq!(glob_base("c.yaml"), "");
        assert_eq!(glob_base("/*.yaml"), "/");
    }

    #[test]
    fn source_names() {
        assert_eq!(source_name("specs/*.yaml", "specs/billing.yaml"), "billing");
        assert_eq!(
            source_name("services/*/api.yaml", "services/billing/api.yaml"),
            "billing_api"
        );
        assert_eq!(source_name("**/*.yaml", "a/b/c.yaml"), "a_b_c");
        assert_eq!(source_name("*.yaml", "my.service.yaml"), "my.service");
        assert_eq!(
            source_name("/abs/cfg/specs/*.yml", "/abs/cfg/specs/x.yml"),
            "x"
        );
        assert_eq!(source_name("specs/*", "specs/.hidden"), ".hidden");
    }

    #[test]
    fn hidden_entries() {
        assert!(is_hidden_below(
            "specs",
            "specs/*.yaml",
            "specs/.hidden.yaml"
        ));
        assert!(is_hidden_below(
            "specs",
            "specs/**/*.yaml",
            "specs/.git/x.yaml"
        ));
        assert!(!is_hidden_below("specs", "specs/.*.yaml", "specs/.a.yaml"));
        assert!(!is_hidden_below("specs", "specs/*.yaml", "specs/a.yaml"));
        assert!(!is_hidden_below(
            ".cfg/specs",
            ".cfg/specs/*.yaml",
            ".cfg/specs/a.yaml"
        ));
    }
}
