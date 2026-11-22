use std::path::Path;

use openapi_aggregator::{
    aggregate, aggregate_from_file, aggregate_with_options, aggregate_with_report, load_config,
    load_source, merge_specs, AggregateOptions, Config, ConflictStrategy, MergeConfig, Source,
};

fn fixtures() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures"))
}

#[tokio::test]
async fn load_yaml_file_source() {
    let source = Source::File {
        name: Some("petstore".into()),
        path: fixtures().join("petstore.yaml"),
        tag_prefix: None,
        additional_blocks: None,
        filter: None,
    };
    let (name, spec) = load_source(&source).await.unwrap();
    assert_eq!(name, "petstore");
    assert_eq!(spec["info"]["title"], "Petstore API");
    assert!(spec["paths"]["/pets"].is_object());
}

#[tokio::test]
async fn load_json_file_source() {
    let source = Source::File {
        name: Some("conflict".into()),
        path: fixtures().join("conflict.json"),
        tag_prefix: None,
        additional_blocks: None,
        filter: None,
    };
    let (name, spec) = load_source(&source).await.unwrap();
    assert_eq!(name, "conflict");
    assert_eq!(spec["info"]["title"], "Another Pet API");
}

#[tokio::test]
async fn aggregate_two_specs_from_config() {
    let config = Config {
        sources: vec![
            Source::File {
                name: Some("petstore".into()),
                path: fixtures().join("petstore.yaml"),
                tag_prefix: None,
                additional_blocks: None,
                filter: None,
            },
            Source::File {
                name: Some("users".into()),
                path: fixtures().join("users.yaml"),
                tag_prefix: None,
                additional_blocks: None,
                filter: None,
            },
        ],
        output: Default::default(),
        merge: MergeConfig::default(),
    };

    let merged = aggregate(&config).await.unwrap();

    // Paths from both sources are present
    assert!(merged["paths"]["/pets"].is_object());
    assert!(merged["paths"]["/pets/{petId}"].is_object());
    assert!(merged["paths"]["/users"].is_object());
    assert!(merged["paths"]["/users/{userId}"].is_object());

    // Schemas from both sources are present
    assert!(merged["components"]["schemas"]["Pet"].is_object());
    assert!(merged["components"]["schemas"]["User"].is_object());

    // Tags are merged and deduplicated
    let tags = merged["tags"].as_array().unwrap();
    assert_eq!(tags.len(), 2);
}

#[tokio::test]
async fn aggregate_applies_additional_blocks_per_source() {
    let config = Config {
        sources: vec![
            Source::File {
                name: Some("petstore".into()),
                path: fixtures().join("petstore.yaml"),
                tag_prefix: None,
                additional_blocks: Some(serde_json::json!({
                    "x-custom-root": {
                        "owner": "platform"
                    },
                    "paths": {
                        "/pets": {
                            "get": {
                                "x-custom-operation": {
                                    "rate_limit": 100
                                }
                            }
                        }
                    }
                })),
                filter: None,
            },
            Source::File {
                name: Some("users".into()),
                path: fixtures().join("users.yaml"),
                tag_prefix: None,
                additional_blocks: None,
                filter: None,
            },
        ],
        output: Default::default(),
        merge: MergeConfig::default(),
    };

    let merged = aggregate(&config).await.unwrap();
    assert_eq!(merged["x-custom-root"]["owner"], "platform");
    assert_eq!(
        merged["paths"]["/pets"]["get"]["x-custom-operation"]["rate_limit"],
        100
    );
}

#[tokio::test]
async fn aggregate_conflict_errors_by_default() {
    let config = Config {
        sources: vec![
            Source::File {
                name: Some("petstore".into()),
                path: fixtures().join("petstore.yaml"),
                tag_prefix: None,
                additional_blocks: None,
                filter: None,
            },
            Source::File {
                name: Some("conflict".into()),
                path: fixtures().join("conflict.json"),
                tag_prefix: None,
                additional_blocks: None,
                filter: None,
            },
        ],
        output: Default::default(),
        merge: MergeConfig::default(), // conflict_strategy: Error
    };

    let result = aggregate(&config).await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("duplicate path"));
}

#[tokio::test]
async fn aggregate_conflict_rename_rewrites_refs() {
    let config = Config {
        sources: vec![
            Source::File {
                name: Some("petstore".into()),
                path: fixtures().join("petstore.yaml"),
                tag_prefix: None,
                additional_blocks: None,
                filter: None,
            },
            Source::File {
                name: Some("alt".into()),
                path: fixtures().join("conflict.json"),
                tag_prefix: None,
                additional_blocks: None,
                filter: None,
            },
        ],
        output: Default::default(),
        merge: MergeConfig {
            conflict_strategy: ConflictStrategy::Rename,
            ..Default::default()
        },
    };

    let merged = aggregate(&config).await.unwrap();

    // Original Pet schema kept, alt's renamed
    assert!(merged["components"]["schemas"]["Pet"].is_object());
    assert!(merged["components"]["schemas"]["alt_Pet"].is_object());

    // Paths: /pets conflict → second source prefixed
    assert!(merged["paths"]["/pets"].is_object());
    assert!(merged["paths"]["/alt/pets"].is_object());

    // $ref in the alt source's paths should be rewritten
    let alt_ref = &merged["paths"]["/alt/pets"]["get"]["responses"]["200"]["content"]
        ["application/json"]["schema"]["items"]["$ref"];
    assert_eq!(alt_ref, "#/components/schemas/alt_Pet");
}

#[tokio::test]
async fn aggregate_from_config_file() {
    // Write a temporary config file
    let dir = tempfile::tempdir().unwrap();
    let config_content = format!(
        r#"
sources:
  - name: petstore
    path: {petstore}
  - name: users
    path: {users}
merge:
  conflict_strategy: error
"#,
        petstore = fixtures().join("petstore.yaml").display(),
        users = fixtures().join("users.yaml").display(),
    );

    let config_path = dir.path().join("config.yaml");
    std::fs::write(&config_path, config_content).unwrap();

    let merged = aggregate_from_file(&config_path).await.unwrap();
    assert!(merged["paths"]["/pets"].is_object());
    assert!(merged["paths"]["/users"].is_object());
}

#[tokio::test]
async fn merge_specs_preserves_openapi_version() {
    let specs = vec![
        (
            "a".into(),
            "a".into(),
            serde_json::json!({
                "openapi": "3.0.3",
                "info": {"title": "A", "version": "1"},
                "paths": {}
            }),
        ),
        (
            "b".into(),
            "b".into(),
            serde_json::json!({
                "openapi": "3.0.3",
                "info": {"title": "B", "version": "1"},
                "paths": {}
            }),
        ),
    ];

    let merged = merge_specs(specs, &MergeConfig::default()).unwrap();
    assert_eq!(merged["openapi"], "3.0.3");
}

#[tokio::test]
async fn http_source_with_headers() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/openapi.json")
        .match_header("Authorization", "Bearer test-token")
        .with_header("content-type", "application/json")
        .with_body(
            serde_json::json!({
                "openapi": "3.0.3",
                "info": {"title": "Remote API", "version": "1.0"},
                "paths": {
                    "/items": { "get": { "summary": "List items" } }
                }
            })
            .to_string(),
        )
        .create_async()
        .await;

    let source = Source::Http {
        name: Some("remote".into()),
        url: format!("{}/openapi.json", server.url()),
        headers: [("Authorization".into(), "Bearer test-token".into())]
            .into_iter()
            .collect(),
        tag_prefix: None,
        additional_blocks: None,
        filter: None,
    };

    let (name, spec) = load_source(&source).await.unwrap();
    assert_eq!(name, "remote");
    assert_eq!(spec["info"]["title"], "Remote API");
    assert!(spec["paths"]["/items"].is_object());
}

#[tokio::test]
async fn http_source_rejects_non_success_status() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/missing.json")
        .with_status(404)
        .with_header("content-type", "application/json")
        .with_body(
            serde_json::json!({
                "openapi": "3.0.3",
                "info": {"title": "Not Found", "version": "1.0"},
                "paths": {}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let source = Source::Http {
        name: Some("missing".into()),
        url: format!("{}/missing.json", server.url()),
        headers: Default::default(),
        tag_prefix: None,
        additional_blocks: None,
        filter: None,
    };

    let error = load_source(&source).await.unwrap_err();
    assert!(error.to_string().contains("HTTP request failed"));
}

#[tokio::test]
async fn http_source_expands_env_vars_in_headers() {
    std::env::set_var("OPENAPI_AGGREGATOR_TEST_TOKEN", "from-env");

    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/openapi.json")
        .match_header("Authorization", "Bearer from-env")
        .with_body(r#"{"openapi":"3.0.3","info":{"title":"Env","version":"1"},"paths":{}}"#)
        .create_async()
        .await;

    let source = Source::Http {
        name: Some("env".into()),
        url: format!("{}/openapi.json", server.url()),
        headers: [(
            "Authorization".into(),
            "Bearer ${OPENAPI_AGGREGATOR_TEST_TOKEN}".into(),
        )]
        .into_iter()
        .collect(),
        tag_prefix: None,
        additional_blocks: None,
        filter: None,
    };

    let (_, spec) = load_source(&source).await.unwrap();
    assert_eq!(spec["info"]["title"], "Env");
}

#[tokio::test]
async fn aggregate_with_report_returns_version_warnings() {
    let dir = tempfile::tempdir().unwrap();
    let v31 = dir.path().join("v31.yaml");
    std::fs::write(
        &v31,
        "openapi: 3.1.0\ninfo: {title: New, version: '1'}\npaths: {}\n",
    )
    .unwrap();

    let config = Config {
        sources: vec![
            Source::File {
                name: Some("petstore".into()),
                path: fixtures().join("petstore.yaml"),
                tag_prefix: None,
                additional_blocks: None,
                filter: None,
            },
            Source::File {
                name: Some("new".into()),
                path: v31,
                tag_prefix: None,
                additional_blocks: None,
                filter: None,
            },
        ],
        output: Default::default(),
        merge: MergeConfig::default(),
    };

    let report = aggregate_with_report(&config).await.unwrap();
    assert_eq!(report.spec["openapi"], "3.0.3");
    assert_eq!(report.warnings.len(), 1);
}

fn write_cli_config(dir: &Path, format: &str) -> std::path::PathBuf {
    let config_path = dir.join("config.yaml");
    std::fs::write(
        &config_path,
        format!(
            "sources:\n  - path: {}\noutput:\n  format: {format}\n",
            fixtures().join("petstore.yaml").display()
        ),
    )
    .unwrap();
    config_path
}

#[test]
fn cli_uses_output_format_from_config() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = write_cli_config(dir.path(), "json");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_openapi-aggregator"))
        .arg("-c")
        .arg(&config_path)
        .output()
        .unwrap();

    assert!(output.status.success());
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout)
        .expect("stdout should be JSON when config sets output.format: json");
    assert_eq!(parsed["info"]["title"], "Petstore API");
}

#[test]
fn cli_format_flag_overrides_config() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = write_cli_config(dir.path(), "json");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_openapi-aggregator"))
        .arg("-c")
        .arg(&config_path)
        .args(["-f", "yaml"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("openapi:"));
}

#[tokio::test]
async fn http_errors_do_not_leak_expanded_env_secrets() {
    std::env::set_var("OPENAPI_AGGREGATOR_TEST_SECRET", "s3cr3t-value");

    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", mockito::Matcher::Any)
        .with_status(500)
        .create_async()
        .await;

    let source = Source::Http {
        name: Some("secret".into()),
        url: format!(
            "{}/spec?token=${{OPENAPI_AGGREGATOR_TEST_SECRET}}",
            server.url()
        ),
        headers: Default::default(),
        tag_prefix: None,
        additional_blocks: None,
        filter: None,
    };

    let err = load_source(&source).await.unwrap_err();
    let message = format!("{err} {:?}", std::error::Error::source(&err));
    assert!(!message.contains("s3cr3t-value"), "{message}");
    assert!(
        message.contains("${OPENAPI_AGGREGATOR_TEST_SECRET}"),
        "{message}"
    );
}

fn collect_refs(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                match v {
                    serde_json::Value::String(s) if k == "$ref" => out.push(s.clone()),
                    _ => collect_refs(v, out),
                }
            }
        }
        serde_json::Value::Array(items) => items.iter().for_each(|v| collect_refs(v, out)),
        _ => {}
    }
}

#[tokio::test]
async fn swagger2_source_merges_with_openapi3_and_refs_resolve() {
    let config = Config {
        sources: vec![
            Source::File {
                name: Some("petstore".into()),
                path: fixtures().join("petstore.yaml"),
                tag_prefix: None,
                additional_blocks: None,
                filter: None,
            },
            Source::File {
                name: Some("legacy".into()),
                path: fixtures().join("swagger2.yaml"),
                tag_prefix: None,
                additional_blocks: None,
                filter: None,
            },
        ],
        output: Default::default(),
        merge: MergeConfig {
            conflict_strategy: ConflictStrategy::Rename,
            ..Default::default()
        },
    };
    let report = aggregate_with_report(&config).await.unwrap();
    assert_eq!(report.spec["openapi"], "3.0.3");
    assert!(report.spec["paths"]["/legacy-pets"]["post"]["requestBody"].is_object());
    assert!(report
        .warnings
        .iter()
        .any(|w| w.contains("source 'legacy': converted from Swagger 2.0")));

    let mut refs = Vec::new();
    collect_refs(&report.spec, &mut refs);
    assert!(!refs.is_empty());
    for r in refs {
        let pointer = r.trim_start_matches('#');
        assert!(report.spec.pointer(pointer).is_some(), "dangling $ref {r}");
    }
}

// ---------------------------------------------------------------------------
// external $refs
// ---------------------------------------------------------------------------

const ROOT_SPEC: &str = "openapi: 3.0.3\ninfo: {title: Root, version: '1'}\npaths:\n  /pets:\n    get:\n      responses:\n        '200':\n          description: ok\n          content:\n            application/json:\n              schema:\n                $ref: REF\n";

fn root_spec(reference: &str) -> String {
    ROOT_SPEC.replace("REF", &format!("'{reference}'"))
}

fn write(path: &std::path::Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// Aggregate the config at `dir/cfg.yaml` with default options (sandboxed) or with the escape hatch.
async fn run_config(
    dir: &std::path::Path,
    allow_outside: bool,
) -> Result<openapi_aggregator::MergeReport, openapi_aggregator::Error> {
    let config = load_config(&dir.join("cfg.yaml")).unwrap();
    let options = AggregateOptions {
        allow_outside_config_dir: allow_outside,
        config_dir: Some(dir.to_path_buf()),
    };
    aggregate_with_options(&config, &options).await
}

fn assert_refs_resolve(spec: &serde_json::Value) {
    let mut refs = Vec::new();
    collect_refs(spec, &mut refs);
    assert!(!refs.is_empty());
    for r in refs {
        assert!(
            spec.pointer(r.trim_start_matches('#')).is_some(),
            "dangling $ref {r}"
        );
    }
}

#[tokio::test]
async fn multi_file_spec_merges_and_refs_resolve() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("cfg.yaml"),
        "sources:\n  - name: pets\n    path: openapi.yaml\n",
    );
    write(
        &dir.path().join("openapi.yaml"),
        &root_spec("./schemas/pet.yaml#/Pet"),
    );
    write(
        &dir.path().join("schemas/pet.yaml"),
        "Pet:\n  type: object\n  properties:\n    owner:\n      $ref: './owner.yaml#/Owner'\n",
    );
    write(
        &dir.path().join("schemas/owner.yaml"),
        "Owner:\n  type: string\n",
    );

    let report = run_config(dir.path(), false).await.unwrap();
    assert_refs_resolve(&report.spec);
    assert_eq!(
        report.spec["components"]["schemas"]["Owner"]["type"],
        "string"
    );
}

#[tokio::test]
async fn ref_outside_config_dir_is_rejected_unless_allowed() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("cfg");
    write(
        &cfg.join("cfg.yaml"),
        "sources:\n  - name: pets\n    path: openapi.yaml\n",
    );
    write(&cfg.join("openapi.yaml"), &root_spec("../outside.yaml#/X"));
    write(&dir.path().join("outside.yaml"), "X:\n  type: string\n");

    let err = run_config(&cfg, false).await.unwrap_err().to_string();
    assert!(err.contains("is outside the config directory"), "{err}");
    assert!(err.contains("--allow-outside-config-dir"), "{err}");

    let report = run_config(&cfg, true).await.unwrap();
    assert_eq!(report.spec["components"]["schemas"]["X"]["type"], "string");
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_escape_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("cfg");
    write(
        &cfg.join("cfg.yaml"),
        "sources:\n  - name: pets\n    path: openapi.yaml\n",
    );
    write(&cfg.join("openapi.yaml"), &root_spec("./link.yaml#/X"));
    write(&dir.path().join("outside.yaml"), "X:\n  type: string\n");
    std::os::unix::fs::symlink(dir.path().join("outside.yaml"), cfg.join("link.yaml")).unwrap();

    let err = run_config(&cfg, false).await.unwrap_err().to_string();
    assert!(err.contains("is outside the config directory"), "{err}");
}

#[tokio::test]
async fn absolute_path_ref_outside_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("cfg");
    let outside = dir.path().join("outside.yaml");
    write(&outside, "X:\n  type: string\n");
    write(
        &cfg.join("cfg.yaml"),
        "sources:\n  - name: pets\n    path: openapi.yaml\n",
    );
    write(
        &cfg.join("openapi.yaml"),
        &root_spec(&format!("{}#/X", outside.display())),
    );

    let err = run_config(&cfg, false).await.unwrap_err().to_string();
    assert!(err.contains("is outside the config directory"), "{err}");
}

#[tokio::test]
async fn source_outside_config_dir_may_reference_its_siblings() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("cfg");
    write(
        &cfg.join("cfg.yaml"),
        "sources:\n  - name: shared\n    path: ../shared/spec.yaml\n",
    );
    write(
        &dir.path().join("shared/spec.yaml"),
        &root_spec("./defs.yaml#/Def"),
    );
    write(
        &dir.path().join("shared/defs.yaml"),
        "Def:\n  type: string\n",
    );

    let report = run_config(&cfg, false).await.unwrap();
    assert_eq!(
        report.spec["components"]["schemas"]["Def"]["type"],
        "string"
    );
}

#[tokio::test]
async fn missing_ref_file_names_source_and_referrer() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("cfg.yaml"),
        "sources:\n  - name: pets\n    path: openapi.yaml\n",
    );
    write(
        &dir.path().join("openapi.yaml"),
        &root_spec("./nope.yaml#/X"),
    );

    let err = run_config(dir.path(), false).await.unwrap_err().to_string();
    assert!(
        err.contains("pets") && err.contains("nope.yaml") && err.contains("openapi.yaml"),
        "{err}"
    );
}

#[tokio::test]
async fn too_many_external_documents_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("cfg.yaml"),
        "sources:\n  - name: pets\n    path: openapi.yaml\n",
    );
    write(&dir.path().join("openapi.yaml"), &root_spec("./d1.yaml#/D"));
    for i in 1..=101 {
        write(
            &dir.path().join(format!("d{i}.yaml")),
            &format!(
                "D:\n  properties:\n    n:\n      $ref: './d{}.yaml#/D'\n",
                i + 1
            ),
        );
    }
    let err = run_config(dir.path(), false).await.unwrap_err().to_string();
    assert!(err.contains("limit 100"), "{err}");
}

fn spec_json(reference: &str) -> String {
    serde_json::json!({
        "openapi": "3.0.3",
        "info": {"title": "Remote", "version": "1"},
        "paths": {"/r": {"get": {"responses": {"200": {"description": "ok",
            "content": {"application/json": {"schema": {"$ref": reference}}}}}}}}
    })
    .to_string()
}

fn http_config(url: String, headers: &[(&str, &str)]) -> Config {
    Config {
        sources: vec![Source::Http {
            name: Some("remote".into()),
            url,
            headers: headers
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            tag_prefix: None,
            additional_blocks: None,
            filter: None,
        }],
        output: Default::default(),
        merge: MergeConfig::default(),
    }
}

#[tokio::test]
async fn url_source_resolves_relative_refs_over_http() {
    let mut server = mockito::Server::new_async().await;
    let _spec = server
        .mock("GET", "/api/spec.json")
        .with_body(spec_json("defs.yaml#/Def"))
        .create_async()
        .await;
    let _defs = server
        .mock("GET", "/api/defs.yaml")
        .with_body("Def:\n  type: string\n")
        .create_async()
        .await;

    let config = http_config(format!("{}/api/spec.json", server.url()), &[]);
    let report = aggregate_with_options(&config, &AggregateOptions::default())
        .await
        .unwrap();
    assert_eq!(
        report.spec["components"]["schemas"]["Def"]["type"],
        "string"
    );
}

#[tokio::test]
async fn headers_are_only_sent_to_the_same_origin() {
    let mut a = mockito::Server::new_async().await;
    let mut b = mockito::Server::new_async().await;
    let reference = format!("{}/x.yaml#/Foreign", b.url());
    let _spec = a
        .mock("GET", "/spec.json")
        .match_header("Authorization", "Bearer t")
        .with_body(spec_json("./local.yaml#/Local"))
        .create_async()
        .await;
    let _spec2 = a
        .mock("GET", "/spec2.json")
        .match_header("Authorization", "Bearer t")
        .with_body(spec_json(&reference))
        .create_async()
        .await;
    let _local = a
        .mock("GET", "/local.yaml")
        .match_header("Authorization", "Bearer t")
        .with_body("Local:\n  type: string\n")
        .create_async()
        .await;
    let foreign = b
        .mock("GET", "/x.yaml")
        .match_header("Authorization", mockito::Matcher::Missing)
        .with_body("Foreign:\n  type: integer\n")
        .create_async()
        .await;

    let same = http_config(
        format!("{}/spec.json", a.url()),
        &[("Authorization", "Bearer t")],
    );
    let report = aggregate_with_options(&same, &AggregateOptions::default())
        .await
        .unwrap();
    assert_eq!(
        report.spec["components"]["schemas"]["Local"]["type"],
        "string"
    );

    let cross = http_config(
        format!("{}/spec2.json", a.url()),
        &[("Authorization", "Bearer t")],
    );
    let report = aggregate_with_options(&cross, &AggregateOptions::default())
        .await
        .unwrap();
    assert_eq!(
        report.spec["components"]["schemas"]["Foreign"]["type"],
        "integer"
    );
    foreign.assert_async().await;
}

#[tokio::test]
async fn url_source_cannot_read_local_files() {
    let mut server = mockito::Server::new_async().await;
    let _spec = server
        .mock("GET", "/spec.json")
        .with_body(spec_json("file:///etc/hostname#/x"))
        .create_async()
        .await;
    let config = http_config(format!("{}/spec.json", server.url()), &[]);
    let err = aggregate_with_options(&config, &AggregateOptions::default())
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("file:"), "{err}");
}

// ---------------------------------------------------------------------------
// operation filters
// ---------------------------------------------------------------------------

fn service_spec(name: &str, public_path: &str) -> String {
    format!(
        "openapi: 3.0.3
info: {{title: {name}, version: '1'}}
tags: [{{name: internal}}, {{name: public}}]
paths:
  {public_path}:
    get:
      tags: [public]
      responses:
        '200':
          description: ok
          content:
            application/json:
              schema: {{$ref: '#/components/schemas/Shared'}}
  /internal/metrics:
    get:
      tags: [internal]
      responses:
        '200':
          description: ok
          content:
            application/json:
              schema: {{$ref: '#/components/schemas/{name}Internal'}}
components:
  schemas:
    Shared: {{type: string}}
    {name}Internal: {{type: object}}
"
    )
}

fn filter_dir(config: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write(&dir.path().join("a.yaml"), &service_spec("A", "/a"));
    write(&dir.path().join("b.yaml"), &service_spec("B", "/b"));
    write(&dir.path().join("cfg.yaml"), config);
    dir
}

const TWO_SOURCES: &str =
    "sources:\n  - name: a\n    path: a.yaml\n  - name: b\n    path: b.yaml\n";

#[tokio::test]
async fn excluded_operations_no_longer_conflict_and_are_pruned() {
    let dir = filter_dir(&format!(
        "{TWO_SOURCES}merge:\n  filter:\n    exclude:\n      paths: ['/internal/**']\n"
    ));
    let report = run_config(dir.path(), false).await.unwrap();
    let paths: Vec<&String> = report.spec["paths"].as_object().unwrap().keys().collect();
    assert_eq!(paths, vec!["/a", "/b"]);
    let schemas: Vec<&String> = report.spec["components"]["schemas"]
        .as_object()
        .unwrap()
        .keys()
        .collect();
    assert_eq!(schemas, vec!["Shared"]);
    let tags = serde_json::to_string(&report.spec["tags"]).unwrap();
    assert!(!tags.contains("internal"), "{tags}");
}

#[tokio::test]
async fn without_a_filter_the_same_sources_still_conflict() {
    let dir = filter_dir(TWO_SOURCES);
    assert!(run_config(dir.path(), false).await.is_err());
}

#[tokio::test]
async fn source_level_include_narrows_only_that_source() {
    let dir = filter_dir(
        "sources:\n  - name: a\n    path: a.yaml\n    filter:\n      include:\n        tags: [public]\n  - name: b\n    path: b.yaml\n    filter:\n      exclude:\n        tags: [internal]\n",
    );
    let report = run_config(dir.path(), false).await.unwrap();
    let paths: Vec<&String> = report.spec["paths"].as_object().unwrap().keys().collect();
    assert_eq!(paths, vec!["/a", "/b"]);
}

// ---------------------------------------------------------------------------
// glob sources
// ---------------------------------------------------------------------------

fn small_spec(title: &str, path: &str) -> String {
    format!(
        "openapi: 3.0.3\ninfo: {{title: {title}, version: '1'}}\npaths:\n  {path}:\n    get:\n      operationId: get{title}\n      responses:\n        '200': {{description: ok}}\n"
    )
}

fn glob_dir(pattern: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("cfg.yaml"),
        &format!("sources:\n  - path: '{pattern}'\n"),
    );
    dir
}

fn merged_paths(report: &openapi_aggregator::MergeReport) -> Vec<String> {
    report.spec["paths"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect()
}

#[tokio::test]
async fn glob_merges_all_matches_in_sorted_order() {
    let dir = glob_dir("specs/*.yaml");
    write(&dir.path().join("specs/b.yaml"), &small_spec("B", "/b"));
    write(&dir.path().join("specs/a.yaml"), &small_spec("A", "/a"));
    write(&dir.path().join("specs/c.yaml"), &small_spec("C", "/c"));
    write(&dir.path().join("specs/notes.txt"), "not a spec");

    // notes.txt does not match *.yaml; the first sorted file supplies `info`
    let report = run_config(dir.path(), false).await.unwrap();
    assert_eq!(merged_paths(&report), vec!["/a", "/b", "/c"]);
    assert_eq!(report.spec["info"]["title"], "A");

    // adding another matching file does not disturb the others
    write(&dir.path().join("specs/zzz.yaml"), &small_spec("Z", "/z"));
    let report = run_config(dir.path(), false).await.unwrap();
    assert_eq!(merged_paths(&report), vec!["/a", "/b", "/c", "/z"]);
    assert_eq!(report.spec["info"]["title"], "A");
}

#[tokio::test]
async fn nested_glob_names_sources_by_relative_path() {
    let dir = glob_dir("services/*/api.yaml");
    write(
        &dir.path().join("services/billing/api.yaml"),
        &small_spec("Billing", "/items"),
    );
    write(
        &dir.path().join("services/users/api.yaml"),
        &small_spec("Users", "/items"),
    );
    write(
        &dir.path().join("cfg.yaml"),
        "sources:\n  - path: 'services/*/api.yaml'\nmerge:\n  conflict_strategy: rename\n",
    );

    let report = run_config(dir.path(), false).await.unwrap();
    // `billing_api` sorts first and keeps /items; `users_api` is renamed with its source name
    assert_eq!(merged_paths(&report), vec!["/items", "/users_api/items"]);
}

#[tokio::test]
async fn double_star_skips_hidden_directories_and_symlinked_directories() {
    let dir = glob_dir("specs/**/*.yaml");
    write(
        &dir.path().join("specs/deep/b.yaml"),
        &small_spec("B", "/b"),
    );
    write(
        &dir.path().join("specs/.git/c.yaml"),
        &small_spec("C", "/c"),
    );
    write(&dir.path().join("outside/x.yaml"), &small_spec("X", "/x"));
    #[cfg(unix)]
    std::os::unix::fs::symlink(dir.path().join("outside"), dir.path().join("specs/link")).unwrap();

    let report = run_config(dir.path(), false).await.unwrap();
    assert_eq!(merged_paths(&report), vec!["/b"]);
}

#[tokio::test]
async fn a_pattern_without_matches_is_an_error() {
    let dir = glob_dir("specs/*.yaml");
    let err = run_config(dir.path(), false).await.unwrap_err().to_string();
    assert!(err.contains("matched no files"), "{err}");
}

#[tokio::test]
async fn explicit_sources_keep_their_position_next_to_a_glob() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("first.yaml"),
        &small_spec("First", "/first"),
    );
    write(&dir.path().join("specs/a.yaml"), &small_spec("A", "/a"));
    write(
        &dir.path().join("cfg.yaml"),
        "sources:\n  - name: first\n    path: first.yaml\n  - path: 'specs/*.yaml'\n",
    );
    let report = run_config(dir.path(), false).await.unwrap();
    assert_eq!(merged_paths(&report), vec!["/first", "/a"]);
    assert_eq!(report.spec["info"]["title"], "First");
}

#[tokio::test]
async fn hand_built_configs_with_a_glob_work() {
    let dir = tempfile::tempdir().unwrap();
    write(&dir.path().join("specs/a.yaml"), &small_spec("A", "/a"));
    write(&dir.path().join("specs/b.yaml"), &small_spec("B", "/b"));
    let config = Config::from_yaml(&format!(
        "sources:\n  - path: '{}/specs/*.yaml'\n",
        dir.path().display()
    ))
    .unwrap();
    let report = aggregate_with_options(&config, &AggregateOptions::default())
        .await
        .unwrap();
    assert_eq!(merged_paths(&report), vec!["/a", "/b"]);
}

#[tokio::test]
async fn expanded_sources_still_resolve_and_sandbox_external_refs() {
    let root = tempfile::tempdir().unwrap();
    let cfg = root.path().join("cfg");
    write(
        &cfg.join("cfg.yaml"),
        "sources:\n  - path: 'specs/*.api.yaml'\n",
    );
    let with_ref = |target: &str| {
        format!(
            "openapi: 3.0.3\ninfo: {{title: R, version: '1'}}\npaths:\n  /r:\n    get:\n      responses:\n        '200':\n          description: ok\n          content:\n            application/json:\n              schema: {{$ref: '{target}'}}\n"
        )
    };
    write(
        &cfg.join("specs/a.api.yaml"),
        &with_ref("./shared/defs.yaml#/Def"),
    );
    write(
        &cfg.join("specs/shared/defs.yaml"),
        "Def:\n  type: string\n",
    );
    let report = run_config(&cfg, false).await.unwrap();
    assert_eq!(
        report.spec["components"]["schemas"]["Def"]["type"],
        "string"
    );

    write(
        &cfg.join("specs/a.api.yaml"),
        &with_ref("../../outside.yaml#/X"),
    );
    write(&root.path().join("outside.yaml"), "X:\n  type: string\n");
    let err = run_config(&cfg, false).await.unwrap_err().to_string();
    assert!(err.contains("outside the config directory"), "{err}");
}
