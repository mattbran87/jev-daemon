use crate::error::WireError;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The protocol version that `hello` carries.
pub const PROTOCOL_VERSION: u32 = 1;

/// The longest request line that jevd reads, newline not counted.
pub const MAX_LINE: usize = 1 << 20;

/// One request line from a client to jevd.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Request {
    Hello {
        protocol: u32,
    },
    Status,
    Pair {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profile: Option<String>,
    },
    PairStatus {
        request_id: String,
    },
    Ask {
        token: String,
        state: Value,
        questions: Value,
    },
    /// Admin: list the pending pairing requests, for `jev allow`.
    Pending,
    Approve {
        code: String,
        profile: String,
    },
    Deny {
        code: String,
    },
    Revoke {
        name: String,
    },
    Clients,
    /// `limit` is `<count>/<seconds>`, or absent for the default.
    SetLimit {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<String>,
    },
    Reload,
    Stop,
}

/// Like the derived `Debug`, except that tokens, request IDs, codes, and request text are hidden.
impl std::fmt::Debug for Request {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Request::Hello { protocol } => {
                f.debug_struct("Hello").field("protocol", protocol).finish()
            }
            Request::Status => f.write_str("Status"),
            Request::Pair { name, profile } => f
                .debug_struct("Pair")
                .field("name", name)
                .field("profile", profile)
                .finish(),
            Request::PairStatus { .. } => f
                .debug_struct("PairStatus")
                .field("request_id", &"<redacted>")
                .finish(),
            Request::Ask { .. } => f
                .debug_struct("Ask")
                .field("token", &"<redacted>")
                .field("state", &"<redacted>")
                .field("questions", &"<redacted>")
                .finish(),
            Request::Pending => f.write_str("Pending"),
            Request::Approve { profile, .. } => f
                .debug_struct("Approve")
                .field("code", &"<redacted>")
                .field("profile", profile)
                .finish(),
            Request::Deny { code } => f.debug_struct("Deny").field("code", code).finish(),
            Request::Revoke { name } => f.debug_struct("Revoke").field("name", name).finish(),
            Request::Clients => f.write_str("Clients"),
            Request::SetLimit { name, limit } => f
                .debug_struct("SetLimit")
                .field("name", name)
                .field("limit", limit)
                .finish(),
            Request::Reload => f.write_str("Reload"),
            Request::Stop => f.write_str("Stop"),
        }
    }
}

impl Request {
    /// True for the messages that jevd accepts only from its own `jev` binary.
    pub fn is_admin(&self) -> bool {
        matches!(
            self,
            Request::Pending
                | Request::Approve { .. }
                | Request::Deny { .. }
                | Request::Revoke { .. }
                | Request::Clients
                | Request::SetLimit { .. }
                | Request::Reload
                | Request::Stop
        )
    }
}

/// One reply line from jevd. Success data sits beside `ok`.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Reply {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<WireError>,
    #[serde(flatten)]
    pub data: Map<String, Value>,
}

/// Prints `ok`, `error`, and only the keys of `data`: replies carry tokens.
impl std::fmt::Debug for Reply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reply")
            .field("ok", &self.ok)
            .field("error", &self.error)
            .field("data_keys", &self.data.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl Reply {
    /// A success reply. `data` must be a JSON object; anything else gives no data.
    /// The keys `ok` and `error` are reserved: they are deleted from `data`.
    pub fn success(data: Value) -> Reply {
        let mut data = match data {
            Value::Object(map) => map,
            _ => Map::new(),
        };
        data.remove("ok");
        data.remove("error");
        Reply {
            ok: true,
            error: None,
            data,
        }
    }

    pub fn failure(error: WireError) -> Reply {
        Reply {
            ok: false,
            error: Some(error),
            data: Map::new(),
        }
    }

    /// The reply as one line of JSON, with its newline.
    pub fn to_line(&self) -> String {
        let mut line = serde_json::to_string(self).expect("a reply always serializes");
        line.push('\n');
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ErrorCode, WireError};
    use serde_json::json;

    #[test]
    fn a_reply_debug_shows_keys_and_never_values() {
        let reply = Reply::success(json!({"token": "jevt_secretvalue", "state": "approved"}));
        let text = format!("{reply:?}");
        assert!(text.contains("token"), "{text}");
        assert!(!text.contains("jevt_secretvalue"), "{text}");
        assert!(!text.contains("approved"), "{text}");
    }

    fn wire(request: &Request) -> Value {
        serde_json::to_value(request).unwrap()
    }

    #[test]
    fn requests_use_a_type_tag() {
        assert_eq!(
            wire(&Request::Hello { protocol: 1 }),
            json!({"type": "hello", "protocol": 1})
        );
        assert_eq!(wire(&Request::Status), json!({"type": "status"}));
        assert_eq!(
            wire(&Request::PairStatus {
                request_id: "ab".into()
            }),
            json!({"type": "pair-status", "request_id": "ab"})
        );
        assert_eq!(
            wire(&Request::Pair {
                name: "x".into(),
                profile: None
            }),
            json!({"type": "pair", "name": "x"})
        );
        assert_eq!(
            wire(&Request::SetLimit {
                name: "x".into(),
                limit: Some("10/60".into())
            }),
            json!({"type": "set-limit", "name": "x", "limit": "10/60"})
        );
    }

    #[test]
    fn requests_read_back() {
        let line =
            r#"{"type":"ask","token":"jevt_1","state":"s","questions":{"q":{"type":"noul"}}}"#;
        let request: Request = serde_json::from_str(line).unwrap();
        assert_eq!(
            request,
            Request::Ask {
                token: "jevt_1".into(),
                state: json!("s"),
                questions: json!({"q": {"type": "noul"}}),
            }
        );
        assert!(serde_json::from_str::<Request>(r#"{"type":"nope"}"#).is_err());
    }

    #[test]
    fn only_admin_messages_are_admin() {
        let admin = [
            Request::Pending,
            Request::Approve {
                code: "C".into(),
                profile: "default".into(),
            },
            Request::Deny { code: "C".into() },
            Request::Revoke { name: "x".into() },
            Request::Clients,
            Request::SetLimit {
                name: "x".into(),
                limit: None,
            },
            Request::Reload,
            Request::Stop,
        ];
        assert!(admin.iter().all(Request::is_admin));
        let open = [
            Request::Hello { protocol: 1 },
            Request::Status,
            Request::Pair {
                name: "x".into(),
                profile: None,
            },
            Request::PairStatus {
                request_id: "r".into(),
            },
            Request::Ask {
                token: "t".into(),
                state: json!(null),
                questions: json!({}),
            },
        ];
        assert!(!open.iter().any(Request::is_admin));
    }

    #[test]
    fn success_drops_reserved_keys() {
        let line = Reply::success(json!({"ok": false, "error": "x", "n": 1})).to_line();
        let back: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(back, json!({"ok": true, "n": 1}));
        assert_eq!(line.matches("\"ok\"").count(), 1);
    }

    #[test]
    fn debug_hides_ask_secrets() {
        let ask = Request::Ask {
            token: "jevt_secret".into(),
            state: json!("private text"),
            questions: json!({}),
        };
        let shown = format!("{:?}", ask);
        assert!(!shown.contains("jevt_secret"));
        assert!(!shown.contains("private text"));
        assert!(shown.contains("Ask"));
        assert_eq!(format!("{:?}", Request::Status), "Status");
    }

    #[test]
    fn debug_hides_pair_status_request_id_and_approve_code() {
        let status = Request::PairStatus {
            request_id: "req-sample-id".into(),
        };
        let approve = Request::Approve {
            code: "CODE2345".into(),
            profile: "default".into(),
        };
        let shown = format!("{:?} {:?}", status, approve);
        assert!(!shown.contains("req-sample-id"));
        assert!(!shown.contains("CODE2345"));
        assert!(shown.contains("PairStatus"));
        assert!(shown.contains("Approve"));
    }

    #[test]
    fn replies_carry_ok_and_flat_data() {
        let reply = Reply::success(json!({"code": "ABCD2345"}));
        assert_eq!(
            serde_json::to_value(&reply).unwrap(),
            json!({"ok": true, "code": "ABCD2345"})
        );
        let failure = Reply::failure(WireError::new(ErrorCode::Forbidden, "admin only"));
        assert_eq!(
            serde_json::to_value(&failure).unwrap(),
            json!({"ok": false, "error": {"code": "forbidden", "message": "admin only"}})
        );
        let back: Reply = serde_json::from_str(r#"{"ok":true,"state":"pending"}"#).unwrap();
        assert!(back.ok);
        assert_eq!(back.data["state"], "pending");
        assert!(failure.to_line().ends_with('\n'));
        assert_eq!(failure.to_line().matches('\n').count(), 1);
    }

    #[test]
    fn a_reply_with_an_unknown_code_still_parses() {
        use crate::WireCode;
        let reply: Reply = serde_json::from_str(
            r#"{"ok":false,"error":{"code":"quota-exceeded","message":"try later","retry_after":30}}"#,
        )
        .unwrap();
        let error = reply.error.unwrap();
        assert_eq!(error.code, WireCode::Unknown("quota-exceeded".to_string()));
        assert_eq!(error.message, "try later");
        assert_eq!(error.retry_after, Some(30));
        let bare: Reply = serde_json::from_str(r#"{"ok":false,"error":{}}"#).unwrap();
        let error = bare.error.unwrap();
        assert_eq!(error.code, WireCode::Unknown("malformed".to_string()));
        assert_eq!(error.message, "");
    }
}
