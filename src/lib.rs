pub mod config;
pub mod error;
pub mod merge;
pub mod source;

pub use config::{
    Config, ConflictStrategy, InfoOverride, MergeConfig, OutputFormat, ServerEntry, Source,
    TagEntry, TagPrefixStrategy,
};
pub use error::Error;
pub use merge::merge_specs;
pub use source::load_source;

use std::path::Path;

/// Load all sources defined in `config` and merge them into a single OpenAPI spec.
pub async fn aggregate(config: &Config) -> Result<serde_json::Value, Error> {
    if config.sources.is_empty() {
        return Err(Error::NoSources);
    }

    let mut specs = Vec::with_capacity(config.sources.len());
    for src in &config.sources {
        let (name, spec) = load_source(src).await?;
        let tag_prefix = src.tag_prefix();
        specs.push((name, tag_prefix, spec));
    }

    merge_specs(specs, &config.merge)
}

/// Read a config file, resolve relative source paths against its directory,
/// then aggregate.
pub async fn aggregate_from_file(path: &Path) -> Result<serde_json::Value, Error> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| Error::Config(format!("failed to read config file: {e}")))?;
    let mut config: Config = serde_yaml::from_str(&content)
        .map_err(|e| Error::Config(format!("failed to parse config file: {e}")))?;

    // Resolve relative file paths against the config file's directory
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

    aggregate(&config).await
}
