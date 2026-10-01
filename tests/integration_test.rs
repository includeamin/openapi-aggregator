use std::path::Path;

use openapi_aggregator::{
    aggregate, aggregate_from_file, aggregate_with_report, load_source, merge_specs, Config,
    ConflictStrategy, MergeConfig, Source,
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
            },
            Source::File {
                name: Some("users".into()),
                path: fixtures().join("users.yaml"),
                tag_prefix: None,
                additional_blocks: None,
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
            },
            Source::File {
                name: Some("users".into()),
                path: fixtures().join("users.yaml"),
                tag_prefix: None,
                additional_blocks: None,
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
            },
            Source::File {
                name: Some("conflict".into()),
                path: fixtures().join("conflict.json"),
                tag_prefix: None,
                additional_blocks: None,
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
            },
            Source::File {
                name: Some("alt".into()),
                path: fixtures().join("conflict.json"),
                tag_prefix: None,
                additional_blocks: None,
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
            },
            Source::File {
                name: Some("new".into()),
                path: v31,
                tag_prefix: None,
                additional_blocks: None,
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
