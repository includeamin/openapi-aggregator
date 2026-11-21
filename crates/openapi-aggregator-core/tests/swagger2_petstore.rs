use openapi_aggregator_core::swagger2::convert;
use serde_json::Value;

fn contains_str(value: &Value, needle: &str) -> bool {
    match value {
        Value::String(s) => s.contains(needle),
        Value::Array(a) => a.iter().any(|v| contains_str(v, needle)),
        Value::Object(m) => m.values().any(|v| contains_str(v, needle)),
        _ => false,
    }
}

#[test]
fn petstore2_converts_to_valid_shape() {
    let input: Value = serde_json::from_str(include_str!("fixtures/petstore2.json")).unwrap();
    let (out, notes) = convert(input);

    assert_eq!(out["openapi"], "3.0.3");
    assert!(out.get("swagger").is_none() && out.get("definitions").is_none());
    assert!(!notes.is_empty());

    let pet_id = out["paths"]["/pet/{petId}"]["get"]["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "petId")
        .unwrap();
    assert_eq!(pet_id["schema"]["type"], "integer");

    let content = &out["paths"]["/pet"]["post"]["requestBody"]["content"];
    assert!(content["application/json"].is_object() && content["application/xml"].is_object());

    assert!(out["components"]["securitySchemes"]["petstore_auth"]["flows"]["implicit"].is_object());

    let upload = &out["paths"]["/pet/{petId}/uploadImage"]["post"]["requestBody"]["content"]
        ["multipart/form-data"]["schema"]["properties"];
    assert_eq!(upload["file"]["format"], "binary");

    assert!(!contains_str(&out, "#/definitions/"));
    assert!(!contains_str(&out, "#/parameters/") && !contains_str(&out, "#/responses/"));
}
