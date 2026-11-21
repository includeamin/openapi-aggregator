//! Bundle external `$ref`s into a source's own `components`.
//!
//! Pure and I/O-free: front ends discover the documents a spec needs with
//! [`missing_documents`], load them, and call [`bundle`].

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
}
