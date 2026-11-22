//! Browser bindings for openapi-aggregator. Fetching happens in JS;
//! this crate only parses the config and merges already-loaded spec text.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use openapi_aggregator_core::{
    bundle_source, check_document_cap, expand_sources, filter_source, is_glob, merge_sources,
    missing_documents, parse_content, prepare_source_with_notes, validate, Config, Error,
    OutputFormat, PreparedSource, Source, SourceSpec,
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
    /// Validation findings for the merged spec.
    pub findings: Vec<FindingOut>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct FindingOut {
    pub severity: &'static str,
    pub code: &'static str,
    pub message: String,
    pub location: String,
}

fn format_name(format: &OutputFormat) -> &'static str {
    match format {
        OutputFormat::Yaml => "yaml",
        OutputFormat::Json => "json",
    }
}

/// Parse the config and expand glob sources against the workspace file names.
fn load_config(yaml: &str, files: &[String]) -> Result<Config, String> {
    let mut config = Config::from_yaml(yaml).map_err(|e| e.to_string())?;
    let candidates: Vec<String> = files.iter().map(|f| workspace_key(f)).collect();
    for source in &mut config.sources {
        if let Source::File { path, .. } = source {
            let text = path.to_string_lossy().into_owned();
            if is_glob(&text) {
                *path = PathBuf::from(workspace_key(&text));
            }
        }
    }
    expand_sources(config, &candidates).map_err(|e| {
        let message = e.to_string();
        if message.contains("matched no files") {
            format!("{message}; add files with matching names to the workspace")
        } else {
            message
        }
    })
}

pub fn summarize_config(yaml: &str) -> Result<ConfigSummary, String> {
    summarize_config_with_files(yaml, &[])
}

/// Like [`summarize_config`], expanding glob sources against `files` (workspace file names).
pub fn summarize_config_with_files(yaml: &str, files: &[String]) -> Result<ConfigSummary, String> {
    let config = load_config(yaml, files)?;
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

/// A document a source references that has not been supplied yet.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MissingDocumentOut {
    pub location: String,
    pub referenced_from: String,
}

/// For each source, the external documents it still needs.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct PendingRefs {
    pub sources: Vec<Vec<MissingDocumentOut>>,
}

/// Strip leading `./` so a config `path:` matches a workspace file name.
fn workspace_key(path: &str) -> String {
    let mut key = path;
    while let Some(rest) = key.strip_prefix("./") {
        key = rest;
    }
    key.to_string()
}

/// The location a source's relative `$ref`s are resolved against.
fn base_of(source: &Source) -> String {
    match source {
        Source::File { path, .. } => workspace_key(&path.to_string_lossy()),
        Source::Http { url, .. } => url.clone(),
    }
}

fn parse_docs(docs: HashMap<String, String>) -> Result<HashMap<String, serde_json::Value>, String> {
    docs.into_iter()
        .map(|(location, text)| {
            parse_content(&text)
                .map(|value| (location.clone(), value))
                .map_err(|e| format!("document '{location}': {e}"))
        })
        .collect()
}

fn prepare_all(config: &Config, contents: &[String]) -> Result<Vec<PreparedSource>, String> {
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
    config
        .sources
        .iter()
        .zip(contents)
        .map(|(source, content)| {
            prepare_source_with_notes(content, source).map_err(|e| match e {
                Error::Parse(_) => format!("source '{}': {e}", source.display_name()),
                other => other.to_string(),
            })
        })
        .collect()
}

/// For each source, the external documents (by location) still missing from `docs`.
pub fn pending_refs(
    yaml: &str,
    contents: Vec<String>,
    docs: HashMap<String, String>,
) -> Result<PendingRefs, String> {
    pending_refs_with_files(yaml, contents, docs, &[])
}

/// Like [`pending_refs`], expanding glob sources against `files`.
pub fn pending_refs_with_files(
    yaml: &str,
    contents: Vec<String>,
    docs: HashMap<String, String>,
    files: &[String],
) -> Result<PendingRefs, String> {
    let config = load_config(yaml, files)?;
    let prepared = prepare_all(&config, &contents)?;
    let docs = parse_docs(docs)?;
    let sources = config
        .sources
        .iter()
        .zip(&prepared)
        .map(|(source, p)| {
            missing_documents(&p.spec, &base_of(source), &docs)
                .into_iter()
                .map(|m| MissingDocumentOut {
                    location: m.location,
                    referenced_from: m.referenced_from,
                })
                .collect()
        })
        .collect();
    Ok(PendingRefs { sources })
}

pub fn aggregate_contents(
    yaml: &str,
    contents: Vec<String>,
    docs: HashMap<String, String>,
) -> Result<AggregateOutput, String> {
    aggregate_contents_with_files(yaml, contents, docs, &[])
}

/// Like [`aggregate_contents`], expanding glob sources against `files`.
pub fn aggregate_contents_with_files(
    yaml: &str,
    contents: Vec<String>,
    docs: HashMap<String, String>,
    files: &[String],
) -> Result<AggregateOutput, String> {
    let config = load_config(yaml, files)?;
    let prepared = prepare_all(&config, &contents)?;
    let docs = parse_docs(docs)?;

    let mut notes = Vec::new();
    let mut specs = Vec::new();
    for (source, prepared) in config.sources.iter().zip(prepared) {
        check_document_cap(docs.len(), &prepared.name).map_err(|e| e.to_string())?;
        let prepared =
            bundle_source(prepared, &base_of(source), &docs).map_err(|e| e.to_string())?;
        let prepared = filter_source(prepared, config.merge.filter.as_ref(), source.filter());
        notes.extend(prepared.notes);
        specs.push(SourceSpec {
            name: prepared.name,
            tag_prefix: source.tag_prefix(),
            path_prefix: source.path_prefix().map(String::from),
            spec: prepared.spec,
        });
    }

    let mut report = merge_sources(specs, &config.merge).map_err(|e| e.to_string())?;
    notes.append(&mut report.warnings);
    report.warnings = notes;
    let text = match config.output.format {
        OutputFormat::Yaml => serde_yaml::to_string(&report.spec).map_err(|e| e.to_string())?,
        OutputFormat::Json => {
            serde_json::to_string_pretty(&report.spec).map_err(|e| e.to_string())? + "\n"
        }
    };
    let findings = validate(&report.spec)
        .into_iter()
        .map(|f| FindingOut {
            severity: f.severity.as_str(),
            code: f.code,
            message: f.message,
            location: f.location,
        })
        .collect();
    Ok(AggregateOutput {
        text,
        warnings: report.warnings,
        findings,
    })
}

fn to_js<T: Serialize>(value: &T) -> Result<JsValue, JsError> {
    value
        .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .map_err(|e| JsError::new(&e.to_string()))
}

/// Parse and validate a config; returns a `ConfigSummary` or throws.
#[wasm_bindgen(js_name = parseConfig)]
pub fn parse_config(yaml: &str, files: Option<Vec<String>>) -> Result<JsValue, JsError> {
    to_js(
        &summarize_config_with_files(yaml, &files.unwrap_or_default())
            .map_err(|e| JsError::new(&e))?,
    )
}

fn docs_from_js(docs: JsValue) -> Result<HashMap<String, String>, JsError> {
    if docs.is_undefined() || docs.is_null() {
        return Ok(HashMap::new());
    }
    serde_wasm_bindgen::from_value(docs)
        .map_err(|e| JsError::new(&format!("invalid external documents: {e}")))
}

/// Which external documents each source still needs (`docs` maps location -> text);
/// returns `PendingRefs` or throws.
#[wasm_bindgen(js_name = pendingRefs)]
pub fn pending_refs_js(
    yaml: &str,
    contents: Vec<String>,
    docs: JsValue,
    files: Option<Vec<String>>,
) -> Result<JsValue, JsError> {
    to_js(
        &pending_refs_with_files(
            yaml,
            contents,
            docs_from_js(docs)?,
            &files.unwrap_or_default(),
        )
        .map_err(|e| JsError::new(&e))?,
    )
}

/// Merge already-loaded spec texts (`contents[i]` belongs to `sources[i]`), bundling
/// external refs from `docs` (location -> text); returns an `AggregateOutput` or throws.
#[wasm_bindgen]
pub fn aggregate(
    yaml: &str,
    contents: Vec<String>,
    docs: JsValue,
    files: Option<Vec<String>>,
) -> Result<JsValue, JsError> {
    to_js(
        &aggregate_contents_with_files(
            yaml,
            contents,
            docs_from_js(docs)?,
            &files.unwrap_or_default(),
        )
        .map_err(|e| JsError::new(&e))?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const PETS: &str = "openapi: 3.0.3\ninfo: {title: Pets, version: '1'}\npaths:\n  /pets:\n    get: {summary: list, responses: {'200': {description: ok}}}\n";
    const USERS: &str = "openapi: 3.0.3\ninfo: {title: Users, version: '1'}\npaths:\n  /users:\n    get: {summary: list, responses: {'200': {description: ok}}}\n";

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
            Default::default(),
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
            Default::default(),
        )
        .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&out.text).unwrap();
        assert_eq!(parsed["info"]["title"], "Pets");
    }

    #[test]
    fn unparseable_content_names_the_source() {
        let err = aggregate_contents(
            "sources:\n  - path: pets.yaml\n",
            vec!["{: nope".into()],
            Default::default(),
        )
        .unwrap_err();
        assert!(err.contains("source 'pets'"), "{err}");
    }

    #[test]
    fn content_count_must_match_sources() {
        let err = aggregate_contents(
            "sources:\n  - path: pets.yaml\n",
            vec![],
            Default::default(),
        )
        .unwrap_err();
        assert!(err.contains("expected 1"), "{err}");
    }

    #[test]
    fn swagger2_sources_are_converted_with_a_warning() {
        let legacy = "swagger: '2.0'\ninfo: {title: Legacy, version: '1'}\npaths:\n  /legacy:\n    get:\n      responses:\n        '200': {description: ok}\n";
        let out = aggregate_contents(
            "sources:\n  - name: legacy\n    path: legacy.yaml\n  - path: pets.yaml\n",
            vec![legacy.into(), PETS.into()],
            Default::default(),
        )
        .unwrap();
        assert!(out.text.contains("/legacy:") && out.text.contains("/pets:"));
        assert!(
            out.warnings
                .iter()
                .any(|w| w.contains("source 'legacy': converted from Swagger 2.0")),
            "{:?}",
            out.warnings
        );
    }

    const ROOT_WITH_REF: &str = "openapi: 3.0.3\ninfo: {title: Root, version: '1'}\npaths:\n  /p:\n    get:\n      responses:\n        '200':\n          description: ok\n          content:\n            application/json:\n              schema:\n                $ref: './a.yaml#/Pet'\n";

    fn docs(entries: &[(&str, &str)]) -> HashMap<String, String> {
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn pending_refs_lists_missing_documents_per_source() {
        let yaml = "sources:\n  - path: ./root.yaml\n  - path: pets.yaml\n";
        let pending = pending_refs(
            yaml,
            vec![ROOT_WITH_REF.into(), PETS.into()],
            HashMap::new(),
        )
        .unwrap();
        assert_eq!(
            pending.sources,
            vec![
                vec![MissingDocumentOut {
                    location: "a.yaml".into(),
                    referenced_from: "root.yaml".into()
                }],
                vec![]
            ]
        );

        let a = "Pet:\n  properties:\n    t:\n      $ref: 'b.yaml#/T'\n";
        let pending = pending_refs(
            yaml,
            vec![ROOT_WITH_REF.into(), PETS.into()],
            docs(&[("a.yaml", a)]),
        )
        .unwrap();
        assert_eq!(
            pending.sources[0],
            vec![MissingDocumentOut {
                location: "b.yaml".into(),
                referenced_from: "a.yaml".into()
            }]
        );

        let json = serde_json::to_value(&pending).unwrap();
        assert_eq!(json["sources"][0][0]["referencedFrom"], "a.yaml");
    }

    #[test]
    fn pending_refs_names_unparseable_documents() {
        let err = pending_refs(
            "sources:\n  - path: root.yaml\n",
            vec![ROOT_WITH_REF.into()],
            docs(&[("a.yaml", "{: nope")]),
        )
        .unwrap_err();
        assert!(err.contains("a.yaml"), "{err}");
    }

    #[test]
    fn aggregate_bundles_external_documents() {
        let out = aggregate_contents(
            "sources:\n  - path: root.yaml\n",
            vec![ROOT_WITH_REF.into()],
            docs(&[("a.yaml", "Pet:\n  type: object\n")]),
        )
        .unwrap();
        assert!(
            out.text.contains("components:") && out.text.contains("Pet:"),
            "{}",
            out.text
        );
        assert!(!out.text.contains("a.yaml#"), "{}", out.text);
    }

    #[test]
    fn aggregate_without_the_document_names_source_and_location() {
        let err = aggregate_contents(
            "sources:\n  - path: root.yaml\n",
            vec![ROOT_WITH_REF.into()],
            HashMap::new(),
        )
        .unwrap_err();
        assert!(err.contains("root") && err.contains("a.yaml"), "{err}");
    }

    #[test]
    fn findings_are_reported_for_the_merged_spec() {
        let dangling = "openapi: 3.0.3\ninfo: {title: D, version: '1'}\npaths:\n  /d:\n    get:\n      responses:\n        '200':\n          description: ok\n          content:\n            application/json:\n              schema:\n                $ref: '#/components/schemas/Ghost'\n";
        let out = aggregate_contents(
            "sources:\n  - path: d.yaml\n",
            vec![dangling.into()],
            HashMap::new(),
        )
        .unwrap();
        assert_eq!(out.findings.len(), 1);
        assert_eq!(
            (out.findings[0].severity, out.findings[0].code),
            ("error", "dangling-ref")
        );
        assert!(
            out.findings[0].location.ends_with("/schema"),
            "{}",
            out.findings[0].location
        );

        let clean = aggregate_contents(
            "sources:\n  - path: pets.yaml\n",
            vec![PETS.into()],
            HashMap::new(),
        )
        .unwrap();
        assert!(clean.findings.is_empty(), "{:?}", clean.findings);
    }

    #[test]
    fn filters_hide_operations_in_the_output() {
        let spec = "openapi: 3.0.3\ninfo: {title: S, version: '1'}\npaths:\n  /pub:\n    get:\n      responses: {'200': {description: ok}}\n  /internal/x:\n    get:\n      responses: {'200': {description: ok}}\n";
        let config = "sources:\n  - path: s.yaml\nmerge:\n  filter:\n    exclude:\n      paths: ['/internal/**']\n";
        let out = aggregate_contents(config, vec![spec.into()], HashMap::new()).unwrap();
        assert!(
            out.text.contains("/pub:") && !out.text.contains("/internal"),
            "{}",
            out.text
        );
    }

    fn names(files: &[&str]) -> Vec<String> {
        files.iter().map(|f| f.to_string()).collect()
    }

    #[test]
    fn glob_sources_expand_from_workspace_file_names() {
        let summary = summarize_config_with_files(
            "sources:\n  - path: ./specs/*.yaml\n",
            &names(&[
                "specs/b.yaml",
                "./specs/a.yaml",
                "other.yaml",
                "specs/nested/x.yaml",
            ]),
        )
        .unwrap();
        let listed: Vec<(&str, Option<&str>)> = summary
            .sources
            .iter()
            .map(|s| (s.name.as_str(), s.path.as_deref()))
            .collect();
        assert_eq!(
            listed,
            vec![("a", Some("specs/a.yaml")), ("b", Some("specs/b.yaml"))]
        );
        assert!(summary.sources.iter().all(|s| s.kind == "file"));
    }

    #[test]
    fn a_glob_without_matches_is_a_config_error_with_a_hint() {
        let err = summarize_config_with_files(
            "sources:\n  - path: specs/*.yaml\n",
            &names(&["other.yaml"]),
        )
        .unwrap_err();
        assert!(err.contains("matched no files"), "{err}");
        assert!(err.contains("workspace"), "{err}");
        // without any file list a glob cannot match
        assert!(summarize_config("sources:\n  - path: specs/*.yaml\n").is_err());
    }

    #[test]
    fn aggregate_and_pending_refs_use_the_expanded_sources() {
        let config = "sources:\n  - path: ./specs/*.yaml\n";
        let files = names(&["specs/pets.yaml", "specs/users.yaml"]);
        let out = aggregate_contents_with_files(
            config,
            vec![PETS.into(), USERS.into()],
            HashMap::new(),
            &files,
        )
        .unwrap();
        assert!(
            out.text.contains("/pets:") && out.text.contains("/users:"),
            "{}",
            out.text
        );

        let pending = pending_refs_with_files(
            config,
            vec![PETS.into(), USERS.into()],
            HashMap::new(),
            &files,
        )
        .unwrap();
        assert_eq!(pending.sources.len(), 2);

        // one content per expanded source is required
        let err = aggregate_contents_with_files(config, vec![PETS.into()], HashMap::new(), &files)
            .unwrap_err();
        assert!(err.contains("expected 2 source contents"), "{err}");
    }

    #[test]
    fn path_prefixes_apply_in_the_output() {
        let config = "sources:\n  - name: a\n    path: a.yaml\n    path_prefix: /v2\n  - name: b\n    path: b.yaml\n";
        let out =
            aggregate_contents(config, vec![PETS.into(), PETS.into()], HashMap::new()).unwrap();
        assert!(
            out.text.contains("/v2/pets:") && out.text.contains("\n  /pets:"),
            "{}",
            out.text
        );
    }
}
