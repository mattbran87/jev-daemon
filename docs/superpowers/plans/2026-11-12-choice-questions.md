# `choice` questions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** jevd, `jev ask`, and the plugin types support TypeSafe `choice` questions, and `jev ask` flags can give `criteria` to choice and score questions.

**Architecture:** jevd's shape check accepts `choice` when it has 1 to 255 `criteria` keys, and its answer check makes the answer's `choice` one of those keys. `jev ask` gets 3 flags (`--choice`, `--option`, `--level`) that build the same JSON a caller could send, and `--value` prints the option name of a choice answer. The plugin only gains types; its code passes questions and answers through.

**Tech Stack:** Rust (workspace crates `jevd` and `jev`, `serde_json`, `clap`), TypeScript types and tests for the Claude Code plugin (`claude plugin test`).

**Spec:** `docs/superpowers/specs/2026-11-12-choice-questions-design.md`

**Branch:** `feat/choice-questions` (the spec commit is on it).

**Commit identity for every commit:**

```bash
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "<subject>" -m "Part of #8." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

**Line ends:** `core.autocrlf=true`. Keep each file's existing line ends. Check with `git ls-files --eol <file>` before and after, and check that `git diff --stat` shows no whole-file rewrite.

---

## File map

| File | Change |
| --- | --- |
| `crates/jevd/src/shape.rs` | `choice` in `QUESTION_TYPES`, `MAX_CHOICES`, a `criteria` rule for choice questions |
| `crates/jevd/src/answer.rs` | A `choice` arm in the answer check |
| `crates/jev/src/cli.rs` | Flags `--choice`, `--option`, `--level`; help text of `--score` and `--value` |
| `crates/jev/src/ask.rs` | Build `criteria` from the new flags; `--value` prints a choice name |
| `plugins/claude-code/types/index.d.ts` | `choice` in `JevQuestion` and `JevAnswer` |
| `plugins/claude-code/hooks/register.test.ts` | A choice question and answer pass through |
| `docs/superpowers/specs/2026-11-02-jev-daemon-design.md` | Section 4 steps 1 and 7, section 6 input and `--value` |
| `CHANGELOG.md` | **Unreleased** entries |

---

### Task 1: jevd accepts choice questions with criteria

**Files:**
- Modify: `crates/jevd/src/shape.rs`

- [ ] **Step 1: Write the failing test**

Add this test inside `mod tests` in `crates/jevd/src/shape.rs`, after `rules_hold_at_their_edges`:

```rust
    #[test]
    fn a_choice_needs_1_to_255_criteria() {
        let options = |count: usize| -> Value {
            let criteria: serde_json::Map<String, Value> =
                (0..count).map(|i| (format!("o{i}"), Value::Null)).collect();
            json!({"c": {"type": "choice", "criteria": criteria}})
        };
        assert!(valid_questions(&options(1)));
        assert!(valid_questions(&options(MAX_CHOICES)));
        assert!(!valid_questions(&options(0)));
        assert!(!valid_questions(&options(MAX_CHOICES + 1)));
        assert!(valid_questions(
            &json!({"c": {"type": "choice", "instructions": "Mood?", "criteria": {"angry": "Upset", "calm": null}}})
        ));
        assert!(!valid_questions(&json!({"c": {"type": "choice"}})));
        assert!(!valid_questions(
            &json!({"c": {"type": "choice", "criteria": ["angry", "calm"]}})
        ));
        // jevd leaves score criteria to Jev, as before.
        assert!(valid_questions(&json!({"s": {"type": "score"}})));
        assert!(check_ask(&json!("s"), &json!({"c": {"type": "choice"}})).is_err());
    }
```

- [ ] **Step 2: Run the test to check that it fails**

Run: `cargo test -p jevd --locked --lib shape::tests::a_choice_needs_1_to_255_criteria`
Expected: compile error, `cannot find value MAX_CHOICES`.

- [ ] **Step 3: Write the implementation**

In `crates/jevd/src/shape.rs`, replace the line

```rust
pub const QUESTION_TYPES: [&str; 2] = ["noul", "score"];
```

with

```rust
pub const QUESTION_TYPES: [&str; 3] = ["noul", "score", "choice"];
/// The most options one choice question can have (TypeSafe's limit).
pub const MAX_CHOICES: usize = 255;
```

Replace the whole `valid_questions` function, with its doc comment, with:

```rust
/// A choice question needs `criteria`: an object of 1 to 255 options by name.
/// The answer check needs those names.
fn valid_choice(question: &Value) -> bool {
    question
        .get("criteria")
        .and_then(Value::as_object)
        .is_some_and(|options| (1..=MAX_CHOICES).contains(&options.len()))
}

/// 1 to 20 ids, each mapped to an object whose `type` is `noul`, `score`, or
/// `choice`. A `choice` also needs valid `criteria`.
pub fn valid_questions(questions: &Value) -> bool {
    let Some(map) = questions.as_object() else {
        return false;
    };
    if map.is_empty() || map.len() > MAX_QUESTIONS {
        return false;
    }
    map.iter().all(|(id, question)| {
        let kind = question.get("type").and_then(Value::as_str);
        valid_question_id(id)
            && kind.is_some_and(|kind| QUESTION_TYPES.contains(&kind))
            && (kind != Some("choice") || valid_choice(question))
    })
}
```

In `check_ask`, replace the message

```rust
        return Err("questions must map 1 to 20 ids to objects with type noul or score");
```

with

```rust
        return Err("questions must map 1 to 20 ids to objects with type noul, score, or choice; a choice needs criteria with 1 to 255 options");
```

- [ ] **Step 4: Run the tests to check that they pass**

Run: `cargo test -p jevd --locked --lib shape::`
Expected: every `shape::tests` test passes.

- [ ] **Step 5: Commit**

```bash
git add crates/jevd/src/shape.rs
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "feat(jevd): accept choice questions with 1 to 255 criteria" -m "Part of #8." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: jevd checks choice answers

**Files:**
- Modify: `crates/jevd/src/answer.rs`

- [ ] **Step 1: Write the failing test**

Add this test inside `mod tests` in `crates/jevd/src/answer.rs`, after `extra_answers_and_fields_pass_through`:

```rust
    #[test]
    fn a_choice_answer_names_one_of_the_criteria() {
        let choice = json!({"m": {"type": "choice", "criteria": {"angry": null, "calm": "Polite"}}});
        let good = json!({"m": {"type": "choice", "choice": "calm", "confidence": 0.7,
            "probabilities": {"angry": 0.3, "calm": 0.7}}});
        assert!(valid_answers(&good, &choice));
        // The edges of confidence pass; probabilities are not checked.
        assert!(valid_answers(
            &json!({"m": {"type": "choice", "choice": "angry", "confidence": 0}}),
            &choice
        ));
        assert!(valid_answers(
            &json!({"m": {"type": "choice", "choice": "angry", "confidence": 1}}),
            &choice
        ));
        for bad in [
            json!({"type": "choice", "choice": "excited", "confidence": 0.7}),
            json!({"type": "choice", "choice": 1, "confidence": 0.7}),
            json!({"type": "choice", "confidence": 0.7}),
            json!({"type": "choice", "choice": "calm"}),
            json!({"type": "choice", "choice": "calm", "confidence": 1.5}),
            json!({"type": "choice", "choice": "calm", "confidence": "0.7"}),
        ] {
            assert!(!valid_answers(&json!({"m": bad}), &choice), "{bad}");
        }
        // A question with no criteria has no valid choice.
        assert!(!valid_answers(
            &json!({"m": {"type": "choice", "choice": "calm", "confidence": 0.7}}),
            &json!({"m": {"type": "choice"}})
        ));
    }
```

- [ ] **Step 2: Run the test to check that it fails**

Run: `cargo test -p jevd --locked --lib answer::tests::a_choice_answer_names_one_of_the_criteria`
Expected: FAIL. The first bad answer passes, because today the `_ => true` arm accepts every `choice` answer.

- [ ] **Step 3: Write the implementation**

In `valid_answers` in `crates/jevd/src/answer.rs`, add this arm before `_ => true,`:

```rust
            Some("choice") => {
                let options = question.get("criteria").and_then(Value::as_object);
                let named = answer
                    .get("choice")
                    .and_then(Value::as_str)
                    .is_some_and(|name| options.is_some_and(|options| options.contains_key(name)));
                named
                    && finite(answer.get("confidence")).is_some_and(|n| (0.0..=1.0).contains(&n))
            }
```

- [ ] **Step 4: Run the tests to check that they pass**

Run: `cargo test -p jevd --locked --lib answer::`
Expected: every `answer::tests` test passes, including `answer_type_and_shape_edges`, whose unknown type `other` still passes.

- [ ] **Step 5: Commit**

```bash
git add crates/jevd/src/answer.rs
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "feat(jevd): a choice answer must name one of the criteria" -m "Part of #8." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: `jev ask` flags for choice options and score levels

**Files:**
- Modify: `crates/jev/src/cli.rs:118-123`
- Modify: `crates/jev/src/ask.rs`

- [ ] **Step 1: Add the flags to `AskArgs`**

In `crates/jev/src/cli.rs`, replace

```rust
    /// A score question, <id>=<question>. Repeatable.
    #[arg(long)]
    pub score: Vec<String>,
```

with

```rust
    /// A score question, <id>=<question>. Repeatable. Give its levels with
    /// --level.
    #[arg(long)]
    pub score: Vec<String>,
    /// A choice question, <id>=<question>. Repeatable. Give its options with
    /// --option.
    #[arg(long)]
    pub choice: Vec<String>,
    /// One option of choice question <id>, <id>=<name>. Repeatable, 1 to 255
    /// per question.
    #[arg(long)]
    pub option: Vec<String>,
    /// One level of score question <id>, <id>=<text>, from the lowest to the
    /// highest. Repeatable, 1 to 10 per question.
    #[arg(long)]
    pub level: Vec<String>,
```

In the doc comment of the `Ask(AskArgs)` variant, replace `with --noul or --score` with `with --noul, --score, or --choice`.

- [ ] **Step 2: Write the failing tests**

In `mod tests` in `crates/jev/src/ask.rs`, add this helper after `fn args`:

```rust
    fn owned_args(line: &[String]) -> AskArgs {
        let refs: Vec<&str> = line.iter().map(String::as_str).collect();
        args(&refs)
    }
```

Replace the body of `flags_and_stdin_json_give_the_same_request` up to `let piped` with this, so the score question has levels:

```rust
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
```

In `question_ids_follow_jevds_rule`, replace

```rust
        let good = format!("{}=x", "a".repeat(64));
        for flag in ["Q_1-b=x", &good] {
            build(
                &args(&["--state", "s", "--score", flag]),
                &mut env(Some("")),
            )
            .unwrap();
        }
```

with

```rust
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
```

Add these tests after `bad_flags_are_usage_errors`:

```rust
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
            &["--state", "s", "--choice", "c=x", "--option", "c=a", "--level", "c=low"],
            &["--state", "s", "--choice", "c=x", "--option", "c=a", "--option", "c=a"],
            &["--state", "s", "--choice", "c=x", "--option", "no-equals"],
            &["--state", "s", "--choice", "c=x", "--option", "other=a"],
            &["--state", "s", "--choice", "c=x", "--noul", "c=y", "--option", "c=a"],
            &["--option", "c=a"],
            &["--request", ONE_QUESTION, "--option", "c=a"],
            &["--request", ONE_QUESTION, "--level", "c=a"],
        ] {
            let failure = build(&args(line), &mut env(Some(""))).unwrap_err();
            assert_eq!(failure.exit_code(), 2, "{line:?}");
        }
    }
```

- [ ] **Step 3: Run the tests to check that they fail**

Run: `cargo test -p jev --locked --lib ask::tests`
Expected: FAIL. `choice_and_level_flags_build_criteria` and `flags_and_stdin_json_give_the_same_request` differ, because `build` ignores the new flags. Some lines in `bad_choice_and_level_flags_are_usage_errors` pass `build`.

- [ ] **Step 4: Write the implementation**

In `crates/jev/src/ask.rs`, change the import

```rust
use jevd::shape::valid_question_id;
```

to

```rust
use jevd::shape::{valid_question_id, MAX_CHOICES};
```

After the imports, add:

```rust
/// The most levels one score question can have (TypeSafe's limit).
const MAX_LEVELS: usize = 10;
```

In `split`, change the message `"--{flag} takes <id>=<question>, not {text:?}"` to `"--{flag} takes <id>=<text>, not {text:?}"`.

At the start of `build`, replace

```rust
    let flags = args.state.is_some() || !args.noul.is_empty() || !args.score.is_empty();
    if flags && args.request.is_some() {
        return Err(Failure::usage(
            "--request cannot go with --state, --noul, or --score",
        ));
    }
```

with

```rust
    let flags = args.state.is_some()
        || [&args.noul, &args.score, &args.choice, &args.option, &args.level]
            .iter()
            .any(|list| !list.is_empty());
    if flags && args.request.is_some() {
        return Err(Failure::usage(
            "--request cannot go with --state or a question flag",
        ));
    }
```

In `build`, change the message `"give the request as JSON on stdin, with --request, or with --state and --noul or --score"` to `"give the request as JSON on stdin, with --request, or with --state and --noul, --score, or --choice"`.

Replace the whole `from_flags` function with:

```rust
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
```

- [ ] **Step 5: Run the tests to check that they pass**

Run: `cargo test -p jev --locked --lib ask::tests`
Expected: every `ask::tests` test passes.

Run: `cargo test -p jev --locked`
Expected: every `jev` test passes. If a test in `crates/jev/tests/` builds a `--score` question with no `--level`, add a `--level` to it, and report the change.

- [ ] **Step 6: Commit**

```bash
git add crates/jev/src/cli.rs crates/jev/src/ask.rs
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "feat(jev): --choice, --option, and --level for jev ask" -m "A --score question from flags now needs at least one --level, because the API needs criteria. Part of #8." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: `--value` prints the name of a choice

**Files:**
- Modify: `crates/jev/src/ask.rs` (`print_reply`, the module comment, its tests)
- Modify: `crates/jev/src/cli.rs` (help of `--value`)

- [ ] **Step 1: Write the failing tests**

In `mod tests` in `crates/jev/src/ask.rs`, add after `value_prints_the_number_of_the_answers_type`:

```rust
    #[test]
    fn value_prints_the_name_of_a_choice() {
        let reply = jev_proto::Reply::success(json!({"answers": {"mood": {
            "type": "choice",
            "choice": "calm",
            "confidence": 0.7,
            "probabilities": {"angry": 0.3, "calm": 0.7},
        }}}));
        assert_eq!(
            printed(&["--value", "mood"], reply),
            (0, "calm\n".to_string(), String::new())
        );
    }
```

In `value_with_a_wrong_answer_is_bad_answer`, add these 2 entries to the array:

```rust
            json!({"type": "choice", "choice": 1}),
            json!({"type": "choice"}),
```

- [ ] **Step 2: Run the tests to check that they fail**

Run: `cargo test -p jev --locked --lib ask::tests::value_`
Expected: FAIL. `value_prints_the_name_of_a_choice` gets exit 7 and `bad-answer`.

- [ ] **Step 3: Write the implementation**

In `print_reply`, replace

```rust
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
```

with

```rust
    // The answer's own type names the field that holds its value.
    let answer = data.get("answers").and_then(|answers| answers.get(id));
    let value = answer.and_then(|answer| match answer.get("type")?.as_str()? {
        kind @ ("noul" | "score") => answer.get(kind)?.as_f64().map(|number| number.to_string()),
        // jevd checks that the name is one of the request's own options.
        "choice" => answer.get("choice")?.as_str().map(clean),
        _ => None,
    });
    match value {
        Some(value) => {
            let _ = writeln!(env.out, "{value}");
            0
        }
```

Change the module comment at the top of `crates/jev/src/ask.rs` from

```rust
//! except with `--value`, which prints only that answer's number.
```

to

```rust
//! except with `--value`, which prints only that answer's value: the number
//! of a `noul` or `score` answer, or the option name of a `choice` answer.
```

In `crates/jev/src/cli.rs`, change the help of `value` from

```rust
    /// Print only this answer's number.
```

to

```rust
    /// Print only this answer's value: a number for noul and score, the
    /// option name for choice.
```

- [ ] **Step 4: Run the tests to check that they pass**

Run: `cargo test -p jev --locked --lib ask::tests`
Expected: every `ask::tests` test passes.

- [ ] **Step 5: Commit**

```bash
git add crates/jev/src/ask.rs crates/jev/src/cli.rs
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "feat(jev): --value prints the option name of a choice answer" -m "Part of #8." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Plugin types for choice

**Files:**
- Modify: `plugins/claude-code/types/index.d.ts:61-72`
- Modify: `plugins/claude-code/hooks/register.test.ts`

- [ ] **Step 1: Write the failing test**

In `plugins/claude-code/hooks/register.test.ts`, inside the `command.run` handler of `ACME`, add this branch before the final `else`:

```ts
      else if (args === 'choice') out = await $.jev.ask({ state: 's', questions: { mood: { type: 'choice', criteria: { angry: null, calm: null } } } })
```

Add this test after the test `'$.jev.ask sends the request on stdin as the client cc-<plugin>'`:

```ts
test('a choice question and its answer pass through', { plugins: [ACME] }, async ($, on) => {
  resetForTests()
  const answers = { mood: { type: 'choice', choice: 'calm', confidence: 0.7, probabilities: { angry: 0.3, calm: 0.7 } } }
  const seen = engine(on, { jev: args => (args[0] === 'ask' ? line({ ok: true, answers }) : line({ ok: true })) })
  expect(await acme($, 'choice')).toEqual({ ok: true, answers })
  const run = calls(seen.runs, 'ask')[0]
  expect(JSON.parse(run?.stdin ?? '')).toEqual({ state: 's', questions: { mood: { type: 'choice', criteria: { angry: null, calm: null } } } })
})
```

- [ ] **Step 2: Run the plugin tests**

Run: `claude plugin test plugins/claude-code`
Expected: the new test passes at run time, because the mod passes questions through. If the runner checks types, the `choice` branch fails to compile until Step 3. Report which of the two happened.

- [ ] **Step 3: Write the types**

In `plugins/claude-code/types/index.d.ts`, replace

```ts
/**
 * One question. `noul` asks for a probability from 0 to 1 that the answer is
 * yes. `score` asks for a score and a confidence, and takes `criteria`: one
 * description for each level, from the lowest to the highest.
 */
export type JevQuestion =
  | { type: 'noul'; instructions: string; [field: string]: unknown }
  | { type: 'score'; instructions: string; criteria: string[]; [field: string]: unknown }

export type JevAnswer =
  | { type: 'noul'; noul: number; [field: string]: unknown }
  | { type: 'score'; score: number; confidence: number; [field: string]: unknown }
```

with

```ts
/**
 * One question. `noul` asks for a probability from 0 to 1 that the answer is
 * yes. `score` asks for a score and a confidence, and takes `criteria`: one
 * description for each level, from the lowest to the highest. `choice` asks
 * the model to pick one option, and takes `criteria`: 1 to 255 options by
 * name, each with a description or `null`.
 */
export type JevQuestion =
  | { type: 'noul'; instructions: string; [field: string]: unknown }
  | { type: 'score'; instructions: string; criteria: string[]; [field: string]: unknown }
  | { type: 'choice'; instructions?: string; criteria: Record<string, unknown>; [field: string]: unknown }

/** A `choice` answer names one of the question's options in `choice`. */
export type JevAnswer =
  | { type: 'noul'; noul: number; [field: string]: unknown }
  | { type: 'score'; score: number; confidence: number; [field: string]: unknown }
  | { type: 'choice'; choice: string; confidence: number; probabilities: Record<string, number>; [field: string]: unknown }
```

- [ ] **Step 4: Run the plugin tests to check that they pass**

Run: `claude plugin test plugins/claude-code`
Expected: every test passes.

- [ ] **Step 5: Commit**

```bash
git add plugins/claude-code/types/index.d.ts plugins/claude-code/hooks/register.test.ts
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "feat(plugin): choice questions and answers in the types" -m "Part of #8." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: Docs

**Files:**
- Modify: `docs/superpowers/specs/2026-11-02-jev-daemon-design.md` (lines 202, 208, 256, 263 at the time of writing)
- Modify: `CHANGELOG.md`

Load the skill `simple-english:simple-english-docs` before this task. Keep each file's line ends.

- [ ] **Step 1: Section 4, step 1 (shape)**

In the main design spec, in the line that starts `1. **Shape.**`, replace `Type `noul` or `score`.` with:

```markdown
Type `noul`, `score`, or `choice`. A `choice` needs `criteria`: an object with 1 to 255 options by name.
```

- [ ] **Step 2: Section 4, step 7 (answer check)**

In the line that starts `7. **Answer check.**`, after `` `score` and `confidence` are finite numbers.``, add:

```markdown
A `choice` answer's `choice` is a key of the question's `criteria`, and its `confidence` is a finite number from 0 to 1.
```

- [ ] **Step 3: Section 6, input and `--value`**

In the line that starts `- **Input.**`, replace ``and repeatable `--noul <id>=<question>` and `--score <id>=<question>`.`` with:

```markdown
and repeatable `--noul <id>=<question>`, `--score <id>=<question>`, and `--choice <id>=<question>`. Each `--score` needs 1 to 10 `--level <id>=<text>`, from the lowest level to the highest. Each `--choice` needs 1 to 255 `--option <id>=<name>`; an option from a flag has no description. An `--option` or `--level` that names no question of its type, or a name given twice on one question, exits with code 2.
```

Replace the line that starts `- **`--value`.**` with:

```markdown
- **`--value`.** An ID that names no question in the request exits with code 2. `jev` takes the value from the field that the answer's `type` names: the number in `noul` or `score`, or the option name in `choice`. So `--value` prints a name, not a number, for a choice question. Any other answer is `bad-answer` (exit 7).
```

- [ ] **Step 4: CHANGELOG**

In `CHANGELOG.md`, add these lines at the end of the `## Unreleased` list:

```markdown
- jevd accepts `choice` questions. A choice question needs `criteria` with 1 to 255 options, and the answer's `choice` must be one of them. An older `jevd` refuses a choice question with `bad-request`; run `jev stop`, and the next request starts the new `jevd`.
- `jev ask` has 3 new flags: `--choice <id>=<question>`, `--option <id>=<name>`, and `--level <id>=<text>`. `--level` gives the levels of a `--score` question, from the lowest to the highest.
- `jev ask --value` prints the option name of a choice answer, not a number.
- `jev ask --score` with no `--level` now exits 2 and calls nothing. Before, it reached Jev, and Jev refused it because the API needs `criteria`.
- The plugin's types describe `choice` questions and answers.
```

- [ ] **Step 5: Style check**

Run the linter from the `simple-english-docs` skill on both files. Fix each hit on a line this task changed. Leave hits on other lines.

- [ ] **Step 6: Commit**

```bash
git add docs/superpowers/specs/2026-11-02-jev-daemon-design.md CHANGELOG.md
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "docs: choice questions and the new jev ask flags" -m "Closes #8." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: Final check

- [ ] **Step 1: Run the CI commands**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo check --workspace --locked
claude plugin test plugins/claude-code
```

Expected: no format change, no warnings, every test passes with 1 ignored (the real-keyring test), and every plugin test passes.

- [ ] **Step 2: Check the branch**

Run: `git log --oneline main..HEAD` and `git status --short`
Expected: 8 commits (the spec, the plan, then Tasks 1 to 6) and a clean tree.

Push, the pull request, and the merge wait for the person's approval.
