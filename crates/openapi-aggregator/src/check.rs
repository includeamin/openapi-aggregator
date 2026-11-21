//! `--check` mode: validate the merged spec and report the outcome for CI.

use openapi_aggregator_core::{validate, Finding, MergeReport, Severity};

/// What a check run concluded; maps to the process exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Everything is fine (exit 0).
    Passed,
    /// Validation errors, denied warnings, or a merge failure (exit 1).
    Failed,
    /// The output file is missing or out of date (exit 2).
    #[allow(dead_code)] // constructed once output comparison lands
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
}
