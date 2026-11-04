//! The jev command line (spec section 6). `run` parses the arguments, runs
//! one command against an `Env`, prints its output, and returns the exit
//! code. Tests call `run` with an in-memory `Env`.

pub mod ask;
pub mod cli;
pub mod config;
pub mod daemon;
pub mod env;
pub mod keys;
pub mod link;
pub mod out;
pub mod pairing;
pub mod term;

use std::ffi::OsString;
use std::io::Write;

use clap::Parser;

use crate::cli::{Cli, ClientsCommand, Command, ConfigCommand, KeyCommand};
use crate::env::Env;
use jev_proto::ErrorCode;

use crate::out::{clean, clean_lines, escape_json, failure_json, report, report_to, Failure};

/// True when a bad command line must also print its error as JSON on
/// stdout: with `--json`, or for `jev ask`, which always prints JSON.
fn wants_json(args: &[OsString]) -> bool {
    let words = args.iter().skip(1).filter_map(|arg| arg.to_str());
    let mut json = false;
    let mut first = None;
    for word in words {
        if word == "--" {
            break;
        }
        if word == "--json" {
            json = true;
        } else if first.is_none() && !word.starts_with('-') {
            first = Some(word);
        }
    }
    json || first == Some("ask")
}

/// Runs one command line. Returns the exit code (spec section 6).
pub fn run(args: Vec<OsString>, env: &mut Env) -> i32 {
    match parse(args, &mut *env.out, &mut *env.err) {
        Ok(cli) => run_cli(&cli, env),
        Err(code) => code,
    }
}

/// Parses a command line. Help and version text go to `out`, and give
/// `Err(0)`; a bad command line goes to `err`, also as one failure JSON line
/// on `out` when `wants_json`, and gives `Err(2)`. clap's text goes through
/// `clean_lines`, because it can quote an argument.
pub fn parse(args: Vec<OsString>, out: &mut dyn Write, err: &mut dyn Write) -> Result<Cli, i32> {
    let wants_json = wants_json(&args);
    let error = match Cli::try_parse_from(args) {
        Ok(cli) => return Ok(cli),
        Err(error) => error,
    };
    let code = error.exit_code();
    let text = clean_lines(&error.render().to_string());
    if code == 0 {
        // Help and version: text on stdout, also with --json.
        let _ = write!(out, "{text}");
        return Err(code);
    }
    let _ = write!(err, "{text}");
    if wants_json {
        let message = text
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("bad command line");
        let message = message.strip_prefix("error: ").unwrap_or(message);
        let failure = Failure::usage(message);
        let _ = writeln!(out, "{}", escape_json(&failure_json(&failure).to_string()));
    }
    Err(code)
}

/// Reports that jev could not build its `Env`, as the command would report
/// a failure: `jev ask` and `--json` print one failure JSON line on `out`
/// (`jev ask --value` prints the code on `err`), the others print to `err`.
pub fn report_no_env(cli: &Cli, problem: &str, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let failure = Failure::new(ErrorCode::Internal, problem);
    match &cli.command {
        Command::Ask(args) if args.value.is_some() => {
            let _ = writeln!(err, "{}", clean(failure.error.code.as_str()));
            failure.exit_code()
        }
        Command::Ask(_) => report_to(out, err, true, &failure),
        _ => report_to(out, err, cli.json, &failure),
    }
}

/// Runs a parsed command line against `env`. Returns the exit code.
pub fn run_cli(cli: &Cli, env: &mut Env) -> i32 {
    let json = cli.json;
    let outcome = match &cli.command {
        Command::Ask(args) => return ask::run(env, args),
        Command::Start => daemon::start(env, json),
        Command::Stop => daemon::stop(env, json),
        Command::Status => daemon::status(env, json),
        Command::Key { command } => match command {
            KeyCommand::Add {
                profile,
                stdin,
                check,
            } => keys::add(env, json, profile, *stdin, *check),
            KeyCommand::Remove { profile, force } => keys::remove(env, json, profile, *force),
            KeyCommand::List => keys::list(env, json),
        },
        Command::Pair {
            name,
            profile,
            no_prompt,
        } => pairing::pair(env, json, name, profile.as_deref(), !no_prompt),
        Command::Allow { code } => pairing::allow(env, json, code.as_deref()),
        Command::Deny { code } => pairing::deny(env, json, code),
        Command::Revoke { name } => pairing::revoke(env, json, name),
        Command::Clients { command } => match command {
            None => pairing::clients(env, json),
            Some(ClientsCommand::Limit { name, limit }) => pairing::limit(env, json, name, limit),
        },
        Command::Config { command } => match command {
            ConfigCommand::Get { name } => config::get(env, json, name),
            ConfigCommand::Set { name, value } => config::set(env, json, name, value),
            ConfigCommand::List => config::list(env, json),
        },
    };
    match outcome {
        Ok(()) => 0,
        Err(failure) => report(env, json, &failure),
    }
}
