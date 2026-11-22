//! Expand glob file sources by listing the real file system.

use std::fs;
use std::path::Path;

use openapi_aggregator_core::{
    expand_sources, glob_base, is_glob, Config, Error as CoreError, Source,
};

use crate::error::Error;

/// Stop listing after visiting this many directory entries (a pattern like `**/*.yaml` from a
/// huge tree is almost certainly a mistake).
const MAX_ENTRIES: usize = 100_000;

fn config_error(message: String) -> Error {
    CoreError::Config(message).into()
}

/// Replace every glob file source of `config` by one source per matching file.
/// Configs without globs are returned untouched.
pub(crate) fn expand_config(config: Config) -> Result<Config, Error> {
    let patterns: Vec<String> = config
        .sources
        .iter()
        .filter_map(|s| match s {
            Source::File { path, .. } => Some(path.to_string_lossy().into_owned()),
            Source::Http { .. } => None,
        })
        .filter(|p| is_glob(p))
        .collect();
    if patterns.is_empty() {
        return Ok(config);
    }

    let mut candidates = Vec::new();
    for pattern in &patterns {
        let base = glob_base(pattern);
        let max_depth = if pattern.contains("**") {
            None
        } else {
            let relative = pattern.get(base.len()..).unwrap_or("");
            Some(relative.trim_start_matches('/').split('/').count())
        };
        candidates.extend(list_files(base, max_depth, MAX_ENTRIES).map_err(|e| match e {
            ListError::TooMany => config_error(format!(
                "source pattern '{pattern}' visited more than {MAX_ENTRIES} directory entries; narrow the pattern"
            )),
            ListError::Io(e) => config_error(format!(
                "source pattern '{pattern}': failed to list files: {e}"
            )),
        })?);
    }
    candidates.sort();
    candidates.dedup();
    Ok(expand_sources(config, &candidates)?)
}

#[derive(Debug)]
pub(crate) enum ListError {
    TooMany,
    Io(std::io::Error),
}

/// List files below `base` (`""` means the current directory) as strings that start with `base`
/// followed by `/`, exactly like the pattern text, so they can be matched against it.
///
/// Hidden entries are skipped, directory symlinks are not followed (a symlink to a file is
/// listed), `max_depth` limits how many levels are descended, and more than `cap` visited
/// entries is an error. A missing base directory yields no files.
pub(crate) fn list_files(
    base: &str,
    max_depth: Option<usize>,
    cap: usize,
) -> Result<Vec<String>, ListError> {
    let root = if base.is_empty() { "." } else { base };
    let mut files = Vec::new();
    let mut visited = 0usize;
    let mut stack = vec![(root.to_string(), base.to_string(), 1usize)];
    while let Some((dir, prefix, depth)) = stack.pop() {
        let entries = match fs::read_dir(Path::new(&dir)) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(ListError::Io(e)),
        };
        for entry in entries {
            let entry = entry.map_err(ListError::Io)?;
            visited += 1;
            if visited > cap {
                return Err(ListError::TooMany);
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            let file_type = entry.file_type().map_err(ListError::Io)?;
            if file_type.is_dir() {
                if max_depth.is_none_or(|max| depth < max) {
                    stack.push((entry.path().to_string_lossy().into_owned(), path, depth + 1));
                }
            } else if file_type.is_file() {
                files.push(path);
            } else if file_type.is_symlink() {
                // a symlink to a file is a file; a symlink to a directory is never followed
                if fs::metadata(entry.path()).is_ok_and(|m| m.is_file()) {
                    files.push(path);
                }
            }
        }
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listing_stops_at_the_cap() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..10 {
            fs::write(dir.path().join(format!("f{i}.yaml")), "x").unwrap();
        }
        let base = dir.path().to_string_lossy().into_owned();
        assert!(matches!(
            list_files(&base, None, 3),
            Err(ListError::TooMany)
        ));
        assert_eq!(list_files(&base, None, 100).unwrap().len(), 10);
    }

    #[test]
    fn listing_respects_depth_and_skips_hidden_entries() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("a/b")).unwrap();
        fs::create_dir_all(dir.path().join(".hidden")).unwrap();
        fs::write(dir.path().join("top.yaml"), "x").unwrap();
        fs::write(dir.path().join("a/mid.yaml"), "x").unwrap();
        fs::write(dir.path().join("a/b/deep.yaml"), "x").unwrap();
        fs::write(dir.path().join(".hidden/h.yaml"), "x").unwrap();
        fs::write(dir.path().join(".dot.yaml"), "x").unwrap();
        let base = dir.path().to_string_lossy().into_owned();

        let shallow: Vec<String> = list_files(&base, Some(1), 100).unwrap();
        assert_eq!(shallow, vec![format!("{base}/top.yaml")]);
        let mut all = list_files(&base, None, 100).unwrap();
        all.sort();
        assert_eq!(
            all,
            vec![
                format!("{base}/a/b/deep.yaml"),
                format!("{base}/a/mid.yaml"),
                format!("{base}/top.yaml")
            ]
        );
    }

    #[test]
    fn a_missing_base_has_no_files() {
        assert!(list_files("/definitely/not/here", None, 10)
            .unwrap()
            .is_empty());
    }
}
