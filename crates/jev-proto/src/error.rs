use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

/// Every error a client can get. Clients branch on these, never on message text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorCode {
    Internal,
    Pending,
    NotPaired,
    Revoked,
    RateLimited,
    NoKey,
    BadKey,
    JevUnauthorized,
    JevHttpError,
    JevTimeout,
    JevUnreachable,
    BadAnswer,
    DaemonUnavailable,
    ProtocolUnsupported,
    Forbidden,
    BadRequest,
    TooLarge,
}

impl ErrorCode {
    pub const ALL: [ErrorCode; 17] = [
        ErrorCode::Internal,
        ErrorCode::Pending,
        ErrorCode::NotPaired,
        ErrorCode::Revoked,
        ErrorCode::RateLimited,
        ErrorCode::NoKey,
        ErrorCode::BadKey,
        ErrorCode::JevUnauthorized,
        ErrorCode::JevHttpError,
        ErrorCode::JevTimeout,
        ErrorCode::JevUnreachable,
        ErrorCode::BadAnswer,
        ErrorCode::DaemonUnavailable,
        ErrorCode::ProtocolUnsupported,
        ErrorCode::Forbidden,
        ErrorCode::BadRequest,
        ErrorCode::TooLarge,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::Internal => "internal",
            ErrorCode::Pending => "pending",
            ErrorCode::NotPaired => "not-paired",
            ErrorCode::Revoked => "revoked",
            ErrorCode::RateLimited => "rate-limited",
            ErrorCode::NoKey => "no-key",
            ErrorCode::BadKey => "bad-key",
            ErrorCode::JevUnauthorized => "jev-unauthorized",
            ErrorCode::JevHttpError => "jev-http-error",
            ErrorCode::JevTimeout => "jev-timeout",
            ErrorCode::JevUnreachable => "jev-unreachable",
            ErrorCode::BadAnswer => "bad-answer",
            ErrorCode::DaemonUnavailable => "daemon-unavailable",
            ErrorCode::ProtocolUnsupported => "protocol-unsupported",
            ErrorCode::Forbidden => "forbidden",
            ErrorCode::BadRequest => "bad-request",
            ErrorCode::TooLarge => "too-large",
        }
    }

    /// The `jev` CLI's exit code for this error (spec section 6).
    pub fn exit_code(self) -> i32 {
        match self {
            ErrorCode::Internal => 1,
            ErrorCode::Pending => 3,
            ErrorCode::NotPaired | ErrorCode::Revoked => 4,
            ErrorCode::RateLimited => 5,
            ErrorCode::NoKey | ErrorCode::BadKey | ErrorCode::JevUnauthorized => 6,
            ErrorCode::JevHttpError
            | ErrorCode::JevTimeout
            | ErrorCode::JevUnreachable
            | ErrorCode::BadAnswer => 7,
            ErrorCode::DaemonUnavailable | ErrorCode::ProtocolUnsupported => 8,
            ErrorCode::Forbidden | ErrorCode::BadRequest | ErrorCode::TooLarge => 9,
        }
    }

    /// The code for a wire name, or `None` for a name this version does not know.
    pub fn from_name(name: &str) -> Option<ErrorCode> {
        ErrorCode::ALL
            .into_iter()
            .find(|code| code.as_str() == name)
    }
}

/// The raw text of a code that is missing, is not a string, or has a bad shape.
const MALFORMED: &str = "malformed";

/// An error code as a client receives it (spec section 2). A code this
/// version does not know is `Unknown`, so the reply still parses.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum WireCode {
    Known(ErrorCode),
    /// Read from the wire, the raw text matches `[a-z0-9-]{1,64}`, or is `malformed`.
    Unknown(String),
}

fn valid_raw_code(raw: &str) -> bool {
    (1..=64).contains(&raw.len())
        && raw
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn malformed() -> WireCode {
    WireCode::Unknown(MALFORMED.to_string())
}

impl WireCode {
    /// Reads a received code. A server code named `malformed` reads the same as a malformed one.
    pub fn from_raw(raw: &str) -> WireCode {
        match ErrorCode::from_name(raw) {
            Some(code) => WireCode::Known(code),
            None if valid_raw_code(raw) => WireCode::Unknown(raw.to_string()),
            None => malformed(),
        }
    }

    /// The code's wire text.
    pub fn as_str(&self) -> &str {
        match self {
            WireCode::Known(code) => code.as_str(),
            WireCode::Unknown(raw) => raw,
        }
    }

    /// The known code, or `None` for an unknown one.
    pub fn known(&self) -> Option<ErrorCode> {
        match self {
            WireCode::Known(code) => Some(*code),
            WireCode::Unknown(_) => None,
        }
    }

    /// The `jev` exit code. An unknown code is an unexpected error (1).
    pub fn exit_code(&self) -> i32 {
        self.known().map_or(1, ErrorCode::exit_code)
    }
}

impl From<ErrorCode> for WireCode {
    fn from(code: ErrorCode) -> WireCode {
        WireCode::Known(code)
    }
}

impl Serialize for WireCode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for WireCode {
    /// Any JSON value is accepted, so a bad code never fails the reply.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<WireCode, D::Error> {
        Ok(match Value::deserialize(deserializer)? {
            Value::String(raw) => WireCode::from_raw(&raw),
            _ => malformed(),
        })
    }
}

/// The `error` object of a failed reply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireError {
    /// A missing code reads as `malformed`.
    #[serde(default = "malformed")]
    pub code: WireCode,
    #[serde(default)]
    pub message: String,
    /// Seconds until a `rate-limited` client can try again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after: Option<u64>,
    /// The HTTP status for `jev-http-error` and `jev-unauthorized`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    /// The profile whose key failed, for `no-key`, `bad-key`, and
    /// `jev-unauthorized`, so a client can name it to the person.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
}

impl WireError {
    /// The usual way to build an error to send. `jevd` sets `code` only through `new`, so it sends known codes only.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        WireError {
            code: WireCode::Known(code),
            message: message.into(),
            retry_after: None,
            status: None,
            profile: None,
        }
    }

    /// Names `profile` on an error about its key: `no-key`, `bad-key`, or
    /// `jev-unauthorized`. Other errors stay as they are.
    pub fn for_profile(mut self, profile: &str) -> Self {
        if matches!(
            self.code,
            WireCode::Known(ErrorCode::NoKey | ErrorCode::BadKey | ErrorCode::JevUnauthorized)
        ) {
            self.profile = Some(profile.to_string());
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_code_serializes_as_its_kebab_name() {
        for code in ErrorCode::ALL {
            let json = serde_json::to_string(&code).unwrap();
            assert_eq!(json, format!("\"{}\"", code.as_str()));
            let back: ErrorCode = serde_json::from_str(&json).unwrap();
            assert_eq!(back, code);
        }
    }

    #[test]
    fn for_profile_names_the_profile_on_key_errors_only() {
        for code in [
            ErrorCode::NoKey,
            ErrorCode::BadKey,
            ErrorCode::JevUnauthorized,
        ] {
            let error = WireError::new(code, "x").for_profile("work");
            assert_eq!(error.profile.as_deref(), Some("work"), "{code:?}");
            let line = serde_json::to_string(&error).unwrap();
            assert!(line.contains(r#""profile":"work""#), "{line}");
        }
        let other = WireError::new(ErrorCode::RateLimited, "x").for_profile("work");
        assert_eq!(other.profile, None);
        assert!(!serde_json::to_string(&other).unwrap().contains("profile"));
    }

    #[test]
    fn exit_codes_match_the_spec_table() {
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
            assert_eq!(code.exit_code(), exit, "{}", code.as_str());
        }
    }

    #[test]
    fn wire_error_leaves_out_empty_fields() {
        let error = WireError::new(ErrorCode::NotPaired, "pair first");
        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            serde_json::json!({"code": "not-paired", "message": "pair first"})
        );
        let limited = WireError {
            retry_after: Some(12),
            ..WireError::new(ErrorCode::RateLimited, "slow down")
        };
        assert_eq!(serde_json::to_value(&limited).unwrap()["retry_after"], 12);
    }

    #[test]
    fn every_known_code_reads_as_known() {
        for code in ErrorCode::ALL {
            let wire: WireCode = serde_json::from_value(serde_json::json!(code.as_str())).unwrap();
            assert_eq!(wire, WireCode::Known(code));
            assert_eq!(
                serde_json::to_value(&wire).unwrap(),
                serde_json::json!(code.as_str())
            );
            assert_eq!(ErrorCode::from_name(code.as_str()), Some(code));
        }
        assert_eq!(ErrorCode::from_name("no-such-code"), None);
    }

    #[test]
    fn an_unknown_code_keeps_its_raw_text() {
        let wire: WireCode = serde_json::from_value(serde_json::json!("quota-exceeded")).unwrap();
        assert_eq!(wire, WireCode::Unknown("quota-exceeded".to_string()));
        assert_eq!(wire.as_str(), "quota-exceeded");
        assert_eq!(wire.known(), None);
        assert_eq!(wire.exit_code(), 1);
        assert_eq!(
            serde_json::to_value(&wire).unwrap(),
            serde_json::json!("quota-exceeded")
        );
        let longest = "a1-".repeat(21) + "z";
        assert_eq!(
            WireCode::from_raw(&longest),
            WireCode::Unknown(longest.clone())
        );
    }

    #[test]
    fn a_bad_raw_code_reads_as_malformed() {
        let malformed = WireCode::Unknown(MALFORMED.to_string());
        for raw in [
            serde_json::json!(""),
            serde_json::json!("Not-Paired"),
            serde_json::json!("a b"),
            serde_json::json!("a_b"),
            serde_json::json!("\u{1b}[31mred"),
            serde_json::json!("a".repeat(65)),
            serde_json::json!("ü"),
            serde_json::json!(7),
            serde_json::json!(null),
            serde_json::json!({"code": "internal"}),
        ] {
            let wire: WireCode = serde_json::from_value(raw.clone()).unwrap();
            assert_eq!(wire, malformed, "{raw}");
        }
    }

    #[test]
    fn a_known_code_keeps_its_exit_code() {
        assert_eq!(WireCode::Known(ErrorCode::RateLimited).exit_code(), 5);
        assert_eq!(
            WireCode::from(ErrorCode::NotPaired),
            WireCode::Known(ErrorCode::NotPaired)
        );
        assert_eq!(
            WireError::new(ErrorCode::Forbidden, "no").code,
            WireCode::Known(ErrorCode::Forbidden)
        );
    }
}
