use serde_json::Value;

use crate::config::Source;
use crate::error::Error;

/// Load an OpenAPI spec from a [`Source`], returning `(name, parsed_value)`.
pub async fn load_source(source: &Source) -> Result<(String, Value), Error> {
    match source {
        Source::File { path, .. } => {
            let content = std::fs::read_to_string(path).map_err(|e| Error::FileRead {
                path: path.display().to_string(),
                source: e,
            })?;
            let value = parse_content(&content)?;
            validate_openapi(&value, &source.display_name())?;
            Ok((source.display_name(), value))
        }
        Source::Http { url, headers, .. } => {
            let client = reqwest::Client::new();
            let mut request = client.get(url);
            for (key, value) in headers {
                request = request.header(key, value);
            }
            let response = request.send().await.map_err(|e| Error::HttpRequest {
                url: url.clone(),
                source: e,
            })?;
            let content = response.text().await.map_err(|e| Error::HttpRequest {
                url: url.clone(),
                source: e,
            })?;
            let value = parse_content(&content)?;
            validate_openapi(&value, &source.display_name())?;
            Ok((source.display_name(), value))
        }
    }
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
}
