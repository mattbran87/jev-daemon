//! The answer check (spec section 4, step 7).

use serde_json::Value;

fn finite(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(Value::as_f64)
        .filter(|number| number.is_finite())
}

/// Every question has an answer of its own type with usable numbers.
/// Extra answers and extra fields pass through: the check tests only what jev reads.
pub fn valid_answers(answers: &Value, questions: &Value) -> bool {
    let (Some(answers), Some(questions)) = (answers.as_object(), questions.as_object()) else {
        return false;
    };
    questions.iter().all(|(id, question)| {
        let Some(answer) = answers.get(id).filter(|answer| answer.is_object()) else {
            return false;
        };
        let kind = question.get("type");
        if answer.get("type") != kind {
            return false;
        }
        match kind.and_then(Value::as_str) {
            Some("noul") => finite(answer.get("noul")).is_some_and(|n| (0.0..=1.0).contains(&n)),
            Some("score") => {
                finite(answer.get("score")).is_some() && finite(answer.get("confidence")).is_some()
            }
            _ => true,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn answers_match_their_questions() {
        let noul = json!({"q1": {"type": "noul"}});
        assert!(valid_answers(
            &json!({"q1": {"type": "noul", "noul": 0.8}}),
            &noul
        ));
        assert!(valid_answers(
            &json!({"q1": {"type": "noul", "noul": 0}}),
            &noul
        ));
        assert!(valid_answers(
            &json!({"q1": {"type": "noul", "noul": 1}}),
            &noul
        ));
        assert!(!valid_answers(
            &json!({"q1": {"type": "noul", "noul": 1.5}}),
            &noul
        ));
        assert!(!valid_answers(
            &json!({"q1": {"type": "noul", "noul": true}}),
            &noul
        ));
        assert!(!valid_answers(
            &json!({"q1": {"type": "score", "score": 1, "confidence": 1}}),
            &noul
        ));
        assert!(!valid_answers(&json!({}), &noul));
        assert!(!valid_answers(&json!([]), &noul));
        assert!(!valid_answers(&json!({"q1": 0.8}), &noul));

        let score = json!({"s": {"type": "score"}});
        assert!(valid_answers(
            &json!({"s": {"type": "score", "score": 7, "confidence": 0.5}}),
            &score
        ));
        assert!(!valid_answers(
            &json!({"s": {"type": "score", "score": 7}}),
            &score
        ));
        assert!(!valid_answers(
            &json!({"s": {"type": "score", "score": "7", "confidence": 1}}),
            &score
        ));
    }

    #[test]
    fn answer_type_and_shape_edges() {
        // The answer must be an object even when the question names no type.
        assert!(!valid_answers(&json!({"q": 5}), &json!({"q": {}})));
        assert!(valid_answers(&json!({"q": {}}), &json!({"q": {}})));
        // The answer's type must equal the question's, even when the fields would pass.
        let score = json!({"q": {"type": "score"}});
        assert!(!valid_answers(
            &json!({"q": {"type": "noul", "score": 1, "confidence": 1}}),
            &score
        ));
        let noul = json!({"q": {"type": "noul"}});
        assert!(!valid_answers(
            &json!({"q": {"type": "score", "noul": 0.5}}),
            &noul
        ));
        // A question type the check does not know passes when the types match.
        let other = json!({"q": {"type": "other"}});
        assert!(valid_answers(&json!({"q": {"type": "other"}}), &other));
    }

    #[test]
    fn extra_answers_and_fields_pass_through() {
        let noul = json!({"q1": {"type": "noul"}});
        let answers = json!({"q1": {"type": "noul", "noul": 0.2, "why": "x"}, "q9": {}});
        assert!(valid_answers(&answers, &noul));
    }
}
