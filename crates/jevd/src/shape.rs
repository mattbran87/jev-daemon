//! The shape check on each `ask` (spec section 4, step 1).

use serde_json::Value;

pub const MAX_QUESTIONS: usize = 20;
pub const MAX_DEPTH: usize = 64;
pub const QUESTION_TYPES: [&str; 2] = ["noul", "score"];

/// 1 to 64 characters from `[A-Za-z0-9_-]`.
pub fn valid_question_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// 1 to 20 ids, each mapped to an object whose `type` is `noul` or `score`.
pub fn valid_questions(questions: &Value) -> bool {
    let Some(map) = questions.as_object() else {
        return false;
    };
    if map.is_empty() || map.len() > MAX_QUESTIONS {
        return false;
    }
    map.iter().all(|(id, question)| {
        valid_question_id(id)
            && question
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| QUESTION_TYPES.contains(&kind))
    })
}

// `depth` is recursive and works on an already parsed tree. Its stack safety
// depends on `serde_json`'s default recursion limit of 128, so a caller must
// not parse with `unbounded_depth` or `disable_recursion_limit`.
/// How many arrays and objects sit inside each other at the deepest point.
pub fn depth(value: &Value) -> usize {
    match value {
        Value::Array(items) => 1 + items.iter().map(depth).max().unwrap_or(0),
        Value::Object(map) => 1 + map.values().map(depth).max().unwrap_or(0),
        _ => 0,
    }
}

/// Ok when an `ask` has the right shape, else the problem in a few words.
pub fn check_ask(state: &Value, questions: &Value) -> Result<(), &'static str> {
    if depth(state) > MAX_DEPTH {
        return Err("state is nested too deeply");
    }
    if !valid_questions(questions) {
        return Err("questions must map 1 to 20 ids to objects with type noul or score");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn nested(levels: usize) -> Value {
        let mut value = json!("leaf");
        for _ in 0..levels {
            value = json!([value]);
        }
        value
    }

    #[test]
    fn questions_follow_the_prototype_rules() {
        assert!(valid_questions(
            &json!({"q1": {"type": "noul", "text": "Is this a test?"}})
        ));
        assert!(valid_questions(&json!({"s": {"type": "score"}})));
        assert!(!valid_questions(&json!({})));
        assert!(!valid_questions(&json!([])));
        assert!(!valid_questions(&json!({"bad id!": {"type": "noul"}})));
        assert!(!valid_questions(&json!({"q": {"type": "other"}})));
        assert!(!valid_questions(&json!({"q": "noul"})));
        let mut long_id = serde_json::Map::new();
        long_id.insert("a".repeat(65), json!({"type": "noul"}));
        assert!(!valid_questions(&Value::Object(long_id)));
        let twenty: serde_json::Map<String, Value> = (0..20)
            .map(|i| (format!("q{i}"), json!({"type": "noul"})))
            .collect();
        assert!(valid_questions(&Value::Object(twenty.clone())));
        let mut twenty_one = twenty;
        twenty_one.insert("q20".into(), json!({"type": "noul"}));
        assert!(!valid_questions(&Value::Object(twenty_one)));
    }

    #[test]
    fn rules_hold_at_their_edges() {
        assert!(!valid_question_id(""));
        assert!(valid_question_id(&"a".repeat(64)));
        assert!(!valid_question_id(&"a".repeat(65)));
        assert!(valid_question_id("a_b-c"));
        assert!(!valid_question_id("a b"));
        assert!(!valid_question_id("a.b"));
        assert!(!valid_question_id("é"));

        assert!(valid_questions(&json!({"q": {"type": "noul"}})));
        assert!(valid_questions(&json!({"q": {"type": "score"}})));
        assert!(!valid_questions(&json!({"q": {"type": "x"}})));
        assert!(!valid_questions(&json!({"q": {"type": 1}})));
        // One bad question next to a good one must fail.
        assert!(!valid_questions(
            &json!({"q1": {"type": "noul"}, "q2": {"type": "bad"}})
        ));

        assert_eq!(depth(&json!({"a": [1], "b": [[1]]})), 3);

        // A JSON null counts as a present `state`: serde rejects a missing
        // `state`, and the spec only asks that `state` is present.
        assert_eq!(
            check_ask(&json!(null), &json!({"q": {"type": "noul"}})),
            Ok(())
        );
    }

    #[test]
    fn depth_counts_arrays_and_objects() {
        assert_eq!(depth(&json!("text")), 0);
        assert_eq!(depth(&json!({"a": [1]})), 2);
        assert_eq!(depth(&nested(64)), 64);
    }

    #[test]
    fn check_ask_refuses_deep_state_and_bad_questions() {
        let questions = json!({"q": {"type": "noul"}});
        assert_eq!(check_ask(&nested(64), &questions), Ok(()));
        assert_eq!(
            check_ask(&nested(65), &questions),
            Err("state is nested too deeply")
        );
        assert!(check_ask(&json!("s"), &json!({})).is_err());
    }
}
