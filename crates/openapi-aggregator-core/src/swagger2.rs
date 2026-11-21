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
    let top_responses = root.remove("responses");
    if let Some(Value::Object(schemes)) = root.remove("securityDefinitions") {
        let schemes = schemes
            .into_iter()
            .map(|(name, scheme)| (name, convert_security_scheme(scheme, &mut notes)))
            .collect();
        components.insert("securitySchemes".into(), Value::Object(schemes));
    }

    let root_consumes = string_list(root.remove("consumes"));
    let root_produces = string_list(root.remove("produces"));

    let top_params = match root.remove("parameters") {
        Some(Value::Object(m)) => m,
        _ => Map::new(),
    };
    let mut ctx = Ctx {
        top_params: &top_params,
        root_consumes: &root_consumes,
        root_produces: &root_produces,
        notes: &mut notes,
    };
    if let Some(Value::Object(responses)) = top_responses {
        let converted = responses
            .into_iter()
            .map(|(name, r)| (name, convert_response(r, ctx.root_produces)))
            .collect();
        components.insert("responses".into(), Value::Object(converted));
    }
    let (params, bodies) = ctx.convert_top_level_parameters();
    if !params.is_empty() {
        components.insert("parameters".into(), Value::Object(params));
    }
    if !bodies.is_empty() {
        components.insert("requestBodies".into(), Value::Object(bodies));
    }
    if let Some(Value::Object(paths)) = root.get_mut("paths") {
        for (path, item) in paths.iter_mut() {
            ctx.convert_path_item(path, item);
        }
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

pub(crate) const METHODS: [&str; 8] = [
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];

/// Keys of a Swagger 2.0 non-body parameter that belong in the 3.0 `schema`.
const SCHEMA_KEYS: [&str; 14] = [
    "type",
    "format",
    "items",
    "enum",
    "default",
    "minimum",
    "maximum",
    "exclusiveMinimum",
    "exclusiveMaximum",
    "minLength",
    "maxLength",
    "pattern",
    "minItems",
    "maxItems",
];

fn string_list(value: Option<Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items
            .into_iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        _ => Vec::new(),
    }
}

/// A classified Swagger 2.0 parameter.
enum Param {
    /// Non-body, non-form: stays a parameter (already converted or a `$ref`).
    Plain(Value),
    /// `in: body`: the finished 3.0 `requestBody`.
    Body(Value),
    /// `in: formData`: the raw 2.0 parameter.
    Form(Map<String, Value>),
}

struct Ctx<'a> {
    top_params: &'a Map<String, Value>,
    root_consumes: &'a [String],
    root_produces: &'a [String],
    notes: &'a mut Vec<String>,
}

impl Ctx<'_> {
    fn convert_top_level_parameters(&mut self) -> (Map<String, Value>, Map<String, Value>) {
        let mut params = Map::new();
        let mut bodies = Map::new();
        for (name, raw) in self.top_params {
            match self.classify(raw.clone(), &[], "") {
                Param::Plain(p) => {
                    params.insert(name.clone(), p);
                }
                Param::Body(b) => {
                    bodies.insert(name.clone(), b);
                }
                Param::Form(_) => {}
            }
        }
        (params, bodies)
    }

    fn classify(&mut self, raw: Value, consumes: &[String], ctx_label: &str) -> Param {
        let Value::Object(mut map) = raw else {
            return Param::Plain(raw);
        };
        if let Some(Value::String(target)) = map.get("$ref") {
            let referenced = target
                .strip_prefix("#/parameters/")
                .and_then(|name| self.top_params.get(name).map(|p| (name.to_string(), p)));
            if let Some((name, Value::Object(p))) = referenced {
                match p.get("in").and_then(Value::as_str) {
                    Some("body") => {
                        return Param::Body(
                            serde_json::json!({"$ref": format!("#/components/requestBodies/{name}")}),
                        )
                    }
                    Some("formData") => return Param::Form(p.clone()),
                    _ => {}
                }
            }
            return Param::Plain(Value::Object(map));
        }
        match map.get("in").and_then(Value::as_str) {
            Some("body") => {
                let consumes = if consumes.is_empty() {
                    self.root_consumes
                } else {
                    consumes
                };
                Param::Body(build_request_body(map, consumes))
            }
            Some("formData") => Param::Form(map),
            _ => {
                self.convert_plain_parameter(&mut map, ctx_label);
                Param::Plain(Value::Object(map))
            }
        }
    }

    /// Move schema-ish keys into `schema` and map `collectionFormat` to `style`/`explode`.
    fn convert_plain_parameter(&mut self, param: &mut Map<String, Value>, ctx_label: &str) {
        let mut schema = take_schema_keys(param);
        self.strip_nested_collection_format(&mut schema, ctx_label);
        let location = param
            .get("in")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let collection = param
            .remove("collectionFormat")
            .and_then(|v| v.as_str().map(String::from));
        if schema.get("type").and_then(Value::as_str) == Some("array") {
            let format = collection.as_deref().unwrap_or("csv");
            let style = match (format, location.as_str()) {
                ("csv", "query" | "cookie") => Some(("form", false)),
                ("csv", "path" | "header") => Some(("simple", false)),
                ("multi", "query") => Some(("form", true)),
                ("ssv", "query") => Some(("spaceDelimited", false)),
                ("pipes", "query") => Some(("pipeDelimited", false)),
                _ => None,
            };
            match style {
                Some((style, explode)) => {
                    param.insert("style".into(), style.into());
                    param.insert("explode".into(), explode.into());
                }
                None => self.notes.push(format!(
                    "collectionFormat '{format}' is not supported in OpenAPI 3; dropped"
                )),
            }
        }
        let mut schema = Value::Object(schema);
        fix_schema(&mut schema);
        param.insert("schema".into(), schema);
    }

    fn strip_nested_collection_format(&mut self, schema: &mut Map<String, Value>, ctx_label: &str) {
        let mut current = schema.get_mut("items");
        while let Some(Value::Object(items)) = current {
            if items.remove("collectionFormat").is_some() {
                self.notes.push(format!(
                    "collectionFormat on nested array items{ctx_label} is not supported in OpenAPI 3; dropped"
                ));
            }
            current = items.get_mut("items");
        }
    }

    fn convert_path_item(&mut self, path: &str, item: &mut Value) {
        let Value::Object(item) = item else { return };

        let path_params: Vec<Param> = match item.remove("parameters") {
            Some(Value::Array(list)) => list
                .into_iter()
                .map(|p| self.classify(p, &[], &format!(" on {path}")))
                .collect(),
            Some(other) => {
                item.insert("parameters".into(), other);
                Vec::new()
            }
            None => Vec::new(),
        };
        let mut shared_plain = Vec::new();
        let mut shared_body_form = Vec::new();
        for p in path_params {
            match p {
                Param::Plain(v) => shared_plain.push(v),
                other => shared_body_form.push(other),
            }
        }
        if !shared_plain.is_empty() {
            item.insert("parameters".into(), Value::Array(shared_plain));
        }

        for method in METHODS {
            if let Some(Value::Object(op)) = item.get_mut(method) {
                let label = format!("{} {path}", method.to_uppercase());
                self.convert_operation(&label, op, &shared_body_form);
            }
        }
    }

    fn convert_operation(
        &mut self,
        label: &str,
        op: &mut Map<String, Value>,
        shared_body_form: &[Param],
    ) {
        let consumes = string_list(op.remove("consumes"));
        let produces = match op.remove("produces") {
            Some(v) => string_list(Some(v)),
            None => self.root_produces.to_vec(),
        };
        if let Some(Value::Object(responses)) = op.get_mut("responses") {
            for response in responses.values_mut() {
                *response = convert_response(response.take(), &produces);
            }
        }
        let own: Vec<Param> = match op.remove("parameters") {
            Some(Value::Array(list)) => list
                .into_iter()
                .map(|p| self.classify(p, &consumes, &format!(" on {label}")))
                .collect(),
            Some(other) => {
                op.insert("parameters".into(), other);
                Vec::new()
            }
            None => Vec::new(),
        };
        let mut plain = Vec::new();
        let mut bodies = Vec::new();
        let mut forms = Vec::new();
        for p in own {
            match p {
                Param::Plain(v) => plain.push(v),
                Param::Body(b) => bodies.push(b),
                Param::Form(f) => forms.push(f),
            }
        }
        if bodies.is_empty() && forms.is_empty() {
            for p in shared_body_form {
                match p {
                    Param::Body(b) => bodies.push(b.clone()),
                    Param::Form(f) => forms.push(f.clone()),
                    Param::Plain(_) => {}
                }
            }
        }
        if !plain.is_empty() {
            op.insert("parameters".into(), Value::Array(plain));
        }
        if bodies.len() > 1 {
            self.notes.push(format!(
                "multiple 'in: body' parameters on {label}; kept the first"
            ));
        }
        if !bodies.is_empty() && !forms.is_empty() {
            self.notes.push(format!(
                "'in: body' and 'in: formData' parameters on {label}; kept the body"
            ));
        }
        let request_body = if let Some(body) = bodies.into_iter().next() {
            Some(body)
        } else if !forms.is_empty() {
            let consumes = if consumes.is_empty() {
                self.root_consumes
            } else {
                &consumes
            };
            let (body, media) = build_form_body(forms, consumes);
            self.notes.push(format!(
                "formData parameters on {label} converted to {media} request body"
            ));
            Some(body)
        } else {
            None
        };
        if let Some(body) = request_body {
            op.insert("requestBody".into(), body);
        }
    }
}

/// Remove the schema-related keys from a parameter and return them as a schema map.
fn take_schema_keys(param: &mut Map<String, Value>) -> Map<String, Value> {
    let mut schema = Map::new();
    for key in SCHEMA_KEYS {
        if let Some(v) = param.remove(key) {
            schema.insert(key.into(), v);
        }
    }
    schema
}

fn media_types(consumes: &[String]) -> Vec<String> {
    if consumes.is_empty() {
        vec!["application/json".to_string()]
    } else {
        consumes.to_vec()
    }
}

fn build_request_body(mut param: Map<String, Value>, consumes: &[String]) -> Value {
    let mut schema = param
        .remove("schema")
        .unwrap_or_else(|| serde_json::json!({}));
    fix_schema(&mut schema);
    let content: Map<String, Value> = media_types(consumes)
        .into_iter()
        .map(|m| (m, serde_json::json!({ "schema": schema.clone() })))
        .collect();
    let mut body = Map::new();
    for key in ["description", "required"] {
        if let Some(v) = param.remove(key) {
            body.insert(key.into(), v);
        }
    }
    body.insert("content".into(), Value::Object(content));
    Value::Object(body)
}

/// Build a form request body; returns it with the media type used.
fn build_form_body(forms: Vec<Map<String, Value>>, consumes: &[String]) -> (Value, &'static str) {
    let has_file = forms
        .iter()
        .any(|f| f.get("type").and_then(Value::as_str) == Some("file"));
    let multipart = has_file || consumes.iter().any(|c| c == "multipart/form-data");
    let media = if multipart {
        "multipart/form-data"
    } else {
        "application/x-www-form-urlencoded"
    };
    let mut properties = Map::new();
    let mut required = Vec::new();
    for mut form in forms {
        let Some(name) = form.get("name").and_then(Value::as_str).map(String::from) else {
            continue;
        };
        if form.get("required") == Some(&Value::Bool(true)) {
            required.push(Value::String(name.clone()));
        }
        let mut schema = take_schema_keys(&mut form);
        if let Some(Value::String(d)) = form.remove("description") {
            schema.insert("description".into(), Value::String(d));
        }
        form.remove("collectionFormat");
        let mut schema = Value::Object(schema);
        fix_schema(&mut schema);
        properties.insert(name, schema);
    }
    let mut schema = serde_json::json!({"type": "object", "properties": properties});
    let has_required = !required.is_empty();
    if has_required {
        schema["required"] = Value::Array(required);
    }
    let mut body = serde_json::json!({"content": {media: {"schema": schema}}});
    if has_required {
        body["required"] = Value::Bool(true);
    }
    (body, media)
}

/// Convert a Swagger 2.0 response object (`schema`, `headers`, `examples`) to 3.0.
fn convert_response(response: Value, produces: &[String]) -> Value {
    let Value::Object(mut map) = response else {
        return response;
    };
    if map.contains_key("$ref") {
        return Value::Object(map);
    }
    let mut content: Map<String, Value> = Map::new();
    if let Some(mut schema) = map.remove("schema") {
        fix_schema(&mut schema);
        for media in media_types(produces) {
            content.insert(media, serde_json::json!({ "schema": schema.clone() }));
        }
    }
    if let Some(Value::Object(examples)) = map.remove("examples") {
        for (media, example) in examples {
            let entry = content
                .entry(media)
                .or_insert_with(|| Value::Object(Map::new()));
            if let Value::Object(entry) = entry {
                entry.insert("example".into(), example);
            }
        }
    }
    if !content.is_empty() {
        map.insert("content".into(), Value::Object(content));
    }
    if let Some(Value::Object(headers)) = map.get_mut("headers") {
        for header in headers.values_mut() {
            if let Value::Object(h) = header {
                let mut schema = Value::Object(take_schema_keys(h));
                h.remove("collectionFormat");
                fix_schema(&mut schema);
                h.insert("schema".into(), schema);
            }
        }
    }
    Value::Object(map)
}

fn convert_security_scheme(scheme: Value, notes: &mut Vec<String>) -> Value {
    let Value::Object(mut map) = scheme else {
        return scheme;
    };
    match map.get("type").and_then(Value::as_str) {
        Some("basic") => {
            map.insert("type".into(), "http".into());
            map.insert("scheme".into(), "basic".into());
        }
        Some("apiKey") => {}
        Some("oauth2") => {
            let flow = map
                .remove("flow")
                .and_then(|v| v.as_str().map(String::from));
            let (target, keys): (&str, &[&str]) = match flow.as_deref() {
                Some("implicit") => ("implicit", &["authorizationUrl"]),
                Some("password") => ("password", &["tokenUrl"]),
                Some("application") => ("clientCredentials", &["tokenUrl"]),
                Some("accessCode") => ("authorizationCode", &["authorizationUrl", "tokenUrl"]),
                other => {
                    notes.push(format!(
                        "unknown OAuth2 flow '{}' passed through",
                        other.unwrap_or("")
                    ));
                    return Value::Object(map);
                }
            };
            let mut flow_obj = Map::new();
            for key in keys {
                if let Some(v) = map.remove(*key) {
                    flow_obj.insert((*key).into(), v);
                }
            }
            let scopes = map
                .remove("scopes")
                .unwrap_or_else(|| Value::Object(Map::new()));
            flow_obj.insert("scopes".into(), scopes);
            map.insert("flows".into(), serde_json::json!({ target: flow_obj }));
        }
        other => notes.push(format!(
            "unknown security scheme type '{}' passed through",
            other.unwrap_or("")
        )),
    }
    Value::Object(map)
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

    fn op(v: Value) -> Value {
        conv(json!({"swagger":"2.0","paths":{"/pets":{"post":v}}})).0["paths"]["/pets"]["post"]
            .clone()
    }

    #[test]
    fn non_body_param_gets_schema() {
        let o = op(json!({"parameters":[
            {"name":"limit","in":"query","type":"integer","format":"int32","minimum":1,"default":10,"description":"d","required":false},
            {"name":"tags","in":"query","type":"array","items":{"type":"string","enum":["a","b"]},"collectionFormat":"multi"}
        ]}));
        assert_eq!(
            o["parameters"][0],
            json!({"name":"limit","in":"query","description":"d","required":false,
            "schema":{"type":"integer","format":"int32","minimum":1,"default":10}})
        );
        assert_eq!(
            o["parameters"][1]["schema"],
            json!({"type":"array","items":{"type":"string","enum":["a","b"]}})
        );
    }

    #[test]
    fn collection_format_maps_to_style() {
        let style = |cf: Option<&str>, loc: &str| {
            let mut p = json!({"name":"x","in":loc,"type":"array","items":{"type":"string"}});
            if let Some(cf) = cf {
                p["collectionFormat"] = json!(cf);
            }
            let (out, notes) =
                conv(json!({"swagger":"2.0","paths":{"/p":{"get":{"parameters":[p]}}}}));
            (out["paths"]["/p"]["get"]["parameters"][0].clone(), notes)
        };
        let (p, _) = style(Some("csv"), "query");
        assert_eq!(
            (&p["style"], &p["explode"]),
            (&json!("form"), &json!(false))
        );
        let (p, _) = style(None, "query");
        assert_eq!(
            (&p["style"], &p["explode"]),
            (&json!("form"), &json!(false))
        );
        let (p, _) = style(Some("multi"), "query");
        assert_eq!((&p["style"], &p["explode"]), (&json!("form"), &json!(true)));
        assert_eq!(style(Some("pipes"), "query").0["style"], "pipeDelimited");
        assert_eq!(style(Some("ssv"), "query").0["style"], "spaceDelimited");
        assert_eq!(style(Some("csv"), "path").0["style"], "simple");
        assert_eq!(style(Some("csv"), "header").0["style"], "simple");
        let (p, notes) = style(Some("tsv"), "query");
        assert!(p.get("style").is_none() && p.get("collectionFormat").is_none());
        assert!(notes
            .iter()
            .any(|n| n.contains("collectionFormat 'tsv' is not supported in OpenAPI 3; dropped")));
    }

    #[test]
    fn body_param_becomes_request_body() {
        let body = json!({"in":"body","name":"b","required":true,"description":"d","schema":{"$ref":"#/definitions/Pet"}});
        let o = op(json!({"consumes":["application/xml"],"parameters":[body.clone()]}));
        assert_eq!(
            o["requestBody"],
            json!({"description":"d","required":true,
            "content":{"application/xml":{"schema":{"$ref":"#/components/schemas/Pet"}}}})
        );
        assert!(o.get("parameters").is_none() && o.get("consumes").is_none());

        let o = op(json!({"parameters":[body.clone()]}));
        assert!(o["requestBody"]["content"]
            .get("application/json")
            .is_some());

        let (out, _) = conv(
            json!({"swagger":"2.0","consumes":["text/plain"],"paths":{"/p":{
            "post":{"parameters":[body.clone()]},
            "put":{"consumes":["application/xml"],"parameters":[body]}}}}),
        );
        assert!(out["paths"]["/p"]["post"]["requestBody"]["content"]
            .get("text/plain")
            .is_some());
        assert!(out["paths"]["/p"]["put"]["requestBody"]["content"]
            .get("application/xml")
            .is_some());
        assert!(out.get("consumes").is_none());
    }

    #[test]
    fn form_data_becomes_form_body() {
        let o = op(json!({"parameters":[
            {"name":"a","in":"formData","type":"string","required":true},
            {"name":"b","in":"formData","type":"integer"}]}));
        let c = &o["requestBody"]["content"]["application/x-www-form-urlencoded"]["schema"];
        assert_eq!(c["type"], "object");
        assert_eq!(c["properties"]["a"], json!({"type":"string"}));
        assert_eq!(c["properties"]["b"], json!({"type":"integer"}));
        assert_eq!(c["required"], json!(["a"]));

        let (out, notes) = conv(
            json!({"swagger":"2.0","paths":{"/pets":{"post":{"parameters":[
            {"name":"f","in":"formData","type":"file"}]}}}}),
        );
        let o = &out["paths"]["/pets"]["post"];
        assert_eq!(
            o["requestBody"]["content"]["multipart/form-data"]["schema"]["properties"]["f"],
            json!({"type":"string","format":"binary"})
        );
        assert!(notes.iter().any(|n| n.contains(
            "formData parameters on POST /pets converted to multipart/form-data request body"
        )));
    }

    #[test]
    fn multiple_bodies_keep_first_with_note() {
        let (out, notes) = conv(
            json!({"swagger":"2.0","paths":{"/pets":{"post":{"parameters":[
            {"in":"body","name":"a","schema":{"type":"string"}},
            {"in":"body","name":"b","schema":{"type":"integer"}}]}}}}),
        );
        let s =
            &out["paths"]["/pets"]["post"]["requestBody"]["content"]["application/json"]["schema"];
        assert_eq!(s["type"], "string");
        assert!(notes
            .iter()
            .any(|n| n.contains("multiple 'in: body' parameters on POST /pets; kept the first")));
    }

    #[test]
    fn body_and_form_keep_body_with_note() {
        let (out, notes) = conv(
            json!({"swagger":"2.0","paths":{"/pets":{"post":{"parameters":[
            {"in":"body","name":"a","schema":{"type":"string"}},
            {"in":"formData","name":"f","type":"string"}]}}}}),
        );
        assert!(out["paths"]["/pets"]["post"]["requestBody"]["content"]
            .get("application/json")
            .is_some());
        assert!(notes.iter().any(
            |n| n.contains("'in: body' and 'in: formData' parameters on POST /pets; kept the body")
        ));
    }

    #[test]
    fn top_level_parameters_split() {
        let (out, _) = conv(json!({"swagger":"2.0",
            "parameters":{
                "Body":{"in":"body","name":"b","required":true,"schema":{"$ref":"#/definitions/Pet"}},
                "Limit":{"in":"query","name":"limit","type":"integer"}},
            "definitions":{"Pet":{}},
            "paths":{"/p":{"post":{"parameters":[{"$ref":"#/parameters/Body"},{"$ref":"#/parameters/Limit"}]}}}}));
        assert!(out.get("parameters").is_none());
        assert_eq!(
            out["components"]["parameters"]["Limit"]["schema"],
            json!({"type":"integer"})
        );
        assert!(out["components"]["parameters"].get("Body").is_none());
        assert_eq!(
            out["components"]["requestBodies"]["Body"]["content"]["application/json"]["schema"]
                ["$ref"],
            "#/components/schemas/Pet"
        );
        let o = &out["paths"]["/p"]["post"];
        assert_eq!(
            o["requestBody"],
            json!({"$ref":"#/components/requestBodies/Body"})
        );
        assert_eq!(
            o["parameters"],
            json!([{"$ref":"#/components/parameters/Limit"}])
        );
    }

    #[test]
    fn path_item_level_parameters_converted() {
        let (out, _) = conv(json!({"swagger":"2.0","paths":{"/p/{id}":{
            "parameters":[{"name":"id","in":"path","required":true,"type":"string"},
                          {"in":"body","name":"b","schema":{"type":"string"}}],
            "get":{},
            "post":{"parameters":[{"in":"body","name":"own","schema":{"type":"integer"}}]}}}}));
        let item = &out["paths"]["/p/{id}"];
        assert_eq!(
            item["parameters"],
            json!([{"name":"id","in":"path","required":true,"schema":{"type":"string"}}])
        );
        assert_eq!(
            item["get"]["requestBody"]["content"]["application/json"]["schema"]["type"],
            "string"
        );
        assert_eq!(
            item["post"]["requestBody"]["content"]["application/json"]["schema"]["type"],
            "integer"
        );
    }

    fn resp_of(root: Value, op: Value) -> Value {
        let mut spec = root;
        spec["swagger"] = json!("2.0");
        spec["paths"] = json!({"/p":{"get":op}});
        convert(spec).0["paths"]["/p"]["get"]["responses"]["200"].clone()
    }

    #[test]
    fn response_schema_becomes_content() {
        let r = resp_of(
            json!({"produces":["application/json","application/xml"]}),
            json!({"responses":{"200":{"description":"ok","schema":{"$ref":"#/definitions/Pet"}}}}),
        );
        let want = json!({"schema":{"$ref":"#/components/schemas/Pet"}});
        assert_eq!(
            r["content"],
            json!({"application/json":want.clone(),"application/xml":want})
        );
        assert!(r.get("schema").is_none());

        let r = resp_of(
            json!({"produces":["application/json"]}),
            json!({"produces":["text/plain"],"responses":{"200":{"description":"ok","schema":{"type":"string"}}}}),
        );
        assert!(
            r["content"].get("text/plain").is_some()
                && r["content"].get("application/json").is_none()
        );

        let r = resp_of(
            json!({}),
            json!({"responses":{"200":{"description":"ok","schema":{"type":"string"}}}}),
        );
        assert!(r["content"].get("application/json").is_some());

        let r = resp_of(json!({}), json!({"responses":{"200":{"description":"ok"}}}));
        assert!(r.get("content").is_none());
    }

    #[test]
    fn response_headers_and_examples() {
        let r = resp_of(
            json!({}),
            json!({"responses":{"200":{"description":"ok","schema":{"type":"object"},
            "headers":{"X-Rate":{"type":"integer","description":"limit"}},
            "examples":{"application/json":{"a":1}}}}}),
        );
        assert_eq!(
            r["headers"]["X-Rate"],
            json!({"description":"limit","schema":{"type":"integer"}})
        );
        assert_eq!(r["content"]["application/json"]["example"], json!({"a":1}));
        assert!(r.get("examples").is_none());
    }

    #[test]
    fn response_refs_and_top_level_responses() {
        let (out, _) = conv(json!({"swagger":"2.0","produces":["application/json"],
            "responses":{"NotFound":{"description":"nf","schema":{"type":"string"}}},
            "paths":{"/p":{"get":{"responses":{"404":{"$ref":"#/responses/NotFound"}}}}}}));
        assert_eq!(
            out["paths"]["/p"]["get"]["responses"]["404"]["$ref"],
            "#/components/responses/NotFound"
        );
        assert!(
            out["components"]["responses"]["NotFound"]["content"]["application/json"].is_object()
        );
        assert!(out.get("produces").is_none());
    }

    #[test]
    fn security_definitions() {
        let (out, notes) = conv(
            json!({"swagger":"2.0","security":[{"k":[]}],"securityDefinitions":{
            "b":{"type":"basic","description":"d"},
            "k":{"type":"apiKey","name":"X","in":"header"},
            "i":{"type":"oauth2","flow":"implicit","authorizationUrl":"https://a","scopes":{"r":"read"}},
            "p":{"type":"oauth2","flow":"password","tokenUrl":"https://t","scopes":{}},
            "a":{"type":"oauth2","flow":"application","tokenUrl":"https://t","scopes":{}},
            "c":{"type":"oauth2","flow":"accessCode","authorizationUrl":"https://a","tokenUrl":"https://t","scopes":{}},
            "x":{"type":"mystery"}}}),
        );
        let s = &out["components"]["securitySchemes"];
        assert!(out.get("securityDefinitions").is_none());
        assert_eq!(
            s["b"],
            json!({"type":"http","scheme":"basic","description":"d"})
        );
        assert_eq!(s["k"], json!({"type":"apiKey","name":"X","in":"header"}));
        assert_eq!(
            s["i"],
            json!({"type":"oauth2","flows":{"implicit":{"authorizationUrl":"https://a","scopes":{"r":"read"}}}})
        );
        assert_eq!(s["p"]["flows"]["password"]["tokenUrl"], "https://t");
        assert_eq!(
            s["a"]["flows"]["clientCredentials"]["tokenUrl"],
            "https://t"
        );
        assert_eq!(
            s["c"]["flows"]["authorizationCode"],
            json!({"authorizationUrl":"https://a","tokenUrl":"https://t","scopes":{}})
        );
        assert_eq!(s["x"], json!({"type":"mystery"}));
        assert!(notes
            .iter()
            .any(|n| n.contains("unknown security scheme type 'mystery'")));
        assert_eq!(out["security"], json!([{"k":[]}]));
    }
}
