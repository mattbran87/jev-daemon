//! One request to TypeSafe's Jev model: the body, and what a response means
//! (spec section 4, steps 6 and 7). `jev_http` does the sending.

use jev_proto::{ErrorCode, WireError};
use serde_json::{json, Value};

use crate::answer::valid_answers;
use crate::redact::{redact, redact_value, REDACTED};
use crate::shape::{depth, MAX_DEPTH};

pub const ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";
pub const MODEL: &str = "jev-latest";
/// Characters, counted after redaction.
pub const DETAIL_LIMIT: usize = 300;
pub const MAX_RESPONSE_BYTES: usize = 1 << 20;
pub const MAX_ERROR_BYTES: usize = 4096;

/// Why a request to Jev failed. `detail` is already redacted and cut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JevFailure {
    pub code: ErrorCode,
    pub status: Option<u16>,
    pub detail: Option<String>,
}

impl JevFailure {
    pub fn new(code: ErrorCode) -> JevFailure {
        JevFailure {
            code,
            status: None,
            detail: None,
        }
    }

    /// The wire error for this failure. The detail goes into the message.
    pub fn to_wire(&self) -> WireError {
        let summary = match self.code {
            ErrorCode::JevUnauthorized => "Jev refused the key".to_string(),
            ErrorCode::JevHttpError => match self.status {
                Some(status) => format!("Jev returned HTTP {status}"),
                None => "Jev returned an HTTP error".to_string(),
            },
            ErrorCode::JevTimeout => "Jev did not answer in time".to_string(),
            ErrorCode::JevUnreachable => "Jev could not be reached".to_string(),
            ErrorCode::BadAnswer => "Jev's answer failed the answer check".to_string(),
            other => format!("the request to Jev failed ({})", other.as_str()),
        };
        let message = match &self.detail {
            Some(detail) => format!("{summary}: {detail}"),
            None => summary,
        };
        WireError {
            status: self.status,
            ..WireError::new(self.code, message)
        }
    }
}

/// The body for Jev. Question text is caller content too, so it gets the same
/// redaction as state. `key` must come from `check_key`.
pub fn request_body(state: &Value, questions: &Value, key: &str, strict: bool) -> Value {
    json!({
        "model": MODEL,
        "state": redact_value(state, Some(key), strict),
        "questions": redact_value(questions, Some(key), strict),
    })
}

/// Redacted first, then cleaned, then cut. A cut first could leave part of a
/// token too short to match, and the PEM rule needs the newlines that the
/// cleaning removes. `None` when nothing is left.
pub fn make_detail(text: &str, key: &str) -> Option<String> {
    let mut text = text.to_string();
    for form in key_forms(key) {
        text = text.replace(&form, REDACTED);
    }
    let cleaned: String = redact(&text, Some(key))
        .chars()
        .map(|c| if is_unsafe(c) { ' ' } else { c })
        .collect();
    let detail: String = cleaned.trim().chars().take(DETAIL_LIMIT).collect();
    if detail.is_empty() {
        None
    } else {
        Some(detail)
    }
}

/// A character that could move the cursor, change the terminal, or reorder
/// text when the detail is printed.
fn is_unsafe(c: char) -> bool {
    c.is_control()
        || ('\u{202a}'..='\u{202e}').contains(&c)
        || ('\u{2066}'..='\u{2069}').contains(&c)
        || matches!(c, '\u{200e}' | '\u{200f}' | '\u{061c}')
}

/// The encoded forms of the key that an error page might echo: JSON-escaped,
/// JSON-escaped with `\/`, and percent-encoded. Longest first, so a shorter
/// form never cuts into a longer one. Forms equal to the literal key are left
/// to `redact`.
fn key_forms(key: &str) -> Vec<String> {
    let quoted = serde_json::to_string(key).expect("a string always serializes");
    let json = quoted[1..quoted.len() - 1].to_string();
    let slashed = json.replace('/', "\\/");
    let mut forms: Vec<String> = Vec::new();
    for form in [json, slashed, percent_encode(key)] {
        if form != key && !forms.contains(&form) {
            forms.push(form);
        }
    }
    forms.sort_by_key(|form| std::cmp::Reverse(form.len()));
    forms
}

/// Every byte that is not an ASCII letter, digit, `-`, `.`, `_`, or `~`
/// becomes `%XX`, uppercase.
fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// True when an object key, at any level, contains the key. Such an answer is
/// refused rather than renamed, since renamed keys could collide. Called only
/// after the depth check, so the recursion is bounded.
fn names_key(value: &Value, key: &str) -> bool {
    match value {
        Value::Array(items) => items.iter().any(|item| names_key(item, key)),
        Value::Object(map) => map
            .iter()
            .any(|(name, item)| name.contains(key) || names_key(item, key)),
        _ => false,
    }
}

/// The encoded forms of the key and then the literal key, longest first, as
/// `make_detail` replaces them. Escaping only adds characters, so every form
/// that differs from the key is longer than it, and the key goes last.
fn scrub_forms(key: &str) -> Vec<String> {
    let mut forms = key_forms(key);
    forms.push(key.to_string());
    forms
}

/// Replaces the key and each of its encoded `forms` in every string value.
/// Object keys are checked by `names_key` instead. Called only after the
/// depth check, so the recursion is bounded.
fn scrub_key(value: &mut Value, forms: &[String]) {
    match value {
        Value::String(text) => {
            for form in forms {
                if text.contains(form.as_str()) {
                    *text = text.replace(form.as_str(), REDACTED);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| scrub_key(item, forms)),
        Value::Object(map) => map.values_mut().for_each(|item| scrub_key(item, forms)),
        _ => {}
    }
}

/// What one final response means. `body` is `None` when it went over its cap.
/// Retries happen before this, so a 429 or a 529 here is final.
pub fn read_response(
    status: u16,
    body: Option<&[u8]>,
    questions: &Value,
    key: &str,
) -> Result<Value, JevFailure> {
    if status == 200 {
        let answers = body
            .and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok())
            .and_then(|mut value| value.get_mut("answers").map(Value::take));
        return match answers {
            Some(mut answers)
                if depth(&answers) <= MAX_DEPTH
                    && valid_answers(&answers, questions)
                    && !names_key(&answers, key) =>
            {
                scrub_key(&mut answers, &scrub_forms(key));
                Ok(answers)
            }
            _ => Err(JevFailure::new(ErrorCode::BadAnswer)),
        };
    }
    let code = if status == 401 || status == 403 {
        ErrorCode::JevUnauthorized
    } else {
        ErrorCode::JevHttpError
    };
    let detail = body.and_then(|bytes| make_detail(&String::from_utf8_lossy(bytes), key));
    Err(JevFailure {
        code,
        status: Some(status),
        detail,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use jev_proto::WireCode;

    /// A key that matches no redaction shape, so only the key rule can catch it.
    const KEY: &str = "plainkey123";

    fn questions() -> Value {
        json!({"q": {"type": "noul", "text": "is it?"}})
    }

    #[test]
    fn the_body_names_the_model_and_redacts_the_key() {
        let body = request_body(
            &json!({"note": "my key is plainkey123"}),
            &json!({"q": {"type": "noul", "text": "is plainkey123 ok?"}}),
            KEY,
            false,
        );
        assert_eq!(body["model"], "jev-latest");
        assert_eq!(body["state"]["note"], "my key is [redacted]");
        assert_eq!(body["questions"]["q"]["text"], "is [redacted] ok?");
        assert_eq!(body["questions"]["q"]["type"], "noul");
        assert!(!body.to_string().contains(KEY));
    }

    #[test]
    fn strict_redaction_reaches_the_body() {
        let state = json!({"password": "hunter2 has spaces"});
        assert_eq!(
            request_body(&state, &questions(), KEY, false)["state"]["password"],
            "hunter2 has spaces"
        );
        assert_eq!(
            request_body(&state, &questions(), KEY, true)["state"]["password"],
            "[redacted]"
        );
    }

    #[test]
    fn a_good_answer_passes() {
        let body = br#"{"answers": {"q": {"type": "noul", "noul": 0.8}}, "extra": 1}"#;
        assert_eq!(
            read_response(200, Some(body), &questions(), KEY),
            Ok(json!({"q": {"type": "noul", "noul": 0.8}}))
        );
    }

    #[test]
    fn a_bad_or_oversized_answer_is_bad_answer() {
        let bodies: [Option<&[u8]>; 6] = [
            Some(b"not json"),
            Some(br#"{"answers": {"q": {"type": "score"}}}"#),
            Some(br#"{"answers": {"q": {"type": "noul", "noul": 2}}}"#),
            Some(br#"{"other": 1}"#),
            Some(b"[1, 2]"),
            None,
        ];
        for body in bodies {
            assert_eq!(
                read_response(200, body, &questions(), KEY),
                Err(JevFailure::new(ErrorCode::BadAnswer))
            );
        }
    }

    #[test]
    fn statuses_map_to_codes() {
        assert_eq!(
            read_response(401, Some(b"unknown key plainkey123"), &questions(), KEY),
            Err(JevFailure {
                code: ErrorCode::JevUnauthorized,
                status: Some(401),
                detail: Some("unknown key [redacted]".to_string()),
            })
        );
        assert_eq!(
            read_response(403, None, &questions(), KEY)
                .unwrap_err()
                .code,
            ErrorCode::JevUnauthorized
        );
        for status in [302, 400, 429, 500, 529] {
            assert_eq!(
                read_response(status, Some(b""), &questions(), KEY),
                Err(JevFailure {
                    code: ErrorCode::JevHttpError,
                    status: Some(status),
                    detail: None,
                })
            );
        }
    }

    #[test]
    fn the_detail_is_redacted_before_it_is_cut() {
        // The token starts 10 characters before the cut. Cut first, its stub
        // "sk-ABCDEFG" would be too short for the sk- rule and would leak.
        let text = format!("{} sk-ABCDEFGHIJKLMNOPQRSTUVWX", "x".repeat(289));
        let detail = make_detail(&text, KEY).unwrap();
        assert!(!detail.contains("sk-"), "{detail}");
        assert!(detail.ends_with("[redacted]"), "{detail}");
        assert_eq!(detail.chars().count(), DETAIL_LIMIT);
    }

    #[test]
    fn the_detail_counts_characters_not_bytes() {
        let detail = make_detail(&"é".repeat(400), KEY).unwrap();
        assert_eq!(detail.chars().count(), DETAIL_LIMIT);
    }

    #[test]
    fn an_empty_detail_is_none() {
        assert_eq!(make_detail("  \n", KEY), None);
        assert_eq!(make_detail("", KEY), None);
    }

    #[test]
    fn failures_become_wire_errors() {
        let wire = JevFailure {
            code: ErrorCode::JevHttpError,
            status: Some(502),
            detail: Some("bad gateway".to_string()),
        }
        .to_wire();
        assert_eq!(wire.code, WireCode::Known(ErrorCode::JevHttpError));
        assert_eq!(wire.status, Some(502));
        assert_eq!(wire.message, "Jev returned HTTP 502: bad gateway");
        let timeout = JevFailure::new(ErrorCode::JevTimeout).to_wire();
        assert_eq!(timeout.message, "Jev did not answer in time");
        assert_eq!(timeout.status, None);
    }

    #[test]
    fn strict_redaction_reaches_the_questions() {
        let questions =
            json!({"q": {"type": "noul", "text": "is it?", "password": "hunter2 has spaces"}});
        assert_eq!(
            request_body(&json!({}), &questions, KEY, false)["questions"]["q"]["password"],
            "hunter2 has spaces"
        );
        assert_eq!(
            request_body(&json!({}), &questions, KEY, true)["questions"]["q"]["password"],
            "[redacted]"
        );
    }

    #[test]
    fn only_200_is_an_answer() {
        let body = br#"{"answers": {"q": {"type": "noul", "noul": 0.8}}}"#;
        for status in [201, 204] {
            assert_eq!(
                read_response(status, Some(body), &questions(), KEY)
                    .unwrap_err()
                    .code,
                ErrorCode::JevHttpError
            );
        }
    }

    #[test]
    fn each_failure_has_its_summary() {
        let cases = [
            (ErrorCode::JevUnauthorized, "Jev refused the key"),
            (ErrorCode::JevHttpError, "Jev returned an HTTP error"),
            (ErrorCode::JevUnreachable, "Jev could not be reached"),
            (ErrorCode::BadAnswer, "Jev's answer failed the answer check"),
            (ErrorCode::Internal, "the request to Jev failed (internal)"),
        ];
        for (code, message) in cases {
            assert_eq!(JevFailure::new(code).to_wire().message, message);
        }
    }

    #[test]
    fn control_characters_leave_the_detail() {
        let text = "a\x1b[2Jb\x1b]0;x\x07c\rd\ne\tf\u{202e}g";
        let detail = make_detail(text, KEY).unwrap();
        assert_eq!(detail, "a [2Jb ]0;x c d e f g");
        for c in ['\x1b', '\x07', '\r', '\n', '\t', '\u{202e}'] {
            assert!(!detail.contains(c), "{detail:?}");
        }
    }

    #[test]
    fn bidi_controls_leave_the_detail_and_their_neighbours_stay() {
        let marks = ['\u{200e}', '\u{200f}', '\u{061c}'];
        for c in ('\u{202a}'..='\u{202e}')
            .chain('\u{2066}'..='\u{2069}')
            .chain(marks)
        {
            assert_eq!(make_detail(&format!("a{c}b"), KEY).as_deref(), Some("a b"));
        }
        for c in [
            '\u{2029}', '\u{202f}', '\u{2065}', '\u{206a}', '\u{200d}', '\u{2010}', '\u{061b}',
            '\u{061d}',
        ] {
            let text = format!("a{c}b");
            assert_eq!(make_detail(&text, KEY), Some(text));
        }
    }

    #[test]
    fn encoded_echoes_of_the_key_leave_the_detail() {
        let key = "tsk/ab+cd\"ef";
        for text in [
            "bad key tsk\\/ab+cd\\\"ef",
            "bad key tsk/ab+cd\\\"ef",
            "bad key tsk%2Fab%2Bcd%22ef",
            "bad key tsk/ab+cd\"ef",
        ] {
            assert_eq!(
                make_detail(text, key).as_deref(),
                Some("bad key [redacted]"),
                "{text}"
            );
        }
    }

    #[test]
    fn a_private_key_is_redacted_before_its_newlines_go() {
        let text = "-----BEGIN RSA PRIVATE KEY-----\nQUJD\nREVG\n- done";
        assert_eq!(make_detail(text, KEY).as_deref(), Some("[redacted] - done"));
    }

    #[test]
    fn the_longest_key_form_is_replaced_first() {
        // The JSON form `/ab\"cd` sits inside the `\/` form `\/ab\"cd`.
        let key = "/ab\"cd";
        assert_eq!(
            make_detail("bad key \\/ab\\\"cd", key).as_deref(),
            Some("bad key [redacted]")
        );
    }

    #[test]
    fn the_percent_form_keeps_the_unreserved_characters() {
        let key = "Az09-._~/ é";
        assert_eq!(percent_encode(key), "Az09-._~%2F%20%C3%A9");
        assert_eq!(
            make_detail("bad key Az09-._~%2F%20%C3%A9", key).as_deref(),
            Some("bad key [redacted]")
        );
    }

    #[test]
    fn the_encoded_key_forms_leave_the_answers() {
        let key = "tsk/ab+cd\"ef";
        let body = serde_json::to_vec(&json!({"answers": {"q": {
            "type": "noul", "noul": 0.8,
            "why": "tsk%2Fab%2Bcd%22ef",
            "notes": ["tsk/ab+cd\\\"ef", "tsk\\/ab+cd\\\"ef", "x tsk/ab+cd\"ef y"],
        }}}))
        .unwrap();
        assert_eq!(
            read_response(200, Some(&body), &questions(), key),
            Ok(
                json!({"q": {"type": "noul", "noul": 0.8, "why": "[redacted]",
                "notes": ["[redacted]", "[redacted]", "x [redacted] y"]}})
            )
        );
        // The key sits inside its JSON-escaped form, so the longer form goes first.
        let key = "\\tsk\\";
        let body = serde_json::to_vec(&json!({"answers": {"q": {
            "type": "noul", "noul": 0.8, "why": "\\\\tsk\\\\",
        }}}))
        .unwrap();
        assert_eq!(
            read_response(200, Some(&body), &questions(), key),
            Ok(json!({"q": {"type": "noul", "noul": 0.8, "why": "[redacted]"}}))
        );
    }

    #[test]
    fn the_key_leaves_the_answers() {
        let body = br#"{"answers": {"q": {"type": "noul", "noul": 0.8, "why": "your key plainkey123", "notes": ["plainkey123", {"deep": "x plainkey123 y"}]}}}"#;
        assert_eq!(
            read_response(200, Some(body), &questions(), KEY),
            Ok(
                json!({"q": {"type": "noul", "noul": 0.8, "why": "your key [redacted]",
                "notes": ["[redacted]", {"deep": "x [redacted] y"}]}})
            )
        );
        let clean = br#"{"answers": {"q": {"type": "noul", "noul": 0.8, "why": "sk-ABCDEFGHIJKLMNOPQRSTUVWX"}}}"#;
        assert_eq!(
            read_response(200, Some(clean), &questions(), KEY),
            Ok(json!({"q": {"type": "noul", "noul": 0.8, "why": "sk-ABCDEFGHIJKLMNOPQRSTUVWX"}}))
        );
        // Only the key goes; the shape rules do not run on answers.
        let both = br#"{"answers": {"q": {"type": "noul", "noul": 0.8, "why": "plainkey123 sk-ABCDEFGHIJKLMNOPQRSTUVWX"}}}"#;
        assert_eq!(
            read_response(200, Some(both), &questions(), KEY),
            Ok(
                json!({"q": {"type": "noul", "noul": 0.8, "why": "[redacted] sk-ABCDEFGHIJKLMNOPQRSTUVWX"}})
            )
        );
    }

    #[test]
    fn an_answer_that_names_the_key_is_bad_answer() {
        let bodies: [&[u8]; 3] = [
            br#"{"answers": {"q": {"type": "noul", "noul": 0.8, "why": {"plainkey123": 1}}}}"#,
            br#"{"answers": {"q": {"type": "noul", "noul": 0.8}, "plainkey123": {}}}"#,
            br#"{"answers": {"q": {"type": "noul", "noul": 0.8, "why": [{"x plainkey123 y": 1}]}}}"#,
        ];
        for body in bodies {
            assert_eq!(
                read_response(200, Some(body), &questions(), KEY),
                Err(JevFailure::new(ErrorCode::BadAnswer)),
                "{}",
                String::from_utf8_lossy(body)
            );
        }
        let fine = br#"{"answers": {"q": {"type": "noul", "noul": 0.8, "why": {"plainkey12": [{"note": 1}]}}}}"#;
        assert_eq!(
            read_response(200, Some(fine), &questions(), KEY),
            Ok(json!({"q": {"type": "noul", "noul": 0.8, "why": {"plainkey12": [{"note": 1}]}}}))
        );
    }

    /// An answer body whose `answers` value is `depth` levels deep.
    fn deep_answer(depth: usize) -> Vec<u8> {
        // `answers` and `q` are two levels; the arrays in `deep` are the rest.
        let mut deep = json!([]);
        for _ in 1..depth - 2 {
            deep = json!([deep]);
        }
        let answers = json!({"q": {"type": "noul", "noul": 0.8, "deep": deep}});
        assert_eq!(crate::shape::depth(&answers), depth);
        serde_json::to_vec(&json!({"answers": answers})).unwrap()
    }

    #[test]
    fn answers_past_the_depth_cap_are_bad_answer() {
        assert!(read_response(200, Some(&deep_answer(64)), &questions(), KEY).is_ok());
        assert_eq!(
            read_response(200, Some(&deep_answer(65)), &questions(), KEY),
            Err(JevFailure::new(ErrorCode::BadAnswer))
        );
    }

    #[test]
    fn an_error_body_that_is_not_utf8_still_gives_a_detail() {
        assert_eq!(
            read_response(502, Some(b"bad \xff\xfe gateway"), &questions(), KEY)
                .unwrap_err()
                .detail
                .as_deref(),
            Some("bad \u{fffd}\u{fffd} gateway")
        );
    }

    #[test]
    fn an_extra_question_id_in_the_answers_passes() {
        let body = br#"{"answers": {"q": {"type": "noul", "noul": 0.8}, "extra": {"type": "noul", "noul": 0.1}}}"#;
        assert_eq!(
            read_response(200, Some(body), &questions(), KEY),
            Ok(json!({"q": {"type": "noul", "noul": 0.8}, "extra": {"type": "noul", "noul": 0.1}}))
        );
    }
}
