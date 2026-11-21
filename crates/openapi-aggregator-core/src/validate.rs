//! Semantic validation of a merged OpenAPI document: the problems an aggregation can
//! actually introduce (dangling refs, duplicate operation ids, path template mismatches, ...).
//!
//! This is deliberately not a full structural validation against the OpenAPI JSON Schema.

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
}
