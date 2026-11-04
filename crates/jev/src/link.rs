//! The connection to jevd (spec section 6, "Shared behavior"): every command
//! starts jevd when no daemon runs, except `jev stop` and `jev status`.

use jev_proto::client::{connect_or_start, Connection};
use jev_proto::{ErrorCode, Reply, Request, WireError};
use jevd::pairing::{REQUEST_ID_HEX_LENGTH, TOKEN_HEX_LENGTH, TOKEN_PREFIX};
use serde_json::{Map, Value};
use zeroize::Zeroizing;

use crate::env::Env;
use crate::out::clean;

/// A connection to jevd. With `start`, starts jevd when no daemon runs.
pub fn open(env: &Env, start: bool) -> Result<Connection, WireError> {
    if start {
        connect_or_start(&env.paths, &env.jevd)
    } else {
        Connection::connect(&env.paths)
    }
}

/// Sends one request. A reply with `ok: false` becomes its error.
pub fn call(
    connection: &mut Connection,
    request: &Request,
) -> Result<Map<String, Value>, WireError> {
    reply_result(connection.call(request)?)
}

/// A reply's data, or its error when it has `ok: false`.
pub fn reply_result(reply: Reply) -> Result<Map<String, Value>, WireError> {
    if reply.ok {
        return Ok(reply.data);
    }
    Err(reply
        .error
        .unwrap_or_else(|| WireError::new(ErrorCode::Internal, "jevd failed with no error")))
}

/// The paired clients, as jevd lists them now: one object per client.
pub fn clients(connection: &mut Connection) -> Result<Vec<Value>, WireError> {
    let data = call(connection, &Request::Clients)?;
    Ok(match data.get("clients") {
        Some(Value::Array(list)) => list.clone(),
        _ => Vec::new(),
    })
}

/// Sends `reload` when a daemon runs, so it reads the settings and the keys
/// again. No daemon: nothing to do, because jevd reads both at its start. A
/// daemon that is busy, full, or does not answer is an error, because it
/// would keep the old settings.
pub fn reload_if_running(env: &Env) -> Result<(), WireError> {
    match Connection::connect_if_running(&env.paths)? {
        Some(mut connection) => call(&mut connection, &Request::Reload).map(|_| ()),
        None => Ok(()),
    }
}

/// A string field of a reply, passed through `out::clean`, so it is safe to
/// print and holds no line breaks.
pub fn text(data: &Map<String, Value>, field: &str) -> Result<String, WireError> {
    data.get(field)
        .and_then(Value::as_str)
        .map(clean)
        .ok_or_else(|| {
            WireError::new(
                ErrorCode::Internal,
                format!("jevd's reply has no {field} field"),
            )
        })
}

fn lower_hex(text: &str, length: usize) -> bool {
    text.len() == length && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// A token as jevd makes it: `TOKEN_PREFIX`, then 64 lowercase hex digits.
pub fn is_token(text: &str) -> bool {
    text.strip_prefix(TOKEN_PREFIX)
        .is_some_and(|hex| lower_hex(hex, TOKEN_HEX_LENGTH))
}

/// A request id as jevd makes it: 32 lowercase hex digits.
pub fn is_request_id(text: &str) -> bool {
    lower_hex(text, REQUEST_ID_HEX_LENGTH)
}

/// Takes a string field out of a reply as it is, with no copy and no
/// cleaning, into memory that is wiped when it is dropped. A value that
/// `valid` refuses is an error that never shows the value.
pub fn secret(
    data: &mut Map<String, Value>,
    field: &str,
    valid: fn(&str) -> bool,
) -> Result<Zeroizing<String>, WireError> {
    let value = match data.remove(field) {
        Some(Value::String(text)) => Zeroizing::new(text),
        Some(_) => return Err(malformed(field)),
        None => {
            return Err(WireError::new(
                ErrorCode::Internal,
                format!("jevd's reply has no {field} field"),
            ))
        }
    };
    if !valid(&value) {
        return Err(malformed(field));
    }
    Ok(value)
}

fn malformed(field: &str) -> WireError {
    WireError::new(
        ErrorCode::Internal,
        format!("jevd sent a malformed {field}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::Duration;

    use jev_proto::Paths;
    use jevd::daemon::{Config, Daemon};
    use jevd::jev_http::JevClient;
    use jevd::keystore::MemoryKeyring;
    use jevd::peer::AdminCheck;
    use jevd::settings::Settings;
    use serde_json::json;

    use crate::env::{Buffer, TextInput};
    use crate::term::ScriptedTerminal;

    fn env_in(home: &Path) -> Env {
        Env {
            paths: Paths::under(home).unwrap(),
            jevd: home.join("no-such-jevd"),
            store: Arc::new(MemoryKeyring::new()),
            terminal: Box::new(ScriptedTerminal::none()),
            stdin: Box::new(TextInput(Some(String::new()))),
            out: Box::new(Buffer::default()),
            err: Box::new(Buffer::default()),
            vars: Box::new(|_| None),
            jev_client: Box::new(|| Err("no Jev in this test".to_string())),
        }
    }

    /// Connects and keeps the connection. Retries for up to 5 s while jevd
    /// is full, because it can still count a connection that just closed.
    fn hold(paths: &Paths) -> Connection {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            match Connection::connect(paths) {
                Ok(connection) => return connection,
                Err(error)
                    if error.code.known() == Some(ErrorCode::DaemonUnavailable)
                        && std::time::Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(error) => panic!("cannot hold a connection: {}", error.message),
            }
        }
    }

    /// A real daemon in this process that allows `max` connections.
    fn start_daemon(home: &Path, max: &str) -> tokio::task::JoinHandle<()> {
        let paths = Paths::under(home).unwrap();
        let mut settings = Settings::default();
        settings.add_profile("default").unwrap();
        let settings = settings
            .with_change("max-connections", max)
            .unwrap()
            .settings;
        settings.save(&paths.settings_file()).unwrap();
        let jev = JevClient::with_endpoint(
            "http://127.0.0.1:9/",
            Duration::from_secs(5),
            Duration::from_millis(10),
        )
        .unwrap();
        let daemon = Daemon::new(Config {
            settings_file: paths.settings_file(),
            pairing_file: paths.pairing_file(),
            store: Arc::new(MemoryKeyring::new()),
            jev,
        })
        .unwrap();
        let listener = jevd::listen::bind(&paths).unwrap();
        let me: PathBuf = std::env::current_exe().unwrap();
        tokio::spawn(jevd::run::run(daemon, listener, AdminCheck::new(&me)))
    }

    #[test]
    fn text_cleans_the_field() {
        let data = json!({"name": "a\u{202e}b\nc"});
        let data = data.as_object().unwrap();
        assert_eq!(text(data, "name").unwrap(), "a b c");
        assert!(text(data, "other").is_err());
    }

    #[test]
    fn secret_takes_a_well_formed_value_out_of_the_reply() {
        let token = format!("{TOKEN_PREFIX}{}", "0a".repeat(32));
        let id = "f".repeat(32);
        let mut data = json!({"token": token, "request_id": id})
            .as_object()
            .unwrap()
            .clone();
        assert_eq!(
            secret(&mut data, "token", is_token).unwrap().as_str(),
            token
        );
        assert!(!data.contains_key("token"), "the field is taken out");
        assert_eq!(
            secret(&mut data, "request_id", is_request_id)
                .unwrap()
                .as_str(),
            id
        );
        assert!(secret(&mut data, "token", is_token).is_err(), "no field");
    }

    #[test]
    fn secret_refuses_a_malformed_value_and_never_changes_it() {
        let hex = "a".repeat(63);
        for bad in [
            format!("{TOKEN_PREFIX} {hex}"),
            format!("{TOKEN_PREFIX}\u{202e}{hex}"),
            format!("{TOKEN_PREFIX}{}", "A".repeat(64)),
            format!("{TOKEN_PREFIX}{hex}"),
            "a".repeat(69),
        ] {
            let mut data = json!({ "token": bad }).as_object().unwrap().clone();
            let error = secret(&mut data, "token", is_token).unwrap_err();
            assert_eq!(error.code.known(), Some(ErrorCode::Internal), "{bad:?}");
            assert_eq!(error.message, "jevd sent a malformed token");
        }
        for bad in [format!("{} ", "a".repeat(31)), "a".repeat(33)] {
            let mut data = json!({ "request_id": bad }).as_object().unwrap().clone();
            assert!(secret(&mut data, "request_id", is_request_id).is_err());
        }
        let mut data = json!({"token": 7}).as_object().unwrap().clone();
        assert!(secret(&mut data, "token", is_token).is_err(), "not text");
    }

    #[test]
    fn reload_with_no_daemon_does_nothing() {
        let home = tempfile::tempdir().unwrap();
        reload_if_running(&env_in(home.path())).unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn reload_reaches_a_running_daemon_and_reports_a_full_one() {
        let home = tempfile::tempdir().unwrap();
        let task = start_daemon(home.path(), "1");
        let path = home.path().to_path_buf();
        let (alone, full) = tokio::task::spawn_blocking(move || {
            let env = env_in(&path);
            let alone = reload_if_running(&env);
            let _held = hold(&env.paths);
            (alone, reload_if_running(&env))
        })
        .await
        .unwrap();
        task.abort();
        alone.unwrap();
        let error = full.expect_err("a full daemon is not a missing one");
        assert!(
            error.message.contains("max-connections"),
            "{}",
            error.message
        );
    }
}
