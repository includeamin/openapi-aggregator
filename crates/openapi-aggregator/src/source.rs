use serde_json::Value;
use std::time::Duration;

use crate::config::Source;
use crate::error::Error;
use crate::merge::deep_merge;

/// Load an OpenAPI spec from a [`Source`], returning `(name, parsed_value)`.
pub async fn load_source(source: &Source) -> Result<(String, Value), Error> {
    load_source_with_client(source, &http_client()?).await
}

pub(crate) fn http_client() -> Result<reqwest::Client, Error> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| Error::Config(format!("failed to create HTTP client: {e}")))
}

pub(crate) async fn load_source_with_client(
    source: &Source,
    client: &reqwest::Client,
) -> Result<(String, Value), Error> {
    let content = match source {
        Source::File { path, .. } => {
            tokio::fs::read_to_string(path)
                .await
                .map_err(|e| Error::FileRead {
                    path: path.display().to_string(),
                    source: e,
                })?
        }
        Source::Http { url, headers, .. } => {
            let url = expand_env(url)?;
            let http_err = |e| Error::HttpRequest {
                url: url.clone(),
                source: e,
            };
            let mut request = client.get(&url);
            for (key, value) in headers {
                request = request.header(key, expand_env(value)?);
            }
            request
                .send()
                .await
                .map_err(http_err)?
                .error_for_status()
                .map_err(http_err)?
                .text()
                .await
                .map_err(http_err)?
        }
    };

    let value = parse_and_prepare(&content, source)?;
    validate_openapi(&value, &source.display_name())?;
    Ok((source.display_name(), value))
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

/// Replace `${NAME}` placeholders with environment variables.
fn expand_env(text: &str) -> Result<String, Error> {
    expand_env_with(text, |name| std::env::var(name).ok())
}

fn expand_env_with(text: &str, lookup: impl Fn(&str) -> Option<String>) -> Result<String, Error> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("${") {
        let Some(len) = rest[start + 2..].find('}') else {
            break;
        };
        let name = &rest[start + 2..start + 2 + len];
        let value = lookup(name)
            .ok_or_else(|| Error::Config(format!("environment variable '{name}' is not set")))?;
        out.push_str(&rest[..start]);
        out.push_str(&value);
        rest = &rest[start + 2 + len + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Try JSON first, fall back to YAML.
fn parse_content(content: &str) -> Result<Value, Error> {
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

#[cfg(test)]
mod env_tests {
    use super::*;

    fn lookup(name: &str) -> Option<String> {
        match name {
            "TOKEN" => Some("secret".into()),
            "HOST" => Some("api.test".into()),
            _ => None,
        }
    }

    #[test]
    fn expands_env_placeholders() {
        assert_eq!(
            expand_env_with("Bearer ${TOKEN} @ ${HOST}", lookup).unwrap(),
            "Bearer secret @ api.test"
        );
    }

    #[test]
    fn leaves_text_without_placeholders_alone() {
        assert_eq!(
            expand_env_with("$5 {x} $TOKEN", lookup).unwrap(),
            "$5 {x} $TOKEN"
        );
    }

    #[test]
    fn missing_env_var_is_an_error() {
        let err = expand_env_with("Bearer ${NOPE}", lookup)
            .unwrap_err()
            .to_string();
        assert!(err.contains("NOPE"), "{err}");
    }
}
