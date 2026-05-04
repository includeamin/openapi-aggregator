use std::path::Path;

use openapi_aggregator::{
    aggregate, aggregate_from_file, load_source, merge_specs, Config, ConflictStrategy,
    MergeConfig, Source,
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
            },
            Source::File {
                name: Some("users".into()),
                path: fixtures().join("users.yaml"),
                tag_prefix: None,
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
async fn aggregate_conflict_errors_by_default() {
    let config = Config {
        sources: vec![
            Source::File {
                name: Some("petstore".into()),
                path: fixtures().join("petstore.yaml"),
                tag_prefix: None,
            },
            Source::File {
                name: Some("conflict".into()),
                path: fixtures().join("conflict.json"),
                tag_prefix: None,
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
            },
            Source::File {
                name: Some("alt".into()),
                path: fixtures().join("conflict.json"),
                tag_prefix: None,
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
    };

    let (name, spec) = load_source(&source).await.unwrap();
    assert_eq!(name, "remote");
    assert_eq!(spec["info"]["title"], "Remote API");
    assert!(spec["paths"]["/items"].is_object());
}
