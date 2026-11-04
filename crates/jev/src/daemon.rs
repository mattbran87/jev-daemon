//! `jev start`, `jev stop`, `jev status`.

use jev_proto::client::Connection;
use jev_proto::Request;
use serde_json::{json, Value};

use crate::env::Env;
use crate::link::{call, open};
use crate::out::{clean, success, Outcome};

pub fn start(env: &mut Env, json: bool) -> Outcome {
    let mut connection = open(env, true)?;
    let data = call(&mut connection, &Request::Status)?;
    let version = clean(
        data.get("daemon_version")
            .and_then(Value::as_str)
            .unwrap_or("?"),
    );
    success(
        env,
        json,
        &format!("jevd {version} runs"),
        Value::Object(data),
    );
    Ok(())
}

pub fn status(env: &mut Env, json: bool) -> Outcome {
    let mut connection = open(env, false)?;
    let data = call(&mut connection, &Request::Status)?;
    let version = clean(
        data.get("daemon_version")
            .and_then(Value::as_str)
            .unwrap_or("?"),
    );
    let uptime = data.get("uptime").and_then(Value::as_u64).unwrap_or(0);
    let profiles: Vec<String> = data
        .get("profiles")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(Value::as_str).map(clean).collect())
        .unwrap_or_default();
    let human = format!(
        "jevd {version} runs, up {uptime} s; profiles: {}",
        if profiles.is_empty() {
            "none".to_string()
        } else {
            profiles.join(", ")
        }
    );
    success(env, json, &human, Value::Object(data));
    Ok(())
}

/// Stopping a daemon that does not run succeeds, so a script can always stop.
/// Only a missing channel means no daemon: a daemon that is busy, full,
/// stopping, or unsafe to talk to is an error with its own code.
pub fn stop(env: &mut Env, json: bool) -> Outcome {
    let Some(mut connection) = Connection::connect_if_running(&env.paths)? else {
        success(
            env,
            json,
            "jevd is not running",
            json!({ "running": false }),
        );
        return Ok(());
    };
    call(&mut connection, &Request::Stop)?;
    success(env, json, "jevd stops", json!({ "running": true }));
    Ok(())
}
