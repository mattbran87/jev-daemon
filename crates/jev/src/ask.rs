//! `jev ask` (spec section 6). The request comes from one place: flags,
//! `--request`, or JSON on stdin. The output is always JSON on stdout,
//! except with `--value`, which prints only that answer's number.

use std::io::Write;

use jev_proto::{valid_name, ErrorCode, Request, MAX_LINE};
use jevd::keystore::client_account;
use jevd::shape::{valid_question_id, MAX_CHOICES};
use serde_json::{json, Map, Value};
use zeroize::Zeroizing;

use crate::cli::AskArgs;
use crate::env::{read_capped, Env};
use crate::link::{open, reply_result};
use crate::out::{clean, escape_json, failure_json, keyring_failure, Failure};

/// The most levels one score question can have (TypeSafe's limit).
const MAX_LEVELS: usize = 10;

/// The state and the questions of one request.
#[derive(Debug, PartialEq)]
pub struct Built {
    pub state: Value,
    pub questions: Value,
}

/// `id=question` into its two parts. The id follows jevd's rule.
fn split(flag: &str, text: &str) -> Result<(String, String), Failure> {
    let (id, question) = text
        .split_once('=')
        .ok_or_else(|| Failure::usage(format!("--{flag} takes <id>=<text>, not {text:?}")))?;
    if !valid_question_id(id) {
        return Err(Failure::usage(format!(
            "the question id {id:?} must be 1 to 64 letters, digits, '_', or '-'"
        )));
    }
    Ok((id.to_string(), question.to_string()))
}

/// Text over `MAX_LINE` bytes cannot fit in one request line.
fn too_large(what: &str) -> Failure {
    Failure::new(
        ErrorCode::TooLarge,
        format!("{what} is over {MAX_LINE} bytes, the most one request can hold"),
    )
}

/// All of stdin, or `None` when it holds more than `MAX_LINE` bytes.
fn read_stdin(env: &mut Env) -> Result<Option<String>, Failure> {
    env.stdin
        .read_all(MAX_LINE)
        .map_err(|error| Failure::usage(format!("cannot read stdin: {error}")))
}

/// Builds the request from exactly one source.
pub fn build(args: &AskArgs, env: &mut Env) -> Result<Built, Failure> {
    let flags = args.state.is_some()
        || [
            &args.noul,
            &args.score,
            &args.choice,
            &args.option,
            &args.level,
        ]
        .iter()
        .any(|list| !list.is_empty());
    if flags && args.request.is_some() {
        return Err(Failure::usage(
            "--request cannot go with --state or a question flag",
        ));
    }
    if flags || args.request.is_some() {
        if !env.stdin.is_terminal() {
            // Over the cap is data too.
            let piped = read_stdin(env)?;
            // White space, and a byte order mark first, count as empty.
            let has_data = |text: String| {
                let text = text.trim_start();
                !text
                    .strip_prefix('\u{feff}')
                    .unwrap_or(text)
                    .trim()
                    .is_empty()
            };
            if piped.is_none_or(has_data) {
                return Err(Failure::usage(
                    "the request comes from the command line, and stdin has data too; use one",
                ));
            }
        }
        if let Some(request) = &args.request {
            return from_json(request);
        }
        return from_flags(args);
    }
    if env.stdin.is_terminal() {
        return Err(Failure::usage(
            "give the request as JSON on stdin, with --request, or with --state and --noul, --score, or --choice",
        ));
    }
    let text = read_stdin(env)?.ok_or_else(|| too_large("stdin"))?;
    // A Windows editor or shell can put a byte order mark first.
    from_json(text.strip_prefix('\u{feff}').unwrap_or(&text))
}

fn from_json(text: &str) -> Result<Built, Failure> {
    let value: Value = serde_json::from_str(text)
        .map_err(|error| Failure::usage(format!("the request is not JSON: {error}")))?;
    let (Some(state), Some(questions)) = (value.get("state"), value.get("questions")) else {
        return Err(Failure::usage("the request needs a state and questions"));
    };
    Ok(Built {
        state: state.clone(),
        questions: questions.clone(),
    })
}

/// The `criteria` of question `id`, which must have type `kind`.
fn criteria_of<'a>(
    questions: &'a mut Map<String, Value>,
    id: &str,
    kind: &str,
    flag: &str,
) -> Result<&'a mut Value, Failure> {
    questions
        .get_mut(id)
        .filter(|question| question["type"] == kind)
        .map(|question| &mut question["criteria"])
        .ok_or_else(|| Failure::usage(format!("--{flag} {id:?} names no --{kind} question")))
}

fn from_flags(args: &AskArgs) -> Result<Built, Failure> {
    let state = match args.state.as_deref() {
        None => return Err(Failure::usage("--state is needed with a question flag")),
        Some(text) => match text.strip_prefix('@') {
            Some(path) => std::fs::File::open(path)
                .and_then(|file| read_capped(file, MAX_LINE))
                .map_err(|error| Failure::usage(format!("cannot read {path}: {error}")))?
                .ok_or_else(|| too_large(path))?,
            None => text.to_string(),
        },
    };
    let mut questions = Map::new();
    let flags = [
        ("noul", &args.noul),
        ("score", &args.score),
        ("choice", &args.choice),
    ];
    for (kind, list) in flags {
        for entry in list {
            let (id, question) = split(kind, entry)?;
            if questions.contains_key(&id) {
                return Err(Failure::usage(format!(
                    "the question id {id:?} appears twice"
                )));
            }
            let mut object = json!({ "type": kind, "instructions": question });
            match kind {
                "score" => object["criteria"] = json!([]),
                "choice" => object["criteria"] = json!({}),
                _ => {}
            }
            questions.insert(id, object);
        }
    }
    if questions.is_empty() {
        return Err(Failure::usage(
            "give at least one --noul, --score, or --choice question",
        ));
    }
    for entry in &args.level {
        let (id, text) = split("level", entry)?;
        let levels = criteria_of(&mut questions, &id, "score", "level")?
            .as_array_mut()
            .expect("a score question from flags holds an array");
        if levels.len() == MAX_LEVELS {
            return Err(Failure::usage(format!(
                "the score question {id:?} has more than {MAX_LEVELS} levels"
            )));
        }
        levels.push(Value::String(text));
    }
    for entry in &args.option {
        let (id, name) = split("option", entry)?;
        let options = criteria_of(&mut questions, &id, "choice", "option")?
            .as_object_mut()
            .expect("a choice question from flags holds an object");
        if options.contains_key(&name) {
            return Err(Failure::usage(format!(
                "the option {name:?} appears twice on {id:?}"
            )));
        }
        if options.len() == MAX_CHOICES {
            return Err(Failure::usage(format!(
                "the choice question {id:?} has more than {MAX_CHOICES} options"
            )));
        }
        options.insert(name, Value::Null);
    }
    for (id, question) in &questions {
        let missing = match &question["criteria"] {
            Value::Array(levels) if levels.is_empty() => Some("--level"),
            Value::Object(options) if options.is_empty() => Some("--option"),
            _ => None,
        };
        if let Some(flag) = missing {
            return Err(Failure::usage(format!(
                "the question {id:?} needs at least one {flag}"
            )));
        }
    }
    Ok(Built {
        state: Value::String(state),
        questions: Value::Object(questions),
    })
}

/// Runs `jev ask` and returns its exit code. It prints its own output.
pub fn run(env: &mut Env, args: &AskArgs) -> i32 {
    match ask(env, args) {
        Ok(reply) => print_reply(env, args, reply),
        Err(failure) => {
            if args.value.is_some() {
                let _ = writeln!(env.err, "{}", clean(failure.error.code.as_str()));
            } else {
                let _ = writeln!(env.out, "{}", failure_line(&failure));
            }
            failure.exit_code()
        }
    }
}

fn ask(env: &mut Env, args: &AskArgs) -> Result<jev_proto::Reply, Failure> {
    let client = args
        .client
        .clone()
        .or_else(|| (env.vars)("JEV_CLIENT"))
        .filter(|name| !name.is_empty())
        .ok_or_else(|| Failure::usage("name the client with --client or JEV_CLIENT"))?;
    if !valid_name(&client) {
        return Err(Failure::usage(format!(
            "{client:?} is not a valid client name"
        )));
    }
    let built = build(args, env)?;
    if let Some(id) = &args.value {
        if built.questions.get(id).is_none() {
            return Err(Failure::usage(format!("--value {id:?} names no question")));
        }
    }
    let token = env
        .store
        .get(&client_account(&client))
        .map_err(keyring_failure)?
        .ok_or_else(|| {
            Failure::new(
                ErrorCode::NotPaired,
                format!("client {client} has no token; run jev pair --name {client}"),
            )
        })?;
    let request = Request::Ask {
        token: token.to_string(),
        state: built.state,
        questions: built.questions,
    };
    // jevd refuses a longer line, so do not start or wake it for one.
    // The line holds the token, so it is wiped when dropped.
    let line = Zeroizing::new(serde_json::to_string(&request).map_err(|error| {
        Failure::new(ErrorCode::Internal, format!("cannot write JSON: {error}"))
    })?);
    if line.len() > MAX_LINE {
        return Err(too_large("the request"));
    }
    let mut connection = open(env, true)?;
    Ok(connection.call(&request)?)
}

/// The failure as one JSON line, with unsafe characters escaped.
fn failure_line(failure: &Failure) -> String {
    escape_json(&failure_json(failure).to_string())
}

fn print_reply(env: &mut Env, args: &AskArgs, reply: jev_proto::Reply) -> i32 {
    let data = match reply_result(reply) {
        Ok(data) => data,
        Err(error) => {
            let failure = Failure::from(error);
            if args.value.is_some() {
                let _ = writeln!(env.err, "{}", clean(failure.error.code.as_str()));
            } else {
                let _ = writeln!(env.out, "{}", failure_line(&failure));
            }
            return failure.exit_code();
        }
    };
    let Some(id) = &args.value else {
        let reply = jev_proto::Reply {
            ok: true,
            error: None,
            data,
        };
        let line = serde_json::to_string(&reply).expect("a reply always serializes");
        let _ = writeln!(env.out, "{}", escape_json(&line));
        return 0;
    };
    // The answer's own type names the field that holds its number.
    let answer = data.get("answers").and_then(|answers| answers.get(id));
    let number = answer.and_then(|answer| match answer.get("type")?.as_str()? {
        kind @ ("noul" | "score") => answer.get(kind)?.as_f64(),
        _ => None,
    });
    match number {
        Some(number) => {
            let _ = writeln!(env.out, "{number}");
            0
        }
        None => {
            let _ = writeln!(env.err, "bad-answer");
            ErrorCode::BadAnswer.exit_code()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{Cli, Command};
    use crate::env::{Buffer, TextInput};
    use crate::term::ScriptedTerminal;
    use clap::Parser;
    use jevd::keystore::MemoryKeyring;
    use std::sync::Arc;

    fn env(stdin: Option<&str>) -> Env {
        Env {
            paths: jev_proto::Paths::under(std::path::Path::new("/nowhere")).unwrap(),
            jevd: "/nowhere/jevd".into(),
            store: Arc::new(MemoryKeyring::new()),
            terminal: Box::new(ScriptedTerminal::none()),
            stdin: Box::new(TextInput(stdin.map(str::to_string))),
            out: Box::new(Buffer::default()),
            err: Box::new(Buffer::default()),
            vars: Box::new(|_| None),
            jev_client: Box::new(|| Err("no Jev in this test".to_string())),
        }
    }

    fn args(line: &[&str]) -> AskArgs {
        let mut full = vec!["jev", "ask"];
        full.extend_from_slice(line);
        match Cli::parse_from(full).command {
            Command::Ask(args) => args,
            other => panic!("not ask: {other:?}"),
        }
    }

    fn owned_args(line: &[String]) -> AskArgs {
        let refs: Vec<&str> = line.iter().map(String::as_str).collect();
        args(&refs)
    }

    #[test]
    fn flags_and_stdin_json_give_the_same_request() {
        let flags = build(
            &args(&[
                "--state",
                "the state",
                "--noul",
                "q=Done?",
                "--score",
                "s=How good?",
                "--level",
                "s=Bad",
                "--level",
                "s=Good",
            ]),
            &mut env(Some("")),
        )
        .unwrap();
        let stdin = r#"{"state":"the state","questions":{"q":{"type":"noul","instructions":"Done?"},"s":{"type":"score","instructions":"How good?","criteria":["Bad","Good"]}}}"#;
        let piped = build(&args(&[]), &mut env(Some(stdin))).unwrap();
        assert_eq!(flags, piped);
        let line = |built: &Built| {
            serde_json::to_string(&Request::Ask {
                token: "t".into(),
                state: built.state.clone(),
                questions: built.questions.clone(),
            })
            .unwrap()
        };
        assert_eq!(line(&flags), line(&piped), "byte-identical requests");
        let request = build(&args(&["--request", stdin]), &mut env(Some(""))).unwrap();
        assert_eq!(request, piped);
    }

    #[test]
    fn flags_plus_piped_stdin_is_a_usage_error() {
        let failure = build(
            &args(&["--state", "s", "--noul", "q=x"]),
            &mut env(Some("{}")),
        )
        .unwrap_err();
        assert_eq!(failure.exit_code(), 2);
    }

    #[test]
    fn a_stdin_with_only_a_byte_order_mark_counts_as_empty() {
        for stdin in ["\u{feff}", "\u{feff}\r\n", " \n"] {
            let built = build(
                &args(&["--state", "s", "--noul", "q=x"]),
                &mut env(Some(stdin)),
            );
            assert!(built.is_ok(), "{stdin:?}");
        }
    }

    #[test]
    fn a_terminal_stdin_with_no_flags_is_a_usage_error() {
        let failure = build(&args(&[]), &mut env(None)).unwrap_err();
        assert_eq!(failure.exit_code(), 2);
    }

    #[test]
    fn bad_flags_are_usage_errors() {
        for line in [
            &["--noul", "q=x"][..],
            &["--state", "s"],
            &["--state", "s", "--noul", "no-equals"],
            &["--state", "s", "--noul", "q=a", "--score", "q=b"],
            &["--state", "s", "--request", "{}"],
            &["--request", "not json"],
            &["--request", r#"{"state":"s"}"#],
        ] {
            let failure = build(&args(line), &mut env(Some(""))).unwrap_err();
            assert_eq!(failure.exit_code(), 2, "{line:?}");
        }
    }

    #[test]
    fn choice_and_level_flags_build_criteria() {
        let flags = build(
            &args(&[
                "--state",
                "s",
                "--option",
                "mood=angry",
                "--choice",
                "mood=What mood?",
                "--option",
                "mood=calm",
                "--score",
                "u=How urgent?",
                "--level",
                "u=Can wait",
                "--level",
                "u=Now",
            ]),
            &mut env(Some("")),
        )
        .unwrap();
        let stdin = r#"{"state":"s","questions":{"mood":{"type":"choice","instructions":"What mood?","criteria":{"angry":null,"calm":null}},"u":{"type":"score","instructions":"How urgent?","criteria":["Can wait","Now"]}}}"#;
        assert_eq!(flags, build(&args(&[]), &mut env(Some(stdin))).unwrap());
    }

    #[test]
    fn choice_and_level_flags_hold_their_limits() {
        let mut line: Vec<String> = ["--state", "s", "--choice", "c=x", "--score", "u=x"]
            .map(String::from)
            .to_vec();
        for i in 0..255 {
            line.extend(["--option".to_string(), format!("c=o{i}")]);
        }
        for i in 0..10 {
            line.extend(["--level".to_string(), format!("u=l{i}")]);
        }
        build(&owned_args(&line), &mut env(Some(""))).unwrap();
        let mut too_many_options = line.clone();
        too_many_options.extend(["--option".to_string(), "c=o255".to_string()]);
        let mut too_many_levels = line;
        too_many_levels.extend(["--level".to_string(), "u=l10".to_string()]);
        for line in [too_many_options, too_many_levels] {
            let failure = build(&owned_args(&line), &mut env(Some(""))).unwrap_err();
            assert_eq!(failure.exit_code(), 2);
        }
    }

    #[test]
    fn bad_choice_and_level_flags_are_usage_errors() {
        for line in [
            &["--state", "s", "--choice", "c=x"][..],
            &["--state", "s", "--score", "u=x"],
            &["--state", "s", "--noul", "q=x", "--option", "q=a"],
            &["--state", "s", "--noul", "q=x", "--level", "q=a"],
            &[
                "--state", "s", "--choice", "c=x", "--option", "c=a", "--level", "c=low",
            ],
            &[
                "--state", "s", "--choice", "c=x", "--option", "c=a", "--option", "c=a",
            ],
            &["--state", "s", "--choice", "c=x", "--option", "no-equals"],
            &["--state", "s", "--choice", "c=x", "--option", "other=a"],
            &[
                "--state", "s", "--choice", "c=x", "--noul", "c=y", "--option", "c=a",
            ],
            &["--option", "c=a"],
            &["--request", ONE_QUESTION, "--option", "c=a"],
            &["--request", ONE_QUESTION, "--level", "c=a"],
        ] {
            let failure = build(&args(line), &mut env(Some(""))).unwrap_err();
            assert_eq!(failure.exit_code(), 2, "{line:?}");
        }
    }

    #[test]
    fn state_from_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("state.txt");
        std::fs::write(&file, "from the file").unwrap();
        let state = format!("@{}", file.display());
        let built = build(
            &args(&["--state", &state, "--noul", "q=x"]),
            &mut env(Some("")),
        )
        .unwrap();
        assert_eq!(built.state, json!("from the file"));
    }

    #[test]
    fn no_client_and_an_unknown_value_id_exit_2_before_any_connection() {
        let mut no_client = env(Some(""));
        let code = run(&mut no_client, &args(&["--state", "s", "--noul", "q=x"]));
        assert_eq!(code, 2);
        let mut unknown = env(Some(""));
        let code = run(
            &mut unknown,
            &args(&[
                "--client", "app", "--state", "s", "--noul", "q=x", "--value", "other",
            ]),
        );
        assert_eq!(code, 2);
    }

    #[test]
    fn no_token_is_not_paired_with_json_on_stdout() {
        let out = Buffer::default();
        let mut env = env(Some(""));
        env.out = Box::new(out.clone());
        let code = run(
            &mut env,
            &args(&["--client", "app", "--state", "s", "--noul", "q=x"]),
        );
        assert_eq!(code, 4);
        let value: Value = serde_json::from_str(out.text().trim()).unwrap();
        assert_eq!(value["error"]["code"], "not-paired");
    }

    #[test]
    fn a_failure_with_value_leaves_stdout_empty_and_names_the_code_on_stderr() {
        let (out, err) = (Buffer::default(), Buffer::default());
        let mut env = env(Some(""));
        env.out = Box::new(out.clone());
        env.err = Box::new(err.clone());
        let code = run(
            &mut env,
            &args(&[
                "--client", "app", "--state", "s", "--noul", "q=x", "--value", "q",
            ]),
        );
        assert_eq!(code, 4);
        assert_eq!(out.text(), "");
        assert_eq!(err.text().trim(), "not-paired");
    }

    fn paired(stdin: Option<&str>) -> Env {
        let env = env(stdin);
        env.store.set(&client_account("app"), "jevt_test").unwrap();
        env
    }

    const ONE_QUESTION: &str =
        r#"{"state":"s","questions":{"q":{"type":"noul","instructions":"x"}}}"#;

    #[test]
    fn a_stdin_over_the_line_cap_is_too_large_before_any_connection() {
        let big = "x".repeat(jev_proto::MAX_LINE + 1);
        let out = Buffer::default();
        let mut env = paired(Some(&big));
        env.out = Box::new(out.clone());
        // A connection to /nowhere would give daemon-unavailable, exit 8.
        assert_eq!(run(&mut env, &args(&["--client", "app"])), 9);
        let value: Value = serde_json::from_str(out.text().trim()).unwrap();
        assert_eq!(value["error"]["code"], "too-large");
    }

    #[test]
    fn a_state_file_over_the_line_cap_is_too_large() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("state.txt");
        std::fs::write(&file, "x".repeat(jev_proto::MAX_LINE + 1)).unwrap();
        let state = format!("@{}", file.display());
        let failure = build(
            &args(&["--state", &state, "--noul", "q=x"]),
            &mut env(Some("")),
        )
        .unwrap_err();
        assert_eq!(failure.error.code.known(), Some(ErrorCode::TooLarge));
        assert_eq!(failure.exit_code(), 9);
    }

    #[test]
    fn a_request_line_over_the_cap_is_too_large_before_any_connection() {
        let state = "x".repeat(jev_proto::MAX_LINE);
        let mut env = paired(Some(""));
        let line = ["--client", "app", "--state", &state, "--noul", "q=x"];
        assert_eq!(run(&mut env, &args(&line)), 9);
    }

    #[test]
    fn request_plus_piped_stdin_is_a_usage_error() {
        let failure = build(&args(&["--request", ONE_QUESTION]), &mut env(Some("{}"))).unwrap_err();
        assert_eq!(failure.exit_code(), 2);
    }

    #[test]
    fn whitespace_only_stdin_goes_with_flags() {
        let built = build(
            &args(&["--state", "s", "--noul", "q=x"]),
            &mut env(Some(" \r\n\t\n")),
        )
        .unwrap();
        assert_eq!(built.state, json!("s"));
    }

    #[test]
    fn an_unknown_value_id_with_a_stdin_request_exits_2() {
        let mut env = paired(Some(ONE_QUESTION));
        let code = run(&mut env, &args(&["--client", "app", "--value", "other"]));
        assert_eq!(code, 2);
    }

    #[test]
    fn a_byte_order_mark_before_stdin_json_is_skipped() {
        let plain = build(&args(&[]), &mut env(Some(ONE_QUESTION))).unwrap();
        let marked = format!("\u{feff}{ONE_QUESTION}");
        assert_eq!(build(&args(&[]), &mut env(Some(&marked))).unwrap(), plain);
    }

    #[test]
    fn question_ids_follow_jevds_rule() {
        let long = format!("{}=x", "a".repeat(65));
        for flag in ["bad id!=x", "=x", "é=x", &long] {
            let failure =
                build(&args(&["--state", "s", "--noul", flag]), &mut env(Some(""))).unwrap_err();
            assert_eq!(failure.exit_code(), 2, "{flag:?}");
        }
        let long_id = "a".repeat(64);
        for id in ["Q_1-b", long_id.as_str()] {
            let question = format!("{id}=x");
            let level = format!("{id}=low");
            build(
                &args(&["--state", "s", "--score", &question, "--level", &level]),
                &mut env(Some("")),
            )
            .unwrap();
        }
    }

    /// Runs `print_reply` and returns the exit code, stdout, and stderr.
    fn printed(line: &[&str], reply: jev_proto::Reply) -> (i32, String, String) {
        let (out, err) = (Buffer::default(), Buffer::default());
        let mut env = env(Some(""));
        env.out = Box::new(out.clone());
        env.err = Box::new(err.clone());
        let code = print_reply(&mut env, &args(line), reply);
        (code, out.text(), err.text())
    }

    #[test]
    fn a_success_prints_the_reply_as_one_json_line() {
        let data = json!({"answers": {"q": {"type": "noul", "noul": 0.75}}});
        let (code, out, err) = printed(&[], jev_proto::Reply::success(data));
        assert_eq!((code, err.as_str()), (0, ""));
        assert_eq!(out.lines().count(), 1);
        assert!(out.ends_with('\n'));
        let value: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            value,
            json!({"ok": true, "answers": {"q": {"type": "noul", "noul": 0.75}}})
        );
    }

    #[test]
    fn a_failure_prints_its_json_with_retry_after() {
        let error = jev_proto::WireError {
            retry_after: Some(12),
            ..jev_proto::WireError::new(ErrorCode::RateLimited, "slow down")
        };
        let (code, out, err) = printed(&[], jev_proto::Reply::failure(error));
        assert_eq!((code, err.as_str()), (5, ""));
        let value: Value = serde_json::from_str(out.trim()).unwrap();
        assert_eq!(value["ok"], false);
        assert_eq!(value["error"]["code"], "rate-limited");
        assert_eq!(value["error"]["retry_after"], 12);
    }

    #[test]
    fn value_prints_the_number_of_the_answers_type() {
        let reply = || {
            jev_proto::Reply::success(json!({"answers": {
                "q": {"type": "noul", "noul": 0.75},
                "s": {"type": "score", "score": 3.5, "confidence": 0.9},
            }}))
        };
        assert_eq!(
            printed(&["--value", "q"], reply()),
            (0, "0.75\n".to_string(), String::new())
        );
        assert_eq!(
            printed(&["--value", "s"], reply()),
            (0, "3.5\n".to_string(), String::new())
        );
    }

    #[test]
    fn value_with_a_wrong_answer_is_bad_answer() {
        for answer in [
            json!({"type": "noul", "score": 0.5}),
            json!({"type": "score", "noul": 0.5}),
            json!({"type": "noul", "noul": "0.5"}),
            json!({"noul": 0.5}),
            json!(null),
        ] {
            let reply = jev_proto::Reply::success(json!({"answers": {"q": answer}}));
            assert_eq!(
                printed(&["--value", "q"], reply),
                (7, String::new(), "bad-answer\n".to_string()),
                "{answer}"
            );
        }
    }

    #[test]
    fn unsafe_characters_in_a_reply_come_out_escaped() {
        let data = json!({"answers": {"q\u{202e}": {
            "type": "noul",
            "noul": 0.5,
            "note": "a\u{202e}b\u{1b}c\u{85}d\u{e0041}e\u{2028}f",
        }}});
        let (code, out, _) = printed(&[], jev_proto::Reply::success(data.clone()));
        assert_eq!(code, 0);
        assert_eq!(out.lines().count(), 1);
        assert!(out.ends_with('\n'));
        assert!(out.is_ascii(), "{out}");
        for escape in [r"\u202e", r"\u001b", r"\u0085", r"\udb40\udc41", r"\u2028"] {
            assert!(out.contains(escape), "{escape} in {out}");
        }
        let mut expected = data;
        expected["ok"] = json!(true);
        assert_eq!(serde_json::from_str::<Value>(&out).unwrap(), expected);
    }
}
