use openapi_aggregator_core::swagger2::convert;
use openapi_aggregator_core::{parse_content, validate};
use serde_json::{json, Value};

fn assert_clean(name: &str, spec: &Value) {
    let findings = validate(spec);
    assert!(
        findings.is_empty(),
        "{name}: unexpected findings: {:#?}",
        findings
            .iter()
            .map(|f| format!("{} {} {}", f.code, f.location, f.message))
            .collect::<Vec<_>>()
    );
}

#[test]
fn petstore_30_fixture_has_no_findings() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../openapi-aggregator/tests/fixtures/petstore.yaml"
    ))
    .unwrap();
    assert_clean("petstore.yaml", &parse_content(&text).unwrap());
}

#[test]
fn petstore_swagger2_after_conversion_has_no_findings() {
    let input: Value = serde_json::from_str(include_str!("fixtures/petstore2.json")).unwrap();
    let (converted, _) = convert(input);
    assert_clean("petstore2.json", &converted);
}

#[test]
fn webhooks_only_31_spec_has_no_findings() {
    assert_clean(
        "webhooks",
        &json!({"openapi":"3.1.0","info":{"title":"T","version":"1"},
            "webhooks":{"newPet":{"post":{"responses":{"200":{"description":"ok"}}}}}}),
    );
}

#[test]
fn ref_parameters_and_path_item_parameters_have_no_findings() {
    assert_clean(
        "params",
        &json!({"openapi":"3.0.3","info":{"title":"T","version":"1"},
        "paths":{
            "/pets/{petId}":{
                "parameters":[{"$ref":"#/components/parameters/PetId"}],
                "get":{"operationId":"getPet","tags":["pets"],"responses":{"200":{"description":"ok","content":{
                    "application/json":{"schema":{"$ref":"#/components/schemas/Pet"}}}}}},
                "delete":{"operationId":"deletePet","responses":{"204":{"description":"gone"}}}
            },
            "/owners/{ownerId}/pets/{petId}":{
                "get":{"parameters":[
                    {"name":"ownerId","in":"path","required":true,"schema":{"type":"string"}},
                    {"$ref":"#/components/parameters/PetId"}],
                    "security":[{"key":[]}],
                    "responses":{"default":{"$ref":"#/components/responses/Err"}}}
            }},
        "tags":[{"name":"pets"}],
        "components":{
            "parameters":{"PetId":{"name":"petId","in":"path","required":true,"schema":{"type":"string"}}},
            "schemas":{"Pet":{"type":"object","properties":{"owner":{"$ref":"#/components/schemas/Owner"}}},"Owner":{"type":"object"}},
            "responses":{"Err":{"description":"error"}},
            "securitySchemes":{"key":{"type":"apiKey","name":"k","in":"header"}}
        }}),
    );
}
