//! Hide operations (e.g. internal endpoints) from a source before it is merged, and prune
//! the components, security schemes and tags that only those operations used.

// The internal pieces are wired into `filter_spec` by the next step.
#![allow(dead_code)]

use serde_json::Value;

use crate::config::FilterRules;

#[derive(Clone, Copy, PartialEq)]
enum Token {
    Literal(char),
    /// `*`: any run of characters except `/`.
    Star,
    /// `**`: any run of characters, including `/`.
    DoubleStar,
}

fn tokenize(pattern: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '*' {
            tokens.push(Token::Literal(c));
            continue;
        }
        let mut stars = 1;
        while chars.next_if_eq(&'*').is_some() {
            stars += 1;
        }
        tokens.push(if stars == 1 {
            Token::Star
        } else {
            Token::DoubleStar
        });
    }
    tokens
}

/// Match `text` against a path glob: `*` matches within one segment (never `/`), `**` matches
/// across segments, everything else is literal.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let tokens = tokenize(pattern);
    let text: Vec<char> = text.chars().collect();
    // reachable[j]: the tokens consumed so far can match text[..j].
    let mut reachable = vec![false; text.len() + 1];
    reachable[0] = true;
    for token in tokens {
        let mut next = vec![false; text.len() + 1];
        match token {
            Token::Literal(c) => {
                for j in 0..text.len() {
                    next[j + 1] = reachable[j] && text[j] == c;
                }
            }
            Token::Star | Token::DoubleStar => {
                let mut open = false;
                for j in 0..=text.len() {
                    if reachable[j] {
                        open = true;
                    }
                    next[j] = open;
                    // A single `*` cannot cross a `/`.
                    if token == Token::Star && j < text.len() && text[j] == '/' {
                        open = false;
                    }
                }
            }
        }
        reachable = next;
    }
    reachable[text.len()]
}

fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// Whether an operation matches any criterion of `rules`. `path` is `None` for webhooks, which
/// disables the `paths` criterion.
pub(crate) fn rules_match(
    rules: &FilterRules,
    path: Option<&str>,
    path_item: &Value,
    op: &Value,
) -> bool {
    if let Some(path) = path {
        if rules.paths.iter().any(|glob| glob_match(glob, path)) {
            return true;
        }
    }
    if let Some(Value::Array(tags)) = op.get("tags") {
        if tags
            .iter()
            .filter_map(Value::as_str)
            .any(|tag| rules.tags.iter().any(|t| t == tag))
        {
            return true;
        }
    }
    if let Some(id) = op.get("operationId").and_then(Value::as_str) {
        if rules.operation_ids.iter().any(|i| i == id) {
            return true;
        }
    }
    rules
        .extensions
        .iter()
        .any(|ext| truthy(op.get(ext)) || truthy(path_item.get(ext)))
}

/// Remove every operation that is not kept by the rules; returns whether anything was removed.
///
/// An operation is kept iff no `exclude` rule matches it and every `include` rule matches it.
/// Path items (and webhooks) left without operations are removed too.
pub(crate) fn remove_operations(
    spec: &mut Value,
    include: &[&FilterRules],
    exclude: &[&FilterRules],
) -> bool {
    let paths = filter_items(spec, "paths", true, include, exclude);
    let webhooks = filter_items(spec, "webhooks", false, include, exclude);
    paths | webhooks
}

fn filter_items(
    spec: &mut Value,
    key: &str,
    match_paths: bool,
    include: &[&FilterRules],
    exclude: &[&FilterRules],
) -> bool {
    let Some(Value::Object(items)) = spec.get_mut(key) else {
        return false;
    };
    let mut removed = false;
    let names: Vec<String> = items.keys().cloned().collect();
    for name in names {
        let path = match_paths.then_some(name.as_str());
        let (drop_methods, had_operations, path_excluded) = {
            let item = &items[&name];
            let methods: Vec<&str> = crate::swagger2::METHODS
                .iter()
                .copied()
                .filter(|m| item.get(*m).is_some_and(Value::is_object))
                .collect();
            let drop_methods: Vec<&str> = methods
                .iter()
                .copied()
                .filter(|m| {
                    let op = &item[*m];
                    exclude.iter().any(|r| rules_match(r, path, item, op))
                        || !include.iter().all(|r| rules_match(r, path, item, op))
                })
                .collect();
            let path_excluded = path.is_some_and(|p| {
                exclude
                    .iter()
                    .any(|r| r.paths.iter().any(|glob| glob_match(glob, p)))
            });
            (
                drop_methods
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                !methods.is_empty(),
                path_excluded,
            )
        };

        let Some(Value::Object(item)) = items.get_mut(&name) else {
            continue;
        };
        for method in &drop_methods {
            item.shift_remove(method);
            removed = true;
        }
        let remaining = crate::swagger2::METHODS
            .iter()
            .any(|m| item.get(*m).is_some_and(Value::is_object));
        let drop_item = if had_operations {
            !remaining && !item.contains_key("$ref")
        } else {
            path_excluded
        };
        if drop_item {
            items.shift_remove(&name);
            removed = true;
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn globs() {
        assert!(glob_match("/internal/**", "/internal/a"));
        assert!(glob_match("/internal/**", "/internal/a/b/c"));
        assert!(!glob_match("/internal/**", "/internal"));
        assert!(glob_match("/admin/*", "/admin/x"));
        assert!(!glob_match("/admin/*", "/admin/a/b"));
        assert!(!glob_match("/admin/*", "/admin"));
        assert!(glob_match("/users/{id}", "/users/{id}"));
        assert!(!glob_match("/users/{id}", "/users/1"));
        assert!(glob_match("/a*c", "/abc"));
        assert!(!glob_match("/a*c", "/a/c"));
        assert!(glob_match("/**/health", "/x/y/health"));
        assert!(glob_match("/pets", "/pets"));
        assert!(!glob_match("/pets", "/pets/1"));
        assert!(glob_match("", ""));
        assert!(!glob_match("", "/x"));
        assert!(glob_match("/x/***", "/x/a/b"));
    }

    fn rules(json: Value) -> FilterRules {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn matches_each_criterion() {
        let item = json!({});
        let op = json!({"operationId": "debugDump", "tags": ["internal", "x"], "x-internal": true});
        assert!(rules_match(
            &rules(json!({"paths": ["/internal/**"]})),
            Some("/internal/a"),
            &item,
            &op
        ));
        assert!(!rules_match(
            &rules(json!({"paths": ["/internal/**"]})),
            Some("/pets"),
            &item,
            &op
        ));
        assert!(rules_match(
            &rules(json!({"tags": ["internal"]})),
            Some("/p"),
            &item,
            &op
        ));
        assert!(!rules_match(
            &rules(json!({"tags": ["public"]})),
            Some("/p"),
            &item,
            &op
        ));
        assert!(rules_match(
            &rules(json!({"operation_ids": ["debugDump"]})),
            Some("/p"),
            &item,
            &op
        ));
        assert!(!rules_match(
            &rules(json!({"operation_ids": ["other"]})),
            Some("/p"),
            &item,
            &op
        ));
        assert!(rules_match(
            &rules(json!({"extensions": ["x-internal"]})),
            Some("/p"),
            &item,
            &op
        ));
    }

    #[test]
    fn any_criterion_is_enough_and_empty_rules_match_nothing() {
        let op = json!({"tags": ["a"]});
        let both = rules(json!({"paths": ["/nope"], "tags": ["a"]}));
        assert!(rules_match(&both, Some("/p"), &json!({}), &op));
        assert!(!rules_match(
            &FilterRules::default(),
            Some("/p"),
            &json!({}),
            &op
        ));
    }

    #[test]
    fn extension_truthiness() {
        let r = rules(json!({"extensions": ["x-internal"]}));
        for truthy in [
            json!(true),
            json!("yes"),
            json!(1),
            json!({"a": 1}),
            json!([1]),
        ] {
            assert!(
                rules_match(
                    &r,
                    Some("/p"),
                    &json!({}),
                    &json!({"x-internal": truthy.clone()})
                ),
                "{truthy}"
            );
        }
        for falsy in [json!(false), json!(0), json!(""), json!(null)] {
            assert!(
                !rules_match(
                    &r,
                    Some("/p"),
                    &json!({}),
                    &json!({"x-internal": falsy.clone()})
                ),
                "{falsy}"
            );
        }
        assert!(!rules_match(&r, Some("/p"), &json!({}), &json!({})));
    }

    #[test]
    fn extension_on_the_path_item_matches() {
        let r = rules(json!({"extensions": ["x-internal"]}));
        assert!(rules_match(
            &r,
            Some("/p"),
            &json!({"x-internal": true}),
            &json!({})
        ));
        assert!(!rules_match(
            &r,
            Some("/p"),
            &json!({"x-internal": false}),
            &json!({})
        ));
    }

    #[test]
    fn webhooks_ignore_path_globs() {
        let r = rules(json!({"paths": ["/**"]}));
        assert!(!rules_match(&r, None, &json!({}), &json!({})));
    }

    fn doc(paths: Value) -> Value {
        json!({"openapi":"3.0.3","info":{"title":"T","version":"1"},"paths":paths})
    }

    fn ok() -> Value {
        json!({"200":{"description":"ok"}})
    }

    fn run(spec: &mut Value, include: Vec<FilterRules>, exclude: Vec<FilterRules>) -> bool {
        let include: Vec<&FilterRules> = include.iter().collect();
        let exclude: Vec<&FilterRules> = exclude.iter().collect();
        remove_operations(spec, &include, &exclude)
    }

    #[test]
    fn exclude_by_path_glob_removes_paths() {
        let mut spec = doc(json!({
            "/pets":{"get":{"responses":ok()}},
            "/internal/a":{"get":{"responses":ok()}},
            "/internal/b/c":{"post":{"responses":ok()}}
        }));
        assert!(run(
            &mut spec,
            vec![],
            vec![rules(json!({"paths":["/internal/**"]}))]
        ));
        assert_eq!(
            spec["paths"]
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            vec!["/pets"]
        );
    }

    #[test]
    fn exclude_by_tag_keeps_other_operations_and_shared_parameters() {
        let mut spec = doc(json!({
            "/p/{id}":{
                "parameters":[{"name":"id","in":"path","required":true}],
                "get":{"responses":ok()},
                "delete":{"tags":["internal"],"responses":ok()}},
            "/q":{
                "parameters":[{"name":"x","in":"query"}],
                "get":{"tags":["internal"],"responses":ok()}}
        }));
        assert!(run(
            &mut spec,
            vec![],
            vec![rules(json!({"tags":["internal"]}))]
        ));
        assert!(spec["paths"]["/p/{id}"].get("delete").is_none());
        assert!(spec["paths"]["/p/{id}"].get("get").is_some());
        assert!(spec["paths"]["/p/{id}"].get("parameters").is_some());
        assert!(spec["paths"].get("/q").is_none());
    }

    #[test]
    fn include_is_an_allow_list_and_exclude_wins() {
        let paths = json!({
            "/a":{"get":{"tags":["public"],"responses":ok()}},
            "/b":{"get":{"tags":["public","beta"],"responses":ok()}},
            "/c":{"get":{"tags":["private"],"responses":ok()}}
        });
        let mut spec = doc(paths.clone());
        run(&mut spec, vec![rules(json!({"tags":["public"]}))], vec![]);
        assert_eq!(
            spec["paths"]
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            vec!["/a", "/b"]
        );

        let mut spec = doc(paths.clone());
        run(
            &mut spec,
            vec![rules(json!({"tags":["public"]}))],
            vec![rules(json!({"tags":["beta"]}))],
        );
        assert_eq!(
            spec["paths"]
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            vec!["/a"]
        );

        // two include rules (global + source) must both match
        let mut spec = doc(paths);
        run(
            &mut spec,
            vec![
                rules(json!({"tags":["public"]})),
                rules(json!({"paths":["/b"]})),
            ],
            vec![],
        );
        assert_eq!(
            spec["paths"]
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            vec!["/b"]
        );
    }

    #[test]
    fn path_item_extension_hides_all_its_operations() {
        let mut spec = doc(json!({
            "/a":{"x-internal":true,"get":{"responses":ok()},"post":{"responses":ok()}},
            "/b":{"x-internal":false,"get":{"responses":ok()}}
        }));
        run(
            &mut spec,
            vec![],
            vec![rules(json!({"extensions":["x-internal"]}))],
        );
        assert_eq!(
            spec["paths"]
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            vec!["/b"]
        );
    }

    #[test]
    fn webhooks_follow_the_non_path_rules() {
        let mut spec = json!({"openapi":"3.1.0","info":{"title":"T","version":"1"},"webhooks":{
            "hidden":{"post":{"tags":["internal"],"responses":ok()}},
            "shown":{"post":{"responses":ok()}}}});
        let r = rules(json!({"tags":["internal"],"paths":["/**"]}));
        assert!(run(&mut spec, vec![], vec![r]));
        assert_eq!(
            spec["webhooks"]
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            vec!["shown"]
        );
    }

    #[test]
    fn nothing_matching_changes_nothing() {
        let original = doc(json!({"/a":{"get":{"responses":ok()}},"/ref":{"$ref":"#/paths/~1a"}}));
        let mut spec = original.clone();
        assert!(!run(
            &mut spec,
            vec![],
            vec![rules(json!({"tags":["nope"]}))]
        ));
        assert_eq!(spec, original);
        // a $ref-only path item is kept unless a path glob excludes it
        let mut spec = original.clone();
        assert!(run(
            &mut spec,
            vec![],
            vec![rules(json!({"paths":["/ref"]}))]
        ));
        assert!(spec["paths"].get("/ref").is_none() && spec["paths"].get("/a").is_some());
    }

    #[test]
    fn surviving_paths_keep_their_order() {
        let mut spec = doc(json!({
            "/hidden":{"get":{"tags":["internal"],"responses":ok()}},
            "/b":{"get":{"responses":ok()}},
            "/c":{"get":{"responses":ok()}},
            "/d":{"get":{"responses":ok()}}
        }));
        run(&mut spec, vec![], vec![rules(json!({"tags":["internal"]}))]);
        assert_eq!(
            spec["paths"]
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            vec!["/b", "/c", "/d"]
        );
    }
}
