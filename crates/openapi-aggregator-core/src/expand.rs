//! Expand glob file sources (`path: specs/*.yaml`) into one ordinary source per matching file.
//!
//! Pure and I/O-free: the caller supplies the candidate file paths (the CLI lists a real
//! directory, the playground passes its workspace file names).

use std::path::PathBuf;

use crate::config::{Config, Source};
use crate::error::Error;
use crate::filter::glob_match;

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

/// Replace every glob file source by one source per matching candidate path.
///
/// Matches are sorted and take the pattern's position; non-glob sources are untouched. The
/// pattern's `tag_prefix`, `additional_blocks` and `filter` apply to every match. A pattern
/// without matches is an error. Pattern and candidates must be in the same normalised form.
pub fn expand_sources(config: Config, candidates: &[String]) -> Result<Config, Error> {
    let Config {
        sources,
        output,
        merge,
    } = config;
    let mut expanded = Vec::with_capacity(sources.len());
    for source in sources {
        let Source::File {
            path,
            tag_prefix,
            additional_blocks,
            filter,
            ..
        } = &source
        else {
            expanded.push(source);
            continue;
        };
        let pattern = path.to_string_lossy().into_owned();
        if !is_glob(&pattern) {
            expanded.push(source);
            continue;
        }

        let base = glob_base(&pattern);
        let mut matches: Vec<&String> = candidates
            .iter()
            .filter(|c| glob_match(&pattern, c) && !is_hidden_below(base, &pattern, c))
            .collect();
        matches.sort();
        matches.dedup();
        if matches.is_empty() {
            return Err(Error::Config(format!(
                "source pattern '{pattern}' matched no files"
            )));
        }
        for matched in matches {
            expanded.push(Source::File {
                name: Some(source_name(&pattern, matched)),
                path: PathBuf::from(matched),
                tag_prefix: tag_prefix.clone(),
                additional_blocks: additional_blocks.clone(),
                filter: filter.clone(),
            });
        }
    }
    Ok(Config {
        sources: expanded,
        output,
        merge,
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

    fn config(yaml: &str) -> Config {
        Config::from_yaml(yaml).unwrap()
    }

    fn files(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|p| p.to_string()).collect()
    }

    fn describe(config: &Config) -> Vec<(String, String)> {
        config
            .sources
            .iter()
            .map(|s| match s {
                Source::File { path, .. } => {
                    (s.display_name(), path.to_string_lossy().into_owned())
                }
                Source::Http { url, .. } => (s.display_name(), url.clone()),
            })
            .collect()
    }

    fn pair(a: &str, b: &str) -> (String, String) {
        (a.to_string(), b.to_string())
    }

    #[test]
    fn flat_glob_expands_sorted() {
        let cfg = config("sources:\n  - path: 'specs/*.yaml'\n");
        let candidates = files(&[
            "specs/users.yaml",
            "specs/billing.yaml",
            "specs/nested/x.yaml",
            "other/a.yaml",
        ]);
        let out = expand_sources(cfg, &candidates).unwrap();
        assert_eq!(
            describe(&out),
            vec![
                pair("billing", "specs/billing.yaml"),
                pair("users", "specs/users.yaml")
            ]
        );
    }

    #[test]
    fn nested_and_double_star_patterns() {
        let candidates = files(&[
            "services/users/api.yaml",
            "services/billing/api.yaml",
            "services/billing/other.yaml",
        ]);
        let out = expand_sources(
            config("sources:\n  - path: 'services/*/api.yaml'\n"),
            &candidates,
        )
        .unwrap();
        assert_eq!(
            describe(&out),
            vec![
                pair("billing_api", "services/billing/api.yaml"),
                pair("users_api", "services/users/api.yaml")
            ]
        );

        let deep = files(&["specs/a/b.yaml", "specs/c.yaml"]);
        let out = expand_sources(config("sources:\n  - path: 'specs/**/*.yaml'\n"), &deep).unwrap();
        assert_eq!(describe(&out), vec![pair("a_b", "specs/a/b.yaml")]);
    }

    #[test]
    fn explicit_sources_keep_their_position_and_names() {
        let cfg = config("sources:\n  - name: first\n    path: z.yaml\n  - path: 'specs/*.yaml'\n  - name: last\n    url: https://x.test/*\n");
        let out = expand_sources(cfg, &files(&["specs/b.yaml", "specs/a.yaml"])).unwrap();
        assert_eq!(
            describe(&out),
            vec![
                pair("first", "z.yaml"),
                pair("a", "specs/a.yaml"),
                pair("b", "specs/b.yaml"),
                pair("last", "https://x.test/*")
            ]
        );
    }

    #[test]
    fn options_apply_to_every_match() {
        let cfg = config("sources:\n  - path: 'specs/*.yaml'\n    tag_prefix: p\n    additional_blocks: {x-a: 1}\n    filter:\n      exclude: {tags: [internal]}\n");
        let out = expand_sources(cfg, &files(&["specs/a.yaml", "specs/b.yaml"])).unwrap();
        assert_eq!(out.sources.len(), 2);
        for source in &out.sources {
            assert_eq!(source.tag_prefix(), "p");
            assert!(source.filter().is_some());
            match source {
                Source::File {
                    additional_blocks, ..
                } => assert!(additional_blocks.is_some()),
                Source::Http { .. } => panic!("expected a file source"),
            }
        }
    }

    #[test]
    fn hidden_entries_are_skipped_unless_the_pattern_asks() {
        let candidates = files(&["specs/.hidden.yaml", "specs/.git/x.yaml", "specs/a.yaml"]);
        let out =
            expand_sources(config("sources:\n  - path: 'specs/*.yaml'\n"), &candidates).unwrap();
        assert_eq!(describe(&out), vec![pair("a", "specs/a.yaml")]);
        let dotted =
            expand_sources(config("sources:\n  - path: 'specs/.*.yaml'\n"), &candidates).unwrap();
        assert_eq!(
            describe(&dotted),
            vec![pair(".hidden", "specs/.hidden.yaml")]
        );
        // `specs/.git/x.yaml` is the only file `**` could match, and it is hidden
        let deep = expand_sources(
            config("sources:\n  - path: 'specs/**/*.yaml'\n"),
            &candidates,
        );
        assert!(deep.is_err());
    }

    #[test]
    fn no_match_is_an_error() {
        let err = expand_sources(
            config("sources:\n  - path: 'specs/*.yaml'\n"),
            &files(&["other/a.yaml"]),
        )
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("source pattern 'specs/*.yaml' matched no files"),
            "{err}"
        );
    }

    #[test]
    fn configs_without_globs_are_unchanged_and_expansion_is_idempotent() {
        let plain = config("sources:\n  - path: a.yaml\n  - url: https://x.test/a\n");
        let before = describe(&plain);
        assert_eq!(describe(&expand_sources(plain, &[]).unwrap()), before);

        let candidates = files(&["specs/a.yaml", "specs/b.yaml"]);
        let once =
            expand_sources(config("sources:\n  - path: 'specs/*.yaml'\n"), &candidates).unwrap();
        let twice = expand_sources(once.clone(), &candidates).unwrap();
        assert_eq!(describe(&once), describe(&twice));
    }
}
