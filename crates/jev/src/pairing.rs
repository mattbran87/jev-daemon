//! Pairing from the CLI (spec section 3): `jev pair`, `jev allow`, `jev deny`,
//! `jev revoke`, `jev clients`. For a CLI client, `approve` gives jev the
//! token; jev writes it to the keyring, then commits it with `pair-status`.
//! Replies that carry a token are never printed.

use jev_proto::client::Connection;
use jev_proto::{valid_name, ErrorCode, Request, WireError};
use jevd::keystore::{client_account, KeyStore, StoreError};
use jevd::pairing::normalize_code;
use jevd::units::Limit;
use serde_json::{json, Map, Value};

use crate::config::{confirm_loosening, load};
use crate::env::Env;
use crate::link::{
    call, clients as list_clients, is_request_id, is_token, open, reply_result, secret, text,
};
use crate::out::{clean, keyring_failure, success, Failure, Outcome};

/// One waiting request, as `pending` lists it.
struct Waiting {
    code: String,
    name: String,
    profile: Option<String>,
}

fn waiting(connection: &mut Connection) -> Result<Vec<Waiting>, Failure> {
    let data = call(connection, &Request::Pending)?;
    Ok(data
        .get("requests")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|request| {
            Some(Waiting {
                code: request["code"].as_str()?.to_string(),
                name: request["name"].as_str()?.to_string(),
                profile: request["profile"].as_str().map(str::to_string),
            })
        })
        .collect())
}

fn say(env: &mut Env, text: &str) -> Outcome {
    env.terminal.say(text).map_err(|error| {
        Failure::new(
            ErrorCode::Internal,
            format!("cannot write to the terminal: {error}"),
        )
    })
}

fn ask(env: &mut Env, prompt: &str) -> Result<String, Failure> {
    env.terminal
        .ask(prompt)
        .map(|answer| answer.trim().to_string())
        .map_err(|error| {
            Failure::new(
                ErrorCode::Forbidden,
                format!("no answer from the terminal: {error}"),
            )
        })
}

fn need_terminal(env: &Env, command: &str) -> Outcome {
    if env.terminal.available() {
        return Ok(());
    }
    Err(Failure::new(
        ErrorCode::Forbidden,
        format!("{command} needs a terminal, so that a person approves the client"),
    ))
}

/// `jev pair`. With a terminal, the person approves at once. Without one,
/// or with `--no-prompt` (`prompt` false), it prints the code and exits 3
/// (`pending`); `jev allow` finishes it.
pub fn pair(env: &mut Env, json: bool, name: &str, profile: Option<&str>, prompt: bool) -> Outcome {
    // jevd's rule, checked here, so a bad name starts no daemon.
    if !valid_name(name) {
        return Err(Failure::usage(format!(
            "{:?} is not a valid client name: lowercase letters, digits, and hyphens, 1 to 64 characters",
            clean(name)
        )));
    }
    let mut connection = open(env, true)?;
    let data = call(
        &mut connection,
        &Request::Pair {
            name: name.to_string(),
            profile: profile.map(str::to_string),
        },
    )?;
    let code = text(&data, "code")?;
    // Check prompt first: available() opens the terminal device.
    if !prompt || !env.terminal.available() {
        return Err(Failure::new(
            ErrorCode::Pending,
            format!("approve this client in a terminal: jev allow {code}"),
        )
        .with("pair_code", json!(code))
        .with("name", json!(name)));
    }
    say(env, &format!("Pairing code for {name}: {code}"))?;
    let request = Waiting {
        code,
        name: name.to_string(),
        profile: profile.map(str::to_string),
    };
    approve(env, json, &mut connection, &request)
}

/// `jev allow` with no code, when no request waits. Each error says what to do next.
const NONE_WAITS: &str =
    "no pairing request waits; a request lasts 10 minutes, so ask the client to try again";
/// `jev allow <code>`, when no waiting request has the code.
const NO_SUCH_CODE: &str =
    "no waiting request has that code; run jev allow with no code to see the waiting requests";

/// `jev allow [<code>]`: show the waiting requests, and approve one.
pub fn allow(env: &mut Env, json: bool, code: Option<&str>) -> Outcome {
    need_terminal(env, "jev allow")?;
    let mut connection = open(env, true)?;
    let list = waiting(&mut connection)?;
    let code = match code {
        Some(code) => code.to_string(),
        None => {
            if list.is_empty() {
                return Err(Failure::new(ErrorCode::BadRequest, NONE_WAITS));
            }
            say(env, "Waiting pairing requests:")?;
            for request in &list {
                let line = format!("  {}  {}", clean(&request.code), clean(&request.name));
                say(env, &line)?;
            }
            ask(env, "Code to approve: ")?
        }
    };
    let code = normalize_code(&code);
    let request = list
        .into_iter()
        .find(|request| request.code == code)
        .ok_or_else(|| Failure::new(ErrorCode::BadRequest, NO_SUCH_CODE))?;
    approve(env, json, &mut connection, &request)
}

/// Picks the profile, warns about a replace, asks for a `y`, and approves.
fn approve(env: &mut Env, json: bool, connection: &mut Connection, request: &Waiting) -> Outcome {
    let profiles = load(env)?.profiles;
    if profiles.is_empty() {
        return Err(Failure::new(
            ErrorCode::BadRequest,
            "no profile has a key yet; run jev key add first",
        ));
    }
    let suggested = request
        .profile
        .clone()
        .filter(|profile| profiles.contains(profile))
        .unwrap_or_else(|| profiles[0].clone());
    let name = clean(&request.name);
    say(env, &format!("Profiles: {}", profiles.join(", ")))?;
    let answer = ask(env, &format!("Profile for {name} [{suggested}]: "))?;
    let profile = if answer.is_empty() { suggested } else { answer };
    if !profiles.contains(&profile) {
        return Err(Failure::new(
            ErrorCode::BadRequest,
            format!("no profile named {:?}", clean(&profile)),
        ));
    }
    let exists = list_clients(connection)?
        .iter()
        .any(|client| client["name"] == request.name);
    if exists {
        say(
            env,
            &format!(
                "A client named {name} exists. The new one replaces it when its token is stored. \
                 jev revoke {name} stops the old one at once."
            ),
        )?;
    }
    if ask(
        env,
        &format!("Approve {name} on profile {profile}? Type y: "),
    )? != "y"
    {
        return Err(Failure::new(
            ErrorCode::Forbidden,
            "the approval was not confirmed",
        ));
    }
    let mut data = call(
        connection,
        &Request::Approve {
            code: request.code.clone(),
            profile: profile.clone(),
        },
    )?;
    // Asked before commit_cli takes the token out of the reply.
    let cli = data.contains_key("token");
    if cli {
        commit_cli(env, connection, &request.name, &mut data)?;
    }
    let human = if cli {
        format!("Paired {name} on profile {profile}; its token is in the keyring")
    } else {
        format!("Approved {name} on profile {profile}; the client collects its token")
    };
    success(
        env,
        json,
        &human,
        json!({ "name": request.name, "profile": profile }),
    );
    Ok(())
}

/// The CLI flow (spec section 3): write the token to the keyring, then
/// commit with `pair-status`. If the commit fails, put the old entry back,
/// but only if the keyring still holds this run's token.
///
/// That check keeps the token of a second `jev allow` for the same name in
/// most orders, but not all. For example: run A reads the old token O, run
/// B writes its token and commits, A writes over B's token, A's commit
/// fails because its approval expired, and A puts O back. Then the keyring
/// holds O, which jevd no longer knows, and B's client must pair again. The
/// order "B writes, A writes over it, B commits, A's commit fails" ends the
/// same way.
fn commit_cli(
    env: &mut Env,
    connection: &mut Connection,
    name: &str,
    data: &mut Map<String, Value>,
) -> Outcome {
    let request_id = secret(data, "request_id", is_request_id)?;
    let token = secret(data, "token", is_token)?;
    let account = client_account(name);
    let old = env.store.get(&account).map_err(keyring_failure)?;
    env.store.set(&account, &token).map_err(keyring_failure)?;
    let request = Request::PairStatus {
        request_id: request_id.to_string(),
    };
    let committed = match connection.call(&request) {
        Err(error) => Err(WireError {
            message: format!(
                "{}; if the pairing was committed, pair again",
                error.message
            ),
            ..error
        }),
        Ok(reply) => {
            reply_result(reply).and_then(|data| match data.get("state").and_then(Value::as_str) {
                Some("approved") => Ok(()),
                _ => Err(WireError::new(
                    ErrorCode::Internal,
                    "the approval was cancelled or expired",
                )),
            })
        }
    };
    let Err(error) = committed else {
        return Ok(());
    };
    if let Err(problem) = restore(
        env.store.as_ref(),
        &account,
        old.as_deref().map(String::as_str),
        &token,
    ) {
        return Err(Failure::from(WireError {
            message: format!(
                "{}; then the keyring failed: {problem}; the keyring may hold a token \
                 jevd does not know; run jev pair --name {} again",
                error.message,
                clean(name)
            ),
            ..error
        }));
    }
    Err(error.into())
}

/// After a failed commit: puts `old` back in `account`, or deletes the
/// entry when there was none, but only while the keyring still holds
/// `token`, the token that this run wrote. Another run's newer token stays.
fn restore(
    store: &dyn KeyStore,
    account: &str,
    old: Option<&str>,
    token: &str,
) -> Result<(), StoreError> {
    let current = store.get(account)?;
    if current.as_deref().map(String::as_str) != Some(token) {
        return Ok(());
    }
    match old {
        Some(old) => store.set(account, old),
        None => store.delete(account).map(|_| ()),
    }
}

pub fn deny(env: &mut Env, json: bool, code: &str) -> Outcome {
    let mut connection = open(env, true)?;
    call(
        &mut connection,
        &Request::Deny {
            code: code.to_string(),
        },
    )?;
    success(env, json, "Denied", json!({}));
    Ok(())
}

pub fn revoke(env: &mut Env, json: bool, name: &str) -> Outcome {
    let mut connection = open(env, true)?;
    call(
        &mut connection,
        &Request::Revoke {
            name: name.to_string(),
        },
    )?;
    success(
        env,
        json,
        &format!("Revoked {}", clean(name)),
        json!({ "name": name }),
    );
    Ok(())
}

pub fn clients(env: &mut Env, json: bool) -> Outcome {
    let mut connection = open(env, true)?;
    let data = call(&mut connection, &Request::Clients)?;
    let list = data
        .get("clients")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let human = if list.is_empty() {
        "No paired clients".to_string()
    } else {
        list.iter()
            .map(|client| {
                format!(
                    "{}  profile {}  limit {}",
                    clean(client["name"].as_str().unwrap_or("?")),
                    clean(client["profile"].as_str().unwrap_or("?")),
                    clean(client["limit"].as_str().unwrap_or("default")),
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    success(env, json, &human, Value::Object(data));
    Ok(())
}

/// `jev clients limit <name> <count>/<seconds>|default`. A limit looser than
/// the client's current one needs a terminal and a typed `y`.
pub fn limit(env: &mut Env, json: bool, name: &str, value: &str) -> Outcome {
    let new: Option<Limit> = match value {
        "default" => None,
        text => Some(text.parse().map_err(Failure::usage)?),
    };
    let mut connection = open(env, true)?;
    let client = list_clients(&mut connection)?
        .into_iter()
        .find(|client| client["name"] == name)
        .ok_or_else(|| {
            Failure::new(
                ErrorCode::BadRequest,
                format!("no client named {:?}", clean(name)),
            )
        })?;
    let default = load(env)?.rate_limit;
    let current = client["limit"]
        .as_str()
        .and_then(|text| text.parse::<Limit>().ok())
        .unwrap_or(default);
    let effective = new.unwrap_or(default);
    if effective.looser_than(current) {
        confirm_loosening(
            env,
            &format!("the limit of {} from {current} to {effective}", clean(name)),
        )?;
    }
    call(
        &mut connection,
        &Request::SetLimit {
            name: name.to_string(),
            limit: new.map(|limit| limit.to_string()),
        },
    )?;
    success(
        env,
        json,
        &format!("The limit of {} is {effective}", clean(name)),
        json!({ "name": name, "limit": new.map(|limit| limit.to_string()) }),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use jevd::keystore::MemoryKeyring;

    const ACCOUNT: &str = "client:app";

    #[test]
    fn restore_puts_the_old_token_back() {
        let store = MemoryKeyring::new();
        store.set(ACCOUNT, "mine").unwrap();
        restore(&store, ACCOUNT, Some("old"), "mine").unwrap();
        assert_eq!(store.get(ACCOUNT).unwrap().unwrap().as_str(), "old");
    }

    #[test]
    fn restore_deletes_a_new_entry_when_there_was_none() {
        let store = MemoryKeyring::new();
        store.set(ACCOUNT, "mine").unwrap();
        restore(&store, ACCOUNT, None, "mine").unwrap();
        assert!(store.get(ACCOUNT).unwrap().is_none());
    }

    #[test]
    fn restore_keeps_a_token_that_another_run_wrote() {
        let store = MemoryKeyring::new();
        store.set(ACCOUNT, "theirs").unwrap();
        restore(&store, ACCOUNT, Some("old"), "mine").unwrap();
        assert_eq!(store.get(ACCOUNT).unwrap().unwrap().as_str(), "theirs");
        restore(&store, ACCOUNT, None, "mine").unwrap();
        assert_eq!(store.get(ACCOUNT).unwrap().unwrap().as_str(), "theirs");
    }
}
