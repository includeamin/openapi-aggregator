//! `--check` mode: validate the merged spec and report the outcome for CI.

use std::path::Path;

use openapi_aggregator_core::{
    parse_content, validate, Finding, MergeReport, OutputFormat, Severity,
};
use serde_json::Value;

/// What a check run concluded; maps to the process exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Everything is fine (exit 0).
    Passed,
    /// Validation errors, denied warnings, or a merge failure (exit 1).
    Failed,
    /// The output file is missing or out of date (exit 2).
    Stale,
}

impl Outcome {
    pub fn exit_code(self) -> i32 {
        match self {
            Outcome::Passed => 0,
            Outcome::Failed => 1,
            Outcome::Stale => 2,
        }
    }
}

/// `error[code] /location: message` (or `warning[...]`).
pub fn render_finding(finding: &Finding) -> String {
    let location = if finding.location.is_empty() {
        "/"
    } else {
        &finding.location
    };
    format!(
        "{}[{}] {}: {}",
        finding.severity.as_str(),
        finding.code,
        location,
        finding.message
    )
}

fn plural(count: usize, word: &str) -> String {
    if count == 1 {
        format!("{count} {word}")
    } else {
        format!("{count} {word}s")
    }
}

/// The closing line of a check run.
pub fn summary(errors: usize, warnings: usize, passed: bool) -> String {
    if passed {
        if warnings == 0 {
            "check passed".to_string()
        } else {
            format!("check passed ({})", plural(warnings, "warning"))
        }
    } else {
        format!(
            "check failed: {}, {}",
            plural(errors, "error"),
            plural(warnings, "warning")
        )
    }
}

/// Validate the merged spec, print the findings to stderr, and decide pass/fail.
/// Merge warnings (already printed by the caller) count as warnings.
pub fn run_validation(report: &MergeReport, deny_warnings: bool) -> Outcome {
    let findings = validate(&report.spec);
    for finding in &findings {
        eprintln!("{}", render_finding(finding));
    }
    let errors = findings
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .count();
    let warnings = findings.len() - errors + report.warnings.len();
    let passed = errors == 0 && !(deny_warnings && warnings > 0);
    eprintln!("{}", summary(errors, warnings, passed));
    if passed {
        Outcome::Passed
    } else {
        Outcome::Failed
    }
}

/// How the existing output file compares with the freshly merged spec.
#[derive(Debug)]
pub enum Comparison {
    UpToDate,
    Missing,
    Stale {
        diff: String,
    },
    /// The file exists but is not valid JSON/YAML.
    Unreadable(String),
}

fn render(value: &Value, format: &OutputFormat) -> String {
    match format {
        OutputFormat::Yaml => serde_yaml::to_string(value).unwrap_or_default(),
        OutputFormat::Json => serde_json::to_string_pretty(value).unwrap_or_default() + "\n",
    }
}

/// Compare the file at `path` with `merged` as data, so formatting, format (YAML vs JSON)
/// and key order never matter. On a difference, `diff` is a unified diff of both sides
/// rendered in `format`.
pub fn compare_output(path: &Path, merged: &Value, format: &OutputFormat) -> Comparison {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Comparison::Missing,
        Err(e) => return Comparison::Unreadable(e.to_string()),
    };
    let existing = match parse_content(&text) {
        Ok(value) => value,
        Err(e) => return Comparison::Unreadable(e.to_string()),
    };
    if &existing == merged {
        return Comparison::UpToDate;
    }
    let old = render(&existing, format);
    let new = render(merged, format);
    let diff = similar::TextDiff::from_lines(&old, &new)
        .unified_diff()
        .context_radius(3)
        .header(&path.display().to_string(), "regenerated")
        .to_string();
    Comparison::Stale { diff }
}

/// Check that the output file is up to date, printing what is wrong to stderr.
pub fn check_output(path: &Path, merged: &Value, format: &OutputFormat) -> Outcome {
    let shown = path.display();
    match compare_output(path, merged, format) {
        Comparison::UpToDate => Outcome::Passed,
        Comparison::Missing => {
            eprintln!("output file {shown} does not exist; run without --check to create it");
            Outcome::Stale
        }
        Comparison::Stale { diff } => {
            eprint!("{diff}");
            eprintln!("output file {shown} is out of date; regenerate it without --check");
            Outcome::Stale
        }
        Comparison::Unreadable(reason) => {
            eprintln!("output file {shown} could not be read ({reason}); it is out of date; regenerate it without --check");
            Outcome::Stale
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_findings() {
        let finding = Finding {
            severity: Severity::Error,
            code: "dangling-ref",
            message: "$ref '#/x' does not resolve".into(),
            location: "/paths/~1p".into(),
        };
        assert_eq!(
            render_finding(&finding),
            "error[dangling-ref] /paths/~1p: $ref '#/x' does not resolve"
        );
        let root = Finding {
            severity: Severity::Warning,
            location: String::new(),
            ..finding
        };
        assert!(render_finding(&root).starts_with("warning[dangling-ref] /: "));
    }

    #[test]
    fn summarises_with_plurals() {
        assert_eq!(summary(0, 0, true), "check passed");
        assert_eq!(summary(0, 2, true), "check passed (2 warnings)");
        assert_eq!(summary(1, 0, false), "check failed: 1 error, 0 warnings");
        assert_eq!(summary(2, 1, false), "check failed: 2 errors, 1 warning");
    }

    #[test]
    fn outcomes_map_to_exit_codes() {
        assert_eq!(Outcome::Passed.exit_code(), 0);
        assert_eq!(Outcome::Failed.exit_code(), 1);
        assert_eq!(Outcome::Stale.exit_code(), 2);
    }

    fn merged() -> serde_json::Value {
        serde_json::json!({"openapi": "3.0.3", "info": {"title": "T", "version": "1"}, "paths": {}})
    }

    #[test]
    fn compares_the_output_file_by_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.yaml");

        assert!(matches!(
            compare_output(&path, &merged(), &OutputFormat::Yaml),
            Comparison::Missing
        ));

        std::fs::write(
            &path,
            "paths: {}\ninfo: {version: '1', title: T}\nopenapi: 3.0.3\n",
        )
        .unwrap();
        assert!(matches!(
            compare_output(&path, &merged(), &OutputFormat::Yaml),
            Comparison::UpToDate
        ));

        std::fs::write(
            &path,
            "openapi: 3.0.3\ninfo: {title: Other, version: '1'}\npaths: {}\n",
        )
        .unwrap();
        match compare_output(&path, &merged(), &OutputFormat::Yaml) {
            Comparison::Stale { diff } => {
                assert!(diff.contains("+++ regenerated"), "{diff}");
                assert!(
                    diff.contains("-  title: Other") && diff.contains("+  title: T"),
                    "{diff}"
                );
            }
            other => panic!("expected Stale, got {other:?}"),
        }

        std::fs::write(&path, "{: nope").unwrap();
        assert!(matches!(
            compare_output(&path, &merged(), &OutputFormat::Yaml),
            Comparison::Unreadable(_)
        ));
    }
}
