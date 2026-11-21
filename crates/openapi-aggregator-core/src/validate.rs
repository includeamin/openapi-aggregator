//! Semantic validation of a merged OpenAPI document: the problems an aggregation can
//! actually introduce (dangling refs, duplicate operation ids, path template mismatches, ...).
//!
//! This is deliberately not a full structural validation against the OpenAPI JSON Schema.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

/// How serious a [`Finding`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }
}

/// One problem found in a spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
    /// JSON pointer into the spec, e.g. `/paths/~1pets/get`.
    pub location: String,
}

/// Escape a key for use as a JSON pointer segment (`~` -> `~0`, `/` -> `~1`).
pub(crate) fn pointer_escape(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

fn finding(
    severity: Severity,
    code: &'static str,
    message: impl Into<String>,
    location: impl Into<String>,
) -> Finding {
    Finding {
        severity,
        code,
        message: message.into(),
        location: location.into(),
    }
}

fn error(code: &'static str, message: impl Into<String>, location: impl Into<String>) -> Finding {
    finding(Severity::Error, code, message, location)
}

/// Validate `spec`; findings are sorted by `(location, code)`.
pub fn validate(spec: &Value) -> Vec<Finding> {
    let mut out = Vec::new();
    check_document(spec, &mut out);
    if spec.is_object() {
        check_component_names(spec, &mut out);
        check_operations(spec, &mut out);
        check_refs(spec, &mut out);
        check_operation_ids(spec, &mut out);
        check_paths(spec, &mut out);
        check_security(spec, &mut out);
        check_tags(spec, &mut out);
    }
    out.sort_by(|a, b| (&a.location, a.code).cmp(&(&b.location, b.code)));
    out
}

fn minor_version(spec: &Value) -> Option<u32> {
    spec.get("openapi")?
        .as_str()?
        .strip_prefix("3.")?
        .split('.')
        .next()?
        .parse()
        .ok()
}

fn check_document(spec: &Value, out: &mut Vec<Finding>) {
    let minor = minor_version(spec);
    if minor.is_none() {
        out.push(error(
            "invalid-openapi-version",
            "'openapi' must be a 3.x version string",
            "/openapi",
        ));
    }
    let Some(root) = spec.as_object() else { return };

    match root.get("info") {
        Some(Value::Object(info)) => {
            for field in ["title", "version"] {
                if !info.get(field).is_some_and(Value::is_string) {
                    out.push(error(
                        "missing-field",
                        format!("'info.{field}' is required"),
                        format!("/info/{field}"),
                    ));
                }
            }
        }
        _ => out.push(error("missing-field", "'info' is required", "/info")),
    }

    if minor.is_some_and(|m| m >= 1) {
        if !["paths", "webhooks", "components"]
            .iter()
            .any(|k| root.contains_key(*k))
        {
            out.push(error(
                "missing-field",
                "at least one of 'paths', 'webhooks' or 'components' is required",
                "",
            ));
        }
    } else if !root.contains_key("paths") {
        out.push(error("missing-field", "'paths' is required", "/paths"));
    }
}

fn is_valid_component_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn check_component_names(spec: &Value, out: &mut Vec<Finding>) {
    let Some(Value::Object(components)) = spec.get("components") else {
        return;
    };
    for (section, entries) in components {
        let Value::Object(entries) = entries else {
            continue;
        };
        for name in entries.keys().filter(|n| !is_valid_component_name(n)) {
            out.push(error(
                "invalid-component-name",
                format!("component name '{name}' must match ^[a-zA-Z0-9._-]+$"),
                format!(
                    "/components/{}/{}",
                    pointer_escape(section),
                    pointer_escape(name)
                ),
            ));
        }
    }
}

/// `(path, method, operation)` for every operation in `paths`.
fn operations(spec: &Value) -> Vec<(&str, &str, &Value)> {
    let mut ops = Vec::new();
    if let Some(Value::Object(paths)) = spec.get("paths") {
        for (path, item) in paths {
            for method in crate::swagger2::METHODS {
                if let Some(op) = item.get(method) {
                    ops.push((path.as_str(), method, op));
                }
            }
        }
    }
    ops
}

fn operation_pointer(path: &str, method: &str) -> String {
    format!("/paths/{}/{method}", pointer_escape(path))
}

fn check_refs(spec: &Value, out: &mut Vec<Finding>) {
    crate::bundle::walk_refs_at(spec, &mut |location, reference| {
        let Some(pointer) = reference.strip_prefix('#') else {
            out.push(error(
                "external-ref",
                format!("$ref '{reference}' points outside the document and was not bundled"),
                location,
            ));
            return;
        };
        // `#anchor` style refs (3.1) cannot be resolved here; do not guess.
        if !pointer.is_empty() && !pointer.starts_with('/') {
            return;
        }
        let segments = crate::bundle::decode_pointer(pointer);
        if crate::bundle::lookup(spec, &segments).is_none() {
            out.push(error(
                "dangling-ref",
                format!("$ref '{reference}' does not resolve"),
                location,
            ));
        }
    });
}

fn check_operation_ids(spec: &Value, out: &mut Vec<Finding>) {
    let mut seen: HashMap<&str, (&str, &str)> = HashMap::new();
    for (path, method, op) in operations(spec) {
        let Some(id) = op.get("operationId").and_then(Value::as_str) else {
            continue;
        };
        match seen.get(id) {
            Some((first_path, first_method)) => out.push(error(
                "duplicate-operation-id",
                format!(
                    "operationId '{id}' is already used by {} {first_path}",
                    first_method.to_uppercase()
                ),
                operation_pointer(path, method),
            )),
            None => {
                seen.insert(id, (path, method));
            }
        }
    }
}

/// Names of the `{param}` placeholders in a path template.
fn template_params(path: &str) -> Vec<&str> {
    let mut names = Vec::new();
    let mut rest = path;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else { break };
        names.push(&after[..end]);
        rest = &after[end + 1..];
    }
    names
}

/// The path template with every placeholder name erased, for ambiguity detection.
fn template_shape(path: &str) -> String {
    let mut shape = String::new();
    let mut rest = path;
    while let Some(start) = rest.find('{') {
        shape.push_str(&rest[..start]);
        match rest[start..].find('}') {
            Some(end) => {
                shape.push_str("{}");
                rest = &rest[start + end + 1..];
            }
            None => {
                shape.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    shape.push_str(rest);
    shape
}

/// `(name, in)` of each entry of a `parameters` array, resolving internal `$ref`s once.
fn parameter_keys(spec: &Value, parameters: Option<&Value>) -> Vec<(String, String)> {
    let Some(Value::Array(list)) = parameters else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|p| {
            let resolved = match p.get("$ref").and_then(Value::as_str) {
                Some(reference) => {
                    let pointer = reference.strip_prefix('#')?;
                    crate::bundle::lookup(spec, &crate::bundle::decode_pointer(pointer))?
                }
                None => p,
            };
            Some((
                resolved.get("name")?.as_str()?.to_string(),
                resolved.get("in")?.as_str()?.to_string(),
            ))
        })
        .collect()
}

fn duplicates(keys: &[(String, String)]) -> Vec<&(String, String)> {
    let mut seen = HashSet::new();
    keys.iter().filter(|k| !seen.insert(*k)).collect()
}

fn check_paths(spec: &Value, out: &mut Vec<Finding>) {
    let Some(Value::Object(paths)) = spec.get("paths") else {
        return;
    };
    let mut shapes: HashMap<String, &str> = HashMap::new();
    for (path, item) in paths {
        let item_pointer = format!("/paths/{}", pointer_escape(path));
        match shapes.get(&template_shape(path)) {
            Some(other) => out.push(error(
                "ambiguous-paths",
                format!("path '{path}' is equivalent to '{other}'"),
                item_pointer.clone(),
            )),
            None => {
                shapes.insert(template_shape(path), path);
            }
        }

        // A path item that is a `$ref` takes its operations from elsewhere.
        if item.get("$ref").is_some() {
            continue;
        }

        let template = template_params(path);
        let item_params = parameter_keys(spec, item.get("parameters"));
        let ops: Vec<(&str, &Value)> = crate::swagger2::METHODS
            .iter()
            .filter_map(|m| item.get(*m).map(|op| (*m, op)))
            .collect();

        report_duplicates(&item_params, &item_pointer, out);
        report_unused(&item_params, &template, path, &item_pointer, out);
        if ops.is_empty() {
            report_undeclared(&item_params, &template, path, &item_pointer, out);
        }
        for (method, op) in ops {
            let pointer = operation_pointer(path, method);
            let op_params = parameter_keys(spec, op.get("parameters"));
            report_duplicates(&op_params, &pointer, out);
            report_unused(&op_params, &template, path, &pointer, out);
            let effective: Vec<(String, String)> =
                item_params.iter().chain(&op_params).cloned().collect();
            report_undeclared(&effective, &template, path, &pointer, out);
        }
    }
}

fn report_duplicates(keys: &[(String, String)], location: &str, out: &mut Vec<Finding>) {
    for (name, location_in) in duplicates(keys) {
        out.push(error(
            "duplicate-parameter",
            format!("parameter '{name}' (in: {location_in}) is declared more than once"),
            location,
        ));
    }
}

fn report_unused(
    keys: &[(String, String)],
    template: &[&str],
    path: &str,
    location: &str,
    out: &mut Vec<Finding>,
) {
    for (name, _) in keys
        .iter()
        .filter(|(n, i)| i == "path" && !template.contains(&n.as_str()))
    {
        out.push(finding(
            Severity::Warning,
            "unused-path-param",
            format!("path parameter '{name}' does not appear in '{path}'"),
            location,
        ));
    }
}

fn report_undeclared(
    keys: &[(String, String)],
    template: &[&str],
    path: &str,
    location: &str,
    out: &mut Vec<Finding>,
) {
    for name in template {
        if !keys.iter().any(|(n, i)| i == "path" && n == name) {
            out.push(error(
                "undeclared-path-param",
                format!("'{{{name}}}' in '{path}' has no matching path parameter"),
                location,
            ));
        }
    }
}

fn check_security(spec: &Value, out: &mut Vec<Finding>) {
    let defined = |name: &str| {
        spec.pointer("/components/securitySchemes")
            .and_then(Value::as_object)
            .is_some_and(|schemes| schemes.contains_key(name))
    };
    let mut check = |requirements: Option<&Value>, location: String| {
        let Some(Value::Array(list)) = requirements else {
            return;
        };
        for (index, requirement) in list.iter().enumerate() {
            let Value::Object(requirement) = requirement else {
                continue;
            };
            for scheme in requirement.keys().filter(|n| !defined(n)) {
                out.push(error(
                    "unknown-security-scheme",
                    format!("security requirement names '{scheme}', which is not in components.securitySchemes"),
                    format!("{location}/security/{index}"),
                ));
            }
        }
    };
    check(spec.get("security"), String::new());
    for (path, method, op) in operations(spec) {
        check(op.get("security"), operation_pointer(path, method));
    }
}

fn check_tags(spec: &Value, out: &mut Vec<Finding>) {
    let declared: HashSet<&str> = match spec.get("tags") {
        Some(Value::Array(tags)) if !tags.is_empty() => tags
            .iter()
            .filter_map(|t| t.get("name").and_then(Value::as_str))
            .collect(),
        _ => return,
    };
    for (path, method, op) in operations(spec) {
        let Some(Value::Array(tags)) = op.get("tags") else {
            continue;
        };
        for tag in tags
            .iter()
            .filter_map(Value::as_str)
            .filter(|t| !declared.contains(t))
        {
            out.push(finding(
                Severity::Warning,
                "undeclared-tag",
                format!("tag '{tag}' is not declared in the top-level tags"),
                operation_pointer(path, method),
            ));
        }
    }
}

fn check_operations(spec: &Value, out: &mut Vec<Finding>) {
    for (path, method, op) in operations(spec) {
        let has_responses = op
            .get("responses")
            .and_then(Value::as_object)
            .is_some_and(|r| !r.is_empty());
        if !has_responses {
            out.push(error(
                "missing-responses",
                format!("{} {path} has no responses", method.to_uppercase()),
                operation_pointer(path, method),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn found(spec: &Value) -> Vec<(&'static str, Severity, String)> {
        validate(spec)
            .into_iter()
            .map(|f| (f.code, f.severity, f.location))
            .collect()
    }

    fn minimal() -> Value {
        json!({"openapi":"3.0.3","info":{"title":"T","version":"1"},"paths":{}})
    }

    #[test]
    fn valid_minimal_spec_has_no_findings() {
        assert!(validate(&minimal()).is_empty());
    }

    #[test]
    fn invalid_openapi_version() {
        for bad in [
            json!({"info":{"title":"T","version":"1"},"paths":{}}),
            json!({"openapi":"2.0","info":{"title":"T","version":"1"},"paths":{}}),
            json!({"openapi":3,"info":{"title":"T","version":"1"},"paths":{}}),
        ] {
            assert_eq!(
                found(&bad),
                vec![(
                    "invalid-openapi-version",
                    Severity::Error,
                    "/openapi".to_string()
                )],
                "{bad}"
            );
        }
    }

    #[test]
    fn missing_info_fields() {
        let mut spec = minimal();
        spec.as_object_mut().unwrap().remove("info");
        assert_eq!(
            found(&spec),
            vec![("missing-field", Severity::Error, "/info".into())]
        );
        let mut spec = minimal();
        spec["info"] = json!({"version":"1"});
        assert_eq!(
            found(&spec),
            vec![("missing-field", Severity::Error, "/info/title".into())]
        );
        spec["info"] = json!({"title":"T"});
        assert_eq!(
            found(&spec),
            vec![("missing-field", Severity::Error, "/info/version".into())]
        );
    }

    #[test]
    fn paths_required_in_30_only() {
        let info = json!({"title":"T","version":"1"});
        let spec = json!({"openapi":"3.0.3","info":info});
        assert_eq!(
            found(&spec),
            vec![("missing-field", Severity::Error, "/paths".into())]
        );
        let spec = json!({"openapi":"3.1.0","info":info,"webhooks":{}});
        assert!(validate(&spec).is_empty());
        let spec = json!({"openapi":"3.1.0","info":info,"components":{}});
        assert!(validate(&spec).is_empty());
        let spec = json!({"openapi":"3.1.0","info":info});
        assert_eq!(
            found(&spec),
            vec![("missing-field", Severity::Error, "".into())]
        );
    }

    #[test]
    fn invalid_component_names() {
        let mut spec = minimal();
        spec["components"] = json!({
            "schemas": {"bad name": {}, "Good.Name_1-x": {}},
            "parameters": {"a/b": {"name":"a","in":"query"}}
        });
        assert_eq!(
            found(&spec),
            vec![
                (
                    "invalid-component-name",
                    Severity::Error,
                    "/components/parameters/a~1b".into()
                ),
                (
                    "invalid-component-name",
                    Severity::Error,
                    "/components/schemas/bad name".into()
                ),
            ]
        );
    }

    #[test]
    fn operations_need_responses() {
        let mut spec = minimal();
        spec["paths"] = json!({"/p":{"get":{},"post":{"responses":{}},"put":{"responses":{"200":{"description":"ok"}}},"parameters":[]}});
        assert_eq!(
            found(&spec),
            vec![
                (
                    "missing-responses",
                    Severity::Error,
                    "/paths/~1p/get".into()
                ),
                (
                    "missing-responses",
                    Severity::Error,
                    "/paths/~1p/post".into()
                ),
            ]
        );
    }

    #[test]
    fn escapes_pointer_segments() {
        assert_eq!(pointer_escape("/a~b"), "~1a~0b");
    }

    fn with_schema_ref(reference: &str) -> Value {
        let mut spec = minimal();
        spec["paths"] = json!({"/p":{"get":{"responses":{"200":{"description":"ok","content":{
            "application/json":{"schema":{"$ref": reference}}}}}}}});
        spec
    }

    const SCHEMA_PTR: &str = "/paths/~1p/get/responses/200/content/application~1json/schema";

    #[test]
    fn dangling_ref_is_an_error_at_the_holder() {
        let spec = with_schema_ref("#/components/schemas/Missing");
        let f = validate(&spec);
        assert_eq!(f.len(), 1);
        assert_eq!(
            (f[0].code, f[0].severity),
            ("dangling-ref", Severity::Error)
        );
        assert_eq!(f[0].location, SCHEMA_PTR);
        assert!(
            f[0].message.contains("#/components/schemas/Missing"),
            "{}",
            f[0].message
        );
    }

    #[test]
    fn resolving_refs_are_fine() {
        let mut spec = with_schema_ref("#/components/schemas/Pet");
        spec["components"] = json!({"schemas":{"Pet":{"type":"object"}}});
        assert!(validate(&spec).is_empty());
        let spec = with_schema_ref("#/paths/~1p/get/responses/200");
        assert!(validate(&spec).is_empty());
    }

    #[test]
    fn external_refs_are_errors() {
        let spec = with_schema_ref("./other.yaml#/X");
        assert_eq!(
            found(&spec),
            vec![("external-ref", Severity::Error, SCHEMA_PTR.to_string())]
        );
    }

    #[test]
    fn ref_like_data_is_ignored() {
        let mut spec = minimal();
        spec["components"] = json!({
            "examples": {"E": {"value": {"$ref": "#/nope"}}},
            "schemas": {"S": {"type":"object","example":{"$ref":"#/nope"},"properties":{"x":{"default":{"$ref":"#/nope"}}}}}
        });
        assert!(validate(&spec).is_empty());
    }

    #[test]
    fn property_named_default_is_still_checked() {
        let mut spec = minimal();
        spec["components"] = json!({"schemas":{"S":{"properties":{"default":{"$ref":"#/components/schemas/Ghost"}}}}});
        assert_eq!(
            found(&spec),
            vec![(
                "dangling-ref",
                Severity::Error,
                "/components/schemas/S/properties/default".to_string()
            )]
        );
    }

    fn spec_with_paths(paths: Value) -> Value {
        let mut spec = minimal();
        spec["paths"] = paths;
        spec
    }

    fn ok() -> Value {
        json!({"200":{"description":"ok"}})
    }

    #[test]
    fn duplicate_operation_id() {
        let spec = spec_with_paths(json!({
            "/a":{"get":{"operationId":"list","responses":ok()}},
            "/b":{"get":{"operationId":"list","responses":ok()},"post":{"operationId":"create","responses":ok()}}
        }));
        let f = validate(&spec);
        assert_eq!(f.len(), 1);
        assert_eq!(
            (f[0].code, f[0].location.as_str()),
            ("duplicate-operation-id", "/paths/~1b/get")
        );
        assert!(
            f[0].message.contains("list") && f[0].message.contains("/a"),
            "{}",
            f[0].message
        );
    }

    #[test]
    fn undeclared_path_param() {
        let spec = spec_with_paths(json!({"/pets/{id}":{"get":{"responses":ok()}}}));
        assert_eq!(
            found(&spec),
            vec![(
                "undeclared-path-param",
                Severity::Error,
                "/paths/~1pets~1{id}/get".into()
            )]
        );

        let at_item = spec_with_paths(json!({"/pets/{id}":{
            "parameters":[{"name":"id","in":"path","required":true,"schema":{"type":"string"}}],
            "get":{"responses":ok()}}}));
        assert!(validate(&at_item).is_empty());
        let at_op = spec_with_paths(json!({"/pets/{id}":{"get":{
            "parameters":[{"name":"id","in":"path","required":true}],"responses":ok()}}}));
        assert!(validate(&at_op).is_empty());

        let mut by_ref = spec_with_paths(json!({"/pets/{id}":{"get":{
            "parameters":[{"$ref":"#/components/parameters/Id"}],"responses":ok()}}}));
        by_ref["components"] =
            json!({"parameters":{"Id":{"name":"id","in":"path","required":true}}});
        assert!(validate(&by_ref).is_empty());

        let wrong_in = spec_with_paths(json!({"/pets/{id}":{"get":{
            "parameters":[{"name":"id","in":"query"}],"responses":ok()}}}));
        assert_eq!(
            found(&wrong_in),
            vec![(
                "undeclared-path-param",
                Severity::Error,
                "/paths/~1pets~1{id}/get".into()
            )]
        );
    }

    #[test]
    fn unused_path_param_is_a_warning() {
        let spec = spec_with_paths(json!({"/pets":{"get":{
            "parameters":[{"name":"x","in":"path","required":true}],"responses":ok()}}}));
        assert_eq!(
            found(&spec),
            vec![(
                "unused-path-param",
                Severity::Warning,
                "/paths/~1pets/get".into()
            )]
        );
    }

    #[test]
    fn duplicate_parameter() {
        let dup = spec_with_paths(json!({"/p":{"get":{"parameters":[
            {"name":"q","in":"query"},{"name":"q","in":"query"}],"responses":ok()}}}));
        assert_eq!(
            found(&dup),
            vec![(
                "duplicate-parameter",
                Severity::Error,
                "/paths/~1p/get".into()
            )]
        );
        let different_in = spec_with_paths(json!({"/p":{"get":{"parameters":[
            {"name":"q","in":"query"},{"name":"q","in":"header"}],"responses":ok()}}}));
        assert!(validate(&different_in).is_empty());
        let overridden = spec_with_paths(json!({"/p":{
            "parameters":[{"name":"q","in":"query"}],
            "get":{"parameters":[{"name":"q","in":"query"}],"responses":ok()}}}));
        assert!(validate(&overridden).is_empty());
    }

    #[test]
    fn ambiguous_paths() {
        let spec = spec_with_paths(json!({
            "/a/{id}":{"get":{"parameters":[{"name":"id","in":"path","required":true}],"responses":ok()}},
            "/a/{name}":{"get":{"parameters":[{"name":"name","in":"path","required":true}],"responses":ok()}}
        }));
        assert_eq!(
            found(&spec),
            vec![(
                "ambiguous-paths",
                Severity::Error,
                "/paths/~1a~1{name}".into()
            )]
        );
        let fine = spec_with_paths(json!({
            "/a/{id}":{"get":{"parameters":[{"name":"id","in":"path","required":true}],"responses":ok()}},
            "/b/{id}":{"get":{"parameters":[{"name":"id","in":"path","required":true}],"responses":ok()}}
        }));
        assert!(validate(&fine).is_empty());
    }

    #[test]
    fn unknown_security_scheme() {
        let mut spec = spec_with_paths(
            json!({"/p":{"get":{"security":[{"ghost":[]},{}, {"real":[]}],"responses":ok()}}}),
        );
        spec["security"] = json!([{"ghost": []}]);
        spec["components"] = json!({"securitySchemes":{"real":{"type":"http","scheme":"basic"}}});
        assert_eq!(
            found(&spec),
            vec![
                (
                    "unknown-security-scheme",
                    Severity::Error,
                    "/paths/~1p/get/security/0".into()
                ),
                (
                    "unknown-security-scheme",
                    Severity::Error,
                    "/security/0".into()
                ),
            ]
        );
        spec["security"] = json!([{"real": []}]);
        spec["paths"]["/p"]["get"]["security"] = json!([{"real": []}, {}]);
        assert!(validate(&spec).is_empty());
    }

    #[test]
    fn undeclared_tag_is_a_warning_only_when_tags_are_declared() {
        let mut spec = spec_with_paths(json!({"/p":{"get":{"tags":["a","b"],"responses":ok()}}}));
        assert!(validate(&spec).is_empty());
        spec["tags"] = json!([]);
        assert!(validate(&spec).is_empty());
        spec["tags"] = json!([{"name":"a"}]);
        let f = validate(&spec);
        assert_eq!(f.len(), 1);
        assert_eq!(
            (f[0].code, f[0].severity, f[0].location.as_str()),
            ("undeclared-tag", Severity::Warning, "/paths/~1p/get")
        );
        assert!(f[0].message.contains("'b'"), "{}", f[0].message);
    }

    #[test]
    fn path_item_refs_are_not_checked_for_path_params() {
        let spec = spec_with_paths(json!({
            "/a/{id}":{"get":{"parameters":[{"name":"id","in":"path","required":true}],"responses":ok()}},
            "/b/{id}":{"$ref":"#/paths/~1a~1{id}"}
        }));
        assert!(validate(&spec).is_empty(), "{:?}", validate(&spec));
    }
}
