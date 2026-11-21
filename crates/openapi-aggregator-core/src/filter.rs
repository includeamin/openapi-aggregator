//! Hide operations (e.g. internal endpoints) from a source before it is merged, and prune
//! the components, security schemes and tags that only those operations used.

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

#[allow(dead_code)] // used by `remove_operations` in the next step
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
}
