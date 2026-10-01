use serde_json::Value;

use crate::config::Source;
use crate::error::Error;
use crate::merge::deep_merge;

/// Parse spec text, apply the source's `additional_blocks` and validate it.
/// Returns `(display_name, spec)`.
pub fn prepare_source(content: &str, source: &Source) -> Result<(String, Value), Error> {
    let name = source.display_name();
    let value = parse_and_prepare(content, source)?;
    validate_openapi(&value, &name)?;
    Ok((name, value))
}

fn parse_and_prepare(content: &str, source: &Source) -> Result<Value, Error> {
    let mut value = parse_content(content)?;
    if let Some(blocks) = source_additional_blocks(source) {
        if !blocks.is_object() {
            return Err(Error::InvalidSpec {
                name: source.display_name(),
                reason: "'additional_blocks' must be a mapping/object".into(),
            });
        }
        deep_merge(&mut value, blocks);
    }
    Ok(value)
}

fn source_additional_blocks(source: &Source) -> Option<&Value> {
    match source {
        Source::File {
            additional_blocks, ..
        }
        | Source::Http {
            additional_blocks, ..
        } => additional_blocks.as_ref(),
    }
}

/// Try JSON first, fall back to YAML.
pub fn parse_content(content: &str) -> Result<Value, Error> {
    serde_json::from_str(content).or_else(|_| {
        serde_yaml::from_str::<Value>(content)
            .map_err(|e| Error::Parse(format!("content is neither valid JSON nor YAML: {e}")))
    })
}

/// Minimal validation: the value must be an object with an `openapi` field.
fn validate_openapi(value: &Value, source_name: &str) -> Result<(), Error> {
    match value.get("openapi").and_then(|v| v.as_str()) {
        Some(v) if v.starts_with("3.") => Ok(()),
        Some(v) => Err(Error::InvalidSpec {
            name: source_name.into(),
            reason: format!("unsupported OpenAPI version '{v}' (only 3.x is supported)"),
        }),
        None if value.get("swagger").is_some() => Err(Error::InvalidSpec {
            name: source_name.into(),
            reason: "Swagger 2.0 specs are not supported (only OpenAPI 3.x is supported)".into(),
        }),
        None => Err(Error::InvalidSpec {
            name: source_name.into(),
            reason: "missing 'openapi' field".into(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    #[test]
    fn parse_json_content() {
        let json = r#"{"openapi":"3.0.3","info":{"title":"T","version":"1"},"paths":{}}"#;
        let v = parse_content(json).unwrap();
        assert_eq!(v["openapi"], "3.0.3");
    }

    #[test]
    fn parse_yaml_content() {
        let yaml = "openapi: '3.0.3'\ninfo:\n  title: T\n  version: '1'\npaths: {}";
        let v = parse_content(yaml).unwrap();
        assert_eq!(v["openapi"], "3.0.3");
    }

    #[test]
    fn validate_rejects_missing_openapi() {
        let v = serde_json::json!({"info": {}});
        assert!(validate_openapi(&v, "test").is_err());
    }

    #[test]
    fn validate_rejects_v2() {
        let v = serde_json::json!({"openapi": "2.0"});
        assert!(validate_openapi(&v, "test").is_err());
    }

    #[test]
    fn additional_blocks_are_deep_merged() {
        let source = Source::File {
            name: Some("test".into()),
            path: PathBuf::from("ignored.yaml"),
            tag_prefix: None,
            additional_blocks: Some(json!({
                "x-vendor-root": { "enabled": true },
                "paths": {
                    "/pets": {
                        "get": {
                            "x-vendor-extension": { "timeout": 3000 }
                        }
                    }
                }
            })),
        };

        let base = r#"{
            "openapi": "3.0.3",
            "info": {"title": "T", "version": "1"},
            "paths": {
                "/pets": {
                    "get": {"summary": "list pets"}
                }
            }
        }"#;

        let merged = parse_and_prepare(base, &source).unwrap();
        assert_eq!(merged["x-vendor-root"]["enabled"], true);
        assert_eq!(merged["paths"]["/pets"]["get"]["summary"], "list pets");
        assert_eq!(
            merged["paths"]["/pets"]["get"]["x-vendor-extension"]["timeout"],
            3000
        );
    }

    #[test]
    fn prepare_source_applies_blocks_validates_and_names() {
        let source = Source::File {
            name: Some("pets".into()),
            path: PathBuf::from("ignored.yaml"),
            tag_prefix: None,
            additional_blocks: Some(json!({ "x-team": "core" })),
        };
        let (name, spec) = prepare_source(
            r#"{"openapi":"3.0.3","info":{"title":"T","version":"1"},"paths":{}}"#,
            &source,
        )
        .unwrap();
        assert_eq!(name, "pets");
        assert_eq!(spec["x-team"], "core");
    }

    #[test]
    fn prepare_source_rejects_non_openapi_with_source_name() {
        let source = Source::File {
            name: Some("legacy".into()),
            path: PathBuf::from("ignored.yaml"),
            tag_prefix: None,
            additional_blocks: None,
        };
        let err = prepare_source("swagger: '2.0'\ninfo: {title: T, version: '1'}\n", &source)
            .unwrap_err()
            .to_string();
        assert!(err.contains("legacy"), "{err}");
        assert!(err.contains("Swagger 2.0") && err.contains("3.x"), "{err}");
    }

    #[test]
    fn additional_blocks_must_be_object() {
        let source = Source::File {
            name: Some("test".into()),
            path: PathBuf::from("ignored.yaml"),
            tag_prefix: None,
            additional_blocks: Some(json!([1, 2, 3])),
        };

        let base = r#"{"openapi":"3.0.3","info":{"title":"T","version":"1"},"paths":{}}"#;
        let err = parse_and_prepare(base, &source).unwrap_err().to_string();
        assert!(err.contains("additional_blocks"));
    }
}
