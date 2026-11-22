//! Pure OpenAPI aggregation logic: config types, spec parsing/validation and merging.
//! Contains no I/O, so it also builds for `wasm32-unknown-unknown`.
//!
//! Most users want the [`openapi-aggregator`](https://crates.io/crates/openapi-aggregator)
//! crate, which loads sources from files and HTTP. Use this crate directly when you already
//! have the specs in memory.
//!
//! # Example
//!
//! ```
//! use openapi_aggregator_core::{merge_specs, MergeConfig};
//! use serde_json::json;
//!
//! let users = json!({
//!     "openapi": "3.0.3",
//!     "info": { "title": "Users", "version": "1.0.0" },
//!     "paths": { "/users": { "get": { "responses": { "200": { "description": "ok" } } } } }
//! });
//! let orders = json!({
//!     "openapi": "3.0.3",
//!     "info": { "title": "Orders", "version": "1.0.0" },
//!     "paths": { "/orders": { "get": { "responses": { "200": { "description": "ok" } } } } }
//! });
//!
//! let merged = merge_specs(
//!     vec![
//!         ("users".into(), String::new(), users),
//!         ("orders".into(), String::new(), orders),
//!     ],
//!     &MergeConfig::default(),
//! )
//! .unwrap();
//!
//! assert!(merged["paths"]["/users"].is_object());
//! assert!(merged["paths"]["/orders"].is_object());
//! ```

pub mod bundle;
pub mod config;
pub mod error;
pub mod expand;
pub mod filter;
pub mod merge;
pub mod prepare;
pub mod swagger2;
pub mod validate;

pub use bundle::{
    bundle, check_document_cap, is_url, missing_documents, resolve_location, MissingDocument,
    MAX_DOCUMENTS,
};
pub use config::{
    Config, ConflictStrategy, FilterConfig, FilterRules, InfoOverride, MergeConfig, OutputConfig,
    OutputFormat, ServerEntry, Source, TagEntry, TagPrefixStrategy,
};
pub use error::Error;
pub use expand::{expand_sources, glob_base, is_glob};
pub use filter::{filter_spec, glob_match};
pub use merge::{
    merge_sources, merge_specs, merge_specs_with_report, resolve_path_prefix, MergeReport,
    SourceSpec,
};
pub use prepare::{
    bundle_source, filter_source, parse_content, prepare_source, prepare_source_with_notes,
    PreparedSource,
};
pub use validate::{validate, Finding, Severity};
