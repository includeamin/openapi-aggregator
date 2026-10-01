use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("failed to read file '{path}': {source}")]
    FileRead {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("HTTP request failed for '{url}': {source}")]
    HttpRequest {
        url: String,
        #[source]
        source: reqwest::Error,
    },

    #[error(transparent)]
    Core(#[from] openapi_aggregator_core::Error),
}
