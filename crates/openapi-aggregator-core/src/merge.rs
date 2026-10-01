use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value};

use crate::config::{ConflictStrategy, InfoOverride, MergeConfig, TagPrefixStrategy};
use crate::error::Error;

/// Component types in the order they are emitted in the merged spec.
const COMPONENT_TYPES: &[&str] = &[
    "schemas",
    "responses",
    "parameters",
    "examples",
    "requestBodies",
    "headers",
    "securitySchemes",
    "links",
    "callbacks",
    "pathItems",
];

const HTTP_METHODS: &[&str] = &[
    "get", "post", "put", "patch", "delete", "options", "head", "trace",
];

/// The merged spec plus any non-fatal warnings raised while merging.
#[derive(Debug, Clone)]
pub struct MergeReport {
    pub spec: Value,
    pub warnings: Vec<String>,
}

/// Merge multiple named OpenAPI specs into one according to `config`.
///
/// Each entry is `(source_name, tag_prefix, spec_value)`.
pub fn merge_specs(
    specs: Vec<(String, String, Value)>,
    config: &MergeConfig,
) -> Result<Value, Error> {
    merge_specs_with_report(specs, config).map(|report| report.spec)
}

/// Like [`merge_specs`], but also returns non-fatal warnings
/// (e.g. sources using different OpenAPI minor versions).
pub fn merge_specs_with_report(
    specs: Vec<(String, String, Value)>,
    config: &MergeConfig,
) -> Result<MergeReport, Error> {
    if specs.is_empty() {
        return Err(Error::NoSources);
    }

    let mut merged = Map::new();

    // --- openapi version (from first source) ---
    if let Some(version) = specs[0].2.get("openapi") {
        merged.insert("openapi".into(), version.clone());
    }
    let warnings = version_warnings(&specs);

    // Top-level `security` applies to every operation of its own source. Keep it
    // top-level only when all sources agree; otherwise push it down per source.
    let first_security = specs[0].2.get("security");
    let shared_security = specs
        .iter()
        .all(|(_, _, spec)| spec.get("security") == first_security);
    let top_level_security = shared_security.then(|| first_security.cloned()).flatten();

    // --- info ---
    let info = build_info(&specs, config.info.as_ref())?;
    merged.insert("info".into(), info);

    // --- paths & components (incremental merge) ---
    let mut merged_paths = Map::new();
    let mut merged_webhooks = Map::new();
    let mut merged_components: HashMap<String, Map<String, Value>> = HashMap::new();
    let mut merged_component_extras = Map::new();
    let mut merged_tags: Vec<Value> = Vec::new();
    let mut merged_servers: Vec<Value> = Vec::new();
    let mut merged_custom_top_level = Map::new();

    for (source_name, tag_prefix, mut spec) in specs {
        let ident = sanitize_ident(&source_name);

        if !shared_security {
            if let Some(requirements) = spec.get("security").cloned() {
                push_down_security(&mut spec, &requirements);
            }
        }

        // Phase 1: detect component conflicts and build a $ref rename map
        let rename_map = build_rename_map(&ident, &spec, &merged_components, config);

        // Phase 2: rewrite $refs in the source spec if needed
        if !rename_map.is_empty() {
            rewrite_refs(&mut spec, &rename_map);
            rewrite_security_requirements(&mut spec, &rename_map);
        }

        // Phase 2b: rewrite tag references in operations before merging paths
        if config.tag_prefix == TagPrefixStrategy::SourceName && config.tags.is_none() {
            rewrite_spec_operation_tags(&mut spec, &tag_prefix, config);
        }

        // Phase 3a: merge paths
        merge_path_items(
            &source_name,
            "path",
            spec.get("paths"),
            &mut merged_paths,
            &config.conflict_strategy,
            |path| {
                if config.prefix_paths {
                    format!("/{ident}{path}")
                } else {
                    path.to_string()
                }
            },
            |path, n| match n {
                1 => format!("/{ident}{path}"),
                n => format!("/{ident}_{n}{path}"),
            },
        )?;

        // Phase 3b: merge webhooks (OpenAPI 3.1)
        merge_path_items(
            &source_name,
            "webhook",
            spec.get("webhooks"),
            &mut merged_webhooks,
            &config.conflict_strategy,
            str::to_string,
            |name, n| match n {
                1 => format!("{ident}_{name}"),
                n => format!("{ident}_{n}_{name}"),
            },
        )?;

        // Phase 3c: merge components
        merge_components(
            &source_name,
            &spec,
            &mut merged_components,
            &mut merged_component_extras,
            &rename_map,
            config,
        )?;

        // Phase 3d: merge tags
        if let Some(Value::Array(tags)) = spec.get("tags") {
            for tag in tags {
                let original_name = tag.get("name").and_then(|n| n.as_str());
                let prefixed_name = original_name.map(|n| {
                    if config.tag_prefix == TagPrefixStrategy::SourceName {
                        format!("{}{}{}", tag_prefix, config.tag_separator, n)
                    } else {
                        n.to_string()
                    }
                });

                let check_name = prefixed_name.as_deref();
                let already_exists = check_name.is_some_and(|n| {
                    merged_tags
                        .iter()
                        .any(|t| t.get("name").and_then(|v| v.as_str()) == Some(n))
                });
                if !already_exists {
                    let mut new_tag = tag.clone();
                    if config.tag_prefix == TagPrefixStrategy::SourceName {
                        if let Some(obj) = new_tag.as_object_mut() {
                            if let Some(name) = prefixed_name {
                                obj.insert("name".into(), Value::String(name));
                            }
                        }
                    }
                    merged_tags.push(new_tag);
                }
            }
        }

        // Phase 3e: merge servers (deduplicate by url)
        if let Some(Value::Array(servers)) = spec.get("servers") {
            for server in servers {
                let url = server.get("url").and_then(|u| u.as_str());
                let already_exists = url.is_some_and(|u| {
                    merged_servers
                        .iter()
                        .any(|s| s.get("url").and_then(|v| v.as_str()) == Some(u))
                });
                if !already_exists {
                    merged_servers.push(server.clone());
                }
            }
        }

        // Phase 3f: merge non-standard top-level blocks (e.g. vendor extensions)
        if let Some(root) = spec.as_object() {
            for (key, value) in root {
                if is_reserved_top_level_key(key) {
                    continue;
                }
                if let Some(existing) = merged_custom_top_level.get_mut(key) {
                    deep_merge(existing, value);
                } else {
                    merged_custom_top_level.insert(key.clone(), value.clone());
                }
            }
        }
    }

    for (key, value) in merged_custom_top_level {
        merged.insert(key, value);
    }

    merged.insert("paths".into(), Value::Object(merged_paths));

    if !merged_webhooks.is_empty() {
        merged.insert("webhooks".into(), Value::Object(merged_webhooks));
    }

    let mut comp_obj = Map::new();
    for &ctype in COMPONENT_TYPES {
        if let Some(items) = merged_components.remove(ctype) {
            comp_obj.insert(ctype.into(), Value::Object(items));
        }
    }
    comp_obj.extend(merged_component_extras);
    if !comp_obj.is_empty() {
        merged.insert("components".into(), Value::Object(comp_obj));
    }

    // --- tags: config override takes priority, then merged from sources ---
    if let Some(config_tags) = &config.tags {
        let tags_value: Vec<Value> = config_tags
            .iter()
            .map(|t| {
                let mut obj = Map::new();
                obj.insert("name".into(), Value::String(t.name.clone()));
                if let Some(desc) = &t.description {
                    obj.insert("description".into(), Value::String(desc.clone()));
                }
                Value::Object(obj)
            })
            .collect();
        merged.insert("tags".into(), Value::Array(tags_value));
    } else if !merged_tags.is_empty() {
        merged.insert("tags".into(), Value::Array(merged_tags));
    }

    // --- servers: config override takes priority, then merged from sources ---
    if let Some(config_servers) = &config.servers {
        let servers_value: Vec<Value> = config_servers
            .iter()
            .map(|s| {
                let mut obj = Map::new();
                obj.insert("url".into(), Value::String(s.url.clone()));
                if let Some(desc) = &s.description {
                    obj.insert("description".into(), Value::String(desc.clone()));
                }
                Value::Object(obj)
            })
            .collect();
        merged.insert("servers".into(), Value::Array(servers_value));
    } else if !merged_servers.is_empty() {
        merged.insert("servers".into(), Value::Array(merged_servers));
    }

    if let Some(security) = top_level_security {
        merged.insert("security".into(), security);
    }

    Ok(MergeReport {
        spec: Value::Object(merged),
        warnings,
    })
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn build_info(
    specs: &[(String, String, Value)],
    info_override: Option<&InfoOverride>,
) -> Result<Value, Error> {
    let mut info = specs[0]
        .2
        .get("info")
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()));

    if let Some(ov) = info_override {
        let obj = info.as_object_mut().ok_or_else(|| Error::InvalidSpec {
            name: "merged".into(),
            reason: "'info' must be an object".into(),
        })?;
        if let Some(title) = &ov.title {
            obj.insert("title".into(), Value::String(title.clone()));
        }
        if let Some(version) = &ov.version {
            obj.insert("version".into(), Value::String(version.clone()));
        }
        if let Some(desc) = &ov.description {
            obj.insert("description".into(), Value::String(desc.clone()));
        }
    }

    Ok(info)
}

/// Warn about sources whose `major.minor` OpenAPI version differs from the first source.
fn version_warnings(specs: &[(String, String, Value)]) -> Vec<String> {
    fn minor(version: &str) -> &str {
        match version.match_indices('.').nth(1) {
            Some((idx, _)) => &version[..idx],
            None => version,
        }
    }

    let Some(first) = specs[0].2.get("openapi").and_then(Value::as_str) else {
        return Vec::new();
    };

    specs
        .iter()
        .skip(1)
        .filter_map(|(name, _, spec)| {
            let version = spec.get("openapi")?.as_str()?;
            (minor(version) != minor(first)).then(|| {
                format!(
                    "source '{name}' uses OpenAPI {version}, but the merged spec uses {first} \
                     (taken from the first source)"
                )
            })
        })
        .collect()
}

/// Make a source name safe for use in URL paths and component names
/// (`^[a-zA-Z0-9._-]+$`).
fn sanitize_ident(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn is_reserved_top_level_key(key: &str) -> bool {
    matches!(
        key,
        "openapi" | "info" | "paths" | "webhooks" | "components" | "tags" | "servers" | "security"
    )
}

/// Recursively merge `patch` into `target`. Objects are merged key by key;
/// any other value in `patch` replaces the one in `target`.
pub(crate) fn deep_merge(target: &mut Value, patch: &Value) {
    match (target, patch) {
        (Value::Object(target_map), Value::Object(patch_map)) => {
            for (key, patch_value) in patch_map {
                if let Some(existing) = target_map.get_mut(key) {
                    deep_merge(existing, patch_value);
                } else {
                    target_map.insert(key.clone(), patch_value.clone());
                }
            }
        }
        (target_slot, patch_value) => {
            *target_slot = patch_value.clone();
        }
    }
}

/// Rewrite tag arrays inside all operations of a source spec, prefixing each tag name.
fn rewrite_spec_operation_tags(spec: &mut Value, tag_prefix: &str, config: &MergeConfig) {
    if let Some(Value::Object(paths)) = spec.get_mut("paths") {
        for (_path_key, path_item) in paths.iter_mut() {
            if let Some(obj) = path_item.as_object_mut() {
                for method in HTTP_METHODS {
                    if let Some(operation) = obj.get_mut(*method) {
                        if let Some(Value::Array(tags)) = operation.get_mut("tags") {
                            for tag_val in tags.iter_mut() {
                                if let Some(tag_str) = tag_val.as_str() {
                                    let prefixed = format!(
                                        "{}{}{}",
                                        tag_prefix, config.tag_separator, tag_str
                                    );
                                    *tag_val = Value::String(prefixed);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Mutable access to every operation in `paths` and `webhooks`.
fn for_each_operation(spec: &mut Value, mut f: impl FnMut(&mut Map<String, Value>)) {
    for section in ["paths", "webhooks"] {
        let Some(Value::Object(items)) = spec.get_mut(section) else {
            continue;
        };
        for path_item in items.values_mut() {
            let Some(path_item) = path_item.as_object_mut() else {
                continue;
            };
            for method in HTTP_METHODS {
                if let Some(Value::Object(operation)) = path_item.get_mut(*method) {
                    f(operation);
                }
            }
        }
    }
}

/// Copy a source's top-level security requirements into each of its operations
/// that doesn't declare its own, so they keep their meaning after merging.
fn push_down_security(spec: &mut Value, requirements: &Value) {
    for_each_operation(spec, |operation| {
        if !operation.contains_key("security") {
            operation.insert("security".into(), requirements.clone());
        }
    });
}

/// Security requirements name schemes by key, not `$ref`: rename those keys
/// for any renamed `securitySchemes` entry.
fn rewrite_security_requirements(spec: &mut Value, rename_map: &HashMap<String, String>) {
    const PREFIX: &str = "#/components/securitySchemes/";
    let renames: HashMap<&str, &str> = rename_map
        .iter()
        .filter_map(|(old, new)| Some((old.strip_prefix(PREFIX)?, new.strip_prefix(PREFIX)?)))
        .collect();
    if renames.is_empty() {
        return;
    }

    let rename_requirements = |security: &mut Value| {
        let Value::Array(requirements) = security else {
            return;
        };
        for requirement in requirements {
            if let Value::Object(schemes) = requirement {
                *schemes = std::mem::take(schemes)
                    .into_iter()
                    .map(|(name, scopes)| match renames.get(name.as_str()) {
                        Some(new) => (new.to_string(), scopes),
                        None => (name, scopes),
                    })
                    .collect();
            }
        }
    };

    if let Some(security) = spec.get_mut("security") {
        rename_requirements(security);
    }
    for_each_operation(spec, |operation| {
        if let Some(security) = operation.get_mut("security") {
            rename_requirements(security);
        }
    });
}

fn component_ref(ctype: &str, name: &str) -> String {
    format!("#/components/{ctype}/{name}")
}

/// For the rename strategy: figure out which component names in `spec` clash
/// with already-merged, *different* components and return a map from
/// old `$ref` → new `$ref`.
///
/// Identical components are shared rather than renamed. Because renaming one
/// component changes the `$ref`s inside the components that point at it, the
/// comparison is repeated until no new renames appear.
fn build_rename_map(
    ident: &str,
    spec: &Value,
    merged_components: &HashMap<String, Map<String, Value>>,
    config: &MergeConfig,
) -> HashMap<String, String> {
    let mut rename_map = HashMap::new();

    if config.conflict_strategy != ConflictStrategy::Rename {
        return rename_map;
    }
    let Some(components) = spec.get("components") else {
        return rename_map;
    };

    // Names that a renamed component must not take.
    let mut taken: HashMap<&str, HashSet<String>> = HashMap::new();
    for &ctype in COMPONENT_TYPES {
        let names = taken.entry(ctype).or_default();
        if let Some(existing) = merged_components.get(ctype) {
            names.extend(existing.keys().cloned());
        }
        if let Some(Value::Object(items)) = components.get(ctype) {
            names.extend(items.keys().cloned());
        }
    }

    loop {
        let mut candidate = components.clone();
        rewrite_refs(&mut candidate, &rename_map);

        let mut changed = false;
        for &ctype in COMPONENT_TYPES {
            let (Some(Value::Object(items)), Some(existing)) =
                (candidate.get(ctype), merged_components.get(ctype))
            else {
                continue;
            };
            for (name, value) in items {
                let old_ref = component_ref(ctype, name);
                if rename_map.contains_key(&old_ref)
                    || existing.get(name).is_none_or(|e| e == value)
                {
                    continue;
                }
                let names = taken.entry(ctype).or_default();
                let mut n = 1;
                let new_name = loop {
                    let candidate_name = match n {
                        1 => format!("{ident}_{name}"),
                        n => format!("{ident}_{n}_{name}"),
                    };
                    if !names.contains(&candidate_name) {
                        break candidate_name;
                    }
                    n += 1;
                };
                names.insert(new_name.clone());
                rename_map.insert(old_ref, component_ref(ctype, &new_name));
                changed = true;
            }
        }

        if !changed {
            return rename_map;
        }
    }
}

/// Walk the JSON tree and rewrite any `$ref` that points at (or into) a
/// component found in `rename_map`.
fn rewrite_refs(value: &mut Value, rename_map: &HashMap<String, String>) {
    match value {
        Value::Object(map) => {
            let new_ref = map
                .get("$ref")
                .and_then(|v| v.as_str())
                .and_then(|s| renamed_ref(s, rename_map));

            if let Some(new_ref) = new_ref {
                map.insert("$ref".into(), Value::String(new_ref));
            }

            if let Some(Value::Object(mapping)) = map
                .get_mut("discriminator")
                .and_then(|d| d.get_mut("mapping"))
            {
                for target in mapping.values_mut() {
                    if let Some(new_target) = target
                        .as_str()
                        .and_then(|t| renamed_mapping_target(t, rename_map))
                    {
                        *target = Value::String(new_target);
                    }
                }
            }

            for v in map.values_mut() {
                rewrite_refs(v, rename_map);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                rewrite_refs(v, rename_map);
            }
        }
        _ => {}
    }
}

/// A discriminator mapping value is either a `$ref`-style string or a bare
/// schema name (`Dog`, meaning `#/components/schemas/Dog`).
fn renamed_mapping_target(target: &str, rename_map: &HashMap<String, String>) -> Option<String> {
    if target.contains('/') {
        return renamed_ref(target, rename_map);
    }
    let new_ref = rename_map.get(&component_ref("schemas", target))?;
    new_ref.rsplit('/').next().map(str::to_string)
}

/// `#/components/schemas/Pet/properties/id` → `#/components/schemas/b_Pet/properties/id`
fn renamed_ref(reference: &str, rename_map: &HashMap<String, String>) -> Option<String> {
    let component_end = reference
        .match_indices('/')
        .nth(3)
        .map_or(reference.len(), |(idx, _)| idx);
    let (component, rest) = reference.split_at(component_end);
    rename_map
        .get(component)
        .map(|new_ref| format!("{new_ref}{rest}"))
}

/// Fields of `incoming` that `existing` also defines with a different value.
fn conflicting_fields(existing: &Value, incoming: &Value) -> Vec<String> {
    match (existing.as_object(), incoming.as_object()) {
        (Some(existing), Some(incoming)) => incoming
            .iter()
            .filter(|(key, value)| existing.get(*key).is_some_and(|e| e != *value))
            .map(|(key, _)| key.clone())
            .collect(),
        _ if existing != incoming => vec!["entire item".into()],
        _ => Vec::new(),
    }
}

/// Merge a map of Path Item objects (`paths` or `webhooks`) into `merged`.
///
/// Items under the same key are combined field by field (e.g. `GET` from one
/// source and `POST` from another). Only fields that both define with
/// different values are conflicts, resolved according to `strategy`.
fn merge_path_items(
    source_name: &str,
    kind: &str,
    items: Option<&Value>,
    merged: &mut Map<String, Value>,
    strategy: &ConflictStrategy,
    key_for: impl Fn(&str) -> String,
    renamed_key: impl Fn(&str, usize) -> String,
) -> Result<(), Error> {
    let Some(items) = items.and_then(Value::as_object) else {
        return Ok(());
    };

    for (name, item) in items {
        let key = key_for(name);
        let Some(existing) = merged.get_mut(&key) else {
            merged.insert(key, item.clone());
            continue;
        };

        let conflicts = conflicting_fields(existing, item);
        if conflicts.is_empty() || *strategy == ConflictStrategy::Overwrite {
            match (existing.as_object_mut(), item.as_object()) {
                (Some(existing), Some(item)) => {
                    for (field, value) in item {
                        existing.insert(field.clone(), value.clone());
                    }
                }
                _ => *existing = item.clone(),
            }
            continue;
        }

        match strategy {
            ConflictStrategy::Error => {
                return Err(Error::MergeConflict(format!(
                    "duplicate {kind} '{key}' (conflicting: {}) from source '{source_name}'",
                    conflicts.join(", ")
                )));
            }
            ConflictStrategy::Rename => {
                let mut n = 1;
                let mut renamed = renamed_key(name, n);
                while merged.contains_key(&renamed) {
                    n += 1;
                    renamed = renamed_key(name, n);
                }
                merged.insert(renamed, item.clone());
            }
            ConflictStrategy::Overwrite => unreachable!("handled above"),
        }
    }

    Ok(())
}

fn merge_components(
    source_name: &str,
    spec: &Value,
    merged: &mut HashMap<String, Map<String, Value>>,
    extras: &mut Map<String, Value>,
    rename_map: &HashMap<String, String>,
    config: &MergeConfig,
) -> Result<(), Error> {
    let Some(components) = spec.get("components").and_then(Value::as_object) else {
        return Ok(());
    };

    for (ctype, items) in components {
        // Vendor extensions and unknown keys under `components` are deep-merged as-is.
        if !COMPONENT_TYPES.contains(&ctype.as_str()) {
            match extras.get_mut(ctype) {
                Some(existing) => deep_merge(existing, items),
                None => {
                    extras.insert(ctype.clone(), items.clone());
                }
            }
            continue;
        }
        let Some(items) = items.as_object() else {
            continue;
        };

        let merged_type = merged.entry(ctype.clone()).or_default();

        for (item_name, item_value) in items {
            if let Some(new_ref) = rename_map.get(&component_ref(ctype, item_name)) {
                let new_name = new_ref.rsplit('/').next().unwrap_or(item_name);
                merged_type.insert(new_name.to_string(), item_value.clone());
                continue;
            }

            match merged_type.get(item_name) {
                Some(existing) if existing == item_value => {}
                Some(_) if config.conflict_strategy == ConflictStrategy::Error => {
                    return Err(Error::MergeConflict(format!(
                        "duplicate component {ctype}/{item_name} from source '{source_name}'"
                    )));
                }
                _ => {
                    merged_type.insert(item_name.clone(), item_value.clone());
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn petstore_spec() -> Value {
        json!({
            "openapi": "3.0.3",
            "info": { "title": "Petstore", "version": "1.0" },
            "paths": {
                "/pets": {
                    "get": { "summary": "List pets" }
                }
            },
            "components": {
                "schemas": {
                    "Pet": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "integer" },
                            "name": { "type": "string" }
                        }
                    }
                }
            }
        })
    }

    fn users_spec() -> Value {
        json!({
            "openapi": "3.0.3",
            "info": { "title": "Users", "version": "1.0" },
            "paths": {
                "/users": {
                    "get": { "summary": "List users" }
                }
            },
            "components": {
                "schemas": {
                    "User": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "integer" },
                            "email": { "type": "string" }
                        }
                    }
                }
            }
        })
    }

    #[test]
    fn merge_no_conflicts() {
        let specs = vec![
            ("petstore".into(), "petstore".into(), petstore_spec()),
            ("users".into(), "users".into(), users_spec()),
        ];
        let config = MergeConfig::default();
        let merged = merge_specs(specs, &config).unwrap();

        assert!(merged["paths"]["/pets"].is_object());
        assert!(merged["paths"]["/users"].is_object());
        assert!(merged["components"]["schemas"]["Pet"].is_object());
        assert!(merged["components"]["schemas"]["User"].is_object());
    }

    #[test]
    fn merge_conflict_error_strategy() {
        let mut alt = petstore_spec();
        alt["paths"]["/pets"]["get"]["summary"] = json!("Different");
        let specs = vec![
            ("a".into(), "a".into(), petstore_spec()),
            ("b".into(), "b".into(), alt),
        ];
        let config = MergeConfig::default(); // Error strategy
        let result = merge_specs(specs, &config);
        assert!(result.is_err());
    }

    #[test]
    fn merge_conflict_overwrite_strategy() {
        let mut alt = petstore_spec();
        alt["paths"]["/pets"]["get"]["summary"] = json!("Overwritten");

        let specs = vec![
            ("a".into(), "a".into(), petstore_spec()),
            ("b".into(), "b".into(), alt),
        ];
        let config = MergeConfig {
            conflict_strategy: ConflictStrategy::Overwrite,
            ..Default::default()
        };
        let merged = merge_specs(specs, &config).unwrap();
        assert_eq!(merged["paths"]["/pets"]["get"]["summary"], "Overwritten");
    }

    #[test]
    fn merge_conflict_rename_strategy() {
        let mut alt = petstore_spec();
        alt["components"]["schemas"]["Pet"]["properties"]["species"] = json!({ "type": "string" });

        let specs = vec![
            ("a".into(), "a".into(), petstore_spec()),
            ("b".into(), "b".into(), alt),
        ];
        let config = MergeConfig {
            conflict_strategy: ConflictStrategy::Rename,
            ..Default::default()
        };
        let merged = merge_specs(specs, &config).unwrap();

        // Original kept as Pet, duplicate renamed to b_Pet
        assert!(merged["components"]["schemas"]["Pet"].is_object());
        assert!(merged["components"]["schemas"]["b_Pet"].is_object());
    }

    #[test]
    fn merge_conflict_rename_does_not_overwrite_path() {
        let mut first = petstore_spec();
        first["paths"]["/b/pets"] = json!({"get": {"summary": "Existing"}});

        let mut second = petstore_spec();
        second["paths"]["/pets"]["get"]["summary"] = json!("List b pets");

        let specs = vec![
            ("a".into(), "a".into(), first),
            ("b".into(), "b".into(), second),
        ];
        let config = MergeConfig {
            conflict_strategy: ConflictStrategy::Rename,
            ..Default::default()
        };
        let merged = merge_specs(specs, &config).unwrap();

        assert_eq!(merged["paths"]["/b/pets"]["get"]["summary"], "Existing");
        assert_eq!(
            merged["paths"]["/b_2/pets"]["get"]["summary"],
            "List b pets"
        );
    }

    #[test]
    fn merge_with_prefix_paths() {
        let specs = vec![
            ("petstore".into(), "petstore".into(), petstore_spec()),
            ("users".into(), "users".into(), users_spec()),
        ];
        let config = MergeConfig {
            prefix_paths: true,
            ..Default::default()
        };
        let merged = merge_specs(specs, &config).unwrap();
        assert!(merged["paths"]["/petstore/pets"].is_object());
        assert!(merged["paths"]["/users/users"].is_object());
    }

    #[test]
    fn merge_with_info_override() {
        let specs = vec![("a".into(), "a".into(), petstore_spec())];
        let config = MergeConfig {
            info: Some(InfoOverride {
                title: Some("Custom Title".into()),
                version: Some("2.0".into()),
                description: None,
            }),
            ..Default::default()
        };
        let merged = merge_specs(specs, &config).unwrap();
        assert_eq!(merged["info"]["title"], "Custom Title");
        assert_eq!(merged["info"]["version"], "2.0");
    }

    #[test]
    fn merge_rejects_non_object_info_with_override() {
        let specs = vec![(
            "a".into(),
            "a".into(),
            json!({"openapi": "3.0.3", "info": "invalid", "paths": {}}),
        )];
        let config = MergeConfig {
            info: Some(InfoOverride {
                title: Some("Custom Title".into()),
                version: None,
                description: None,
            }),
            ..Default::default()
        };

        let error = merge_specs(specs, &config).unwrap_err();
        assert!(error.to_string().contains("'info' must be an object"));
    }

    #[test]
    fn rewrite_refs_updates_values() {
        let mut spec = json!({
            "paths": {
                "/pets": {
                    "get": {
                        "responses": {
                            "200": {
                                "content": {
                                    "application/json": {
                                        "schema": {
                                            "$ref": "#/components/schemas/Pet"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });

        let mut rename_map = HashMap::new();
        rename_map.insert(
            "#/components/schemas/Pet".into(),
            "#/components/schemas/b_Pet".into(),
        );

        rewrite_refs(&mut spec, &rename_map);

        let ref_val = &spec["paths"]["/pets"]["get"]["responses"]["200"]["content"]
            ["application/json"]["schema"]["$ref"];
        assert_eq!(ref_val, "#/components/schemas/b_Pet");
    }

    #[test]
    fn merge_tags_deduplicated() {
        let mut a = petstore_spec();
        a["tags"] = json!([{"name": "pets", "description": "Pets operations"}]);
        let mut b = users_spec();
        b["tags"] = json!([
            {"name": "pets", "description": "Duplicate"},
            {"name": "users", "description": "Users operations"}
        ]);

        let specs = vec![("a".into(), "a".into(), a), ("b".into(), "b".into(), b)];
        let config = MergeConfig::default();
        let merged = merge_specs(specs, &config).unwrap();

        let tags = merged["tags"].as_array().unwrap();
        assert_eq!(tags.len(), 2);
    }

    #[test]
    fn merge_empty_returns_error() {
        let config = MergeConfig::default();
        assert!(merge_specs(vec![], &config).is_err());
    }

    #[test]
    fn merge_tags_with_source_name_prefix() {
        let mut a = petstore_spec();
        a["tags"] = json!([{"name": "pets", "description": "Pets operations"}]);
        a["paths"]["/pets"]["get"]["tags"] = json!(["pets"]);

        let mut b = users_spec();
        b["tags"] = json!([{"name": "users", "description": "Users operations"}]);
        b["paths"]["/users"]["get"]["tags"] = json!(["users"]);

        let specs = vec![
            ("petstore".into(), "petstore".into(), a),
            ("users".into(), "users".into(), b),
        ];
        let config = MergeConfig {
            tag_prefix: TagPrefixStrategy::SourceName,
            ..Default::default()
        };
        let merged = merge_specs(specs, &config).unwrap();

        let tags = merged["tags"].as_array().unwrap();
        let tag_names: Vec<&str> = tags
            .iter()
            .filter_map(|t| t.get("name").and_then(|n| n.as_str()))
            .collect();
        assert!(tag_names.contains(&"petstore/pets"));
        assert!(tag_names.contains(&"users/users"));

        // Operations should also reference the prefixed tags
        let pet_tags = merged["paths"]["/pets"]["get"]["tags"].as_array().unwrap();
        assert_eq!(pet_tags[0], "petstore/pets");
    }

    #[test]
    fn merge_tags_with_custom_separator() {
        let mut a = petstore_spec();
        a["tags"] = json!([{"name": "pets"}]);
        a["paths"]["/pets"]["get"]["tags"] = json!(["pets"]);

        let specs = vec![("petstore".into(), "petstore".into(), a)];
        let config = MergeConfig {
            tag_prefix: TagPrefixStrategy::SourceName,
            tag_separator: " - ".into(),
            ..Default::default()
        };
        let merged = merge_specs(specs, &config).unwrap();

        let tags = merged["tags"].as_array().unwrap();
        assert_eq!(tags[0]["name"], "petstore - pets");
    }

    #[test]
    fn merge_tags_with_custom_tag_prefix() {
        let mut a = petstore_spec();
        a["tags"] = json!([{"name": "pets"}]);
        a["paths"]["/pets"]["get"]["tags"] = json!(["pets"]);

        // Use a custom tag prefix different from the source name
        let specs = vec![("petstore".into(), "MyPets".into(), a)];
        let config = MergeConfig {
            tag_prefix: TagPrefixStrategy::SourceName,
            ..Default::default()
        };
        let merged = merge_specs(specs, &config).unwrap();

        let tags = merged["tags"].as_array().unwrap();
        assert_eq!(tags[0]["name"], "MyPets/pets");

        let op_tags = merged["paths"]["/pets"]["get"]["tags"].as_array().unwrap();
        assert_eq!(op_tags[0], "MyPets/pets");
    }

    #[test]
    fn merge_with_servers_override() {
        use crate::config::ServerEntry;

        let specs = vec![("a".into(), "a".into(), petstore_spec())];
        let config = MergeConfig {
            servers: Some(vec![
                ServerEntry {
                    url: "https://api.example.com".into(),
                    description: Some("Production".into()),
                },
                ServerEntry {
                    url: "https://staging.example.com".into(),
                    description: None,
                },
            ]),
            ..Default::default()
        };
        let merged = merge_specs(specs, &config).unwrap();

        let servers = merged["servers"].as_array().unwrap();
        assert_eq!(servers.len(), 2);
        assert_eq!(servers[0]["url"], "https://api.example.com");
        assert_eq!(servers[0]["description"], "Production");
        assert_eq!(servers[1]["url"], "https://staging.example.com");
        assert!(servers[1].get("description").is_none());
    }

    #[test]
    fn merge_with_tags_override() {
        use crate::config::TagEntry;

        let mut a = petstore_spec();
        a["tags"] = json!([{"name": "pets", "description": "From source"}]);

        let specs = vec![("a".into(), "a".into(), a)];
        let config = MergeConfig {
            tags: Some(vec![
                TagEntry {
                    name: "animals".into(),
                    description: Some("Animal operations".into()),
                },
                TagEntry {
                    name: "admin".into(),
                    description: None,
                },
            ]),
            ..Default::default()
        };
        let merged = merge_specs(specs, &config).unwrap();

        // Config tags override source tags entirely
        let tags = merged["tags"].as_array().unwrap();
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0]["name"], "animals");
        assert_eq!(tags[0]["description"], "Animal operations");
        assert_eq!(tags[1]["name"], "admin");
    }

    #[test]
    fn explicit_tags_are_not_prefixed_in_operations() {
        let mut source = petstore_spec();
        source["tags"] = json!([{"name": "pets"}]);
        source["paths"]["/pets"]["get"]["tags"] = json!(["pets"]);

        let config = MergeConfig {
            tag_prefix: TagPrefixStrategy::SourceName,
            tags: Some(vec![crate::config::TagEntry {
                name: "pets".into(),
                description: None,
            }]),
            ..Default::default()
        };
        let merged =
            merge_specs(vec![("source".into(), "source".into(), source)], &config).unwrap();

        assert_eq!(merged["tags"][0]["name"], "pets");
        assert_eq!(merged["paths"]["/pets"]["get"]["tags"][0], "pets");
    }

    fn spec_with(paths: Value, components: Value) -> Value {
        json!({
            "openapi": "3.0.3",
            "info": { "title": "T", "version": "1" },
            "paths": paths,
            "components": components
        })
    }

    #[test]
    fn components_are_emitted_in_canonical_order() {
        let mut components = Map::new();
        for &ctype in COMPONENT_TYPES.iter().rev() {
            components.insert(ctype.into(), json!({ "X": {} }));
        }
        let spec = spec_with(json!({}), Value::Object(components));

        let merged = merge_specs(
            vec![("a".into(), "a".into(), spec)],
            &MergeConfig::default(),
        )
        .unwrap();

        let keys: Vec<&str> = merged["components"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, COMPONENT_TYPES);
    }

    #[test]
    fn rename_does_not_clobber_existing_renamed_name() {
        let a = spec_with(
            json!({}),
            json!({ "schemas": {
                "Pet": { "description": "a pet" },
                "b_Pet": { "description": "a's own b_Pet" }
            }}),
        );
        let b = spec_with(
            json!({ "/b-pets": { "get": { "responses": { "200": {
                "description": "ok",
                "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Pet" } } }
            }}}}}),
            json!({ "schemas": { "Pet": { "description": "b pet" } } }),
        );
        let config = MergeConfig {
            conflict_strategy: ConflictStrategy::Rename,
            ..Default::default()
        };
        let merged = merge_specs(
            vec![("a".into(), "a".into(), a), ("b".into(), "b".into(), b)],
            &config,
        )
        .unwrap();

        let schemas = &merged["components"]["schemas"];
        assert_eq!(schemas["b_Pet"]["description"], "a's own b_Pet");
        assert_eq!(schemas["b_2_Pet"]["description"], "b pet");
        assert_eq!(
            merged["paths"]["/b-pets"]["get"]["responses"]["200"]["content"]["application/json"]
                ["schema"]["$ref"],
            "#/components/schemas/b_2_Pet"
        );
    }

    #[test]
    fn disjoint_methods_on_same_path_are_combined() {
        let a = spec_with(
            json!({ "/pets": { "get": { "summary": "list" } } }),
            json!({}),
        );
        let b = spec_with(
            json!({ "/pets": { "post": { "summary": "create" } } }),
            json!({}),
        );

        let merged = merge_specs(
            vec![("a".into(), "a".into(), a), ("b".into(), "b".into(), b)],
            &MergeConfig::default(),
        )
        .unwrap();

        assert_eq!(merged["paths"]["/pets"]["get"]["summary"], "list");
        assert_eq!(merged["paths"]["/pets"]["post"]["summary"], "create");
    }

    #[test]
    fn same_method_on_same_path_conflicts() {
        let a = spec_with(
            json!({ "/pets": { "get": { "summary": "list" } } }),
            json!({}),
        );
        let b = spec_with(
            json!({ "/pets": { "get": { "summary": "other" } } }),
            json!({}),
        );

        let err = merge_specs(
            vec![("a".into(), "a".into(), a), ("b".into(), "b".into(), b)],
            &MergeConfig::default(),
        )
        .unwrap_err()
        .to_string();

        assert!(err.contains("duplicate path '/pets'"), "{err}");
        assert!(err.contains("get"), "{err}");
    }

    #[test]
    fn identical_operations_are_not_conflicts() {
        let health = json!({ "/health": { "get": { "summary": "health" } } });
        let a = spec_with(health.clone(), json!({}));
        let b = spec_with(health, json!({}));

        let merged = merge_specs(
            vec![("a".into(), "a".into(), a), ("b".into(), "b".into(), b)],
            &MergeConfig::default(),
        )
        .unwrap();

        assert_eq!(merged["paths"]["/health"]["get"]["summary"], "health");
    }

    #[test]
    fn identical_components_are_not_conflicts() {
        let auth =
            json!({ "securitySchemes": { "bearer": { "type": "http", "scheme": "bearer" } } });
        let a = spec_with(json!({}), auth.clone());
        let b = spec_with(json!({}), auth);

        for strategy in [ConflictStrategy::Error, ConflictStrategy::Rename] {
            let config = MergeConfig {
                conflict_strategy: strategy,
                ..Default::default()
            };
            let merged = merge_specs(
                vec![
                    ("a".into(), "a".into(), a.clone()),
                    ("b".into(), "b".into(), b.clone()),
                ],
                &config,
            )
            .unwrap();
            let schemes = merged["components"]["securitySchemes"].as_object().unwrap();
            assert_eq!(schemes.keys().collect::<Vec<_>>(), ["bearer"]);
        }
    }

    #[test]
    fn identical_component_with_renamed_dependency_is_renamed() {
        let error = json!({ "properties": { "code": { "$ref": "#/components/schemas/Code" } } });
        let a = spec_with(
            json!({}),
            json!({ "schemas": { "Error": error.clone(), "Code": { "type": "integer" } } }),
        );
        let b = spec_with(
            json!({ "/b": { "get": { "responses": { "default": {
                "description": "err",
                "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Error" } } }
            }}}}}),
            json!({ "schemas": { "Error": error, "Code": { "type": "string" } } }),
        );
        let config = MergeConfig {
            conflict_strategy: ConflictStrategy::Rename,
            ..Default::default()
        };
        let merged = merge_specs(
            vec![("a".into(), "a".into(), a), ("b".into(), "b".into(), b)],
            &config,
        )
        .unwrap();

        let schemas = &merged["components"]["schemas"];
        assert_eq!(schemas["b_Code"]["type"], "string");
        assert_eq!(
            schemas["b_Error"]["properties"]["code"]["$ref"],
            "#/components/schemas/b_Code"
        );
        assert_eq!(
            merged["paths"]["/b"]["get"]["responses"]["default"]["content"]["application/json"]
                ["schema"]["$ref"],
            "#/components/schemas/b_Error"
        );
    }

    #[test]
    fn source_names_are_sanitized_in_paths_and_component_names() {
        let a = spec_with(
            json!({}),
            json!({ "schemas": { "Pet": { "type": "object" } } }),
        );
        let b = spec_with(
            json!({ "/pets": { "get": { "summary": "list" } } }),
            json!({ "schemas": { "Pet": { "type": "string" } } }),
        );
        let config = MergeConfig {
            conflict_strategy: ConflictStrategy::Rename,
            prefix_paths: true,
            ..Default::default()
        };
        let merged = merge_specs(
            vec![
                ("a".into(), "a".into(), a),
                ("my api".into(), "my api".into(), b),
            ],
            &config,
        )
        .unwrap();

        assert!(merged["paths"]["/my_api/pets"].is_object());
        assert!(merged["components"]["schemas"]["my_api_Pet"].is_object());
    }

    #[test]
    fn webhooks_are_merged_with_conflict_detection() {
        let mut a = spec_with(json!({}), json!({}));
        a["webhooks"] = json!({ "petCreated": { "post": { "summary": "a" } } });
        let mut b = spec_with(json!({}), json!({}));
        b["webhooks"] = json!({ "userCreated": { "post": { "summary": "b" } } });
        let mut c = spec_with(json!({}), json!({}));
        c["webhooks"] = json!({ "petCreated": { "post": { "summary": "c" } } });

        let merged = merge_specs(
            vec![
                ("a".into(), "a".into(), a.clone()),
                ("b".into(), "b".into(), b),
            ],
            &MergeConfig::default(),
        )
        .unwrap();
        assert!(merged["webhooks"]["petCreated"].is_object());
        assert!(merged["webhooks"]["userCreated"].is_object());

        let err = merge_specs(
            vec![("a".into(), "a".into(), a), ("c".into(), "c".into(), c)],
            &MergeConfig::default(),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("petCreated"), "{err}");
    }

    #[test]
    fn identical_top_level_security_is_kept() {
        let mut a = spec_with(json!({ "/a": { "get": { "summary": "a" } } }), json!({}));
        a["security"] = json!([{ "bearer": [] }]);
        let mut b = spec_with(json!({ "/b": { "get": { "summary": "b" } } }), json!({}));
        b["security"] = json!([{ "bearer": [] }]);

        let merged = merge_specs(
            vec![("a".into(), "a".into(), a), ("b".into(), "b".into(), b)],
            &MergeConfig::default(),
        )
        .unwrap();

        assert_eq!(merged["security"], json!([{ "bearer": [] }]));
        assert!(merged["paths"]["/a"]["get"].get("security").is_none());
    }

    #[test]
    fn differing_top_level_security_is_pushed_down_to_operations() {
        let mut a = spec_with(json!({ "/a": { "get": { "summary": "a" } } }), json!({}));
        a["security"] = json!([{ "oauth": ["read"] }]);
        let mut b = spec_with(
            json!({
                "/b": { "get": { "summary": "b" } },
                "/public": { "get": { "summary": "p", "security": [] } }
            }),
            json!({}),
        );
        b["security"] = json!([{ "apiKey": [] }]);
        let c = spec_with(json!({ "/c": { "get": { "summary": "c" } } }), json!({}));

        let merged = merge_specs(
            vec![
                ("a".into(), "a".into(), a),
                ("b".into(), "b".into(), b),
                ("c".into(), "c".into(), c),
            ],
            &MergeConfig::default(),
        )
        .unwrap();

        assert!(merged.get("security").is_none(), "{merged:#}");
        let paths = &merged["paths"];
        assert_eq!(
            paths["/a"]["get"]["security"],
            json!([{ "oauth": ["read"] }])
        );
        assert_eq!(paths["/b"]["get"]["security"], json!([{ "apiKey": [] }]));
        assert_eq!(paths["/public"]["get"]["security"], json!([]));
        assert!(paths["/c"]["get"].get("security").is_none());
    }

    #[test]
    fn renamed_security_scheme_updates_requirements() {
        let a = spec_with(
            json!({}),
            json!({ "securitySchemes": { "bearer": { "type": "http", "scheme": "bearer" } } }),
        );
        let mut b = spec_with(
            json!({ "/b": { "get": { "summary": "b", "security": [{ "bearer": [] }] } } }),
            json!({ "securitySchemes": {
                "bearer": { "type": "http", "scheme": "bearer", "bearerFormat": "JWT" }
            }}),
        );
        b["security"] = json!([{ "bearer": [] }]);
        b["paths"]["/b2"] = json!({ "get": { "summary": "b2" } });
        let config = MergeConfig {
            conflict_strategy: ConflictStrategy::Rename,
            ..Default::default()
        };

        let merged = merge_specs(
            vec![("a".into(), "a".into(), a), ("b".into(), "b".into(), b)],
            &config,
        )
        .unwrap();

        assert!(merged["components"]["securitySchemes"]["b_bearer"].is_object());
        assert_eq!(
            merged["paths"]["/b"]["get"]["security"],
            json!([{ "b_bearer": [] }])
        );
        assert_eq!(
            merged["paths"]["/b2"]["get"]["security"],
            json!([{ "b_bearer": [] }])
        );
    }

    #[test]
    fn renamed_schema_updates_discriminator_mapping() {
        let a = spec_with(
            json!({}),
            json!({ "schemas": { "Dog": { "type": "object" } } }),
        );
        let b = spec_with(
            json!({}),
            json!({ "schemas": {
                "Dog": { "type": "object", "properties": { "bark": { "type": "string" } } },
                "Pet": {
                    "oneOf": [{ "$ref": "#/components/schemas/Dog" }],
                    "discriminator": {
                        "propertyName": "kind",
                        "mapping": { "dog": "#/components/schemas/Dog", "doggo": "Dog" }
                    }
                }
            }}),
        );
        let config = MergeConfig {
            conflict_strategy: ConflictStrategy::Rename,
            ..Default::default()
        };

        let merged = merge_specs(
            vec![("a".into(), "a".into(), a), ("b".into(), "b".into(), b)],
            &config,
        )
        .unwrap();

        let mapping = &merged["components"]["schemas"]["Pet"]["discriminator"]["mapping"];
        assert_eq!(mapping["dog"], "#/components/schemas/b_Dog");
        assert_eq!(mapping["doggo"], "b_Dog");
    }

    #[test]
    fn mixed_openapi_minor_versions_produce_warning() {
        let a = spec_with(json!({}), json!({}));
        let mut b = spec_with(json!({}), json!({}));
        b["openapi"] = json!("3.1.0");
        let mut c = spec_with(json!({}), json!({}));
        c["openapi"] = json!("3.0.1");

        let report = merge_specs_with_report(
            vec![
                ("a".into(), "a".into(), a.clone()),
                ("b".into(), "b".into(), b),
            ],
            &MergeConfig::default(),
        )
        .unwrap();
        assert_eq!(report.spec["openapi"], "3.0.3");
        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].contains("3.1.0"));

        let report = merge_specs_with_report(
            vec![("a".into(), "a".into(), a), ("c".into(), "c".into(), c)],
            &MergeConfig::default(),
        )
        .unwrap();
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn merge_servers_from_sources_when_no_override() {
        let mut a = petstore_spec();
        a["servers"] = json!([{"url": "https://a.example.com"}]);
        let mut b = users_spec();
        b["servers"] = json!([
            {"url": "https://a.example.com"},
            {"url": "https://b.example.com"}
        ]);

        let specs = vec![("a".into(), "a".into(), a), ("b".into(), "b".into(), b)];
        let config = MergeConfig::default();
        let merged = merge_specs(specs, &config).unwrap();

        let servers = merged["servers"].as_array().unwrap();
        // Deduplicated by url
        assert_eq!(servers.len(), 2);
    }
}
