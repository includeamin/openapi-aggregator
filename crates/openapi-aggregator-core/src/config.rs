use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;

/// Top-level configuration for the aggregator.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub sources: Vec<Source>,
    #[serde(default)]
    pub output: OutputConfig,
    #[serde(default)]
    pub merge: MergeConfig,
}

impl Config {
    /// Parse a YAML (or JSON) config document.
    pub fn from_yaml(text: &str) -> Result<Self, crate::error::Error> {
        let config: Self = serde_yaml::from_str(text).map_err(|e| {
            crate::error::Error::Config(format!("failed to parse config file: {e}"))
        })?;
        if let Some(filter) = &config.merge.filter {
            filter.validate("merge.filter").map_err(|e| {
                crate::error::Error::Config(format!("failed to parse config file: {e}"))
            })?;
        }
        Ok(config)
    }
}

/// A source of an OpenAPI specification.
///
/// Detected automatically: if `url` is present it is treated as an HTTP source,
/// if `path` is present it is a local file source. Exactly one must be set.
///
/// In HTTP sources, `${ENV_VAR}` placeholders in `url` and header values are
/// replaced with environment variables when the source is loaded.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged, try_from = "RawSource")]
pub enum Source {
    Http {
        name: Option<String>,
        url: String,
        #[serde(default)]
        headers: HashMap<String, String>,
        /// Custom tag prefix for this source (used when `tag_prefix` is `source_name`).
        /// If set, overrides the source name as the prefix.
        tag_prefix: Option<String>,
        /// Additional blocks deep-merged into this source spec before merge.
        /// Can be used for vendor extensions or any custom OpenAPI blocks.
        additional_blocks: Option<Value>,
        /// Operations to hide from (or keep of) this source, in addition to `merge.filter`.
        filter: Option<FilterConfig>,
    },
    File {
        name: Option<String>,
        path: PathBuf,
        /// Custom tag prefix for this source (used when `tag_prefix` is `source_name`).
        /// If set, overrides the source name as the prefix.
        tag_prefix: Option<String>,
        /// Additional blocks deep-merged into this source spec before merge.
        /// Can be used for vendor extensions or any custom OpenAPI blocks.
        additional_blocks: Option<Value>,
        /// Operations to hide from (or keep of) this source, in addition to `merge.filter`.
        filter: Option<FilterConfig>,
    },
}

/// Flat representation of a source, used to give precise errors
/// before deciding which [`Source`] variant it is.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSource {
    name: Option<String>,
    url: Option<String>,
    path: Option<PathBuf>,
    headers: Option<HashMap<String, String>>,
    tag_prefix: Option<String>,
    additional_blocks: Option<Value>,
    filter: Option<FilterConfig>,
}

impl TryFrom<RawSource> for Source {
    type Error = String;

    fn try_from(raw: RawSource) -> Result<Self, Self::Error> {
        let label = raw
            .name
            .as_deref()
            .map(|n| format!("source '{n}'"))
            .unwrap_or_else(|| "source".into());
        if let Some(filter) = &raw.filter {
            filter.validate(&format!("{label} filter"))?;
        }
        match (raw.url, raw.path) {
            (Some(url), None) => Ok(Source::Http {
                name: raw.name,
                url,
                headers: raw.headers.unwrap_or_default(),
                tag_prefix: raw.tag_prefix,
                additional_blocks: raw.additional_blocks,
                filter: raw.filter,
            }),
            (None, Some(path)) => {
                if raw.headers.is_some() {
                    return Err(format!(
                        "{label}: 'headers' is only supported for 'url' sources"
                    ));
                }
                Ok(Source::File {
                    name: raw.name,
                    path,
                    tag_prefix: raw.tag_prefix,
                    additional_blocks: raw.additional_blocks,
                    filter: raw.filter,
                })
            }
            (Some(_), Some(_)) => Err(format!(
                "{label} has both 'path' and 'url'; set exactly one"
            )),
            (None, None) => Err(format!("{label} needs either 'path' or 'url'")),
        }
    }
}

/// Extract the host from a URL, e.g. `https://user@api.test:8443/x` → `api.test`.
fn url_host(url: &str) -> Option<&str> {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = rest.split(['/', '?', '#']).next()?;
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = match host_port.strip_prefix('[') {
        Some(ipv6) => ipv6.split(']').next()?,
        None => host_port.split(':').next()?,
    };
    (!host.is_empty()).then_some(host)
}

impl Source {
    /// Return the user-provided name or derive one from the url host / filename.
    pub fn display_name(&self) -> String {
        match self {
            Source::Http { name, url, .. } => name.clone().unwrap_or_else(|| {
                url_host(url)
                    .map(str::to_string)
                    .unwrap_or_else(|| url.clone())
            }),
            Source::File { name, path, .. } => name.clone().unwrap_or_else(|| {
                path.file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "unknown".into())
            }),
        }
    }

    /// The per-source operation filter, if any.
    pub fn filter(&self) -> Option<&FilterConfig> {
        match self {
            Source::Http { filter, .. } | Source::File { filter, .. } => filter.as_ref(),
        }
    }

    /// Return the tag prefix for this source: custom `tag_prefix` field if set, otherwise the display name.
    pub fn tag_prefix(&self) -> String {
        match self {
            Source::Http { tag_prefix, .. } | Source::File { tag_prefix, .. } => {
                tag_prefix.clone().unwrap_or_else(|| self.display_name())
            }
        }
    }
}

/// Which operations a filter rule applies to. An operation matches when **any** criterion
/// matches. See [`FilterConfig`].
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FilterRules {
    /// Path globs (`*` within one segment, `**` across segments), e.g. `/internal/**`.
    #[serde(default)]
    pub paths: Vec<String>,
    /// Operations having any of these tags.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Operations with any of these `operationId`s.
    #[serde(default)]
    pub operation_ids: Vec<String>,
    /// Vendor extensions (`x-...`) that are truthy on the operation or its path item.
    #[serde(default)]
    pub extensions: Vec<String>,
}

impl FilterRules {
    fn validate(&self, context: &str) -> Result<(), String> {
        if self.paths.is_empty()
            && self.tags.is_empty()
            && self.operation_ids.is_empty()
            && self.extensions.is_empty()
        {
            return Err(format!(
                "{context} must list at least one rule (paths, tags, operation_ids or extensions)"
            ));
        }
        for path in &self.paths {
            if !path.starts_with('/') {
                return Err(format!(
                    "{context}.paths entry '{path}' must start with '/'"
                ));
            }
        }
        for extension in &self.extensions {
            if !extension.starts_with("x-") {
                return Err(format!(
                    "{context}.extensions entry '{extension}' must start with 'x-'"
                ));
            }
        }
        for (key, entries) in [("tags", &self.tags), ("operation_ids", &self.operation_ids)] {
            if entries.iter().any(|e| e.is_empty()) {
                return Err(format!("{context}.{key} entries must not be empty"));
            }
        }
        Ok(())
    }
}

/// Select which operations end up in the merged spec: `exclude` hides matching operations,
/// `include` (an allow-list) keeps only matching ones. Exclude wins.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FilterConfig {
    pub include: Option<FilterRules>,
    pub exclude: Option<FilterRules>,
}

impl FilterConfig {
    /// Check the rules are well-formed; `context` names the config location for errors.
    pub fn validate(&self, context: &str) -> Result<(), String> {
        if let Some(rules) = &self.include {
            rules.validate(&format!("{context}.include"))?;
        }
        if let Some(rules) = &self.exclude {
            rules.validate(&format!("{context}.exclude"))?;
        }
        Ok(())
    }
}

/// Output configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OutputConfig {
    #[serde(default)]
    pub format: OutputFormat,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            format: OutputFormat::Yaml,
        }
    }
}

/// Supported output formats.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    #[default]
    Yaml,
    Json,
}

/// Options that control how specs are merged.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MergeConfig {
    /// Strategy for handling duplicate paths or component names.
    #[serde(default)]
    pub conflict_strategy: ConflictStrategy,
    /// If `true`, every path from each source is prefixed with `/{source_name}`.
    #[serde(default)]
    pub prefix_paths: bool,
    /// Controls how tags from each source are prefixed.
    /// - `none` (default) – tags are merged as-is, deduplicated by name.
    /// - `source_name` – tags are prefixed with `{source_name}/{tag_name}`.
    #[serde(default)]
    pub tag_prefix: TagPrefixStrategy,
    /// Separator used between the prefix and the tag name. Defaults to `/`.
    #[serde(default = "default_tag_separator")]
    pub tag_separator: String,
    /// Override the `info` block in the merged output.
    pub info: Option<InfoOverride>,
    /// Explicit list of servers for the merged spec.
    /// If set, replaces any servers collected from sources.
    pub servers: Option<Vec<ServerEntry>>,
    /// Explicit list of tags for the merged spec.
    /// If set, replaces any tags collected from sources.
    /// (Operation-level tag references are NOT rewritten; these should match.)
    pub tags: Option<Vec<TagEntry>>,
    /// Operations to hide from (or keep of) every source.
    pub filter: Option<FilterConfig>,
}

impl Default for MergeConfig {
    fn default() -> Self {
        Self {
            conflict_strategy: ConflictStrategy::default(),
            prefix_paths: false,
            tag_prefix: TagPrefixStrategy::default(),
            tag_separator: default_tag_separator(),
            info: None,
            servers: None,
            tags: None,
            filter: None,
        }
    }
}

fn default_tag_separator() -> String {
    "/".into()
}

/// How to prefix tags from each source.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TagPrefixStrategy {
    /// Keep original tag names; deduplicate by name.
    #[default]
    None,
    /// Prefix each tag with its source name.
    SourceName,
}

/// How to resolve naming conflicts during merge.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ConflictStrategy {
    /// Fail immediately on any conflict.
    #[default]
    Error,
    /// Last source wins.
    Overwrite,
    /// Prefix the conflicting name with the source name.
    Rename,
}

/// Values used to override the top-level `info` object.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InfoOverride {
    pub title: Option<String>,
    pub version: Option<String>,
    pub description: Option<String>,
}

/// A server entry for the merged spec.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServerEntry {
    pub url: String,
    pub description: Option<String>,
}

/// A tag entry for the merged spec.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TagEntry {
    pub name: String,
    pub description: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(yaml: &str) -> Result<Config, String> {
        serde_yaml::from_str(yaml).map_err(|e| e.to_string())
    }

    #[test]
    fn unknown_merge_field_is_rejected() {
        let err = parse("sources: []\nmerge:\n  conflict_stratgy: rename\n").unwrap_err();
        assert!(err.contains("conflict_stratgy"), "{err}");
    }

    #[test]
    fn unknown_source_field_is_rejected() {
        let err = parse("sources:\n  - path: a.yaml\n    header: {}\n").unwrap_err();
        assert!(err.contains("header"), "{err}");
    }

    #[test]
    fn source_needs_path_or_url() {
        let err = parse("sources:\n  - name: x\n").unwrap_err();
        assert!(err.contains("either 'path' or 'url'"), "{err}");
    }

    #[test]
    fn source_cannot_have_both_path_and_url() {
        let err = parse("sources:\n  - path: a.yaml\n    url: https://x.test/a\n").unwrap_err();
        assert!(err.contains("both 'path' and 'url'"), "{err}");
    }

    #[test]
    fn file_source_cannot_have_headers() {
        let err = parse("sources:\n  - path: a.yaml\n    headers: {A: b}\n").unwrap_err();
        assert!(err.contains("headers"), "{err}");
    }

    #[test]
    fn sources_parse_into_variants() {
        let config =
            parse("sources:\n  - path: a.yaml\n  - url: https://x.test/a\n    headers: {A: b}\n")
                .unwrap();
        assert!(matches!(config.sources[0], Source::File { .. }));
        assert!(matches!(&config.sources[1], Source::Http { headers, .. } if headers["A"] == "b"));
    }

    #[test]
    fn from_yaml_wraps_errors_as_config_errors() {
        let err = Config::from_yaml("sources: [{name: x}]")
            .unwrap_err()
            .to_string();
        assert!(
            err.starts_with("configuration error: failed to parse config file:"),
            "{err}"
        );
        assert!(err.contains("either 'path' or 'url'"), "{err}");
    }

    #[test]
    fn http_source_default_name_is_host() {
        let source = Source::Http {
            name: None,
            url: "https://billing.example.com:8443/v1/openapi.json".into(),
            headers: HashMap::new(),
            tag_prefix: None,
            additional_blocks: None,
            filter: None,
        };
        assert_eq!(source.display_name(), "billing.example.com");
    }

    const FILTER_YAML: &str = "
sources:
  - name: billing
    path: billing.yaml
    filter:
      include:
        tags: [public]
merge:
  filter:
    exclude:
      paths: ['/internal/**', '/admin/*']
      tags: [internal]
      operation_ids: [debugDump]
      extensions: [x-internal]
";

    #[test]
    fn parses_filters() {
        let config = Config::from_yaml(FILTER_YAML).unwrap();
        let global = config.merge.filter.as_ref().unwrap();
        assert!(global.include.is_none());
        let exclude = global.exclude.as_ref().unwrap();
        assert_eq!(exclude.paths, vec!["/internal/**", "/admin/*"]);
        assert_eq!(exclude.tags, vec!["internal"]);
        assert_eq!(exclude.operation_ids, vec!["debugDump"]);
        assert_eq!(exclude.extensions, vec!["x-internal"]);

        let source_filter = config.sources[0].filter().unwrap();
        assert_eq!(source_filter.include.as_ref().unwrap().tags, vec!["public"]);
        assert!(source_filter.exclude.is_none());
    }

    #[test]
    fn configs_without_filters_have_none() {
        let config = Config::from_yaml("sources:\n  - path: a.yaml\n").unwrap();
        assert!(config.merge.filter.is_none());
        assert!(config.sources[0].filter().is_none());
        let http = Config::from_yaml("sources:\n  - url: https://a.test/x\n").unwrap();
        assert!(http.sources[0].filter().is_none());
    }

    #[test]
    fn invalid_filters_are_config_errors() {
        let global =
            |rules: &str| format!("sources:\n  - path: a.yaml\nmerge:\n  filter:\n    {rules}\n");
        let source = |rules: &str| {
            format!("sources:\n  - name: s\n    path: a.yaml\n    filter:\n      {rules}\n")
        };
        let cases = [
            (global("exclude: {tag: [x]}"), "tag"),
            (global("include: {}"), "merge.filter.include"),
            (global("exclude: {tags: []}"), "at least one rule"),
            (
                global("exclude: {paths: ['internal/**']}"),
                "must start with '/'",
            ),
            (
                global("exclude: {extensions: [internal]}"),
                "must start with 'x-'",
            ),
            (global("exclude: {tags: ['']}"), "empty"),
            (source("include: {}"), "source 's'"),
            (source("exclude: {operation_ids: ['']}"), "empty"),
            (source("exclude: {paths: ['x']}"), "must start with '/'"),
        ];
        for (yaml, fragment) in cases {
            let err = Config::from_yaml(&yaml).unwrap_err().to_string();
            assert!(err.contains(fragment), "expected '{fragment}' in: {err}");
        }
    }
}
