use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("failed to parse spec content: {0}")]
    Parse(String),

    #[error("invalid OpenAPI spec from source '{name}': {reason}")]
    InvalidSpec { name: String, reason: String },

    #[error("merge conflict: {0}")]
    MergeConflict(String),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("no sources provided")]
    NoSources,
}
