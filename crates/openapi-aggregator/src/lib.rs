pub mod config;
pub mod error;
pub mod merge;
pub mod source;

pub use config::{
    Config, ConflictStrategy, InfoOverride, MergeConfig, OutputFormat, ServerEntry, Source,
    TagEntry, TagPrefixStrategy,
};
pub use error::Error;
pub use merge::{merge_specs, merge_specs_with_report, MergeReport};
pub use source::load_source;

use std::path::Path;

use futures_util::future::try_join_all;
use source::{http_client, load_source_with_client};

/// Load all sources defined in `config` and merge them into a single OpenAPI spec.
pub async fn aggregate(config: &Config) -> Result<serde_json::Value, Error> {
    aggregate_with_report(config)
        .await
        .map(|report| report.spec)
}

/// Like [`aggregate`], but also returns non-fatal merge warnings.
///
/// Sources are loaded concurrently; the merge order still follows `config.sources`.
pub async fn aggregate_with_report(config: &Config) -> Result<MergeReport, Error> {
    if config.sources.is_empty() {
        return Err(Error::NoSources);
    }

    let client = http_client()?;
    let specs = try_join_all(config.sources.iter().map(|src| async {
        let (name, spec) = load_source_with_client(src, &client).await?;
        Ok::<_, Error>((name, src.tag_prefix(), spec))
    }))
    .await?;

    merge_specs_with_report(specs, &config.merge)
}

/// Read a config file and resolve relative source paths against its directory.
pub fn load_config(path: &Path) -> Result<Config, Error> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| Error::Config(format!("failed to read config file: {e}")))?;
    let mut config: Config = serde_yaml::from_str(&content)
        .map_err(|e| Error::Config(format!("failed to parse config file: {e}")))?;

    if let Some(config_dir) = path.parent() {
        for src in &mut config.sources {
            if let Source::File {
                path: ref mut file_path,
                ..
            } = src
            {
                if file_path.is_relative() {
                    *file_path = config_dir.join(&*file_path);
                }
            }
        }
    }

    Ok(config)
}

/// Read a config file, resolve relative source paths against its directory,
/// then aggregate.
pub async fn aggregate_from_file(path: &Path) -> Result<serde_json::Value, Error> {
    aggregate(&load_config(path)?).await
}
