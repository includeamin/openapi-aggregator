use std::collections::HashMap;

use serde_json::Value;

use crate::bundle::bundle;
use crate::config::Source;
use crate::error::Error;
use crate::merge::deep_merge;
use crate::swagger2;

/// A source after parsing, Swagger 2.0 conversion, `additional_blocks` and validation.
#[derive(Debug, Clone)]
pub struct PreparedSource {
    pub name: String,
    pub spec: Value,
    /// Warnings about anything approximated while converting from Swagger 2.0,
    /// each prefixed with `source '<name>': `. Empty for OpenAPI 3.x sources.
    pub notes: Vec<String>,
}

/// Parse spec text, convert Swagger 2.0 to OpenAPI 3.0, apply the source's
/// `additional_blocks` and validate it. Returns `(display_name, spec)`.
pub fn prepare_source(content: &str, source: &Source) -> Result<(String, Value), Error> {
    prepare_source_with_notes(content, source).map(|p| (p.name, p.spec))
}

/// Like [`prepare_source`], but also returns the conversion notes.
pub fn prepare_source_with_notes(content: &str, source: &Source) -> Result<PreparedSource, Error> {
    let name = source.display_name();
    let (spec, notes) = parse_and_prepare(content, source)?;
    validate_openapi(&spec, &name)?;
    let notes = notes
        .into_iter()
        .map(|n| format!("source '{name}': {n}"))
        .collect();
    Ok(PreparedSource { name, spec, notes })
}

/// Bundle the external `$ref`s of a prepared source (see [`crate::bundle`]). `base` is the
/// source's location; `docs` holds every document it references, already parsed.
/// Notes are appended to the source's notes, prefixed with `source '<name>': `.
pub fn bundle_source(
    mut prepared: PreparedSource,
    base: &str,
    docs: &HashMap<String, Value>,
) -> Result<PreparedSource, Error> {
    let name = prepared.name.clone();
    let (spec, notes) = bundle(prepared.spec, base, docs).map_err(|e| Error::InvalidSpec {
        name: name.clone(),
        reason: e.to_string(),
    })?;
    prepared.spec = spec;
    prepared
        .notes
        .extend(notes.into_iter().map(|n| format!("source '{name}': {n}")));
    Ok(prepared)
}

fn parse_and_prepare(content: &str, source: &Source) -> Result<(Value, Vec<String>), Error> {
    let mut value = parse_content(content)?;
    let mut notes = Vec::new();
    if is_swagger2(&value) {
        (value, notes) = swagger2::convert(value);
    }
    if let Some(blocks) = source_additional_blocks(source) {
        if !blocks.is_object() {
            return Err(Error::InvalidSpec {
                name: source.display_name(),
                reason: "'additional_blocks' must be a mapping/object".into(),
            });
        }
        deep_merge(&mut value, blocks);
    }
    Ok((value, notes))
}

/// A Swagger 2.0 document: has `swagger: 2.x` (string or bare YAML number) and no `openapi`.
fn is_swagger2(value: &Value) -> bool {
    if value.get("openapi").is_some() {
        return false;
    }
    match value.get("swagger") {
        Some(Value::String(v)) => v == "2" || v.starts_with("2."),
        Some(Value::Number(n)) => n.as_f64() == Some(2.0),
        _ => false,
    }
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
            reason: "Swagger 1.x is not supported (only Swagger 2.0 and OpenAPI 3.x are)".into(),
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
            filter: None,
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

        let merged = parse_and_prepare(base, &source).unwrap().0;
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
            filter: None,
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
    fn prepare_source_rejects_swagger_1_x_with_source_name() {
        let source = file_source("legacy", None);
        let err = prepare_source("swagger: '1.2'\ninfo: {title: T, version: '1'}\n", &source)
            .unwrap_err()
            .to_string();
        assert!(err.contains("legacy"), "{err}");
        assert!(err.contains("Swagger 1.x"), "{err}");
    }

    fn file_source(name: &str, blocks: Option<Value>) -> Source {
        Source::File {
            name: Some(name.into()),
            path: PathBuf::from("ignored.yaml"),
            tag_prefix: None,
            additional_blocks: blocks,
            filter: None,
        }
    }

    const SWAGGER2: &str =
        "swagger: '2.0'\ninfo: {title: T, version: '1'}\nhost: api.test\npaths: {}\n";

    #[test]
    fn prepare_converts_swagger2_and_prefixes_notes() {
        let prepared = prepare_source_with_notes(SWAGGER2, &file_source("legacy", None)).unwrap();
        assert_eq!(prepared.name, "legacy");
        assert_eq!(prepared.spec["openapi"], "3.0.3");
        assert!(!prepared.notes.is_empty());
        assert!(prepared
            .notes
            .iter()
            .all(|n| n.starts_with("source 'legacy': ")));
        assert!(prepared.notes[0].contains("converted from Swagger 2.0"));

        let (name, spec) = prepare_source(SWAGGER2, &file_source("legacy", None)).unwrap();
        assert_eq!(
            (name.as_str(), &spec["openapi"]),
            ("legacy", &json!("3.0.3"))
        );
    }

    #[test]
    fn plain_openapi_has_no_notes() {
        let prepared = prepare_source_with_notes(
            r#"{"openapi":"3.0.3","info":{"title":"T","version":"1"},"paths":{}}"#,
            &file_source("x", None),
        )
        .unwrap();
        assert!(prepared.notes.is_empty());
    }

    #[test]
    fn additional_blocks_apply_after_conversion() {
        let blocks =
            json!({"info": {"x-a": 1}, "components": {"schemas": {"Extra": {"type": "object"}}}});
        let prepared =
            prepare_source_with_notes(SWAGGER2, &file_source("legacy", Some(blocks))).unwrap();
        assert_eq!(prepared.spec["info"]["x-a"], 1);
        assert_eq!(
            prepared.spec["components"]["schemas"]["Extra"]["type"],
            "object"
        );
        assert_eq!(prepared.spec["servers"][0]["url"], "https://api.test");
    }

    #[test]
    fn additional_blocks_must_be_object() {
        let source = Source::File {
            name: Some("test".into()),
            path: PathBuf::from("ignored.yaml"),
            tag_prefix: None,
            additional_blocks: Some(json!([1, 2, 3])),
            filter: None,
        };

        let base = r#"{"openapi":"3.0.3","info":{"title":"T","version":"1"},"paths":{}}"#;
        let err = parse_and_prepare(base, &source).unwrap_err().to_string();
        assert!(err.contains("additional_blocks"));
    }

    fn docs_of(entries: Vec<(&str, Value)>) -> std::collections::HashMap<String, Value> {
        entries
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect()
    }

    #[test]
    fn bundle_source_bundles_and_prefixes_notes() {
        let source = file_source("pets", None);
        let prepared = prepare_source_with_notes(
            r#"{"openapi":"3.0.3","info":{"title":"T","version":"1"},"paths":{},
                "components":{"schemas":{"Pet":{"type":"string"}}},
                "x":{"schema":{"$ref":"m/pet.yaml#/Pet"}}}"#,
            &source,
        )
        .unwrap();
        let docs = docs_of(vec![("m/pet.yaml", json!({"Pet": {"type": "object"}}))]);
        let bundled = bundle_source(prepared, "pets.yaml", &docs).unwrap();
        assert_eq!(
            bundled.spec["x"]["schema"]["$ref"],
            "#/components/schemas/pet_Pet"
        );
        assert!(bundled
            .notes
            .iter()
            .all(|n| n.starts_with("source 'pets': ")));
        assert!(bundled.notes[0].contains("hoisted 'm/pet.yaml#/Pet'"));
    }

    #[test]
    fn bundle_source_errors_name_the_source() {
        let source = file_source("pets", None);
        let prepared = prepare_source_with_notes(
            r#"{"openapi":"3.0.3","info":{"title":"T","version":"1"},"paths":{},"x":{"$ref":"d.yaml#/Nope"}}"#,
            &source,
        )
        .unwrap();
        let docs = docs_of(vec![("d.yaml", json!({}))]);
        let err = bundle_source(prepared, "pets.yaml", &docs)
            .unwrap_err()
            .to_string();
        assert!(err.contains("pets") && err.contains("not found"), "{err}");
    }

    #[test]
    fn swagger2_source_converts_then_bundles_external_definitions() {
        let source = file_source("legacy", None);
        let prepared = prepare_source_with_notes(
            "swagger: '2.0'\ninfo: {title: T, version: '1'}\npaths:\n  /p:\n    get:\n      responses:\n        '200':\n          description: ok\n          schema:\n            $ref: defs.yaml#/definitions/Pet\n",
            &source,
        )
        .unwrap();
        let docs = docs_of(vec![(
            "defs.yaml",
            json!({"definitions": {"Pet": {"type": "object"}}}),
        )]);
        let bundled = bundle_source(prepared, "legacy.yaml", &docs).unwrap();
        assert_eq!(
            bundled.spec["paths"]["/p"]["get"]["responses"]["200"]["content"]["application/json"]
                ["schema"]["$ref"],
            "#/components/schemas/Pet"
        );
        assert_eq!(
            bundled.spec["components"]["schemas"]["Pet"]["type"],
            "object"
        );
    }

    #[test]
    fn document_cap_allows_100_and_rejects_101() {
        assert!(crate::bundle::check_document_cap(100, "x").is_ok());
        let err = crate::bundle::check_document_cap(101, "x")
            .unwrap_err()
            .to_string();
        assert!(err.contains("limit 100") && err.contains("x"), "{err}");
    }
}
