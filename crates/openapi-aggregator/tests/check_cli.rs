use std::path::Path;
use std::process::Command;

fn run(dir: &Path, args: &[&str]) -> (i32, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_openapi-aggregator"))
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn write(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).unwrap();
}

fn spec(title: &str, path: &str, extra: &str) -> String {
    format!(
        "openapi: 3.0.3\ninfo: {{title: {title}, version: '1'}}\n{extra}paths:\n  {path}:\n    get:\n      operationId: get{title}\n      responses:\n        '200': {{description: ok}}\n"
    )
}

fn valid_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "a.yaml", &spec("A", "/a", ""));
    write(dir.path(), "b.yaml", &spec("B", "/b", ""));
    write(
        dir.path(),
        "cfg.yaml",
        "sources:\n  - name: a\n    path: a.yaml\n  - name: b\n    path: b.yaml\n",
    );
    dir
}

const DANGLING_CFG: &str = "sources:\n  - name: a\n    path: a.yaml\n    additional_blocks:\n      components:\n        schemas:\n          Bad:\n            $ref: '#/components/schemas/Ghost'\n";

#[test]
fn valid_config_passes() {
    let dir = valid_dir();
    let (code, stdout, stderr) = run(dir.path(), &["--check", "-c", "cfg.yaml"]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.is_empty(), "{stdout}");
    assert!(stderr.contains("check passed"), "{stderr}");
}

#[test]
fn dangling_ref_fails_with_the_finding() {
    let dir = valid_dir();
    write(dir.path(), "cfg.yaml", DANGLING_CFG);
    let (code, stdout, stderr) = run(dir.path(), &["--check", "-c", "cfg.yaml"]);
    assert_eq!(code, 1, "{stderr}");
    assert!(stdout.is_empty(), "{stdout}");
    assert!(stderr.contains("error[dangling-ref]"), "{stderr}");
    assert!(
        stderr.contains("check failed: 1 error, 0 warnings"),
        "{stderr}"
    );
}

#[test]
fn warnings_pass_unless_denied() {
    let dir = valid_dir();
    let tagged = "openapi: 3.0.3\ninfo: {title: T, version: '1'}\ntags: [{name: a}]\npaths:\n  /t:\n    get:\n      tags: [b]\n      responses:\n        '200': {description: ok}\n";
    write(dir.path(), "a.yaml", tagged);
    write(dir.path(), "cfg.yaml", "sources:\n  - path: a.yaml\n");
    let (code, _, stderr) = run(dir.path(), &["--check", "-c", "cfg.yaml"]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stderr.contains("warning[undeclared-tag]"), "{stderr}");

    let (code, _, stderr) = run(
        dir.path(),
        &["--check", "--deny-warnings", "-c", "cfg.yaml"],
    );
    assert_eq!(code, 1, "{stderr}");
    assert!(
        stderr.contains("check failed: 0 errors, 1 warning"),
        "{stderr}"
    );
}

#[test]
fn merge_warnings_are_denied_too() {
    let dir = valid_dir();
    write(
        dir.path(),
        "b.yaml",
        &spec("B", "/b", "").replace("3.0.3", "3.1.0"),
    );
    let (code, _, stderr) = run(dir.path(), &["--check", "-c", "cfg.yaml"]);
    assert_eq!(code, 0, "{stderr}");
    let (code, _, stderr) = run(
        dir.path(),
        &["--check", "--deny-warnings", "-c", "cfg.yaml"],
    );
    assert_eq!(code, 1, "{stderr}");
}

#[test]
fn merge_failure_exits_1() {
    let dir = valid_dir();
    write(dir.path(), "b.yaml", &spec("B", "/a", ""));
    let (code, _, stderr) = run(dir.path(), &["--check", "-c", "cfg.yaml"]);
    assert_eq!(code, 1, "{stderr}");
    assert!(
        stderr.contains("conflict") || stderr.contains("error"),
        "{stderr}"
    );
}

#[test]
fn deny_warnings_requires_check() {
    let dir = valid_dir();
    let (code, _, stderr) = run(dir.path(), &["--deny-warnings", "-c", "cfg.yaml"]);
    assert_eq!(code, 2);
    assert!(stderr.contains("--check"), "{stderr}");
}

#[test]
fn runs_without_check_are_unchanged() {
    let dir = valid_dir();
    write(dir.path(), "cfg.yaml", DANGLING_CFG);
    let (code, stdout, stderr) = run(dir.path(), &["-c", "cfg.yaml"]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("Ghost"), "{stdout}");
    assert!(!stderr.contains("check"), "{stderr}");
}

fn generate(dir: &Path, args: &[&str]) {
    let (code, _, stderr) = run(dir, args);
    assert_eq!(code, 0, "{stderr}");
}

#[test]
fn up_to_date_output_passes() {
    let dir = valid_dir();
    generate(dir.path(), &["-c", "cfg.yaml", "-o", "out.yaml"]);
    let (code, stdout, stderr) = run(dir.path(), &["--check", "-c", "cfg.yaml", "-o", "out.yaml"]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.is_empty());
}

#[test]
fn stale_output_exits_2_with_a_diff_and_is_not_modified() {
    let dir = valid_dir();
    generate(dir.path(), &["-c", "cfg.yaml", "-o", "out.yaml"]);
    let original = std::fs::read_to_string(dir.path().join("out.yaml")).unwrap();
    let stale = original.replace("title: A", "title: Z");
    assert_ne!(stale, original);
    write(dir.path(), "out.yaml", &stale);

    let (code, stdout, stderr) = run(dir.path(), &["--check", "-c", "cfg.yaml", "-o", "out.yaml"]);
    assert_eq!(code, 2, "{stderr}");
    assert!(stdout.is_empty());
    assert!(
        stderr.contains("--- ") && stderr.contains("+++ regenerated"),
        "{stderr}"
    );
    assert!(
        stderr.contains("-  title: Z") && stderr.contains("+  title: A"),
        "{stderr}"
    );
    assert!(stderr.contains("out of date"), "{stderr}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("out.yaml")).unwrap(),
        stale
    );
}

#[test]
fn missing_output_exits_2_and_is_not_created() {
    let dir = valid_dir();
    let (code, _, stderr) = run(
        dir.path(),
        &["--check", "-c", "cfg.yaml", "-o", "nope.yaml"],
    );
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains("does not exist"), "{stderr}");
    assert!(!dir.path().join("nope.yaml").exists());
}

#[test]
fn format_and_key_order_do_not_matter() {
    let dir = valid_dir();
    generate(
        dir.path(),
        &["-c", "cfg.yaml", "-f", "json", "-o", "out.json"],
    );
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join("out.json")).unwrap())
            .unwrap();
    let reversed: serde_json::Map<String, serde_json::Value> = json
        .as_object()
        .unwrap()
        .iter()
        .rev()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    write(
        dir.path(),
        "out.yaml",
        &serde_yaml::to_string(&reversed).unwrap(),
    );

    let (code, _, stderr) = run(dir.path(), &["--check", "-c", "cfg.yaml", "-o", "out.yaml"]);
    assert_eq!(code, 0, "{stderr}");
    let (code, _, stderr) = run(
        dir.path(),
        &["--check", "-c", "cfg.yaml", "-f", "json", "-o", "out.json"],
    );
    assert_eq!(code, 0, "{stderr}");
}

#[test]
fn invalid_and_stale_exits_1_and_still_prints_the_diff() {
    let dir = valid_dir();
    write(dir.path(), "cfg.yaml", DANGLING_CFG);
    write(dir.path(), "out.yaml", "openapi: 3.0.3\n");
    let (code, _, stderr) = run(dir.path(), &["--check", "-c", "cfg.yaml", "-o", "out.yaml"]);
    assert_eq!(code, 1, "{stderr}");
    assert!(
        stderr.contains("error[dangling-ref]") && stderr.contains("out of date"),
        "{stderr}"
    );
}

#[test]
fn unparseable_output_file_is_stale() {
    let dir = valid_dir();
    write(dir.path(), "out.yaml", "{: nope");
    let (code, _, stderr) = run(dir.path(), &["--check", "-c", "cfg.yaml", "-o", "out.yaml"]);
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains("out of date"), "{stderr}");
}
