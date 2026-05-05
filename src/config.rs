use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;

/// Top-level configuration for the aggregator.
#[derive(Debug, Clone, Deserialize, Serialize)]
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
/// otherwise `path` is used as a local file source.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
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

impl Source {
    /// Return the user-provided name or derive one from the url / filename.
    pub fn display_name(&self) -> String {
        match self {
            Source::Http { name, url, .. } => name.clone().unwrap_or_else(|| url.clone()),
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
pub struct InfoOverride {
    pub title: Option<String>,
    pub version: Option<String>,
    pub description: Option<String>,
}

/// A server entry for the merged spec.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerEntry {
    pub url: String,
    pub description: Option<String>,
}

/// A tag entry for the merged spec.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TagEntry {
    pub name: String,
    pub description: Option<String>,
}
