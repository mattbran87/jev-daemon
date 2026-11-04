//! `jev key add`, `jev key remove`, `jev key list` (spec section 6). A key
//! goes from the person straight into the OS keyring; it never passes
//! through the socket, and jev never prints it.

use jev_proto::client::Connection;
use jev_proto::{valid_name, ErrorCode, Request, WireError};
use jevd::key::check_key;
use jevd::keystore::profile_account;
use serde_json::json;
use zeroize::Zeroizing;

use crate::config::{load, reload_after_save, save};
use crate::env::{Env, SECRET_ROOM};
use crate::link::{call, clients, open};
use crate::out::{keyring_failure, success, Failure, Outcome};

fn check_name(profile: &str) -> Outcome {
    if valid_name(profile) {
        return Ok(());
    }
    Err(Failure::usage(format!(
        "{profile:?}: profile names use lowercase letters, digits, and hyphens, 1 to 64 characters"
    )))
}

pub fn add(env: &mut Env, json: bool, profile: &str, stdin: bool, check: bool) -> Outcome {
    check_name(profile)?;
    // Read before the key, so a broken settings file asks for no key.
    let mut settings = load(env)?;
    let raw: Zeroizing<String> = if stdin {
        env.stdin
            .read_secret()
            .map_err(|error| {
                Failure::new(ErrorCode::Internal, format!("cannot read stdin: {error}"))
            })?
            .ok_or_else(|| {
                Failure::new(
                    ErrorCode::BadKey,
                    format!("stdin holds more than {SECRET_ROOM} bytes; a key has at most 512"),
                )
            })?
    } else {
        if !env.terminal.available() {
            return Err(Failure::new(
                ErrorCode::Forbidden,
                "jev key add needs a terminal for its hidden prompt; or pass the key with --stdin",
            ));
        }
        env.terminal
            .ask_hidden(&format!("TypeSafe key for profile {profile}: "))
            .map_err(|error| {
                Failure::new(
                    ErrorCode::Forbidden,
                    format!("cannot read the key: {error}"),
                )
            })?
    };
    let key = check_key(&raw).map_err(|code| {
        let message = match code {
            ErrorCode::NoKey => "the key is empty".to_string(),
            _ => {
                "the key must be printable ASCII with no spaces, at most 512 characters".to_string()
            }
        };
        Failure::from(WireError::new(code, message).for_profile(profile))
    })?;
    if check {
        check_with_jev(env, profile, key)?;
    }
    env.store
        .set(&profile_account(profile), key)
        .map_err(keyring_failure)?;
    settings
        .add_profile(profile)
        .map_err(|problem| Failure::new(ErrorCode::Internal, problem))?;
    save(env, &settings)?;
    reload_after_save(env)?;
    let checked = if check { "; TypeSafe accepted it" } else { "" };
    success(
        env,
        json,
        &format!("Stored the key for profile {profile}{checked}"),
        json!({ "profile": profile, "checked": check }),
    );
    Ok(())
}

/// Sends one small real request with `key`. It spends one request, or two
/// when TypeSafe answers 429 or 529, because the Jev client retries once.
fn check_with_jev(env: &Env, profile: &str, key: &str) -> Outcome {
    let client =
        (env.jev_client)().map_err(|problem| Failure::new(ErrorCode::Internal, problem))?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| {
            Failure::new(
                ErrorCode::Internal,
                format!("cannot start the runtime: {error}"),
            )
        })?;
    let state = json!("jev key add --check");
    let questions = json!({ "check": { "type": "noul", "question": "Is this a test request?" } });
    runtime
        .block_on(client.ask(&state, &questions, key, false))
        .map(|_| ())
        .map_err(|failure| Failure::from(failure.to_wire().for_profile(profile)))
}

/// The names of the paired clients, as jevd lists them now.
fn client_names(connection: &mut Connection) -> Result<Vec<String>, Failure> {
    Ok(clients(connection)?
        .iter()
        .filter_map(|client| client["name"].as_str().map(str::to_string))
        .collect())
}

pub fn remove(env: &mut Env, json: bool, profile: &str, force: bool) -> Outcome {
    check_name(profile)?;
    // Read before any change, so a broken settings file revokes nothing.
    let mut settings = load(env)?;
    let mut connection = open(env, true)?;
    let users: Vec<String> = clients(&mut connection)?
        .iter()
        .filter(|client| client["profile"] == profile)
        .filter_map(|client| client["name"].as_str().map(str::to_string))
        .collect();
    if !users.is_empty() && !force {
        return Err(Failure::new(
            ErrorCode::BadRequest,
            format!(
                "profile {profile} is in use by: {}; jev key remove --force also revokes them",
                users.join(", ")
            ),
        ));
    }
    for name in &users {
        match call(&mut connection, &Request::Revoke { name: name.clone() }) {
            Ok(_) => {}
            // Revoked by someone else in the meantime, if the name is gone.
            Err(error) if error.code.known() == Some(ErrorCode::BadRequest) => {
                if client_names(&mut connection)?.contains(name) {
                    return Err(error.into());
                }
            }
            Err(error) => return Err(error.into()),
        }
    }
    env.store
        .delete(&profile_account(profile))
        .map_err(keyring_failure)?;
    settings.remove_profile(profile);
    save(env, &settings)?;
    call(&mut connection, &Request::Reload)?;
    success(
        env,
        json,
        &format!("Removed profile {profile}; revoked {} clients", users.len()),
        json!({ "profile": profile, "revoked": users }),
    );
    Ok(())
}

pub fn list(env: &mut Env, json: bool) -> Outcome {
    let settings = load(env)?;
    let human = if settings.profiles.is_empty() {
        "No profiles; run jev key add".to_string()
    } else {
        settings.profiles.join("\n")
    };
    success(env, json, &human, json!({ "profiles": settings.profiles }));
    Ok(())
}
