//! Loading the external documents a source's `$ref`s point at, then bundling them.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use openapi_aggregator_core::{
    bundle_source, check_document_cap, is_url, missing_documents, parse_content,
    Error as CoreError, MissingDocument, PreparedSource, Source,
};
use serde_json::Value;

use crate::error::Error;
use crate::source::expand_env;
use crate::AggregateOptions;

pub(crate) struct RefContext<'a> {
    pub client: &'a reqwest::Client,
    pub options: &'a AggregateOptions,
    /// Canonicalised directories external file refs may read from.
    pub allowed_roots: Vec<PathBuf>,
}

/// The directories external file refs may point into: the config directory and the
/// directory of every file source.
pub(crate) fn allowed_roots(sources: &[Source], options: &AggregateOptions) -> Vec<PathBuf> {
    let config_dir = options
        .config_dir
        .clone()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or_else(|| PathBuf::from("."));
    let mut roots = vec![config_dir];
    for source in sources {
        if let Source::File { path, .. } = source {
            let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
            roots.push(parent.map_or_else(|| PathBuf::from("."), Path::to_path_buf));
        }
    }
    roots
        .into_iter()
        .filter_map(|r| std::fs::canonicalize(r).ok())
        .collect()
}

fn invalid(name: &str, reason: String) -> Error {
    CoreError::InvalidSpec {
        name: name.to_string(),
        reason,
    }
    .into()
}

/// Load every document `prepared` references (transitively) and bundle them in.
pub(crate) async fn resolve_external_refs(
    prepared: PreparedSource,
    source: &Source,
    ctx: &RefContext<'_>,
) -> Result<PreparedSource, Error> {
    let base = match source {
        Source::File { path, .. } => path.to_string_lossy().into_owned(),
        Source::Http { url, .. } => url.clone(),
    };
    let mut docs: HashMap<String, Value> = HashMap::new();
    loop {
        let missing = missing_documents(&prepared.spec, &base, &docs);
        if missing.is_empty() {
            break;
        }
        for document in missing {
            check_document_cap(docs.len() + 1, &prepared.name)?;
            let text = load_document(&document, &prepared.name, source, ctx).await?;
            let value = parse_content(&text).map_err(|e| {
                invalid(
                    &prepared.name,
                    format!(
                        "$ref document '{}' could not be parsed: {e}",
                        document.location
                    ),
                )
            })?;
            docs.insert(document.location, value);
        }
    }
    Ok(bundle_source(prepared, &base, &docs)?)
}

async fn load_document(
    document: &MissingDocument,
    name: &str,
    source: &Source,
    ctx: &RefContext<'_>,
) -> Result<String, Error> {
    let location = &document.location;
    let from = &document.referenced_from;
    if location.starts_with("file:") {
        return Err(invalid(
            name,
            format!("$ref '{location}' (from '{from}'): file: URLs are not supported"),
        ));
    }
    if is_url(location) {
        return load_url(location, source, ctx).await;
    }
    if is_url(from) {
        return Err(invalid(
            name,
            format!(
                "$ref '{location}' (from '{from}'): a remote document cannot reference local files"
            ),
        ));
    }
    load_file(location, from, name, ctx).await
}

async fn load_file(
    location: &str,
    from: &str,
    name: &str,
    ctx: &RefContext<'_>,
) -> Result<String, Error> {
    let unreadable = |e: std::io::Error| {
        invalid(
            name,
            format!("$ref '{location}' (from '{from}') could not be read: {e}"),
        )
    };
    let real = tokio::fs::canonicalize(location)
        .await
        .map_err(unreadable)?;
    if !ctx.options.allow_outside_config_dir
        && !ctx.allowed_roots.iter().any(|root| real.starts_with(root))
    {
        return Err(invalid(
            name,
            format!(
                "$ref '{location}' (from '{from}') is outside the config directory; \
                 pass --allow-outside-config-dir to permit it"
            ),
        ));
    }
    tokio::fs::read_to_string(&real).await.map_err(unreadable)
}

async fn load_url(location: &str, source: &Source, ctx: &RefContext<'_>) -> Result<String, Error> {
    // Errors carry the unexpanded URL so `${SECRET}` values never reach logs.
    let http_err = |e: reqwest::Error| Error::HttpRequest {
        url: location.to_string(),
        source: e.without_url(),
    };
    let target = expand_env(location)?;
    let mut request = ctx.client.get(&target);
    if let Source::Http { url, headers, .. } = source {
        if same_origin(&expand_env(url)?, &target) {
            for (key, value) in headers {
                request = request.header(key, expand_env(value)?);
            }
        }
    }
    request
        .send()
        .await
        .map_err(http_err)?
        .error_for_status()
        .map_err(http_err)?
        .text()
        .await
        .map_err(http_err)
}

fn same_origin(a: &str, b: &str) -> bool {
    match (reqwest::Url::parse(a), reqwest::Url::parse(b)) {
        (Ok(a), Ok(b)) => a.origin() == b.origin(),
        _ => false,
    }
}
