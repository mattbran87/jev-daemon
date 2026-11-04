//! What a command prints, and its exit code (spec section 6, "Exit codes").
//! Human text by default; `--json` prints one JSON object on stdout.

use std::io::Write;

use jev_proto::{ErrorCode, WireError};
use serde_json::{json, Map, Value};

use crate::env::Env;

/// Why a command failed.
#[derive(Debug)]
pub struct Failure {
    /// Boxed, so a `Result<_, Failure>` stays small.
    pub error: Box<WireError>,
    /// A bad command line: exit code 2, whatever the error code.
    pub usage: bool,
    /// Extra fields for the JSON output, such as a pairing code.
    pub data: Map<String, Value>,
}

impl Failure {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Failure {
        Failure::from(WireError::new(code, message))
    }

    /// A bad command line. The JSON code is `bad-request`; the exit code is 2.
    pub fn usage(message: impl Into<String>) -> Failure {
        Failure {
            usage: true,
            ..Failure::new(ErrorCode::BadRequest, message)
        }
    }

    pub fn with(mut self, key: &str, value: Value) -> Failure {
        self.data.insert(key.to_string(), value);
        self
    }

    pub fn exit_code(&self) -> i32 {
        if self.usage {
            2
        } else {
            self.error.code.exit_code()
        }
    }
}

impl From<WireError> for Failure {
    fn from(error: WireError) -> Failure {
        Failure {
            error: Box::new(error),
            usage: false,
            data: Map::new(),
        }
    }
}

pub type Outcome = Result<(), Failure>;

/// A keyring error, as `internal`.
pub fn keyring_failure(error: impl std::fmt::Display) -> Failure {
    Failure::new(ErrorCode::Internal, format!("the keyring failed: {error}"))
}

/// Text from jevd or the keyring, safe to print: control, bidi, and
/// invisible format characters become spaces, so a reply cannot move the
/// cursor, hide text, or reorder the line on the screen.
pub fn clean(text: &str) -> String {
    text.chars()
        .map(|c| if unsafe_char(c) { ' ' } else { c })
        .collect()
}

/// As `clean`, for text of several lines: each line is cleaned, and the
/// line breaks between them stay. A `\r\n` line end counts as one break.
///
/// Use it only for text that jev itself writes, so it keeps only jev's own
/// line breaks. A field from jevd or Jev goes through `clean` first.
pub fn clean_lines(text: &str) -> String {
    text.split('\n')
        .map(|line| clean(line.strip_suffix('\r').unwrap_or(line)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Serialized JSON with every character that `clean` would replace written
/// as a `\uXXXX` escape, two escapes (a UTF-16 surrogate pair) above
/// U+FFFF. Such characters can only stand inside JSON strings, so the text
/// stays valid JSON with the same value, and a terminal shows it as is.
pub fn escape_json(json: &str) -> String {
    let mut text = String::with_capacity(json.len());
    for c in json.chars() {
        if unsafe_char(c) {
            let mut units = [0u16; 2];
            for unit in c.encode_utf16(&mut units) {
                text.push_str(&format!("\\u{unit:04x}"));
            }
        } else {
            text.push(c);
        }
    }
    text
}

fn unsafe_char(c: char) -> bool {
    c.is_control()
        || ('\u{200b}'..='\u{200d}').contains(&c)
        || ('\u{202a}'..='\u{202e}').contains(&c)
        || ('\u{2060}'..='\u{2064}').contains(&c)
        || ('\u{2066}'..='\u{2069}').contains(&c)
        || ('\u{206a}'..='\u{206f}').contains(&c)
        || ('\u{fff9}'..='\u{fffb}').contains(&c)
        || ('\u{e0000}'..='\u{e007f}').contains(&c)
        || matches!(
            c,
            '\u{200e}' | '\u{200f}' | '\u{061c}' | '\u{2028}' | '\u{2029}' | '\u{feff}'
        )
}

/// Prints a success: `human` as cleaned lines, or `data` with `"ok": true`
/// as JSON, through `escape_json`. `data` is an object, or null for no fields.
///
/// `human` goes through `clean_lines`, which keeps only the line breaks that
/// jev itself writes. A field from jevd or Jev goes through `clean` first.
pub fn success(env: &mut Env, json: bool, human: &str, data: Value) {
    debug_assert!(data.is_object() || data.is_null());
    if json {
        let mut object = match data {
            Value::Object(map) => map,
            _ => Map::new(),
        };
        object.insert("ok".into(), Value::Bool(true));
        let line = escape_json(&Value::Object(object).to_string());
        let _ = writeln!(env.out, "{line}");
    } else if !human.is_empty() {
        let _ = writeln!(env.out, "{}", clean_lines(human));
    }
}

/// The failure as the JSON object that `--json` and `jev ask` print.
pub fn failure_json(failure: &Failure) -> Value {
    let mut error = serde_json::to_value(&failure.error).expect("an error always serializes");
    error["message"] = json!(clean(&failure.error.message));
    let mut object = failure.data.clone();
    object.insert("ok".into(), Value::Bool(false));
    object.insert("error".into(), error);
    Value::Object(object)
}

/// Prints a failure and returns its exit code. Human text goes to stderr as
/// `jev: <code>: <message>`; with `--json`, the object goes to stdout.
pub fn report(env: &mut Env, json: bool, failure: &Failure) -> i32 {
    report_to(&mut *env.out, &mut *env.err, json, failure)
}

/// As `report`, on these streams, for when there is no `Env`.
pub fn report_to(out: &mut dyn Write, err: &mut dyn Write, json: bool, failure: &Failure) -> i32 {
    if json {
        let line = escape_json(&failure_json(failure).to_string());
        let _ = writeln!(out, "{line}");
    } else {
        let _ = writeln!(
            err,
            "jev: {}: {}",
            clean(failure.error.code.as_str()),
            clean(&failure.error.message)
        );
    }
    failure.exit_code()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use jev_proto::{Paths, WireCode};
    use jevd::jev_http::JevClient;
    use jevd::keystore::MemoryKeyring;

    use crate::env::{Buffer, TextInput};
    use crate::term::ScriptedTerminal;

    /// An Env with in-memory parts, and its stdout and stderr.
    fn test_env() -> (Env, Buffer, Buffer, tempfile::TempDir) {
        let home = tempfile::tempdir().unwrap();
        let out = Buffer::default();
        let err = Buffer::default();
        let env = Env {
            paths: Paths::under(home.path()).unwrap(),
            jevd: home.path().join("jevd"),
            store: Arc::new(MemoryKeyring::new()),
            terminal: Box::new(ScriptedTerminal::none()),
            stdin: Box::new(TextInput(None)),
            out: Box::new(out.clone()),
            err: Box::new(err.clone()),
            vars: Box::new(|_| None),
            jev_client: Box::new(JevClient::new),
        };
        (env, out, err, home)
    }

    #[test]
    fn clean_turns_control_and_bidi_characters_into_spaces() {
        assert_eq!(
            clean("a\nb\x1b[2Jc\u{202e}d\u{2066}e\u{2028}f"),
            "a b [2Jc d e f"
        );
        assert_eq!(clean("plain text"), "plain text");
        for c in [
            '\u{9b}',
            '\u{200f}',
            '\u{061c}',
            '\u{2029}',
            '\u{200b}',
            '\u{200d}',
            '\u{2060}',
            '\u{2064}',
            '\u{206a}',
            '\u{206f}',
            '\u{feff}',
            '\u{fff9}',
            '\u{fffb}',
            '\u{e0000}',
            '\u{e0041}',
            '\u{e007f}',
        ] {
            assert_eq!(clean(&format!("a{c}b")), "a b", "U+{:04X}", c as u32);
        }
    }

    #[test]
    fn clean_lines_keeps_the_line_breaks() {
        assert_eq!(clean_lines("a\u{202e}\r\nb\x1bc\n"), "a \nb c\n");
        assert_eq!(clean_lines("ab\rXY\n"), "ab XY\n");
        assert_eq!(clean_lines("a\r\r\n"), "a \n");
    }

    #[test]
    fn escape_json_escapes_unsafe_characters_and_keeps_the_value() {
        let value = json!({"a\u{202e}": "b\u{85}c\u{e0041}d\u{1b}e", "plain": "f"});
        let text = escape_json(&value.to_string());
        assert!(text.is_ascii(), "{text}");
        let esc = |hex: &str| format!("\\u{hex}");
        assert!(
            text.contains(&esc("202e")) && text.contains(&esc("0085")),
            "{text}"
        );
        assert!(text.contains(&(esc("db40") + &esc("dc41"))), "{text}");
        assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), value);
    }

    #[test]
    fn escape_json_keeps_an_escaped_backslash_apart_from_a_real_character() {
        let value = json!({"a": "\\u202e\u{202e}"});
        let text = escape_json(&value.to_string());
        assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), value);
    }

    #[test]
    fn every_error_code_has_its_spec_exit_code() {
        let table = [
            (ErrorCode::Internal, 1),
            (ErrorCode::Pending, 3),
            (ErrorCode::NotPaired, 4),
            (ErrorCode::Revoked, 4),
            (ErrorCode::RateLimited, 5),
            (ErrorCode::NoKey, 6),
            (ErrorCode::BadKey, 6),
            (ErrorCode::JevUnauthorized, 6),
            (ErrorCode::JevHttpError, 7),
            (ErrorCode::JevTimeout, 7),
            (ErrorCode::JevUnreachable, 7),
            (ErrorCode::BadAnswer, 7),
            (ErrorCode::DaemonUnavailable, 8),
            (ErrorCode::ProtocolUnsupported, 8),
            (ErrorCode::Forbidden, 9),
            (ErrorCode::BadRequest, 9),
            (ErrorCode::TooLarge, 9),
        ];
        assert_eq!(table.len(), ErrorCode::ALL.len());
        for (code, exit) in table {
            assert_eq!(Failure::new(code, "x").exit_code(), exit, "{code:?}");
        }
        assert_eq!(Failure::usage("x").exit_code(), 2);
    }

    #[test]
    fn an_unknown_code_exits_1_and_the_json_keeps_the_raw_code() {
        let failure = Failure::from(WireError {
            code: WireCode::from_raw("quota-exceeded"),
            ..WireError::new(ErrorCode::Internal, "try later")
        });
        assert_eq!(failure.exit_code(), 1);
        assert_eq!(failure_json(&failure)["error"]["code"], "quota-exceeded");
    }

    #[test]
    fn the_json_cleans_the_message_and_carries_extra_fields() {
        let failure = Failure::new(ErrorCode::Pending, "run jev allow\u{202e}")
            .with("pair_code", json!("ABCD2345"));
        let value = failure_json(&failure);
        assert_eq!(value["ok"], false);
        assert_eq!(value["error"]["code"], "pending");
        assert_eq!(value["error"]["message"], "run jev allow ");
        assert_eq!(value["pair_code"], "ABCD2345");
    }

    #[test]
    fn report_writes_a_clean_line_to_stderr_in_human_mode() {
        let (mut env, out, err, _home) = test_env();
        let failure = Failure::new(ErrorCode::NoKey, "add\x1b[2J a\u{202e} key");
        assert_eq!(report(&mut env, false, &failure), 6);
        assert_eq!(err.text(), "jev: no-key: add [2J a  key\n");
        assert_eq!(out.text(), "");
    }

    #[test]
    fn report_writes_one_json_object_to_stdout_in_json_mode() {
        let (mut env, out, err, _home) = test_env();
        let failure = Failure::usage("bad flag");
        assert_eq!(report(&mut env, true, &failure), 2);
        let text = out.text();
        assert_eq!(text.lines().count(), 1);
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["ok"], false);
        assert_eq!(value["error"]["code"], "bad-request");
        assert_eq!(err.text(), "");
    }

    #[test]
    fn success_adds_ok_true_to_the_json() {
        let (mut env, out, err, _home) = test_env();
        success(&mut env, true, "ignored", json!({"count": 2}));
        let text = out.text();
        assert_eq!(text.lines().count(), 1);
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value, json!({"count": 2, "ok": true}));
        assert_eq!(err.text(), "");
    }

    #[test]
    fn success_cleans_the_human_text_and_skips_it_when_empty() {
        let (mut env, out, err, _home) = test_env();
        success(&mut env, false, "", Value::Null);
        assert_eq!(out.text(), "");
        success(&mut env, false, "a\u{202e}b\nc\x1bd", Value::Null);
        assert_eq!(out.text(), "a b\nc d\n");
        assert_eq!(err.text(), "");
    }
}
