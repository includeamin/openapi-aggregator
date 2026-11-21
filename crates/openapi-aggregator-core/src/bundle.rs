//! Bundle external `$ref`s into a source's own `components`.
//!
//! Pure and I/O-free: front ends discover the documents a spec needs with
//! [`missing_documents`], load them, and call [`bundle`].

use std::collections::{HashMap, HashSet};

use serde_json::Value;

/// Whether `location` is an absolute `http(s)` URL.
pub fn is_url(location: &str) -> bool {
    location.starts_with("http://") || location.starts_with("https://")
}

/// Resolve `reference` (document part only, no fragment) against the location of the
/// referencing document `base`.
pub fn resolve_location(base: &str, reference: &str) -> String {
    if reference.is_empty() {
        return base.to_string();
    }
    if is_url(reference) || reference.starts_with("file:") {
        return reference.to_string();
    }
    if is_url(base) {
        let (origin, path) = split_origin(base);
        let path = path.split(['?', '#']).next().unwrap_or("");
        let joined = if reference.starts_with('/') {
            reference.to_string()
        } else {
            let dir = &path[..path.rfind('/').map_or(0, |i| i + 1)];
            format!("{dir}{reference}")
        };
        return format!("{origin}{}", normalize(&joined, true));
    }
    if reference.starts_with('/') {
        return reference.to_string();
    }
    let dir = &base[..base.rfind('/').map_or(0, |i| i + 1)];
    normalize(&format!("{dir}{reference}"), false)
}

/// `("https://host:80", "/path")` for an absolute URL.
fn split_origin(url: &str) -> (&str, &str) {
    let after_scheme = url.find("://").map_or(0, |i| i + 3);
    match url[after_scheme..].find('/') {
        Some(i) => url.split_at(after_scheme + i),
        None => (url, "/"),
    }
}

/// Collapse `.` and `..` segments. Relative paths keep leading `..`; URL paths
/// (`rooted`) cannot go above the root.
fn normalize(path: &str, rooted: bool) -> String {
    let mut out: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if matches!(out.last(), Some(last) if *last != "..") {
                    out.pop();
                } else if !rooted {
                    out.push("..");
                }
            }
            other => out.push(other),
        }
    }
    let joined = out.join("/");
    if rooted {
        format!("/{joined}")
    } else {
        joined
    }
}

/// Split `"a.yaml#/x"` into `("a.yaml", "/x")`.
pub(crate) fn split_ref(reference: &str) -> (&str, &str) {
    match reference.find('#') {
        Some(i) => (&reference[..i], &reference[i + 1..]),
        None => (reference, ""),
    }
}

/// Decode a JSON pointer (as found after `#`) into segments.
pub(crate) fn decode_pointer(pointer: &str) -> Vec<String> {
    let Some(rest) = pointer.strip_prefix('/') else {
        return Vec::new();
    };
    rest.split('/')
        .map(|segment| {
            percent_decode(segment)
                .replace("~1", "/")
                .replace("~0", "~")
        })
        .collect()
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(byte) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A document that a spec (transitively) references but that is not loaded yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingDocument {
    pub location: String,
    pub referenced_from: String,
}

/// Keys whose children are user-chosen names, not OpenAPI keywords.
const NAME_MAPS: [&str; 17] = [
    "properties",
    "patternProperties",
    "definitions",
    "$defs",
    "schemas",
    "parameters",
    "responses",
    "headers",
    "examples",
    "requestBodies",
    "links",
    "callbacks",
    "securitySchemes",
    "paths",
    "content",
    "encoding",
    "mapping",
];

/// Keys that hold example data, not spec structure.
const DATA_KEYS: [&str; 4] = ["example", "default", "enum", "value"];

/// Visit every `$ref` string, skipping example/default/enum data.
pub(crate) fn walk_refs(value: &Value, f: &mut dyn FnMut(&str)) {
    walk(value, false, f);
}

fn walk(value: &Value, names: bool, f: &mut dyn FnMut(&str)) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if !names && DATA_KEYS.contains(&key.as_str()) {
                    continue;
                }
                match child {
                    Value::String(target) if !names && key == "$ref" => f(target),
                    _ => walk(child, !names && NAME_MAPS.contains(&key.as_str()), f),
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|item| walk(item, false, f)),
        _ => {}
    }
}

/// The documents `spec` (located at `base`) needs, following references through the
/// documents already in `docs`, that are not in `docs` yet. Deduplicated, in discovery order.
pub fn missing_documents(
    spec: &Value,
    base: &str,
    docs: &HashMap<String, Value>,
) -> Vec<MissingDocument> {
    let mut missing = Vec::new();
    let mut seen_missing = HashSet::new();
    let mut visited = HashSet::new();
    visited.insert(base.to_string());
    let mut queue: Vec<(String, &Value)> = vec![(base.to_string(), spec)];

    while let Some((location, document)) = queue.pop() {
        let mut found = Vec::new();
        walk_refs(document, &mut |reference| {
            let (doc_part, _) = split_ref(reference);
            if !doc_part.is_empty() {
                found.push(resolve_location(&location, doc_part));
            }
        });
        for target in found {
            if let Some(loaded) = docs.get(&target) {
                if visited.insert(target.clone()) {
                    queue.push((target, loaded));
                }
            } else if target != base && seen_missing.insert(target.clone()) {
                missing.push(MissingDocument {
                    location: target,
                    referenced_from: location.clone(),
                });
            }
        }
    }
    missing
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_relative_file_locations() {
        assert_eq!(
            resolve_location("specs/pets.yaml", "./schemas/pet.yaml"),
            "specs/schemas/pet.yaml"
        );
        assert_eq!(
            resolve_location("specs/pets.yaml", "../common/e.yaml"),
            "common/e.yaml"
        );
        assert_eq!(resolve_location("pets.yaml", "../x.yaml"), "../x.yaml");
        assert_eq!(
            resolve_location("specs/pets.yaml", "/etc/passwd"),
            "/etc/passwd"
        );
        assert_eq!(
            resolve_location("specs/pets.yaml", "https://b.test/x.yaml"),
            "https://b.test/x.yaml"
        );
        assert_eq!(resolve_location("specs/pets.yaml", ""), "specs/pets.yaml");
    }

    #[test]
    fn resolves_relative_url_locations() {
        let base = "https://a.test/api/v1/spec.json";
        assert_eq!(
            resolve_location(base, "defs.yaml"),
            "https://a.test/api/v1/defs.yaml"
        );
        assert_eq!(
            resolve_location(base, "../common.yaml"),
            "https://a.test/api/common.yaml"
        );
        assert_eq!(
            resolve_location(base, "/root.yaml"),
            "https://a.test/root.yaml"
        );
        assert_eq!(
            resolve_location(base, "https://b.test/x.yaml"),
            "https://b.test/x.yaml"
        );
        assert_eq!(
            resolve_location(base, "file:///etc/passwd"),
            "file:///etc/passwd"
        );
        assert_eq!(
            resolve_location("https://a.test:8080/s.json?x=1", "d.yaml"),
            "https://a.test:8080/d.yaml"
        );
        assert_eq!(
            resolve_location("https://a.test/s.json", "../../d.yaml"),
            "https://a.test/d.yaml"
        );
    }

    #[test]
    fn splits_refs() {
        assert_eq!(
            split_ref("a.yaml#/components/schemas/Pet"),
            ("a.yaml", "/components/schemas/Pet")
        );
        assert_eq!(split_ref("#/x"), ("", "/x"));
        assert_eq!(split_ref("a.yaml"), ("a.yaml", ""));
    }

    #[test]
    fn decodes_pointers() {
        assert_eq!(decode_pointer("/a~1b/c%20d/~0"), vec!["a/b", "c d", "~"]);
        assert!(decode_pointer("").is_empty());
    }

    #[test]
    fn is_url_detects_http_only() {
        assert!(is_url("https://a.test/x") && is_url("http://a.test/x"));
        assert!(!is_url("specs/a.yaml") && !is_url("file:///x"));
    }

    use serde_json::json;

    #[test]
    fn finds_missing_documents_transitively() {
        let spec = json!({"a": {"$ref": "./schemas/pet.yaml#/Pet"}, "b": {"$ref": "#/components/schemas/Local"}});
        let mut docs = HashMap::new();
        assert_eq!(
            missing_documents(&spec, "specs/pets.yaml", &docs),
            vec![MissingDocument {
                location: "specs/schemas/pet.yaml".into(),
                referenced_from: "specs/pets.yaml".into()
            }]
        );

        docs.insert("specs/schemas/pet.yaml".to_string(), json!({"Pet": {"$ref": "../common/err.yaml#/E"}, "Other": {"$ref": "../common/err.yaml#/F"}}));
        assert_eq!(
            missing_documents(&spec, "specs/pets.yaml", &docs),
            vec![MissingDocument {
                location: "specs/common/err.yaml".into(),
                referenced_from: "specs/schemas/pet.yaml".into()
            }]
        );
    }

    #[test]
    fn ignores_own_location_and_example_data() {
        let spec = json!({
            "a": {"$ref": "pets.yaml#/x"},
            "b": {"example": {"$ref": "other.yaml"}},
            "c": {"properties": {"default": {"$ref": "real.yaml#/R"}, "example": {"$ref": "real2.yaml#/R"}}}
        });
        let missing: Vec<String> = missing_documents(&spec, "pets.yaml", &HashMap::new())
            .into_iter()
            .map(|m| m.location)
            .collect();
        assert_eq!(missing, vec!["real.yaml", "real2.yaml"]);
    }

    #[test]
    fn circular_documents_terminate() {
        let mut docs = HashMap::new();
        docs.insert("a.yaml".to_string(), json!({"A": {"$ref": "b.yaml#/B"}}));
        docs.insert("b.yaml".to_string(), json!({"B": {"$ref": "a.yaml#/A"}}));
        let spec = json!({"$ref": "a.yaml#/A"});
        assert!(missing_documents(&spec, "root.yaml", &docs).is_empty());
    }
}
