//! Aggregate and merge OpenAPI 3.x specifications from multiple sources (local files and
//! HTTP endpoints) into a single spec.
//!
//! See [`aggregate`] to load and merge the sources of a [`Config`]. The pure merge logic lives
//! in [`openapi_aggregator_core`] and is re-exported here.
//!
//! The crate also ships the `openapi-aggregator` command-line tool.

pub mod error;
mod refs;
pub mod source;

pub use error::Error;
pub use openapi_aggregator_core::{
    bundle_source, check_document_cap, config, filter_source, is_url, merge, merge_specs,
    merge_specs_with_report, missing_documents, parse_content, prepare_source,
    prepare_source_with_notes, resolve_location, validate, Config, ConflictStrategy, Finding,
    InfoOverride, MergeConfig, MergeReport, MissingDocument, OutputConfig, OutputFormat,
    PreparedSource, ServerEntry, Severity, Source, TagEntry, TagPrefixStrategy,
};
pub use source::load_source;

use std::path::{Path, PathBuf};

use futures_util::future::try_join_all;
use openapi_aggregator_core::Error as CoreError;
use source::{http_client, load_source_with_client};

/// Options for [`aggregate_with_options`].
#[derive(Debug, Clone, Default)]
pub struct AggregateOptions {
    /// Allow external `$ref`s to read files outside the config directory (and the
    /// directories of the file sources). Off by default.
    pub allow_outside_config_dir: bool,
    /// The directory the config was loaded from; defaults to the current directory.
    pub config_dir: Option<PathBuf>,
}

/// Load all sources defined in `config` and merge them into a single OpenAPI spec.
pub async fn aggregate(config: &Config) -> Result<serde_json::Value, Error> {
    aggregate_with_report(config)
        .await
        .map(|report| report.spec)
}

/// Like [`aggregate`], but also returns non-fatal merge warnings.
///
/// Sources are loaded concurrently; the merge order still follows `config.sources`.
/// External `$ref`s are resolved with the default (sandboxed) [`AggregateOptions`].
pub async fn aggregate_with_report(config: &Config) -> Result<MergeReport, Error> {
    aggregate_with_options(config, &AggregateOptions::default()).await
}

/// Like [`aggregate_with_report`], with explicit [`AggregateOptions`].
pub async fn aggregate_with_options(
    config: &Config,
    options: &AggregateOptions,
) -> Result<MergeReport, Error> {
    if config.sources.is_empty() {
        return Err(CoreError::NoSources.into());
    }

    let client = http_client()?;
    let ctx = refs::RefContext {
        client: &client,
        options,
        allowed_roots: refs::allowed_roots(&config.sources, options),
    };
    let prepared = try_join_all(config.sources.iter().map(|src| async {
        let prepared = load_source_with_client(src, &client).await?;
        let prepared = refs::resolve_external_refs(prepared, src, &ctx).await?;
        Ok::<_, Error>(filter_source(
            prepared,
            config.merge.filter.as_ref(),
            src.filter(),
        ))
    }))
    .await?;

    let mut notes = Vec::new();
    let mut specs = Vec::new();
    for (src, prepared) in config.sources.iter().zip(prepared) {
        notes.extend(prepared.notes);
        specs.push((prepared.name, src.tag_prefix(), prepared.spec));
    }

    let mut report = merge_specs_with_report(specs, &config.merge)?;
    notes.append(&mut report.warnings);
    report.warnings = notes;
    Ok(report)
}

/// Read a config file and resolve relative source paths against its directory.
pub fn load_config(path: &Path) -> Result<Config, Error> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| CoreError::Config(format!("failed to read config file: {e}")))?;
    let mut config = Config::from_yaml(&content)?;

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
    let options = AggregateOptions {
        config_dir: path.parent().map(Path::to_path_buf),
        ..Default::default()
    };
    aggregate_with_options(&load_config(path)?, &options)
        .await
        .map(|report| report.spec)
}
