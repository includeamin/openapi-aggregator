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
}

impl TryFrom<RawSource> for Source {
    type Error = String;

    fn try_from(raw: RawSource) -> Result<Self, Self::Error> {
        let label = raw
            .name
            .as_deref()
            .map(|n| format!("source '{n}'"))
            .unwrap_or_else(|| "source".into());
        match (raw.url, raw.path) {
            (Some(url), None) => Ok(Source::Http {
                name: raw.name,
                url,
                headers: raw.headers.unwrap_or_default(),
                tag_prefix: raw.tag_prefix,
                additional_blocks: raw.additional_blocks,
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

    /// Return the tag prefix for this source: custom `tag_prefix` field if set, otherwise the display name.
    pub fn tag_prefix(&self) -> String {
        match self {
            Source::Http { tag_prefix, .. } | Source::File { tag_prefix, .. } => {
                tag_prefix.clone().unwrap_or_else(|| self.display_name())
            }
        }
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
    fn http_source_default_name_is_host() {
        let source = Source::Http {
            name: None,
            url: "https://billing.example.com:8443/v1/openapi.json".into(),
            headers: HashMap::new(),
            tag_prefix: None,
            additional_blocks: None,
        };
        assert_eq!(source.display_name(), "billing.example.com");
    }
}
