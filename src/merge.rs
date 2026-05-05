use std::collections::HashMap;

use serde_json::{Map, Value};

use crate::config::{ConflictStrategy, InfoOverride, MergeConfig, TagPrefixStrategy};
use crate::error::Error;

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
];

/// Merge multiple named OpenAPI specs into one according to `config`.
///
/// Each entry is `(source_name, tag_prefix, spec_value)`.
pub fn merge_specs(
    specs: Vec<(String, String, Value)>,
    config: &MergeConfig,
) -> Result<Value, Error> {
    if specs.is_empty() {
        return Err(Error::NoSources);
    }

    let mut merged = Map::new();

    // --- openapi version (from first source) ---
    if let Some(version) = specs[0].2.get("openapi") {
        merged.insert("openapi".into(), version.clone());
    }

    // --- info ---
    let info = build_info(&specs, config.info.as_ref());
    merged.insert("info".into(), info);

    // --- paths & components (incremental merge) ---
    let mut merged_paths = Map::new();
    let mut merged_components: HashMap<String, Map<String, Value>> = HashMap::new();
    let mut merged_tags: Vec<Value> = Vec::new();
    let mut merged_servers: Vec<Value> = Vec::new();
    let mut merged_custom_top_level = Map::new();

    for (source_name, tag_prefix, mut spec) in specs {
        // Phase 1: detect component conflicts and build a $ref rename map
        let rename_map = build_rename_map(&source_name, &spec, &merged_components, config)?;

        // Phase 2: rewrite $refs in the source spec if needed
        if !rename_map.is_empty() {
            rewrite_refs(&mut spec, &rename_map);
        }

        // Phase 2b: rewrite tag references in operations before merging paths
        if config.tag_prefix == TagPrefixStrategy::SourceName {
            rewrite_spec_operation_tags(&mut spec, &tag_prefix, config);
        }

        // Phase 3a: merge paths
        merge_paths(&source_name, &spec, &mut merged_paths, config)?;

        // Phase 3b: merge components
        merge_components(
            &source_name,
            &spec,
            &mut merged_components,
            &rename_map,
            config,
        )?;

        // Phase 3c: merge tags
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

        // Phase 3d: merge servers (deduplicate by url)
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

        // Phase 3e: merge non-standard top-level blocks (e.g. vendor extensions)
        if let Some(root) = spec.as_object() {
            for (key, value) in root {
                if is_reserved_top_level_key(key) {
                    continue;
                }
                if let Some(existing) = merged_custom_top_level.get_mut(key) {
                    deep_merge_value(existing, value);
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

    if !merged_components.is_empty() {
        let mut comp_obj = Map::new();
        for (ctype, items) in merged_components {
            comp_obj.insert(ctype, Value::Object(items));
        }
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

    Ok(Value::Object(merged))
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn build_info(specs: &[(String, String, Value)], info_override: Option<&InfoOverride>) -> Value {
    let mut info = specs[0]
        .2
        .get("info")
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()));

    if let Some(ov) = info_override {
        let obj = info.as_object_mut().expect("info must be object");
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

    info
}

fn is_reserved_top_level_key(key: &str) -> bool {
    matches!(
        key,
        "openapi" | "info" | "paths" | "components" | "tags" | "servers"
    )
}

fn deep_merge_value(target: &mut Value, patch: &Value) {
    match (target, patch) {
        (Value::Object(target_map), Value::Object(patch_map)) => {
            for (key, patch_value) in patch_map {
                if let Some(existing) = target_map.get_mut(key) {
                    deep_merge_value(existing, patch_value);
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
    let http_methods = [
        "get", "post", "put", "patch", "delete", "options", "head", "trace",
    ];

    if let Some(Value::Object(paths)) = spec.get_mut("paths") {
        for (_path_key, path_item) in paths.iter_mut() {
            if let Some(obj) = path_item.as_object_mut() {
                for method in &http_methods {
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

/// For the rename strategy: figure out which component names in `spec` clash
/// with already-merged names and return a map from old `$ref` → new `$ref`.
fn build_rename_map(
    source_name: &str,
    spec: &Value,
    merged_components: &HashMap<String, Map<String, Value>>,
    config: &MergeConfig,
) -> Result<HashMap<String, String>, Error> {
    let mut rename_map = HashMap::new();

    if config.conflict_strategy != ConflictStrategy::Rename {
        return Ok(rename_map);
    }

    for &ctype in COMPONENT_TYPES {
        if let Some(Value::Object(items)) = spec.get("components").and_then(|c| c.get(ctype)) {
            if let Some(existing) = merged_components.get(ctype) {
                for item_name in items.keys() {
                    if existing.contains_key(item_name) {
                        let old_ref = format!("#/components/{ctype}/{item_name}");
                        let new_name = format!("{source_name}_{item_name}");
                        let new_ref = format!("#/components/{ctype}/{new_name}");
                        rename_map.insert(old_ref, new_ref);
                    }
                }
            }
        }
    }

    Ok(rename_map)
}

/// Walk the JSON tree and rewrite any `$ref` value found in `rename_map`.
fn rewrite_refs(value: &mut Value, rename_map: &HashMap<String, String>) {
    match value {
        Value::Object(map) => {
            // Check for $ref to rewrite
            let new_ref = map
                .get("$ref")
                .and_then(|v| v.as_str())
                .and_then(|s| rename_map.get(s))
                .cloned();

            if let Some(new_ref) = new_ref {
                map.insert("$ref".into(), Value::String(new_ref));
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

fn merge_paths(
    source_name: &str,
    spec: &Value,
    merged: &mut Map<String, Value>,
    config: &MergeConfig,
) -> Result<(), Error> {
    let spec_paths = match spec.get("paths").and_then(|p| p.as_object()) {
        Some(p) => p,
        None => return Ok(()),
    };

    for (path, operations) in spec_paths {
        let key = if config.prefix_paths {
            format!("/{source_name}{path}")
        } else {
            path.clone()
        };

        if merged.contains_key(&key) {
            match config.conflict_strategy {
                ConflictStrategy::Error => {
                    return Err(Error::MergeConflict(format!(
                        "duplicate path '{key}' from source '{source_name}'"
                    )));
                }
                ConflictStrategy::Overwrite => {
                    merged.insert(key, operations.clone());
                }
                ConflictStrategy::Rename => {
                    let prefixed = format!("/{source_name}{path}");
                    merged.insert(prefixed, operations.clone());
                }
            }
        } else {
            merged.insert(key, operations.clone());
        }
    }

    Ok(())
}

fn merge_components(
    source_name: &str,
    spec: &Value,
    merged: &mut HashMap<String, Map<String, Value>>,
    rename_map: &HashMap<String, String>,
    config: &MergeConfig,
) -> Result<(), Error> {
    for &ctype in COMPONENT_TYPES {
        let items = match spec
            .get("components")
            .and_then(|c| c.get(ctype))
            .and_then(|v| v.as_object())
        {
            Some(items) => items,
            None => continue,
        };

        let merged_type = merged.entry(ctype.into()).or_default();

        for (item_name, item_value) in items {
            // Determine the key to insert under (may have been renamed)
            let old_ref = format!("#/components/{ctype}/{item_name}");
            let insert_name = if let Some(new_ref) = rename_map.get(&old_ref) {
                // Extract the new component name from the new $ref
                new_ref.rsplit('/').next().unwrap_or(item_name).to_string()
            } else {
                item_name.clone()
            };

            if merged_type.contains_key(&insert_name) && rename_map.get(&old_ref).is_none() {
                match config.conflict_strategy {
                    ConflictStrategy::Error => {
                        return Err(Error::MergeConflict(format!(
                            "duplicate component {ctype}/{item_name} from source '{source_name}'"
                        )));
                    }
                    ConflictStrategy::Overwrite => {
                        merged_type.insert(insert_name, item_value.clone());
                    }
                    ConflictStrategy::Rename => {
                        // Already handled via rename_map – should not reach here
                        merged_type.insert(insert_name, item_value.clone());
                    }
                }
            } else {
                merged_type.insert(insert_name, item_value.clone());
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
        let specs = vec![
            ("a".into(), "a".into(), petstore_spec()),
            ("b".into(), "b".into(), petstore_spec()),
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
