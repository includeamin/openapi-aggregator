use serde::{Deserialize, Serialize};
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
    },
    File {
        name: Option<String>,
        path: PathBuf,
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
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct MergeConfig {
    /// Strategy for handling duplicate paths or component names.
    #[serde(default)]
    pub conflict_strategy: ConflictStrategy,
    /// If `true`, every path from each source is prefixed with `/{source_name}`.
    #[serde(default)]
    pub prefix_paths: bool,
    /// Override the `info` block in the merged output.
    pub info: Option<InfoOverride>,
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
