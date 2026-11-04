//! Replace the API key and anything shaped like a secret before a send
//! (spec section 4, step 5). Ported from the prototype's `redact.py`.

use regex::{Captures, NoExpand, Regex};
use serde_json::Value;
use std::sync::OnceLock;

pub const REDACTED: &str = "[redacted]";

/// A regex compiled once, on first use.
macro_rules! regex {
    ($pattern:expr) => {{
        static CELL: OnceLock<Regex> = OnceLock::new();
        CELL.get_or_init(|| Regex::new($pattern).expect("redaction pattern compiles"))
    }};
}

/// Replace each PEM private key, from BEGIN to END or to the end of its base64 lines.
pub fn redact_private_keys(text: &str) -> String {
    let begin_re = regex!(r"-----BEGIN [A-Z ]*PRIVATE KEY-----");
    let end_re = regex!(r"-----END [A-Z ]*PRIVATE KEY-----");
    let body_re = regex!(r"\A(?:\r?\n[A-Za-z0-9+/=]+)*");
    let mut out = String::with_capacity(text.len());
    let mut pos = 0;
    let mut end_missing = false;
    while let Some(begin) = begin_re.find_at(text, pos) {
        let end = if end_missing {
            None
        } else {
            end_re.find_at(text, begin.end())
        };
        let stop = match end {
            Some(end) => end.end(),
            None => {
                // No END after this point means none after any later BEGIN either.
                end_missing = true;
                begin.end() + body_re.find(&text[begin.end()..]).map_or(0, |m| m.end())
            }
        };
        out.push_str(&text[pos..begin.start()]);
        out.push_str(REDACTED);
        pos = stop;
    }
    out.push_str(&text[pos..]);
    out
}

/// True when `token` is standard padded base64 whose decoded bytes hold a colon.
fn base64_has_colon(token: &str) -> bool {
    let bytes = token.as_bytes();
    if bytes.is_empty() || bytes.len() % 4 != 0 {
        return false;
    }
    let mut decoded = Vec::with_capacity(bytes.len() / 4 * 3);
    let (mut bits, mut count) = (0u32, 0u32);
    for &b in bytes {
        let value = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => break, // padding: only at the end, as the pattern allows
        };
        bits = (bits << 6) | u32::from(value);
        count += 6;
        if count >= 8 {
            count -= 8;
            decoded.push((bits >> count) as u8);
            bits &= (1 << count) - 1;
        }
    }
    decoded.contains(&b':')
}

/// Specific token shapes. They run before the name rule, so "token=sk-..."
/// ends as "token=[redacted]" either way.
fn redact_token_shapes(text: &str) -> String {
    let bearer = regex!(r"(?-u:\b)(Bearer|bearer|Basic|basic)[ \t]+([A-Za-z0-9._~+/=-]{16,})");
    let text = bearer.replace_all(text, |caps: &Captures| {
        let token = &caps[2];
        if token
            .bytes()
            .any(|b| b.is_ascii_digit() || b == b'+' || b == b'/' || b == b'=')
        {
            format!("{} {}", &caps[1], REDACTED)
        } else {
            caps[0].to_string()
        }
    });
    // A short `Basic` value is `user:pass` in base64. It is redacted when it
    // decodes to something with a colon.
    let basic = regex!(r"(?-u:\b)(Basic|basic)[ \t]+([A-Za-z0-9+/]+={0,2})");
    let text = basic.replace_all(&text, |caps: &Captures| {
        if base64_has_colon(&caps[2]) {
            format!("{} {}", &caps[1], REDACTED)
        } else {
            caps[0].to_string()
        }
    });
    let mut text = text.into_owned();
    let shapes = [
        regex!(r"(?-u:\b)(?:sk|tsk)[-_][A-Za-z0-9_-]{16,}"),
        regex!(r"(?-u:\b)gh[pousr]_[A-Za-z0-9]{36,}"),
        regex!(r"(?-u:\b)github_pat_[A-Za-z0-9_]{20,}"),
        regex!(r"(?-u:\b)AKIA[0-9A-Z]{16}(?-u:\b)"),
        regex!(r"(?-u:\b)xox[abprs]-[A-Za-z0-9-]{10,}"),
    ];
    for shape in shapes {
        text = shape.replace_all(&text, NoExpand(REDACTED)).into_owned();
    }
    text
}

/// True when a name holds a secret word, such as `api_key` or `DB-PASSWORD`.
pub fn is_secret_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    regex!(r"(?:\A|[_-])(?:password|passwd|secret|token|api[_-]?key)(?:[_-]|\z)").is_match(&lower)
}

/// Replace the value after each `name = value` or `name: value` whose name holds a secret word.
pub fn redact_names(text: &str) -> String {
    let name_re = regex!(r#"([A-Za-z0-9_-]+)(["']?[ \t]*[:=][ \t]*)"#);
    let value_re = regex!(r#"\A(?:"(?:[^"\\]|\\.)*"|'[^']*'|[^ \t\r\n,;}\]]+)"#);
    let mut out = String::with_capacity(text.len());
    let mut pos = 0;
    while let Some(caps) = name_re.captures_at(text, pos) {
        let whole = caps.get(0).expect("group 0 always exists");
        let name = caps.get(1).expect("the name group always matches");
        out.push_str(&text[pos..whole.end()]);
        pos = whole.end();
        if is_secret_name(name.as_str()) {
            if let Some(value) = value_re.find(&text[pos..]) {
                out.push_str(REDACTED);
                pos += value.end();
            }
        }
    }
    out.push_str(&text[pos..]);
    out
}

/// Redact one string: the key itself, private keys, token shapes, then secret names.
pub fn redact(text: &str, key: Option<&str>) -> String {
    let text = match key.map(str::trim) {
        Some(key) if !key.is_empty() => text.replace(key, REDACTED),
        _ => text.to_string(),
    };
    let text = redact_private_keys(&text);
    let text = redact_token_shapes(&text);
    redact_names(&text)
}

/// `redact` every string inside a JSON value.
///
/// Object keys are not redacted: two keys could become the same text, and one
/// value would then be lost. With `strict`, a string value under a secret-looking
/// key becomes `[redacted]` as a whole.
pub fn redact_value(value: &Value, key: Option<&str>, strict: bool) -> Value {
    match value {
        Value::String(text) => Value::String(redact(text, key)),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| redact_value(item, key, strict))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(name, item)| {
                    let item = if strict && item.is_string() && is_secret_name(name) {
                        Value::String(REDACTED.to_string())
                    } else {
                        redact_value(item, key, strict)
                    };
                    (name.clone(), item)
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::{Duration, Instant};

    fn r(text: &str) -> String {
        redact(text, None)
    }

    #[test]
    fn key_is_trimmed_before_it_is_matched() {
        assert_eq!(
            redact(
                "x tsk1 y",
                Some(
                    "tsk1
"
                )
            ),
            "x [redacted] y"
        );
        assert_eq!(
            redact(
                "x tsk1 y",
                Some(
                    "  
"
                )
            ),
            "x tsk1 y"
        );
    }

    #[test]
    fn known_token_shapes() {
        assert_eq!(
            r("Authorization: Bearer abcdefghijklmnop1234"),
            "Authorization: Bearer [redacted]"
        );
        assert_eq!(
            r("Bearer abcdefghijklmnopqrst"),
            "Bearer abcdefghijklmnopqrst"
        );
        assert_eq!(r("key sk-ABCDEFGHIJKLMNOPQRST"), "key [redacted]");
        assert_eq!(r("key tsk_ABCDEFGHIJKLMNOPQRST"), "key [redacted]");
        assert_eq!(r("token=hunter2"), "token=[redacted]");
        assert_eq!(r("AKIAABCDEFGHIJKLMNOP"), "[redacted]");
        assert_eq!(r(&format!("ghp_{}", "a".repeat(36))), "[redacted]");
        assert_eq!(r(&format!("github_pat_{}", "a".repeat(20))), "[redacted]");
        assert_eq!(r("xoxb-1234567890"), "[redacted]");
    }

    #[test]
    fn the_api_key_itself() {
        assert_eq!(
            redact("x my-plain-key y", Some("my-plain-key")),
            "x [redacted] y"
        );
        assert_eq!(redact("x y", Some("")), "x y");
    }

    #[test]
    fn redact_value_walks_json() {
        let state = json!({"a": ["password: pw1", {"b": "ok"}], "n": 3});
        assert_eq!(
            redact_value(&state, None, false),
            json!({"a": ["password: [redacted]", {"b": "ok"}], "n": 3})
        );
    }

    #[test]
    fn dict_keys_are_not_redacted() {
        assert_eq!(
            redact_value(&json!({"token=k": "v"}), None, false),
            json!({"token=k": "v"})
        );
    }

    #[test]
    fn private_keys() {
        let pem = "a -----BEGIN PRIVATE KEY-----\nQUJD\n-----END PRIVATE KEY----- b";
        assert_eq!(r(pem), "a [redacted] b");
        let cut = "-----BEGIN RSA PRIVATE KEY-----\nQUJD\nREVG\n- done";
        assert_eq!(r(cut), "[redacted]\n- done");
    }

    #[test]
    fn quoted_values() {
        assert_eq!(r("my_api_key: \"a b\""), "my_api_key: [redacted]");
        assert_eq!(r("password='q r'"), "password=[redacted]");
    }

    #[test]
    fn a_value_does_not_hide_a_later_name() {
        assert_eq!(r("tokenx:api-key:v"), "tokenx:api-key:[redacted]");
    }

    #[test]
    fn names_match_without_case() {
        assert_eq!(r("API_KEY=abc"), "API_KEY=[redacted]");
        assert_eq!(r("Password: abc"), "Password: [redacted]");
        assert_eq!(r("tokens=abc"), "tokens=abc");
    }

    #[test]
    fn strict_redaction_is_off_unless_asked() {
        let state = json!({"api_key": "abc", "note": "plain", "nested": {"password": "pw"}});
        assert_eq!(redact_value(&state, None, false), state);
        assert_eq!(
            redact_value(&state, None, true),
            json!({"api_key": "[redacted]", "note": "plain", "nested": {"password": "[redacted]"}})
        );
        assert_eq!(
            redact_value(&json!({"token": 5}), None, true),
            json!({"token": 5})
        );
    }

    #[test]
    fn one_mib_runs_in_linear_time() {
        const MIB: usize = 1 << 20;
        let units = [
            "-----BEGIN PRIVATE KEY-----\n",
            "ab-",
            "token_",
            "token-",
            "a1-secret-",
            "a:",
            "token=\"",
            "token=\" ",
            "password:'",
            "Bearer ",
            "Basic ",
            "Basic QUJD ",
        ];
        let limit = Duration::from_secs(if cfg!(debug_assertions) { 20 } else { 5 });
        for unit in units {
            let text = unit.repeat(MIB / unit.len());
            let started = Instant::now();
            redact(&text, None);
            assert!(started.elapsed() < limit, "{unit:?}");
        }
    }

    #[test]
    fn cjk_text_does_not_hide_tokens() {
        assert_eq!(
            r("APIキーはsk-ABCDEFGHIJKLMNOPQRSTです"),
            "APIキーは[redacted]です"
        );
        assert_eq!(r("キー AKIAABCDEFGHIJKLMNOPです"), "キー [redacted]です");
        assert_eq!(
            r(&format!("トークンghp_{}", "a".repeat(36))),
            "トークン[redacted]"
        );
        assert_eq!(r("xsk-ABCDEFGHIJKLMNOPQRST"), "xsk-ABCDEFGHIJKLMNOPQRST");
    }

    #[test]
    fn short_basic_credentials() {
        assert_eq!(
            r("Authorization: Basic dXNlcjpwYXNz"),
            "Authorization: Basic [redacted]"
        );
        assert_eq!(r("Basic YTpiYw=="), "Basic [redacted]");
        assert_eq!(
            r("Basic authentication is used"),
            "Basic authentication is used"
        );
        assert_eq!(r("Basic QUJD"), "Basic QUJD");
    }

    #[test]
    fn basic_decoder_edges() {
        // Digits and the last two alphabet marks decode, and the colon can come after them.
        assert_eq!(r("Basic YTE6Yg=="), "Basic [redacted]");
        assert_eq!(r("Basic +/+/dXNlcjpwYXNz"), "Basic [redacted]");
        // Uppercase and lowercase letters decode.
        assert_eq!(r("Basic dXNlcjpwYXNz"), "Basic [redacted]");
        assert_eq!(r("Basic OnVzZXI="), "Basic [redacted]");
        // The length must be a multiple of 4, padding included.
        assert_eq!(r("Basic YTpiYw"), "Basic YTpiYw");
        assert_eq!(r("Basic YTpiYw="), "Basic YTpiYw=");
        // At most 2 padding marks belong to the token.
        assert_eq!(r("Basic YTpiYw==="), "Basic [redacted]=");
        // Both spellings of the word, a tab, and the word boundary.
        assert_eq!(r("basic dXNlcjpwYXNz"), "basic [redacted]");
        assert_eq!(r("Basic\tdXNlcjpwYXNz"), "Basic [redacted]");
        assert_eq!(r("xBasic dXNlcjpwYXNz"), "xBasic dXNlcjpwYXNz");
        // No colon in the decoded bytes: "user" and "a b".
        assert_eq!(r("Basic dXNlcg=="), "Basic dXNlcg==");
        assert_eq!(r("Basic YSBi"), "Basic YSBi");
    }

    #[test]
    fn cjk_text_before_each_rule() {
        assert_eq!(
            r("ご利用Bearer abcdefghijklmnop1234"),
            "ご利用Bearer [redacted]"
        );
        assert_eq!(r("ご利用Basic dXNlcjpwYXNz"), "ご利用Basic [redacted]");
        assert_eq!(
            r(&format!("鍵github_pat_{}", "a".repeat(20))),
            "鍵[redacted]"
        );
        assert_eq!(r("鍵AKIAABCDEFGHIJKLMNOP"), "鍵[redacted]");
        assert_eq!(r("鍵xoxb-1234567890"), "鍵[redacted]");
        assert_eq!(r("鍵tsk_ABCDEFGHIJKLMNOPQRST"), "鍵[redacted]");
    }

    #[test]
    fn basic_decoder_alphabet() {
        // Short tokens, so only the Basic step can redact them.
        // The colon comes after every kind of character.
        assert_eq!(r("Basic 0099YTpi"), "Basic [redacted]");
        assert_eq!(r("Basic +AAAYTpi"), "Basic [redacted]");
        assert_eq!(r("Basic /AAAYTpi"), "Basic [redacted]");
        assert_eq!(r("Basic +/+/YTpiY2Q="), "Basic [redacted]");
        // The colon is the last byte of a group.
        assert_eq!(r("Basic AAA6"), "Basic [redacted]");
        // '+' and '/' as the last mark of a group are not colons.
        assert_eq!(r("Basic AAA+"), "Basic AAA+");
        assert_eq!(r("Basic AAA/"), "Basic AAA/");
    }

    #[test]
    fn private_key_edges() {
        // An END line with a type word, a CRLF body, and every base64 character.
        let rsa = "a -----BEGIN RSA PRIVATE KEY-----\nQUJD\n-----END RSA PRIVATE KEY----- b";
        assert_eq!(r(rsa), "a [redacted] b");
        let crlf = "-----BEGIN PRIVATE KEY-----\r\nQUJD\r\n- done";
        assert_eq!(r(crlf), "[redacted]\r\n- done");
        let all = "-----BEGIN PRIVATE KEY-----\nAbz09+/=\n- x";
        assert_eq!(r(all), "[redacted]\n- x");
        // Two keys, each cut short.
        let two =
            "-----BEGIN PRIVATE KEY-----\nQUJD\n- mid -----BEGIN PRIVATE KEY-----\nREVG\n- end";
        assert_eq!(r(two), "[redacted]\n- mid [redacted]\n- end");
    }

    #[test]
    fn bearer_edges() {
        for word in ["Bearer", "bearer", "Basic", "basic"] {
            assert_eq!(
                r(&format!("{word} abcdefghijklmnop1234")),
                format!("{word} [redacted]"),
                "{word}"
            );
        }
        // The word must start at a word boundary.
        assert_eq!(
            r("xBearer abcdefghijklmnop1234"),
            "xBearer abcdefghijklmnop1234"
        );
        // A tab is a separator, and the output uses a space.
        assert_eq!(r("Bearer\tabcdefghijklmnop1234"), "Bearer [redacted]");
        // Sixteen characters match; fifteen do not.
        assert_eq!(r("Bearer abcdefghijklmno1"), "Bearer [redacted]");
        assert_eq!(r("Bearer abcdefghijklmn1"), "Bearer abcdefghijklmn1");
        // Every character of the token class belongs to the token.
        assert_eq!(r("Bearer abcdefghijklmnop1._~+/=-"), "Bearer [redacted]");
        // A token with no digit, plus, slash, or equals sign is a plain word.
        assert_eq!(
            r("Bearer abcdefghijklmnop._~-"),
            "Bearer abcdefghijklmnop._~-"
        );
        // Each of those four marks alone makes it a token.
        for mark in ["1", "+", "/", "="] {
            assert_eq!(
                r(&format!("Bearer abcdefghijklmnop{mark}")),
                "Bearer [redacted]",
                "{mark}"
            );
        }
    }

    #[test]
    fn token_shape_edges() {
        let sk = format!("sk-{}", "A".repeat(16));
        assert_eq!(r(&sk), "[redacted]");
        assert_eq!(
            r(&format!("sk-{}", "A".repeat(15))),
            format!("sk-{}", "A".repeat(15))
        );
        assert_eq!(r(&format!("sk-{}_-b", "A".repeat(16))), "[redacted]");
        assert_eq!(r(&format!("x{sk}")), format!("x{sk}"));

        for c in ["p", "o", "u", "s", "r"] {
            assert_eq!(r(&format!("gh{c}_{}", "a".repeat(36))), "[redacted]", "{c}");
        }
        assert_eq!(
            r(&format!("ghx_{}", "a".repeat(36))),
            format!("ghx_{}", "a".repeat(36))
        );
        assert_eq!(
            r(&format!("ghp_{}", "a".repeat(35))),
            format!("ghp_{}", "a".repeat(35))
        );
        assert_eq!(
            r(&format!(
                "ghp_{}{}{}",
                "A".repeat(12),
                "b".repeat(12),
                "0".repeat(12)
            )),
            "[redacted]"
        );
        assert_eq!(
            r(&format!("xghp_{}", "a".repeat(36))),
            format!("xghp_{}", "a".repeat(36))
        );

        assert_eq!(r("github_pat_aaaaaaaaaa_bbbbbbbbb"), "[redacted]");
        let short = format!("github_pat_{}", "a".repeat(19));
        assert_eq!(r(&short), short);
        let glued = format!("xgithub_pat_{}", "a".repeat(20));
        assert_eq!(r(&glued), glued);

        assert_eq!(r("AKIA0123456789ABCDEF"), "[redacted]");
        assert_eq!(r("AKIAABCDEFGHIJKLMNOPQ"), "AKIAABCDEFGHIJKLMNOPQ");
        assert_eq!(r("AKIAABCDEFGHIJKLMNO"), "AKIAABCDEFGHIJKLMNO");
        assert_eq!(r("AKIAabcdefghijklmnop"), "AKIAabcdefghijklmnop");
        assert_eq!(r("xAKIAABCDEFGHIJKLMNOP"), "xAKIAABCDEFGHIJKLMNOP");

        for c in ["a", "b", "p", "r", "s"] {
            assert_eq!(r(&format!("xox{c}-1234567890")), "[redacted]", "{c}");
        }
        assert_eq!(r("xoxb-12345-67890"), "[redacted]");
        assert_eq!(r("xoxb-123456789"), "xoxb-123456789");
        assert_eq!(r("yxoxb-1234567890"), "yxoxb-1234567890");
    }

    #[test]
    fn secret_name_edges() {
        for name in [
            "password",
            "passwd",
            "secret",
            "token",
            "api_key",
            "api-key",
            "apikey",
            "API_KEY",
            "db_password",
            "db-secret",
            "x-token-y",
            "token_x",
            "token-x",
            "api_key_id",
        ] {
            assert!(is_secret_name(name), "{name}");
        }
        for name in [
            "tokens",
            "mytoken",
            "passwords",
            "token2",
            "secretary",
            "api key",
            "apikeys",
            "2token",
            "",
        ] {
            assert!(!is_secret_name(name), "{name}");
        }
    }

    #[test]
    fn name_rule_edges() {
        // A digit in front of the word keeps it from being a name.
        assert_eq!(r("2token=abc"), "2token=abc");
        // Quoted names, in both quote styles.
        assert_eq!(r("{\"token\": \"abc\"}"), "{\"token\": [redacted]}");
        assert_eq!(r("{'token': 'abc'}"), "{'token': [redacted]}");
        // Tabs around the separator.
        assert_eq!(r("token\t=abc"), "token\t=[redacted]");
        assert_eq!(r("token:\tabc"), "token:\t[redacted]");
        // An escaped quote does not end a quoted value.
        assert_eq!(
            r("password: \"a\\\"b c\" tail"),
            "password: [redacted] tail"
        );
        // Each end mark of an unquoted value.
        for mark in [" ", "\t", "\r", "\n", ",", ";", "}", "]"] {
            assert_eq!(
                r(&format!("token=ab{mark}cd")),
                format!("token=[redacted]{mark}cd"),
                "{mark:?}"
            );
        }
        // No value, nothing to redact.
        assert_eq!(r("token=,x"), "token=,x");
    }

    #[test]
    fn redact_value_passes_the_key_and_strict_down() {
        let state =
            json!({"a": ["x my-plain-key y"], "b": "my-plain-key", "o": {"p": "my-plain-key"}});
        assert_eq!(
            redact_value(&state, Some("my-plain-key"), false),
            json!({"a": ["x [redacted] y"], "b": "[redacted]", "o": {"p": "[redacted]"}})
        );
        assert_eq!(
            redact_value(&json!([{"api_key": "abc"}]), None, true),
            json!([{"api_key": "[redacted]"}])
        );
        assert_eq!(
            redact_value(&json!({"note": "abc"}), None, true),
            json!({"note": "abc"})
        );
        assert_eq!(REDACTED, "[redacted]");
    }
}
