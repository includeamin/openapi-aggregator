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
