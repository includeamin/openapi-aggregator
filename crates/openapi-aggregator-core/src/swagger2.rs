//! Convert Swagger 2.0 documents to OpenAPI 3.0.3.

use serde_json::{Map, Value};

/// Convert a Swagger 2.0 document to OpenAPI 3.0.3.
///
/// Returns the converted spec and human-readable notes describing anything that was
/// approximated or dropped. Never fails: odd input is passed through.
pub fn convert(spec: Value) -> (Value, Vec<String>) {
    let Value::Object(mut root) = spec else {
        return (spec, Vec::new());
    };
    let mut notes = vec!["converted from Swagger 2.0 to OpenAPI 3.0.3".to_string()];
    let mut components = Map::new();

    root.remove("swagger");
    let mut out = Map::new();
    out.insert("openapi".into(), Value::String("3.0.3".into()));

    if let Some(servers) = build_servers(&mut root) {
        notes.push("host/basePath/schemes moved into servers".into());
        out.insert("servers".into(), servers);
    }

    if let Some(Value::Object(mut defs)) = root.remove("definitions") {
        for schema in defs.values_mut() {
            fix_schema(schema);
        }
        components.insert("schemas".into(), Value::Object(defs));
    }
    if let Some(responses @ Value::Object(_)) = root.remove("responses") {
        components.insert("responses".into(), responses);
    }

    for (key, value) in root {
        out.insert(key, value);
    }
    if !components.is_empty() {
        out.insert("components".into(), Value::Object(components));
    }

    let mut out = Value::Object(out);
    rewrite_refs(&mut out);
    (out, notes)
}

/// Build `servers` from `host`, `basePath` and `schemes`, removing those keys.
fn build_servers(root: &mut Map<String, Value>) -> Option<Value> {
    let host = root.remove("host");
    let base_path = root.remove("basePath");
    let schemes = root.remove("schemes");
    let host = host
        .as_ref()
        .and_then(Value::as_str)
        .filter(|h| !h.is_empty());
    let base = base_path.as_ref().and_then(Value::as_str).unwrap_or("");
    let urls: Vec<String> = match host {
        Some(host) => {
            let schemes: Vec<&str> = schemes
                .as_ref()
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).collect())
                .filter(|v: &Vec<&str>| !v.is_empty())
                .unwrap_or_else(|| vec!["https"]);
            schemes
                .iter()
                .map(|s| format!("{s}://{host}{base}"))
                .collect()
        }
        None if !base.is_empty() => vec![base.to_string()],
        None => return None,
    };
    Some(Value::Array(
        urls.into_iter()
            .map(|url| serde_json::json!({ "url": url }))
            .collect(),
    ))
}

/// Apply Swagger 2.0 -> 3.0 schema keyword fixes, recursing through schema-structural keys only.
fn fix_schema(schema: &mut Value) {
    let Value::Object(map) = schema else { return };
    if map.get("type").and_then(Value::as_str) == Some("file") {
        map.insert("type".into(), "string".into());
        map.insert("format".into(), "binary".into());
    }
    if map.get("x-nullable") == Some(&Value::Bool(true)) {
        map.remove("x-nullable");
        map.insert("nullable".into(), Value::Bool(true));
    }
    if let Some(Value::String(name)) = map.get("discriminator") {
        let name = name.clone();
        map.insert(
            "discriminator".into(),
            serde_json::json!({ "propertyName": name }),
        );
    }
    if let Some(Value::Object(props)) = map.get_mut("properties") {
        props.values_mut().for_each(fix_schema);
    }
    for key in ["items", "additionalProperties", "not"] {
        if let Some(v) = map.get_mut(key) {
            fix_schema(v);
        }
    }
    for key in ["allOf", "anyOf", "oneOf"] {
        if let Some(Value::Array(items)) = map.get_mut(key) {
            items.iter_mut().for_each(fix_schema);
        }
    }
}

/// Rewrite Swagger 2.0 `$ref` targets (and `discriminator.mapping` values) to their
/// OpenAPI 3 locations, anywhere in the document.
pub(crate) fn rewrite_refs(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, v) in map.iter_mut() {
                match v {
                    Value::String(s) if key == "$ref" => *s = rewrite_ref(s),
                    Value::Object(mapping) if key == "mapping" => {
                        for target in mapping.values_mut() {
                            if let Value::String(s) = target {
                                *s = rewrite_ref(s);
                            }
                        }
                    }
                    _ => rewrite_refs(v),
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(rewrite_refs),
        _ => {}
    }
}

fn rewrite_ref(target: &str) -> String {
    for (old, new) in [
        ("#/definitions/", "#/components/schemas/"),
        ("#/parameters/", "#/components/parameters/"),
        ("#/responses/", "#/components/responses/"),
    ] {
        if let Some(rest) = target.strip_prefix(old) {
            return format!("{new}{rest}");
        }
    }
    target.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn conv(v: Value) -> (Value, Vec<String>) {
        convert(v)
    }

    #[test]
    fn converts_version_and_info() {
        let (out, notes) =
            conv(json!({"swagger":"2.0","info":{"title":"T","version":"1"},"paths":{}}));
        assert_eq!(out["openapi"], "3.0.3");
        assert_eq!(out["info"], json!({"title":"T","version":"1"}));
        assert!(out.get("swagger").is_none());
        assert!(notes.contains(&"converted from Swagger 2.0 to OpenAPI 3.0.3".to_string()));
    }

    #[test]
    fn builds_servers_from_host_basepath_schemes() {
        let (out, notes) = conv(
            json!({"swagger":"2.0","host":"api.test","basePath":"/v1","schemes":["https","http"]}),
        );
        assert_eq!(
            out["servers"],
            json!([{"url":"https://api.test/v1"},{"url":"http://api.test/v1"}])
        );
        assert!(
            out.get("host").is_none()
                && out.get("basePath").is_none()
                && out.get("schemes").is_none()
        );
        assert!(notes.iter().any(|n| n.contains("servers")));

        let (out, _) = conv(json!({"swagger":"2.0","host":"api.test"}));
        assert_eq!(out["servers"], json!([{"url":"https://api.test"}]));
        let (out, _) = conv(json!({"swagger":"2.0","basePath":"/v1"}));
        assert_eq!(out["servers"], json!([{"url":"/v1"}]));
        let (out, notes) = conv(json!({"swagger":"2.0"}));
        assert!(out.get("servers").is_none());
        assert!(!notes.iter().any(|n| n.contains("servers")));
    }

    #[test]
    fn moves_definitions_to_components_schemas_and_rewrites_refs() {
        let (out, _) = conv(json!({
            "swagger":"2.0",
            "definitions":{
                "Pet":{"type":"object","properties":{
                    "friends":{"type":"array","items":{"$ref":"#/definitions/Pet"}},
                    "owner":{"allOf":[{"$ref":"#/definitions/Owner"}]},
                    "meta":{"type":"object","additionalProperties":{"$ref":"#/definitions/Owner"}}}},
                "Owner":{"type":"object","discriminator":"kind","properties":{"kind":{"type":"string"}}}
            },
            "paths":{"/pets":{"get":{"responses":{"200":{"description":"ok","schema":{"$ref":"#/definitions/Pet"}}}}}}
        }));
        assert!(out.get("definitions").is_none());
        let pet = &out["components"]["schemas"]["Pet"]["properties"];
        assert_eq!(pet["friends"]["items"]["$ref"], "#/components/schemas/Pet");
        assert_eq!(
            pet["owner"]["allOf"][0]["$ref"],
            "#/components/schemas/Owner"
        );
        assert_eq!(
            pet["meta"]["additionalProperties"]["$ref"],
            "#/components/schemas/Owner"
        );
        assert!(!out.to_string().contains("#/definitions/"));
    }

    #[test]
    fn discriminator_mapping_refs_are_rewritten() {
        let (out, _) = conv(json!({"swagger":"2.0","definitions":{
            "A":{"discriminator":{"propertyName":"k","mapping":{"b":"#/definitions/B"}}},"B":{}}}));
        assert_eq!(
            out["components"]["schemas"]["A"]["discriminator"]["mapping"]["b"],
            "#/components/schemas/B"
        );
    }

    #[test]
    fn schema_fixups() {
        let (out, _) = conv(json!({"swagger":"2.0","definitions":{
            "F":{"type":"file"},
            "N":{"type":"string","x-nullable":true},
            "D":{"type":"object","discriminator":"kind"},
            "P":{"type":"object","properties":{"f":{"type":"file"}}}}}));
        let s = &out["components"]["schemas"];
        assert_eq!(s["F"], json!({"type":"string","format":"binary"}));
        assert_eq!(s["N"], json!({"type":"string","nullable":true}));
        assert_eq!(s["D"]["discriminator"], json!({"propertyName":"kind"}));
        assert_eq!(
            s["P"]["properties"]["f"],
            json!({"type":"string","format":"binary"})
        );
    }

    #[test]
    fn tolerates_empty_and_odd_input() {
        let (out, _) = conv(json!({"swagger":"2.0"}));
        assert_eq!(out["openapi"], "3.0.3");
        let _ =
            conv(json!({"swagger":"2.0","paths":[],"definitions":5,"host":7,"schemes":"https"}));
        let _ = conv(json!("not an object"));
    }
}
