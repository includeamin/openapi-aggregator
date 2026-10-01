//! Pure OpenAPI aggregation logic: config types, spec parsing/validation and merging.
//! Contains no I/O, so it also builds for `wasm32-unknown-unknown`.

pub mod config;
pub mod error;
pub mod merge;
pub mod prepare;

pub use config::{
    Config, ConflictStrategy, InfoOverride, MergeConfig, OutputConfig, OutputFormat, ServerEntry,
    Source, TagEntry, TagPrefixStrategy,
};
pub use error::Error;
pub use merge::{merge_specs, merge_specs_with_report, MergeReport};
pub use prepare::{parse_content, prepare_source};
