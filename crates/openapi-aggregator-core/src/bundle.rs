//! Bundle external `$ref`s into a source's own `components`.
//!
//! Pure and I/O-free: front ends discover the documents a spec needs with
//! [`missing_documents`], load them, and call [`bundle`].

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::error::Error;

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

/// Maximum number of external documents a single source may pull in.
pub const MAX_DOCUMENTS: usize = 100;

/// Fail when a source has pulled in more than [`MAX_DOCUMENTS`] external documents.
pub fn check_document_cap(count: usize, source_name: &str) -> Result<(), Error> {
    if count > MAX_DOCUMENTS {
        return Err(Error::InvalidSpec {
            name: source_name.to_string(),
            reason: format!("too many external documents (limit {MAX_DOCUMENTS})"),
        });
    }
    Ok(())
}

/// Bundle every external `$ref` in `spec` (located at `base`) into its `components`.
/// Returns the bundled spec and notes about names that had to be changed.
pub fn bundle(
    mut spec: Value,
    base: &str,
    docs: &HashMap<String, Value>,
) -> Result<(Value, Vec<String>), Error> {
    let mut needs_work = false;
    walk_refs(&spec, &mut |reference| {
        let (doc_part, _) = split_ref(reference);
        needs_work |= !doc_part.is_empty();
    });
    if !needs_work {
        return Ok((spec, Vec::new()));
    }

    let mut bundler = Bundler::new(base, docs, &spec);
    bundler.inline_path_items(&mut spec)?;
    bundler.process(&mut spec, base, &mut Vec::new(), false)?;
    let notes = bundler.finish(&mut spec);
    Ok((spec, notes))
}

/// The component type a ref's position implies, from the keys leading to it
/// (`"[]"` marks an array element).
fn infer_type(stack: &[String]) -> &'static str {
    let by_container = |key: &str| match key {
        "parameters" => "parameters",
        "responses" => "responses",
        "headers" => "headers",
        "examples" => "examples",
        "links" => "links",
        "callbacks" => "callbacks",
        "requestBodies" => "requestBodies",
        "securitySchemes" => "securitySchemes",
        _ => "schemas",
    };
    let keys: Vec<&str> = stack.iter().map(String::as_str).collect();
    match keys.as_slice() {
        [.., parent, "[]"] => by_container(parent),
        [.., "requestBody"] => "requestBodies",
        [.., "schema" | "items" | "additionalProperties" | "not"] => "schemas",
        [.., parent, _] => by_container(parent),
        _ => "schemas",
    }
}

const COMPONENT_TYPES: [&str; 9] = [
    "schemas",
    "parameters",
    "responses",
    "requestBodies",
    "headers",
    "examples",
    "links",
    "callbacks",
    "securitySchemes",
];

/// File name without directories, query or extension: `a/b/pet.yaml?x` -> `pet`.
fn file_stem(location: &str) -> String {
    let name = location.rsplit('/').next().unwrap_or(location);
    let name = name.split(['?', '#']).next().unwrap_or(name);
    match name.rfind('.') {
        Some(i) if i > 0 => name[..i].to_string(),
        _ => name.to_string(),
    }
}

fn sanitize_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "component".to_string()
    } else {
        cleaned
    }
}

fn lookup<'v>(document: &'v Value, segments: &[String]) -> Option<&'v Value> {
    let mut current = document;
    for segment in segments {
        current = match current {
            Value::Object(map) => map.get(segment)?,
            Value::Array(items) => items.get(segment.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(current)
}

struct Bundler<'a> {
    base: String,
    docs: &'a HashMap<String, Value>,
    /// (document, pointer) -> (type, name) for everything already hoisted.
    memo: HashMap<(String, String), (&'static str, String)>,
    taken: HashMap<&'static str, HashSet<String>>,
    hoisted: Vec<(&'static str, String, Value)>,
    notes: Vec<String>,
    inlining: Vec<(String, String)>,
}

impl<'a> Bundler<'a> {
    fn new(base: &str, docs: &'a HashMap<String, Value>, spec: &Value) -> Self {
        let mut taken: HashMap<&'static str, HashSet<String>> = HashMap::new();
        for ty in COMPONENT_TYPES {
            if let Some(Value::Object(existing)) = spec.pointer(&format!("/components/{ty}")) {
                taken
                    .entry(ty)
                    .or_default()
                    .extend(existing.keys().cloned());
            }
        }
        Self {
            base: base.to_string(),
            docs,
            memo: HashMap::new(),
            taken,
            hoisted: Vec::new(),
            notes: Vec::new(),
            inlining: Vec::new(),
        }
    }

    fn unresolved(reference: &str, reason: &str) -> Error {
        Error::UnresolvedRef {
            reference: reference.to_string(),
            reason: reason.to_string(),
        }
    }

    fn target_of(&self, location: &str, reference: &str) -> (String, String) {
        let (doc_part, pointer) = split_ref(reference);
        let target = if doc_part.is_empty() {
            location.to_string()
        } else {
            resolve_location(location, doc_part)
        };
        (target, pointer.to_string())
    }

    fn fetch(&self, target: &str, pointer: &str, reference: &str) -> Result<Value, Error> {
        let document = self.docs.get(target).ok_or_else(|| {
            Self::unresolved(reference, "points to a document that was not loaded")
        })?;
        lookup(document, &decode_pointer(pointer))
            .cloned()
            .ok_or_else(|| Self::unresolved(&format!("{target}#{pointer}"), "not found"))
    }

    /// Replace `paths` entries that are external path-item refs by the referenced item.
    fn inline_path_items(&mut self, spec: &mut Value) -> Result<(), Error> {
        let base = self.base.clone();
        let Some(Value::Object(paths)) = spec.get_mut("paths") else {
            return Ok(());
        };
        for (path, item) in paths.iter_mut() {
            let Some(reference) = item.get("$ref").and_then(Value::as_str).map(String::from) else {
                continue;
            };
            let (doc_part, _) = split_ref(&reference);
            if doc_part.is_empty() {
                continue;
            }
            let (target, pointer) = self.target_of(&base, &reference);
            if target == base {
                continue;
            }
            let key = (target.clone(), pointer.clone());
            if self.inlining.contains(&key) {
                return Err(Self::unresolved(
                    &reference,
                    "is a circular path item reference",
                ));
            }
            let mut inlined = self.fetch(&target, &pointer, &reference)?;
            self.inlining.push(key);
            let mut stack = vec!["paths".to_string(), path.clone()];
            let result = self.process(&mut inlined, &target, &mut stack, false);
            self.inlining.pop();
            result?;
            if let (Value::Object(merged), Value::Object(own)) = (&mut inlined, &*item) {
                for (k, v) in own {
                    if k != "$ref" {
                        merged.insert(k.clone(), v.clone());
                    }
                }
            }
            *item = inlined;
        }
        Ok(())
    }

    fn process(
        &mut self,
        value: &mut Value,
        location: &str,
        stack: &mut Vec<String>,
        names: bool,
    ) -> Result<(), Error> {
        match value {
            Value::Object(map) => {
                for (key, child) in map.iter_mut() {
                    if !names && DATA_KEYS.contains(&key.as_str()) {
                        continue;
                    }
                    match child {
                        Value::String(reference) if !names && key == "$ref" => {
                            if let Some(new) = self.rewrite_ref(reference, location, stack)? {
                                *reference = new;
                            }
                        }
                        _ => {
                            stack.push(key.clone());
                            let child_names = !names && NAME_MAPS.contains(&key.as_str());
                            let result = self.process(child, location, stack, child_names);
                            stack.pop();
                            result?;
                        }
                    }
                }
            }
            Value::Array(items) => {
                for item in items {
                    stack.push("[]".to_string());
                    let result = self.process(item, location, stack, false);
                    stack.pop();
                    result?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// The replacement for `reference`, or `None` to leave it alone.
    fn rewrite_ref(
        &mut self,
        reference: &str,
        location: &str,
        stack: &[String],
    ) -> Result<Option<String>, Error> {
        let (doc_part, _) = split_ref(reference);
        let (target, pointer) = self.target_of(location, reference);
        if target == self.base {
            if location == self.base && doc_part.is_empty() {
                return Ok(None);
            }
            if pointer.is_empty() {
                return Err(Self::unresolved(
                    reference,
                    "refers to the root document itself",
                ));
            }
            return Ok(Some(format!("#{pointer}")));
        }

        let key = (target.clone(), pointer.clone());
        if let Some((ty, name)) = self.memo.get(&key) {
            return Ok(Some(format!("#/components/{ty}/{name}")));
        }
        let mut value = self.fetch(&target, &pointer, reference)?;

        let segments = decode_pointer(&pointer);
        let (ty, wanted) = match segments.as_slice() {
            [c, ty, name] if c == "components" && COMPONENT_TYPES.contains(&ty.as_str()) => (
                *COMPONENT_TYPES.iter().find(|t| **t == ty).unwrap(),
                name.clone(),
            ),
            [d, name] if d == "definitions" || d == "$defs" => ("schemas", name.clone()),
            [.., last] => (infer_type(stack), last.clone()),
            [] => (infer_type(stack), file_stem(&target)),
        };
        let wanted = sanitize_name(&wanted);
        let name = self.unique_name(ty, &wanted, &target, &pointer);
        self.memo.insert(key, (ty, name.clone()));

        let mut nested = vec![ty.to_string(), name.clone()];
        self.process(&mut value, &target, &mut nested, false)?;
        self.hoisted.push((ty, name.clone(), value));
        Ok(Some(format!("#/components/{ty}/{name}")))
    }

    fn unique_name(
        &mut self,
        ty: &'static str,
        wanted: &str,
        target: &str,
        pointer: &str,
    ) -> String {
        let taken = self.taken.entry(ty).or_default();
        if taken.insert(wanted.to_string()) {
            return wanted.to_string();
        }
        let stem = sanitize_name(&file_stem(target));
        let mut candidate = format!("{stem}_{wanted}");
        let mut n = 2;
        while !taken.insert(candidate.clone()) {
            candidate = format!("{stem}_{wanted}_{n}");
            n += 1;
        }
        self.notes.push(format!(
            "hoisted '{target}#{pointer}' as components.{ty}.{candidate} ('{wanted}' was taken)"
        ));
        candidate
    }

    fn finish(self, spec: &mut Value) -> Vec<String> {
        if self.hoisted.is_empty() {
            return self.notes;
        }
        if !spec.is_object() {
            return self.notes;
        }
        let root = spec.as_object_mut().expect("checked object");
        let components = root
            .entry("components")
            .or_insert_with(|| Value::Object(Default::default()));
        if let Value::Object(components) = components {
            for (ty, name, value) in self.hoisted {
                let section = components
                    .entry(ty)
                    .or_insert_with(|| Value::Object(Default::default()));
                if let Value::Object(section) = section {
                    section.insert(name, value);
                }
            }
        }
        self.notes
    }
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

    fn run(spec: Value, base: &str, docs: Vec<(&str, Value)>) -> (Value, Vec<String>) {
        let docs: HashMap<String, Value> =
            docs.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        bundle(spec, base, &docs).unwrap()
    }

    fn root(schema_ref: &str) -> Value {
        json!({"openapi":"3.0.3","info":{"title":"T","version":"1"},"paths":{"/p":{"get":{"responses":{"200":{
            "description":"ok","content":{"application/json":{"schema":{"$ref": schema_ref}}}}}}}}})
    }

    fn schema_ref(spec: &Value) -> &str {
        spec["paths"]["/p"]["get"]["responses"]["200"]["content"]["application/json"]["schema"]
            ["$ref"]
            .as_str()
            .unwrap()
    }

    #[test]
    fn hoists_schema_by_component_pointer() {
        let target = json!({"type":"object","properties":{"m":{"type":"string"}}});
        let (out, notes) = run(
            root("./e.yaml#/components/schemas/Error"),
            "root.yaml",
            vec![(
                "e.yaml",
                json!({"components":{"schemas":{"Error": target.clone()}}}),
            )],
        );
        assert_eq!(schema_ref(&out), "#/components/schemas/Error");
        assert_eq!(out["components"]["schemas"]["Error"], target);
        assert!(notes.is_empty());
    }

    #[test]
    fn hoists_definitions_and_defs_as_schemas() {
        for pointer in ["/definitions/Pet", "/$defs/Pet"] {
            let doc = if pointer.contains("defs/") && pointer.starts_with("/$") {
                json!({"$defs":{"Pet":{"type":"object"}}})
            } else {
                json!({"definitions":{"Pet":{"type":"object"}}})
            };
            let (out, _) = run(
                root(&format!("d.yaml#{pointer}")),
                "root.yaml",
                vec![("d.yaml", doc)],
            );
            assert_eq!(schema_ref(&out), "#/components/schemas/Pet");
            assert_eq!(out["components"]["schemas"]["Pet"]["type"], "object");
        }
    }

    #[test]
    fn infers_type_from_position() {
        let spec = json!({"openapi":"3.0.3","paths":{"/p":{"post":{
            "parameters":[{"$ref":"d.yaml#/Limit"}],
            "requestBody":{"$ref":"d.yaml#/Body"},
            "responses":{"404":{"$ref":"d.yaml#/NotFound"}}}}}});
        let d = json!({"Limit":{"name":"limit","in":"query"},"Body":{"content":{}},"NotFound":{"description":"nf"}});
        let (out, _) = run(spec, "root.yaml", vec![("d.yaml", d)]);
        let op = &out["paths"]["/p"]["post"];
        assert_eq!(op["parameters"][0]["$ref"], "#/components/parameters/Limit");
        assert_eq!(op["requestBody"]["$ref"], "#/components/requestBodies/Body");
        assert_eq!(
            op["responses"]["404"]["$ref"],
            "#/components/responses/NotFound"
        );
        assert!(out["components"]["parameters"]["Limit"].is_object());
        assert!(out["components"]["requestBodies"]["Body"].is_object());
        assert!(out["components"]["responses"]["NotFound"].is_object());
    }

    #[test]
    fn whole_document_ref_is_named_after_the_file_stem() {
        let (out, _) = run(
            root("./models/pet.yaml"),
            "root.yaml",
            vec![("models/pet.yaml", json!({"type":"object"}))],
        );
        assert_eq!(schema_ref(&out), "#/components/schemas/pet");
    }

    #[test]
    fn internal_and_relative_refs_in_external_docs_are_bundled() {
        let (out, _) = run(
            root("./models/pet.yaml#/Pet"),
            "root.yaml",
            vec![
                (
                    "models/pet.yaml",
                    json!({"Pet":{"type":"object","properties":{
                    "owner":{"$ref":"#/Owner"},"tag":{"$ref":"./tag.yaml#/Tag"}}},"Owner":{"type":"string"}}),
                ),
                ("models/tag.yaml", json!({"Tag":{"type":"integer"}})),
            ],
        );
        let s = &out["components"]["schemas"];
        assert_eq!(
            s["Pet"]["properties"]["owner"]["$ref"],
            "#/components/schemas/Owner"
        );
        assert_eq!(
            s["Pet"]["properties"]["tag"]["$ref"],
            "#/components/schemas/Tag"
        );
        assert_eq!(s["Owner"]["type"], "string");
        assert_eq!(s["Tag"]["type"], "integer");
    }

    #[test]
    fn circular_refs_terminate_and_hoist_once() {
        let (out, _) = run(
            root("a.yaml#/A"),
            "root.yaml",
            vec![
                (
                    "a.yaml",
                    json!({"A":{"properties":{"b":{"$ref":"b.yaml#/B"}}}}),
                ),
                (
                    "b.yaml",
                    json!({"B":{"properties":{"a":{"$ref":"a.yaml#/A"}}}}),
                ),
            ],
        );
        let s = out["components"]["schemas"].as_object().unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(s["B"]["properties"]["a"]["$ref"], "#/components/schemas/A");
    }

    #[test]
    fn same_target_is_hoisted_once() {
        let spec =
            json!({"a":{"schema":{"$ref":"d.yaml#/X"}},"b":{"schema":{"$ref":"./d.yaml#/X"}}});
        let (out, _) = run(
            spec,
            "root.yaml",
            vec![("d.yaml", json!({"X":{"type":"string"}}))],
        );
        assert_eq!(out["a"]["schema"]["$ref"], out["b"]["schema"]["$ref"]);
        assert_eq!(out["components"]["schemas"].as_object().unwrap().len(), 1);
    }

    #[test]
    fn name_clash_adds_file_stem_and_note() {
        let mut spec = root("models/pet.yaml#/Pet");
        spec["components"] = json!({"schemas":{"Pet":{"type":"string"}}});
        spec["x"] = json!({"schema":{"$ref":"models/other/pet.yaml#/Pet"}});
        let (out, notes) = run(
            spec,
            "root.yaml",
            vec![
                ("models/pet.yaml", json!({"Pet":{"type":"object"}})),
                ("models/other/pet.yaml", json!({"Pet":{"type":"integer"}})),
            ],
        );
        assert_eq!(schema_ref(&out), "#/components/schemas/pet_Pet");
        assert_eq!(out["x"]["schema"]["$ref"], "#/components/schemas/pet_Pet_2");
        assert_eq!(out["components"]["schemas"]["Pet"]["type"], "string");
        assert!(
            notes[0].contains(
                "hoisted 'models/pet.yaml#/Pet' as components.schemas.pet_Pet ('Pet' was taken)"
            ),
            "{notes:?}"
        );
        assert_eq!(notes.len(), 2);
    }

    #[test]
    fn root_internal_refs_are_left_alone_and_refs_back_into_root_become_internal() {
        let mut spec = root("d.yaml#/D");
        spec["components"] = json!({"schemas":{"X":{"type":"string"}}});
        let (out, _) = run(
            spec,
            "root.yaml",
            vec![(
                "d.yaml",
                json!({"D":{"properties":{"x":{"$ref":"root.yaml#/components/schemas/X"}}}}),
            )],
        );
        assert_eq!(
            out["components"]["schemas"]["D"]["properties"]["x"]["$ref"],
            "#/components/schemas/X"
        );
        assert_eq!(out["components"]["schemas"].as_object().unwrap().len(), 2);

        let internal = root("#/components/schemas/X");
        let (same, notes) = run(internal.clone(), "root.yaml", vec![]);
        assert_eq!(same, internal);
        assert!(notes.is_empty());
    }

    #[test]
    fn path_item_ref_is_inlined() {
        let spec =
            json!({"openapi":"3.0.3","paths":{"/x":{"$ref":"p.yaml#/paths/~1x","summary":"own"}}});
        let p = json!({"paths":{"/x":{"summary":"theirs","get":{"responses":{"200":{"description":"ok","content":{
            "application/json":{"schema":{"$ref":"#/definitions/T"}}}}}}}},"definitions":{"T":{"type":"string"}}});
        let (out, _) = run(spec, "root.yaml", vec![("p.yaml", p)]);
        let item = &out["paths"]["/x"];
        assert!(item.get("$ref").is_none());
        assert_eq!(item["summary"], "own");
        assert_eq!(
            item["get"]["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
            "#/components/schemas/T"
        );
        assert_eq!(out["components"]["schemas"]["T"]["type"], "string");
    }

    #[test]
    fn example_data_is_not_treated_as_a_ref() {
        let spec =
            json!({"a":{"example":{"$ref":"nope.yaml#/x"}},"b":{"schema":{"$ref":"d.yaml#/X"}}});
        let (out, _) = run(spec, "root.yaml", vec![("d.yaml", json!({"X":{}}))]);
        assert_eq!(out["a"]["example"]["$ref"], "nope.yaml#/x");
    }

    #[test]
    fn invalid_component_name_characters_are_replaced() {
        let (out, _) = run(
            root("d.yaml#/My%20Pet"),
            "root.yaml",
            vec![("d.yaml", json!({"My Pet":{"type":"object"}}))],
        );
        assert_eq!(schema_ref(&out), "#/components/schemas/My_Pet");
    }

    #[test]
    fn errors_for_missing_pointer_and_missing_document() {
        let docs: HashMap<String, Value> = [("d.yaml".to_string(), json!({"X":{}}))].into();
        let err = bundle(root("d.yaml#/Nope"), "root.yaml", &docs)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("d.yaml#/Nope") && err.contains("not found"),
            "{err}"
        );
        let err = bundle(root("missing.yaml#/X"), "root.yaml", &docs)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("missing.yaml") && err.contains("not loaded"),
            "{err}"
        );
    }
}
