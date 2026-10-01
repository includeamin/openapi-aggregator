use serde_json::Value;
use std::time::Duration;

use openapi_aggregator_core::{prepare_source, Error as CoreError, Source};

use crate::error::Error;

pub async fn load_source(source: &Source) -> Result<(String, Value), Error> {
    load_source_with_client(source, &http_client()?).await
}

pub(crate) fn http_client() -> Result<reqwest::Client, Error> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| {
            Error::from(CoreError::Config(format!(
                "failed to create HTTP client: {e}"
            )))
        })
}

pub(crate) async fn load_source_with_client(
    source: &Source,
    client: &reqwest::Client,
) -> Result<(String, Value), Error> {
    let content = match source {
        Source::File { path, .. } => {
            tokio::fs::read_to_string(path)
                .await
                .map_err(|e| Error::FileRead {
                    path: path.display().to_string(),
                    source: e,
                })?
        }
        Source::Http { url, headers, .. } => {
            let url = expand_env(url)?;
            let http_err = |e| Error::HttpRequest {
                url: url.clone(),
                source: e,
            };
            let mut request = client.get(&url);
            for (key, value) in headers {
                request = request.header(key, expand_env(value)?);
            }
            request
                .send()
                .await
                .map_err(http_err)?
                .error_for_status()
                .map_err(http_err)?
                .text()
                .await
                .map_err(http_err)?
        }
    };

    Ok(prepare_source(&content, source)?)
}

/// Replace `${NAME}` placeholders with environment variables.
fn expand_env(text: &str) -> Result<String, Error> {
    expand_env_with(text, |name| std::env::var(name).ok())
}

fn expand_env_with(text: &str, lookup: impl Fn(&str) -> Option<String>) -> Result<String, Error> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("${") {
        let Some(len) = rest[start + 2..].find('}') else {
            break;
        };
        let name = &rest[start + 2..start + 2 + len];
        let value = lookup(name).ok_or_else(|| {
            Error::from(CoreError::Config(format!(
                "environment variable '{name}' is not set"
            )))
        })?;
        out.push_str(&rest[..start]);
        out.push_str(&value);
        rest = &rest[start + 2 + len + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod env_tests {
    use super::*;

    fn lookup(name: &str) -> Option<String> {
        match name {
            "TOKEN" => Some("secret".into()),
            "HOST" => Some("api.test".into()),
            _ => None,
        }
    }

    #[test]
    fn expands_env_placeholders() {
        assert_eq!(
            expand_env_with("Bearer ${TOKEN} @ ${HOST}", lookup).unwrap(),
            "Bearer secret @ api.test"
        );
    }

    #[test]
    fn leaves_text_without_placeholders_alone() {
        assert_eq!(
            expand_env_with("$5 {x} $TOKEN", lookup).unwrap(),
            "$5 {x} $TOKEN"
        );
    }

    #[test]
    fn missing_env_var_is_an_error() {
        let err = expand_env_with("Bearer ${NOPE}", lookup)
            .unwrap_err()
            .to_string();
        assert!(err.contains("NOPE"), "{err}");
    }
}
