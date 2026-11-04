//! `jev config get|set|list` (spec section 5). A change that loosens
//! protection needs a terminal and a typed `y`; without one it exits 9.

use jev_proto::{ErrorCode, WireError};
use jevd::settings::{Settings, SETTING_NAMES};
use serde_json::{json, Map, Value};

use crate::env::Env;
use crate::link::reload_if_running;
use crate::out::{success, Failure, Outcome};

/// Asks the person to confirm a change that loosens protection.
/// No terminal, or any answer but `y`: `forbidden` (exit 9).
pub fn confirm_loosening(env: &mut Env, what: &str) -> Outcome {
    if !env.terminal.available() {
        return Err(Failure::new(
            ErrorCode::Forbidden,
            format!("this change loosens protection ({what}); run it in a terminal"),
        ));
    }
    let answer = env
        .terminal
        .ask(&format!(
            "This loosens protection: {what}. Type y to go on: "
        ))
        .map_err(|error| Failure::new(ErrorCode::Forbidden, format!("no answer: {error}")))?;
    if answer.trim() != "y" {
        return Err(Failure::new(
            ErrorCode::Forbidden,
            "the change was not confirmed",
        ));
    }
    Ok(())
}

pub fn load(env: &Env) -> Result<Settings, Failure> {
    Settings::load(&env.paths.settings_file())
        .map_err(|problem| Failure::new(ErrorCode::Internal, problem))
}

pub fn save(env: &Env, settings: &Settings) -> Outcome {
    settings
        .save(&env.paths.settings_file())
        .map_err(|problem| Failure::new(ErrorCode::Internal, problem))
}

/// Sends `reload` after a save, when a daemon runs. A failure keeps the
/// reload error's code and says that the file was saved.
pub fn reload_after_save(env: &Env) -> Outcome {
    reload_if_running(env).map_err(|error| {
        Failure::from(WireError {
            message: format!("saved; the running jevd did not reload: {}", error.message),
            ..error
        })
    })
}

pub fn get(env: &mut Env, json: bool, name: &str) -> Outcome {
    let settings = load(env)?;
    let value = settings.get(name).ok_or_else(|| {
        Failure::usage(format!(
            "no setting named {name:?}; names: {}",
            SETTING_NAMES.join(", ")
        ))
    })?;
    success(env, json, &value, json!({ "name": name, "value": value }));
    Ok(())
}

pub fn list(env: &mut Env, json: bool) -> Outcome {
    let settings = load(env)?;
    let pairs = settings.list();
    let human: Vec<String> = pairs
        .iter()
        .map(|(name, value)| format!("{name} = {value}"))
        .collect();
    let data: Map<String, Value> = pairs
        .into_iter()
        .map(|(name, value)| (name.to_string(), Value::String(value)))
        .collect();
    success(env, json, &human.join("\n"), json!({ "settings": data }));
    Ok(())
}

pub fn set(env: &mut Env, json: bool, name: &str, value: &str) -> Outcome {
    let settings = load(env)?;
    let change = settings.with_change(name, value).map_err(Failure::usage)?;
    if change.loosens {
        let old = settings.get(name).unwrap_or_default();
        confirm_loosening(env, &format!("{name} from {old} to {value}"))?;
    }
    save(env, &change.settings)?;
    reload_after_save(env)?;
    success(
        env,
        json,
        &format!("{name} = {value}"),
        json!({ "name": name, "value": value }),
    );
    Ok(())
}
