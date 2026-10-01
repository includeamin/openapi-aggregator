//! Browser bindings for openapi-aggregator. Fetching happens in JS;
//! this crate only parses the config and merges already-loaded spec text.

use std::collections::BTreeMap;

use openapi_aggregator_core::{
    merge_specs_with_report, prepare_source, Config, Error, OutputFormat, Source,
};
use serde::Serialize;
use wasm_bindgen::prelude::*;

#[derive(Debug, Serialize, PartialEq)]
pub struct SourceSummary {
    pub name: String,
    pub kind: &'static str,
    pub path: Option<String>,
    pub url: Option<String>,
    pub headers: BTreeMap<String, String>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct ConfigSummary {
    pub sources: Vec<SourceSummary>,
    pub format: &'static str,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct AggregateOutput {
    pub text: String,
    pub warnings: Vec<String>,
}

fn format_name(format: &OutputFormat) -> &'static str {
    match format {
        OutputFormat::Yaml => "yaml",
        OutputFormat::Json => "json",
    }
}

pub fn summarize_config(yaml: &str) -> Result<ConfigSummary, String> {
    let config = Config::from_yaml(yaml).map_err(|e| e.to_string())?;
    let sources = config
        .sources
        .iter()
        .map(|source| match source {
            Source::File { path, .. } => SourceSummary {
                name: source.display_name(),
                kind: "file",
                path: Some(path.to_string_lossy().into_owned()),
                url: None,
                headers: BTreeMap::new(),
            },
            Source::Http { url, headers, .. } => SourceSummary {
                name: source.display_name(),
                kind: "url",
                path: None,
                url: Some(url.clone()),
                headers: headers
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            },
        })
        .collect();
    Ok(ConfigSummary {
        sources,
        format: format_name(&config.output.format),
    })
}

pub fn aggregate_contents(yaml: &str, contents: Vec<String>) -> Result<AggregateOutput, String> {
    let config = Config::from_yaml(yaml).map_err(|e| e.to_string())?;
    if config.sources.is_empty() {
        return Err(Error::NoSources.to_string());
    }
    if contents.len() != config.sources.len() {
        return Err(format!(
            "expected {} source contents, got {}",
            config.sources.len(),
            contents.len()
        ));
    }

    let specs = config
        .sources
        .iter()
        .zip(&contents)
        .map(|(source, content)| {
            let (name, spec) = prepare_source(content, source).map_err(|e| match e {
                Error::Parse(_) => format!("source '{}': {e}", source.display_name()),
                other => other.to_string(),
            })?;
            Ok((name, source.tag_prefix(), spec))
        })
        .collect::<Result<Vec<_>, String>>()?;

    let report = merge_specs_with_report(specs, &config.merge).map_err(|e| e.to_string())?;
    let text = match config.output.format {
        OutputFormat::Yaml => serde_yaml::to_string(&report.spec).map_err(|e| e.to_string())?,
        OutputFormat::Json => {
            serde_json::to_string_pretty(&report.spec).map_err(|e| e.to_string())? + "\n"
        }
    };
    Ok(AggregateOutput {
        text,
        warnings: report.warnings,
    })
}

fn to_js<T: Serialize>(value: &T) -> Result<JsValue, JsError> {
    value
        .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .map_err(|e| JsError::new(&e.to_string()))
}

/// Parse and validate a config; returns a `ConfigSummary` or throws.
#[wasm_bindgen(js_name = parseConfig)]
pub fn parse_config(yaml: &str) -> Result<JsValue, JsError> {
    to_js(&summarize_config(yaml).map_err(|e| JsError::new(&e))?)
}

/// Merge already-loaded spec texts (`contents[i]` belongs to `sources[i]`);
/// returns an `AggregateOutput` or throws.
#[wasm_bindgen]
pub fn aggregate(yaml: &str, contents: Vec<String>) -> Result<JsValue, JsError> {
    to_js(&aggregate_contents(yaml, contents).map_err(|e| JsError::new(&e))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PETS: &str = "openapi: 3.0.3\ninfo: {title: Pets, version: '1'}\npaths:\n  /pets:\n    get: {summary: list}\n";
    const USERS: &str = "openapi: 3.0.3\ninfo: {title: Users, version: '1'}\npaths:\n  /users:\n    get: {summary: list}\n";

    #[test]
    fn summarizes_sources_and_format() {
        let summary = summarize_config(
            "sources:\n  - path: ./specs/pets.yaml\n  - name: billing\n    url: https://api.test/openapi.json\n    headers: {Authorization: 'Bearer ${TOKEN}'}\noutput: {format: json}\n",
        )
        .unwrap();

        assert_eq!(summary.format, "json");
        assert_eq!(
            summary.sources[0],
            SourceSummary {
                name: "pets".into(),
                kind: "file",
                path: Some("./specs/pets.yaml".into()),
                url: None,
                headers: BTreeMap::new(),
            }
        );
        assert_eq!(summary.sources[1].kind, "url");
        assert_eq!(
            summary.sources[1].headers["Authorization"],
            "Bearer ${TOKEN}"
        );
    }

    #[test]
    fn config_errors_are_reported() {
        let err = summarize_config("sources: [{name: x}]").unwrap_err();
        assert!(err.contains("either 'path' or 'url'"), "{err}");
    }

    #[test]
    fn aggregates_contents_in_source_order() {
        let out = aggregate_contents(
            "sources:\n  - path: pets.yaml\n  - path: users.yaml\n",
            vec![PETS.into(), USERS.into()],
        )
        .unwrap();
        assert!(out.text.starts_with("openapi: 3.0.3"), "{}", out.text);
        assert!(out.text.contains("/pets:") && out.text.contains("/users:"));
        assert!(out.warnings.is_empty());
    }

    #[test]
    fn json_output_format_is_respected() {
        let out = aggregate_contents(
            "sources:\n  - path: pets.yaml\noutput: {format: json}\n",
            vec![PETS.into()],
        )
        .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&out.text).unwrap();
        assert_eq!(parsed["info"]["title"], "Pets");
    }

    #[test]
    fn unparseable_content_names_the_source() {
        let err = aggregate_contents("sources:\n  - path: pets.yaml\n", vec!["{: nope".into()])
            .unwrap_err();
        assert!(err.contains("source 'pets'"), "{err}");
    }

    #[test]
    fn content_count_must_match_sources() {
        let err = aggregate_contents("sources:\n  - path: pets.yaml\n", vec![]).unwrap_err();
        assert!(err.contains("expected 1"), "{err}");
    }
}
