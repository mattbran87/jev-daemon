# jev-daemon plan 5: the Claude Code mod

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the Claude Code mod in `plugins/claude-code`, which gives other mods `$.jev.ask` and `$.jev.status` through the `jev` CLI, plus the two small `jev` changes the mod needs.

**Architecture:** The mod holds no key, no token, and no socket code. Each `$.jev` call runs `jev` with `$.process.run`: `jev ask --client cc-<plugin>` with the request on stdin, and `jev pair --name cc-<plugin> --no-prompt --json` when the client is not paired. The host names the calling plugin, so one mod cannot ask as another. Toasts use `$.ui.toast`, each one once per process.

**Tech Stack:** Rust 1.85 (the workspace), TypeScript hooks for Claude Code 2.1.300 (`claude plugin test`, `claude plugin validate`, `tsc` 5).

**Spec:** `docs/superpowers/specs/2026-11-02-jev-daemon-design.md`, section 7. **Plans 1 to 4** sit on `main`.

---

## Before you start

- Work in the worktree that the executing skill makes. Run every command from its root.
- Stage explicit paths. Never run `git add -A`.
- End every commit message with this line, as a separate `-m` argument:
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`
- Run each gate on its own, never in a pipe that hides its exit status: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`.
- Build `jevd` before `cargo test`: `cargo build -p jevd --locked`. The `jev` binary tests need it.
- No test sends a request to `api.typesafe.ai`. The mod's tests fake `jev` with a `process.run` hook.
- `claude plugin test` does not type-check. Task 5 runs `tsc`.
- The hooks loader refuses 4 patterns that `tsc` accepts, and then every test fails with one "did not load" error:
  - A function in another file that takes `$` must be a `function` declaration, not an arrow function in a `const`.
  - `$.env.get` takes a literal name.
  - A second hook on an event that already has one needs a matcher.
  - A `$.state` reference spells its `plugin` and `key` as literals.
- An inline test plugin (`Plugin` in a `*.test.ts`) runs in its own scope. It cannot read the test file's constants.
- Each patch below applies with `git apply <file>`. Save the patch text to a file in your scratchpad first, exactly as shown. If `git apply` refuses a patch, stop and report it: do not edit by hand.

## What the probes showed

These facts come from probes on this machine on 2026-11-03, under Claude Code 2.1.300, with a probe plugin under `claude -p`. They shaped the design.

| Fact | What it changes |
| --- | --- |
| A child of `$.process.run` with no `stdin` option reads end of file at once. With `stdin`, it reads the whole text, then end of file. | The request travels on stdin, so no command-line limit applies. The size limit is `jev`'s own 1 MiB cap. |
| `$.process.run` rejects when the binary does not exist ("failed to start: ENOENT") and when the timeout passes ("aborted: still running after N ms"). | Every run sits in a `try`. A missing `jev` is `jev-missing`; a run that cannot finish is `jev-failed`. |
| A noun hook that waits 25 s on `$.process.run` finishes and returns its value. | Time spent waiting on host calls does not count against the hook budget, so `jev.ask` can wait for a real `jev ask`. |
| `$.process.run` reads the child's output until every writer closes. | A `jevd` that kept the pipe would hold each call until its timeout. Plan 4's Windows fix and the Unix null handles prevent this. The smoke test in Task 5 checks it. |
| `jev pair` prompts on the terminal device when one opens. A child of Claude Code can open the terminal Claude Code runs in. | Task 1 adds `jev pair --no-prompt`, which never uses the terminal. |
| Another plugin on this machine also defines `$.jev`. Two plugins cannot both own one noun. | The smoke test turns that plugin off for its one run. `docs` must say that two plugins that both define `$.jev` cannot run together. |
| `/plugin-types` works only in an interactive session. The API types file on this machine comes from Claude Code 2.1.274. | `tsc` checks against that file. Its `TestOptions` has no `options` field, which 2.1.300 reads; the tests pass options through a helper. |

## Decisions the person made on 2026-11-03

- The request travels on stdin through `$.process.run`, not as a `--request` argument.
- The mod takes the plugin name from the host (`next.origin.plugin`). An input `plugin` field must match it, or the call is `bad-request`.

## Choices this plan makes

- The plugin's name is `jev-daemon`, at version 0.1.0, with one option, `jevPath`.
- `jevd` adds an optional `profile` field to `no-key`, `bad-key`, and `jev-unauthorized` errors, so the toast names the profile. Older readers ignore the field, so protocol 1 stays.
- `jevd`'s key hints say `jev key add --profile <p>`. The old text, `jev key add <p>`, names a command that does not exist.
- `jev`'s `Failure` boxes its `WireError`, so `clippy::result_large_err` stays quiet after the new field.
- The mod adds 3 codes of its own: `jev-missing`, `jev-outdated`, and `jev-failed`. Every other code comes from `jev` as it is, so a code from a newer `jev` reaches the caller unchanged.
- With no JSON on stdout, the mod maps `jev`'s exit code to a code, by the spec's exit-code table.
- `jev --version` runs once per process. A failure is not kept, so a `jev` installed later counts.
- `$.jev.status` never starts `jevd`. It runs `jev status`, and `jev clients` only when `jevd` runs. `paired` is `null` when `jevd` does not run.
- Timeouts: 150 s for `jev ask` (jev's 120 s reply timeout plus its 10 s start wait, plus room), 30 s for `pair`, `status`, and `clients`, 10 s for `--version`.
- Toasts: one per pairing code; one per profile for `no-key` and `bad-key` together; one per profile for `jev-unauthorized`; one per process for a missing or old `jev`.
- Code reviews in Tasks 1 to 4 changed some details after the first draft of this plan; the spec is the current record.

## Files

```
crates/jev/src/cli.rs, lib.rs, pairing.rs   jev pair --no-prompt (Task 1)
crates/jev/tests/cli.rs                     tests for Tasks 1 and 2
crates/jev-proto/src/error.rs               WireError.profile, for_profile (Task 2)
crates/jevd/src/ask.rs, keycache.rs         profile on key errors, the key-add hints (Task 2)
crates/jev/src/out.rs                       Failure boxes its WireError (Task 2)
.claude-plugin/marketplace.json             the marketplace file at the repo root
.gitignore                                  ignores plugins/claude-code/.claude-plugin/types/
plugins/claude-code/
  .claude-plugin/plugin.json                name, version 0.1.0, types, userConfig jevPath
  types/index.d.ts                          the public $.jev contract
  hooks/
    hooks.json                              { "modules": ["./register.ts"] }
    text.ts                                 helpers that need no $: names, versions, replies, toast text
    jev.ts                                  find jev, run it, toasts, and the ask and status flows
    register.ts                             the hooks: engine.create, jev.ask, jev.status
    text.test.ts, register.test.ts
```

---

### Task 1: `jev pair --no-prompt`

A `jev pair` that the mod runs inside Claude Code can open the terminal Claude Code runs in, and then it prompts there and waits. With `--no-prompt`, `jev pair` never uses the terminal: it prints the code and exits 3 (`pending`).

**Files:**
- Modify: `crates/jev/src/cli.rs`, `crates/jev/src/lib.rs`, `crates/jev/src/pairing.rs`
- Test: `crates/jev/tests/cli.rs`

- [ ] **Step 1: Add the failing test**

Save this patch as `t1-test.patch` and apply it with `git apply t1-test.patch`. It adds `pair_with_no_prompt_never_uses_the_terminal`: a scripted terminal is there and would answer `y`, and the test expects no prompt and no approval.

```diff
diff --git a/crates/jev/tests/cli.rs b/crates/jev/tests/cli.rs
index 629470d..7f3438c 100644
--- a/crates/jev/tests/cli.rs
+++ b/crates/jev/tests/cli.rs
@@ -431,6 +431,28 @@ async fn pair_without_a_terminal_prints_the_code_and_exits_3() {
     assert_eq!(allow.code, 9, "jev allow needs a terminal");
 }
 
+#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
+async fn pair_with_no_prompt_never_uses_the_terminal() {
+    let setup = running().await;
+    setup.add_key("default").await;
+    let pending = setup
+        .jev(
+            &["pair", "--name", "cc-acme", "--no-prompt", "--json"],
+            ScriptedTerminal::answering(&["y"]),
+            Some(""),
+        )
+        .await;
+    assert_eq!(pending.code, 3, "{}", pending.out);
+    assert_eq!(pending.json()["error"]["code"], "pending");
+    assert!(pending.json()["pair_code"].is_string());
+    assert!(
+        pending.shown.is_empty(),
+        "no prompt on a terminal that is there: {:?}",
+        pending.shown
+    );
+    assert_eq!(setup.token("cc-acme"), None, "nothing was approved");
+}
+
 #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
 async fn the_cli_flow_stores_the_token_and_ask_uses_it() {
     let setup = running().await;
```

- [ ] **Step 2: Run the test to see it fail**

Run: `cargo test -p jev --test cli --locked pair_with_no_prompt`
Expected: FAIL. clap does not know `--no-prompt`, so `jev` exits 2, and the assertion `left: 2, right: 3` fails.

- [ ] **Step 3: Add the flag**

Save this patch as `t1-code.patch` and apply it with `git apply t1-code.patch`.

```diff
diff --git a/crates/jev/src/cli.rs b/crates/jev/src/cli.rs
index 08191c2..363154a 100644
--- a/crates/jev/src/cli.rs
+++ b/crates/jev/src/cli.rs
@@ -32,6 +32,11 @@ pub enum Command {
         /// The profile to suggest at the approval.
         #[arg(long)]
         profile: Option<String>,
+        /// Never use the terminal: print the pairing code and exit 3, so a
+        /// person approves the client with `jev allow`. For a caller, such as
+        /// the Claude Code mod, that runs inside another program's terminal.
+        #[arg(long)]
+        no_prompt: bool,
     },
     /// Approve a waiting pairing request. Needs a terminal.
     Allow { code: Option<String> },
diff --git a/crates/jev/src/lib.rs b/crates/jev/src/lib.rs
index 40c7ae5..56f6736 100644
--- a/crates/jev/src/lib.rs
+++ b/crates/jev/src/lib.rs
@@ -113,7 +113,11 @@ pub fn run_cli(cli: &Cli, env: &mut Env) -> i32 {
             KeyCommand::Remove { profile, force } => keys::remove(env, json, profile, *force),
             KeyCommand::List => keys::list(env, json),
         },
-        Command::Pair { name, profile } => pairing::pair(env, json, name, profile.as_deref()),
+        Command::Pair {
+            name,
+            profile,
+            no_prompt,
+        } => pairing::pair(env, json, name, profile.as_deref(), !no_prompt),
         Command::Allow { code } => pairing::allow(env, json, code.as_deref()),
         Command::Deny { code } => pairing::deny(env, json, code),
         Command::Revoke { name } => pairing::revoke(env, json, name),
diff --git a/crates/jev/src/pairing.rs b/crates/jev/src/pairing.rs
index d17b54d..2737ce8 100644
--- a/crates/jev/src/pairing.rs
+++ b/crates/jev/src/pairing.rs
@@ -73,8 +73,9 @@ fn need_terminal(env: &Env, command: &str) -> Outcome {
 }
 
 /// `jev pair`. With a terminal, the person approves at once. Without one,
-/// it prints the code and exits 3 (`pending`); `jev allow` finishes it.
-pub fn pair(env: &mut Env, json: bool, name: &str, profile: Option<&str>) -> Outcome {
+/// or with `--no-prompt` (`prompt` false), it prints the code and exits 3
+/// (`pending`); `jev allow` finishes it.
+pub fn pair(env: &mut Env, json: bool, name: &str, profile: Option<&str>, prompt: bool) -> Outcome {
     // jevd's rule, checked here, so a bad name starts no daemon.
     if !valid_name(name) {
         return Err(Failure::usage(format!(
@@ -91,7 +92,7 @@ pub fn pair(env: &mut Env, json: bool, name: &str, profile: Option<&str>) -> Out
         },
     )?;
     let code = text(&data, "code")?;
-    if !env.terminal.available() {
+    if !prompt || !env.terminal.available() {
         return Err(Failure::new(
             ErrorCode::Pending,
             format!("approve this client in a terminal: jev allow {code}"),
```

- [ ] **Step 4: Run the test to see it pass**

Run: `cargo test -p jev --test cli --locked pair_`
Expected: PASS, 3 tests: `pair_with_a_bad_name_exits_2_before_any_connection`, `pair_with_no_prompt_never_uses_the_terminal`, and `pair_without_a_terminal_prints_the_code_and_exits_3`.

- [ ] **Step 5: Check that the test guards the rule**

In `crates/jev/src/pairing.rs`, change `if !prompt || !env.terminal.available() {` to `if !env.terminal.available() {`. Run the Step 2 command. Expected: FAIL in `pair_with_no_prompt_never_uses_the_terminal`. Restore the line, and touch the file so cargo builds it again.

- [ ] **Step 6: Run the gates and commit**

Run the 3 gates from "Before you start". Expected: all exit 0.

```bash
git add crates/jev/src/cli.rs crates/jev/src/lib.rs crates/jev/src/pairing.rs crates/jev/tests/cli.rs
git commit -m "feat(jev): jev pair --no-prompt never uses the terminal" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: name the profile on key errors

The mod's key toasts say which profile to fix. `jevd` knows the profile at the ask; this task sends it in an optional `profile` field. It also fixes `jevd`'s key hints, which name `jev key add <p>`, a command that does not exist.

**Files:**
- Modify: `crates/jev-proto/src/error.rs`, `crates/jevd/src/ask.rs`, `crates/jevd/src/keycache.rs`, `crates/jev/src/out.rs`
- Test: the same files' test modules, and `crates/jev/tests/cli.rs`

- [ ] **Step 1: Add the failing tests**

Save this patch as `t2-test.patch` and apply it with `git apply t2-test.patch`. It adds:
- `for_profile_names_the_profile_on_key_errors_only` in `jev-proto`.
- Two asserts in `jevd`'s ask tests, that `no-key` and `jev-unauthorized` carry `profile: "default"`.
- The new hint text in `a_missing_or_blank_key_is_no_key`.
- `a_missing_key_names_its_profile_in_the_ask_json` in `jev`'s CLI tests.

```diff
diff --git a/crates/jev-proto/src/error.rs b/crates/jev-proto/src/error.rs
index 185f24c..2f9b3e7 100644
--- a/crates/jev-proto/src/error.rs
+++ b/crates/jev-proto/src/error.rs
@@ -211,6 +211,23 @@ mod tests {
         }
     }
 
+    #[test]
+    fn for_profile_names_the_profile_on_key_errors_only() {
+        for code in [
+            ErrorCode::NoKey,
+            ErrorCode::BadKey,
+            ErrorCode::JevUnauthorized,
+        ] {
+            let error = WireError::new(code, "x").for_profile("work");
+            assert_eq!(error.profile.as_deref(), Some("work"), "{code:?}");
+            let line = serde_json::to_string(&error).unwrap();
+            assert!(line.contains(r#""profile":"work""#), "{line}");
+        }
+        let other = WireError::new(ErrorCode::RateLimited, "x").for_profile("work");
+        assert_eq!(other.profile, None);
+        assert!(!serde_json::to_string(&other).unwrap().contains("profile"));
+    }
+
     #[test]
     fn exit_codes_match_the_spec_table() {
         let table = [
diff --git a/crates/jev/tests/cli.rs b/crates/jev/tests/cli.rs
index 7f3438c..877e657 100644
--- a/crates/jev/tests/cli.rs
+++ b/crates/jev/tests/cli.rs
@@ -453,6 +453,36 @@ async fn pair_with_no_prompt_never_uses_the_terminal() {
     assert_eq!(setup.token("cc-acme"), None, "nothing was approved");
 }
 
+#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
+async fn a_missing_key_names_its_profile_in_the_ask_json() {
+    let setup = running().await;
+    setup.add_key("default").await;
+    let pending = setup.quiet(&["pair", "--name", "script", "--json"]).await;
+    let code = pending.json()["pair_code"].as_str().unwrap().to_string();
+    let allow = setup
+        .jev(
+            &["allow"],
+            ScriptedTerminal::answering(&[&code, "", "y"]),
+            Some(""),
+        )
+        .await;
+    assert_eq!(allow.code, 0, "{}", allow.err);
+    setup.store.delete(&profile_account("default")).unwrap();
+    let ask = setup.ask("script");
+    let answer = setup.quiet(&strs(&ask)).await;
+    assert_eq!(answer.code, 6, "{}", answer.out);
+    let error = &answer.json()["error"];
+    assert_eq!(error["code"], "no-key");
+    assert_eq!(error["profile"], "default");
+    assert!(
+        error["message"]
+            .as_str()
+            .unwrap()
+            .contains("jev key add --profile default"),
+        "the hint names a command that works: {error}"
+    );
+}
+
 #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
 async fn the_cli_flow_stores_the_token_and_ask_uses_it() {
     let setup = running().await;
diff --git a/crates/jevd/src/ask.rs b/crates/jevd/src/ask.rs
index b20b5ba..0cabfa5 100644
--- a/crates/jevd/src/ask.rs
+++ b/crates/jevd/src/ask.rs
@@ -224,6 +224,10 @@ mod tests {
         assert!(call(&f.daemon, Peer::Jev, one).await.ok);
         let reply = call(&f.daemon, Peer::Other, ask(&token)).await;
         assert_eq!(code_of(&reply), Some(ErrorCode::NoKey));
+        assert_eq!(
+            reply.error.as_ref().unwrap().profile.as_deref(),
+            Some("default")
+        );
         store.set(&profile_account("default"), KEY).unwrap();
         let reply = call(&f.daemon, Peer::Other, ask(&token)).await;
         assert!(reply.ok, "the failed ask used no slot: {reply:?}");
@@ -256,6 +260,10 @@ mod tests {
         let token = pair_socket(&f.daemon, "app", "default").await;
         let reply = call(&f.daemon, Peer::Other, ask(&token)).await;
         assert_eq!(code_of(&reply), Some(ErrorCode::JevUnauthorized));
+        assert_eq!(
+            reply.error.as_ref().unwrap().profile.as_deref(),
+            Some("default")
+        );
         assert!(!reply.to_line().contains(KEY));
     }
 
diff --git a/crates/jevd/src/keycache.rs b/crates/jevd/src/keycache.rs
index 5535d66..a29a435 100644
--- a/crates/jevd/src/keycache.rs
+++ b/crates/jevd/src/keycache.rs
@@ -357,7 +357,7 @@ mod tests {
             .unwrap_err();
         assert_eq!(error.code, WireCode::Known(ErrorCode::NoKey));
         assert!(
-            error.message.contains("jev key add work"),
+            error.message.contains("jev key add --profile work"),
             "{}",
             error.message
         );
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test --workspace --locked --no-run`
Expected: FAIL to compile, with error E0599: no method named `for_profile` found for struct `error::WireError`.

- [ ] **Step 3: Add the field and set it**

Save this patch as `t2-code.patch` and apply it with `git apply t2-code.patch`.

```diff
diff --git a/crates/jev-proto/src/error.rs b/crates/jev-proto/src/error.rs
index 2f9b3e7..0df9c25 100644
--- a/crates/jev-proto/src/error.rs
+++ b/crates/jev-proto/src/error.rs
@@ -183,6 +183,10 @@ pub struct WireError {
     /// The HTTP status for `jev-http-error` and `jev-unauthorized`.
     #[serde(default, skip_serializing_if = "Option::is_none")]
     pub status: Option<u16>,
+    /// The profile whose key failed, for `no-key`, `bad-key`, and
+    /// `jev-unauthorized`, so a client can name it to the person.
+    #[serde(default, skip_serializing_if = "Option::is_none")]
+    pub profile: Option<String>,
 }
 
 impl WireError {
@@ -193,7 +197,20 @@ impl WireError {
             message: message.into(),
             retry_after: None,
             status: None,
+            profile: None,
+        }
+    }
+
+    /// Names `profile` on an error about its key: `no-key`, `bad-key`, or
+    /// `jev-unauthorized`. Other errors stay as they are.
+    pub fn for_profile(mut self, profile: &str) -> Self {
+        if matches!(
+            self.code,
+            WireCode::Known(ErrorCode::NoKey | ErrorCode::BadKey | ErrorCode::JevUnauthorized)
+        ) {
+            self.profile = Some(profile.to_string());
         }
+        self
     }
 }
 
diff --git a/crates/jev/src/out.rs b/crates/jev/src/out.rs
index e78db15..1d129eb 100644
--- a/crates/jev/src/out.rs
+++ b/crates/jev/src/out.rs
@@ -11,7 +11,8 @@ use crate::env::Env;
 /// Why a command failed.
 #[derive(Debug)]
 pub struct Failure {
-    pub error: WireError,
+    /// Boxed, so a `Result<_, Failure>` stays small.
+    pub error: Box<WireError>,
     /// A bad command line: exit code 2, whatever the error code.
     pub usage: bool,
     /// Extra fields for the JSON output, such as a pairing code.
@@ -48,7 +49,7 @@ impl Failure {
 impl From<WireError> for Failure {
     fn from(error: WireError) -> Failure {
         Failure {
-            error,
+            error: Box::new(error),
             usage: false,
             data: Map::new(),
         }
diff --git a/crates/jevd/src/ask.rs b/crates/jevd/src/ask.rs
index 0cabfa5..beb57f8 100644
--- a/crates/jevd/src/ask.rs
+++ b/crates/jevd/src/ask.rs
@@ -29,7 +29,7 @@ impl Daemon {
         // The key check comes before the rate limit, so a bad key uses no slot.
         let key = match self.key(&client.profile).await {
             Ok(key) => key,
-            Err(error) => return Reply::failure(error),
+            Err(error) => return Reply::failure(error.for_profile(&client.profile)),
         };
         let strict = {
             let mut state = self.lock();
@@ -53,7 +53,7 @@ impl Daemon {
         };
         match self.jev.ask(state, questions, &key, strict).await {
             Ok(answers) => Reply::success(json!({ "answers": answers })),
-            Err(failure) => Reply::failure(failure.to_wire()),
+            Err(failure) => Reply::failure(failure.to_wire().for_profile(&client.profile)),
         }
     }
 
diff --git a/crates/jevd/src/keycache.rs b/crates/jevd/src/keycache.rs
index a29a435..87ab5bc 100644
--- a/crates/jevd/src/keycache.rs
+++ b/crates/jevd/src/keycache.rs
@@ -77,7 +77,7 @@ pub fn read_key(store: &dyn KeyStore, profile: &str) -> Result<Zeroizing<String>
     let no_key = || {
         WireError::new(
             ErrorCode::NoKey,
-            format!("profile {profile} has no key; run jev key add {profile}"),
+            format!("profile {profile} has no key; run jev key add --profile {profile}"),
         )
     };
     let raw = store
@@ -93,7 +93,7 @@ pub fn read_key(store: &dyn KeyStore, profile: &str) -> Result<Zeroizing<String>
         ErrorCode::NoKey => no_key(),
         code => WireError::new(
             code,
-            format!("the key of profile {profile} is not a valid key; run jev key add {profile}"),
+            format!("the key of profile {profile} is not a valid key; run jev key add --profile {profile}"),
         ),
     })?;
     Ok(Zeroizing::new(key.to_string()))
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test --locked -p jev-proto for_profile`, then `cargo test --locked -p jevd --lib ask::`, then `cargo test --locked -p jevd --lib keycache::`, then `cargo test --locked -p jev --test cli a_missing_key`.
Expected: each PASS.

- [ ] **Step 5: Check that the tests guard the rule**

In `crates/jevd/src/ask.rs`, change `Err(error) => return Reply::failure(error.for_profile(&client.profile)),` to `Err(error) => return Reply::failure(error),`. Run `cargo test --locked -p jevd --lib ask::`. Expected: FAIL in `a_missing_key_uses_no_rate_limit_slot`. Restore the line and touch the file.

- [ ] **Step 6: Run the gates and commit**

Run the 3 gates. Expected: all exit 0. Before the box in `out.rs`, `clippy` fails with 34 `result_large_err` errors; with it, `clippy` is clean.

```bash
git add crates/jev-proto/src/error.rs crates/jevd/src/ask.rs crates/jevd/src/keycache.rs crates/jev/src/out.rs crates/jev/tests/cli.rs
git commit -m "feat: name the profile on key errors, and fix the key-add hints" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: the plugin's manifest, contract, and helpers

**Files:**
- Create: `.claude-plugin/marketplace.json`, `plugins/claude-code/.claude-plugin/plugin.json`, `plugins/claude-code/hooks/hooks.json`, `plugins/claude-code/types/index.d.ts`, `plugins/claude-code/hooks/text.ts`, `plugins/claude-code/hooks/register.ts` (a first version with no hooks)
- Modify: `.gitignore`
- Test: `plugins/claude-code/hooks/text.test.ts`

`claude plugin test` needs a hooks module to run any test, so this task starts `register.ts` with no hooks. Task 4 replaces it.

- [ ] **Step 1: Write the manifest, the marketplace file, and the contract**

`plugins/claude-code/.claude-plugin/plugin.json`:

```json
{
  "name": "jev-daemon",
  "version": "0.1.0",
  "author": { "name": "Matthew Brandenburg", "email": "mattbran87@users.noreply.github.com" },
  "license": "Apache-2.0",
  "description": "Gives other Claude Code plugins $.jev: they ask TypeSafe's Jev model through the jev CLI, which holds your keys. Each plugin pairs once, and you approve it with jev allow in a terminal.",
  "keywords": ["jev", "typesafe"],
  "types": "./types/index.d.ts",
  "userConfig": {
    "jevPath": {
      "type": "string",
      "title": "Path to jev",
      "description": "The full path of the jev binary. Leave it empty to find jev on the PATH."
    }
  }
}
```

`.claude-plugin/marketplace.json`:

```json
{
  "name": "jev-daemon",
  "owner": { "name": "Matthew Brandenburg", "email": "mattbran87@users.noreply.github.com" },
  "metadata": {
    "description": "jev for Claude Code: lets plugins ask TypeSafe's Jev model through the jev CLI, which holds your keys."
  },
  "plugins": [
    {
      "name": "jev-daemon",
      "source": "./plugins/claude-code",
      "description": "Gives other Claude Code plugins $.jev: they ask TypeSafe's Jev model through the jev CLI, which holds your keys. Each plugin pairs once, and you approve it with jev allow in a terminal.",
      "license": "Apache-2.0"
    }
  ]
}
```

`plugins/claude-code/hooks/hooks.json`:

```json
{ "modules": ["./register.ts"] }
```

`plugins/claude-code/types/index.d.ts` is the public contract that dependent mods compile against:

```ts
// The public contract of the jev-daemon plugin: the `$.jev` noun that other
// mods call. A plugin that lists jev-daemon under `dependencies` gets these
// types in its own .claude-plugin/types/. A breaking change here needs a new
// major version.

/**
 * Why a call returned no answer. Fall back to what your plugin does without
 * Jev. The codes from `jev` follow its exit-code table; a code this list does
 * not name comes from a newer `jev`, so treat it as an unknown failure.
 */
export type JevErrorCode =
  | 'pending'
  | 'not-paired'
  | 'revoked'
  | 'rate-limited'
  | 'no-key'
  | 'bad-key'
  | 'jev-unauthorized'
  | 'jev-http-error'
  | 'jev-timeout'
  | 'jev-unreachable'
  | 'bad-answer'
  | 'daemon-unavailable'
  | 'protocol-unsupported'
  | 'forbidden'
  | 'bad-request'
  | 'too-large'
  | 'internal'
  /** No `jev` on the PATH or at the `jevPath` option. */
  | 'jev-missing'
  /** The `jev` found is older than this plugin needs. */
  | 'jev-outdated'
  /** `jev` did not finish in time, or could not run. */
  | 'jev-failed'
  | (string & {})

export type JevFailure = {
  ok: false
  error: {
    code: JevErrorCode
    message: string
    /** Seconds until a `rate-limited` client can try again. */
    retryAfter?: number
    /** The profile whose key failed, for `no-key`, `bad-key`, and `jev-unauthorized`. */
    profile?: string
    /** For `pending`: the code that the person types at `jev allow`. */
    pairCode?: string
  }
}

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

export type JevAskInput = {
  /** What Jev reads. jevd redacts secrets with known shapes before the send. */
  state: unknown
  /** 1 to 20 questions by id: letters, digits, `_` and `-`. */
  questions: Record<string, JevQuestion>
  /** Your plugin's name. The host names the caller; a different name here is `bad-request`. */
  plugin?: string
}

export type JevAskResult = { ok: true; answers: Record<string, JevAnswer> } | JevFailure

export type JevStatusInput = { plugin?: string }

export type JevStatus =
  | {
      ok: true
      /** Whether this plugin found a `jev` binary. */
      installed: boolean
      /** The version of the `jev` found, or null. */
      version: string | null
      /** Whether that `jev` is new enough for this plugin. */
      compatible: boolean
      /** Whether `jevd` runs. `$.jev.status` never starts it. */
      running: boolean
      /** Whether jevd lists your plugin's client `cc-<plugin>`; null when jevd does not run. */
      paired: boolean | null
    }
  | JevFailure

/**
 * jev for Claude Code plugins. The host names your plugin from where the call
 * came from, so your plugin cannot speak for another. Your plugin pairs once
 * as the client `cc-<plugin>`; the person approves it with `jev allow`. Each
 * method takes one input object. No method throws: every failure is a
 * `JevFailure`.
 *
 * `$.jev` is missing when jev-daemon is off or not installed, and a call then
 * throws. Put each call in a `try`. Write each call as `$.jev.ask(...)`: the
 * hooks loader refuses `$.jev` in a variable and `$.jev?.ask`.
 */
export type Jev = {
  ask: (input: JevAskInput) => Promise<JevAskResult>
  status: (input?: JevStatusInput) => Promise<JevStatus>
}

declare module 'claude-code' {
  interface EngineInterface {
    jev: Jev
  }
}
```

`plugins/claude-code/hooks/register.ts`, the first version:

```ts
import type { Register } from 'claude-code'

// Task 4 adds the hooks.
export const register: Register = () => undefined
```

Add these 2 lines to the end of `.gitignore`:

```
# Claude Code writes this folder when it loads the mod.
/plugins/claude-code/.claude-plugin/types/
```

- [ ] **Step 2: Write the failing tests**

`plugins/claude-code/hooks/text.test.ts`:

```ts
import { expect, test } from 'claude-code/testing'

import { MAX_REQUEST_BYTES, callerName, clientName, olderThan, parseVersion, readReply, requestBytes } from './text'

test('the caller is the host name, and an input plugin must match it', () => {
  expect(callerName('acme-review', {})).toEqual({ ok: true, plugin: 'acme-review' })
  expect(callerName('acme-review', { plugin: 'acme-review' })).toEqual({ ok: true, plugin: 'acme-review' })
  expect(callerName('acme-review', { plugin: 'other' }).ok).toBe(false)
  for (const bad of ['engine', 'client', 'Acme', '-acme', 'a'.repeat(62), 'acme_review', '']) {
    expect(callerName(bad, {}).ok).toBe(false)
  }
  expect(callerName('a'.repeat(61), undefined).ok).toBe(true)
  expect(clientName('a'.repeat(61)).length).toBe(64)
})

test('the version comes from jev --version, and older versions are refused', () => {
  expect(parseVersion('jev 0.1.0\n')).toEqual([0, 1, 0])
  expect(parseVersion('jev 12.3.40')).toEqual([12, 3, 40])
  expect(parseVersion('jevd 0.1.0')).toBe(null)
  expect(parseVersion('jev 0.1')).toBe(null)
  expect(parseVersion('')).toBe(null)
  expect(olderThan([0, 0, 9], [0, 1, 0])).toBe(true)
  expect(olderThan([0, 1, 0], [0, 1, 0])).toBe(false)
  expect(olderThan([1, 0, 0], [0, 9, 9])).toBe(false)
  expect(olderThan([0, 10, 0], [0, 9, 0])).toBe(false)
})

test('the size counts UTF-8 bytes', () => {
  expect(requestBytes('ab')).toBe(2)
  expect(requestBytes('é')).toBe(2)
  expect(requestBytes('\u{1F600}')).toBe(4)
  expect(MAX_REQUEST_BYTES).toBe(1048576)
})

test('a success reply passes its data', () => {
  expect(readReply('{"ok":true,"answers":{"q":{"type":"noul","noul":0.5}}}\n', 0)).toEqual({
    ok: true,
    data: { ok: true, answers: { q: { type: 'noul', noul: 0.5 } } },
  })
})

test('a failure keeps its code, even one this mod does not know', () => {
  expect(readReply('{"ok":false,"error":{"code":"future-code","message":"new"}}', 1)).toEqual({
    ok: false,
    error: { code: 'future-code', message: 'new' },
  })
})

test('a failure passes on retry_after, a valid profile, and a valid pair code only', () => {
  const line = JSON.stringify({
    ok: false,
    error: { code: 'pending', message: 'm', retry_after: 7, profile: 'work' },
    pair_code: 'ABCD2345',
  })
  expect(readReply(line, 3)).toEqual({
    ok: false,
    error: { code: 'pending', message: 'm', retryAfter: 7, profile: 'work', pairCode: 'ABCD2345' },
  })
  const bad = JSON.stringify({ ok: false, error: { code: 'pending', message: 'm', profile: 'Bad Name' }, pair_code: 'abcd0000' })
  expect(readReply(bad, 3)).toEqual({ ok: false, error: { code: 'pending', message: 'm' } })
  const long = JSON.stringify({ ok: false, error: { code: 'internal', message: 'x'.repeat(1000) } })
  const parsed = readReply(long, 1)
  expect(parsed.ok === false && parsed.error.message.length).toBe(300)
})

test('with no JSON, each exit code maps to its code', () => {
  const table: Record<number, string> = {
    0: 'jev-failed',
    1: 'internal',
    2: 'bad-request',
    3: 'pending',
    4: 'not-paired',
    5: 'rate-limited',
    6: 'no-key',
    7: 'jev-http-error',
    8: 'daemon-unavailable',
    9: 'forbidden',
    42: 'jev-failed',
  }
  for (const [exit, code] of Object.entries(table)) {
    const parsed = readReply('not json', Number(exit))
    expect(parsed.ok === false && parsed.error.code).toBe(code)
  }
  const shape = readReply('{"ok":"yes"}', 0)
  expect(shape.ok === false && shape.error.code).toBe('jev-failed')
})
```

- [ ] **Step 3: Run the tests to see them fail**

Run: `claude plugin test plugins/claude-code`
Expected: FAIL. The test file cannot import `./text`.

- [ ] **Step 4: Write the helpers**

`plugins/claude-code/hooks/text.ts`:

```ts
// Helpers that need no `$`: names, versions, sizes, `jev` replies, and the
// text of each toast.

import type { JevFailure } from '../types'

/** A plugin name that the mod turns into the client name `cc-<plugin>` (spec section 7). */
export const PLUGIN_NAME = /^[a-z0-9][a-z0-9-]{0,60}$/
/** Origins that name the host, not a plugin. */
const RESERVED = new Set(['engine', 'client'])
/** The oldest `jev` that has `jev pair --no-prompt`. */
export const MIN_JEV: Version = [0, 1, 0]
/** `jev`'s own cap on a request line (`jev_proto::MAX_LINE`). */
export const MAX_REQUEST_BYTES = 1024 * 1024
/** jevd's pairing codes: 8 characters, no look-alikes. */
const PAIR_CODE = /^[23456789ABCDEFGHJKMNPQRSTUVWXYZ]{8}$/
/** jevd's profile and client name rule (`jev_proto::valid_name`). */
const PROFILE_NAME = /^[a-z0-9][a-z0-9-]{0,63}$/
/** The longest message the mod passes on from `jev`. */
const MAX_MESSAGE = 300

export type Version = readonly [number, number, number]

/** What one `jev` run gave: its JSON result, or one failure. */
export type Parsed = { ok: true; data: Record<string, unknown> } | JevFailure

export function failure(code: string, message: string): JevFailure {
  return { ok: false, error: { code, message } }
}

export function clientName(plugin: string): string {
  return `cc-${plugin}`
}

function isObject(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

/**
 * The calling plugin, as the host names it. An input `plugin` field must name
 * the same plugin, so one mod cannot ask as another.
 */
export function callerName(origin: string, input: unknown): { ok: true; plugin: string } | JevFailure {
  if (!PLUGIN_NAME.test(origin) || RESERVED.has(origin)) {
    return failure('bad-request', `${origin.slice(0, 64)} is not a plugin name that jev accepts`)
  }
  const given = isObject(input) ? input.plugin : undefined
  if (given !== undefined && given !== origin) {
    return failure('bad-request', `the plugin field must name the calling plugin, ${origin}`)
  }
  return { ok: true, plugin: origin }
}

/** The version in `jev --version` output, such as `jev 0.1.0`. */
export function parseVersion(stdout: string): Version | null {
  const match = /^jev (\d+)\.(\d+)\.(\d+)(?:\s|$)/.exec(stdout.trim())
  return match === null ? null : [Number(match[1]), Number(match[2]), Number(match[3])]
}

export function olderThan(found: Version, need: Version): boolean {
  for (let i = 0; i < 3; i++) {
    if (found[i] !== need[i]) return (found[i] ?? 0) < (need[i] ?? 0)
  }
  return false
}

export function versionText(version: Version): string {
  return version.join('.')
}

export function requestBytes(text: string): number {
  return new TextEncoder().encode(text).length
}

/** The error code for each `jev` exit code, for a run that printed no JSON. */
const EXIT_CODES: Record<number, string> = {
  1: 'internal',
  2: 'bad-request',
  3: 'pending',
  4: 'not-paired',
  5: 'rate-limited',
  6: 'no-key',
  7: 'jev-http-error',
  8: 'daemon-unavailable',
  9: 'forbidden',
}

/**
 * One `jev --json` result from its stdout. The JSON's code wins over the exit
 * code, so a code from a newer `jev` reaches the caller as it is. Only the
 * fields the mod knows pass on, each checked.
 */
export function readReply(stdout: string, exitCode: number): Parsed {
  let value: unknown
  try {
    value = JSON.parse(stdout.trim())
  } catch {
    value = undefined
  }
  if (isObject(value) && value.ok === true) return { ok: true, data: value }
  if (isObject(value) && value.ok === false && isObject(value.error) && typeof value.error.code === 'string') {
    const error = value.error
    const out: JevFailure = {
      ok: false,
      error: {
        code: error.code as string,
        message: typeof error.message === 'string' ? error.message.slice(0, MAX_MESSAGE) : '',
      },
    }
    if (typeof error.retry_after === 'number') out.error.retryAfter = error.retry_after
    if (typeof error.profile === 'string' && PROFILE_NAME.test(error.profile)) out.error.profile = error.profile
    if (typeof value.pair_code === 'string' && PAIR_CODE.test(value.pair_code)) out.error.pairCode = value.pair_code
    return out
  }
  const code = exitCode === 0 ? 'jev-failed' : (EXIT_CODES[exitCode] ?? 'jev-failed')
  return failure(code, `jev printed no result (exit ${exitCode})`)
}

export function pendingToast(plugin: string, code: string): string {
  return `jev: the plugin ${plugin} asks to use Jev. Run jev allow ${code} in a terminal to approve it.`
}

export function keyToast(profile: string | undefined): string {
  const name = profile ?? 'default'
  return `jev: profile ${name} has no usable key. Run jev key add --profile ${name}.`
}

export function unauthorizedToast(profile: string | undefined): string {
  const name = profile ?? 'default'
  return `jev: Jev refused the key for profile ${name}. Run jev key add --profile ${name}.`
}

export function missingToast(): string {
  return 'jev: plugins that use Jev need the jev CLI. Install jev and jevd, or set the jevPath option of the jev-daemon plugin.'
}

export function outdatedToast(found: string, need: Version): string {
  return `jev: plugins that use Jev need jev ${versionText(need)} or later, and this one is ${found}. Update jev.`
}
```

- [ ] **Step 5: Run the tests to see them pass**

Run: `claude plugin test plugins/claude-code`
Expected: `7 pass`, `0 fail`.

Run: `claude plugin validate plugins/claude-code`
Expected: `Validation passed`.

- [ ] **Step 6: Check that a test guards the name rule**

In `text.ts`, change `if (given !== undefined && given !== origin) {` to `if (false) {`. Run `claude plugin test plugins/claude-code`. Expected: FAIL in `the caller is the host name, and an input plugin must match it`. Restore the line.

- [ ] **Step 7: Commit**

```bash
git add .claude-plugin/marketplace.json .gitignore plugins/claude-code/.claude-plugin/plugin.json plugins/claude-code/hooks/hooks.json plugins/claude-code/types/index.d.ts plugins/claude-code/hooks/register.ts plugins/claude-code/hooks/text.ts plugins/claude-code/hooks/text.test.ts
git commit -m "feat(mod): the jev-daemon plugin's manifest, contract, and helpers" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: `$.jev.ask` and `$.jev.status`

**Files:**
- Create: `plugins/claude-code/hooks/jev.ts`
- Modify: `plugins/claude-code/hooks/register.ts` (replace it whole)
- Test: `plugins/claude-code/hooks/register.test.ts`

The tests play `jev` with a hook on `process.run` beneath the plugin, and an inline plugin `acme-review` calls `$.jev` as a dependent mod does. For a missing `jev`, the `jevPath` option names a path that does not exist. The fake hands that run to the host, and the host fails to start it, as for a real missing binary. The fake never hands any other run to the host, because the host would then run a real `jev`.

- [ ] **Step 1: Write the failing tests**

`plugins/claude-code/hooks/register.test.ts`:

```ts
import { expect, test, type Plugin } from 'claude-code/testing'
import type { On } from 'claude-code'

import { resetForTests } from './jev'

const ok = <T>(value: T) => ({ value }) as never
/** A `jevPath` that does not exist, so the host itself fails to start it. */
const MISSING = 'C:/no/such/folder/jev-missing.exe'
const ANSWER = { q1: { type: 'noul', noul: 0.4 } }
const QUESTIONS = { q1: { type: 'noul', instructions: 'Is a.py a test?' } }

type Run = { argv: string[]; stdin?: string }
type Result = { exitCode: number; stdout: string }
/** `jev` answers each run from its arguments after the binary. */
type World = {
  jev?: (args: string[], stdin: string | undefined) => Result
  version?: string
  /** A command that the host gets with a binary that does not exist, so the run cannot start. */
  unstartable?: string
}

const line = (value: unknown, exitCode = 0): Result => ({ exitCode, stdout: `${JSON.stringify(value)}\n` })

// The test's own hooks sit beneath the plugin and play the engine and `jev`.
const engine = (on: On, world: World = {}) => {
  const runs: Run[] = []
  const toasts: string[] = []
  on('ui.toast', (_$, e) => {
    toasts.push(String((e as unknown as { text?: unknown }).text ?? e))
    return ok(undefined)
  })
  on('process.run', (_$, e, next) => {
    const { argv, init } = e as { argv: string[]; init?: { stdin?: string } }
    // The host starts nothing at MISSING, as for a jev that is not installed.
    if (argv[0] === MISSING) return next(e)
    runs.push({ argv, stdin: init?.stdin })
    const args = argv.slice(1)
    if (args[0] === world.unstartable) return next({ ...(e as object), argv: [MISSING, ...args] } as never)
    const done = (r: Result) => ok({ exitCode: r.exitCode, stdout: r.stdout, stderr: '', isStdoutTruncated: false, isStderrTruncated: false })
    if (args[0] === '--version') return done({ exitCode: 0, stdout: `jev ${world.version ?? '0.1.0'}\n` })
    return done(world.jev?.(args, init?.stdin) ?? line({ ok: true }))
  })
  return { runs, toasts }
}

/** The runs after `jev --version`, by their command. */
const calls = (runs: Run[], verb: string) => runs.filter(run => run.argv[1] === verb)

/** A dependent plugin, inline, that reaches $.jev as a plugin built on jev-daemon does. */
const ACME: Plugin = {
  name: 'acme-review',
  register(on) {
    on('command.run', { command: 'acme' }, async ($, e) => {
      // An inline plugin runs in its own scope, so it holds its own copy.
      const QUESTIONS = { q1: { type: 'noul', instructions: 'Is a.py a test?' } }
      const args = (e as unknown as { args?: string }).args ?? ''
      let out: unknown
      if (args === 'status') out = await $.jev.status()
      else if (args === 'as-other') out = await $.jev.ask({ plugin: 'other', state: 's', questions: QUESTIONS as never })
      else if (args === 'big') out = await $.jev.ask({ state: 'x'.repeat(1024 * 1024), questions: QUESTIONS as never })
      else out = await $.jev.ask({ state: { files: ['a.py'] }, questions: QUESTIONS as never })
      return { text: JSON.stringify(out) }
    })
  },
}

/**
 * Test options with the plugin's `jevPath` set. Claude Code 2.1.300 reads
 * `options`; older API types do not list it, and a value that is not an
 * object literal passes their check.
 */
const withPath = (jevPath: string) => {
  const options = { plugins: [ACME], options: { jevPath } }
  return options
}

type Api = { command: { run: (input: unknown) => Promise<unknown> } }
const acme = async ($: unknown, args = ''): Promise<Record<string, unknown>> => {
  const out = (await ($ as Api).command.run({ command: 'acme', args, origin: { kind: 'composer' } })) as { text?: unknown }
  return JSON.parse(String(out.text)) as Record<string, unknown>
}
const codeOf = (result: Record<string, unknown>) => (result.error as { code?: unknown } | undefined)?.code

test('$.jev.ask sends the request on stdin as the client cc-<plugin>', { plugins: [ACME] }, async ($, on) => {
  resetForTests()
  const seen = engine(on, { jev: args => (args[0] === 'ask' ? line({ ok: true, answers: ANSWER }) : line({ ok: true })) })
  expect(await acme($)).toEqual({ ok: true, answers: ANSWER })
  const run = calls(seen.runs, 'ask')[0]
  expect(run?.argv).toEqual(['jev', 'ask', '--client', 'cc-acme-review'])
  expect(JSON.parse(run?.stdin ?? '')).toEqual({ state: { files: ['a.py'] }, questions: QUESTIONS })
  expect(seen.toasts).toEqual([])
})

test('the jevPath option names the binary', withPath(' C:/tools/jev.exe '), async ($, on) => {
  resetForTests()
  const seen = engine(on, { jev: () => line({ ok: true, answers: ANSWER }) })
  await acme($)
  expect(seen.runs.map(run => run.argv[0])).toEqual(['C:/tools/jev.exe', 'C:/tools/jev.exe'])
})

test('jev --version runs once per process', { plugins: [ACME] }, async ($, on) => {
  resetForTests()
  const seen = engine(on, { jev: () => line({ ok: true, answers: ANSWER }) })
  await acme($)
  await acme($)
  expect(calls(seen.runs, '--version').length).toBe(1)
  expect(calls(seen.runs, 'ask').length).toBe(2)
})

test('a plugin field that names another plugin is bad-request, and runs nothing', { plugins: [ACME] }, async ($, on) => {
  resetForTests()
  const seen = engine(on)
  expect(codeOf(await acme($, 'as-other'))).toBe('bad-request')
  expect(seen.runs).toEqual([])
})

test('a request over 1 MiB is too-large, and runs nothing', { plugins: [ACME] }, async ($, on) => {
  resetForTests()
  const seen = engine(on)
  expect(codeOf(await acme($, 'big'))).toBe('too-large')
  expect(seen.runs).toEqual([])
})

test('not-paired starts a pairing; the toast shows once per code', { plugins: [ACME] }, async ($, on) => {
  resetForTests()
  const seen = engine(on, {
    jev: args =>
      args[0] === 'ask'
        ? line({ ok: false, error: { code: 'not-paired', message: 'no client' } }, 4)
        : line({ ok: false, error: { code: 'pending', message: 'approve it' }, pair_code: 'ABCD2345', name: 'cc-acme-review' }, 3),
  })
  const first = await acme($)
  expect(first).toEqual({ ok: false, error: { code: 'pending', message: 'approve it', pairCode: 'ABCD2345' } })
  await acme($)
  const pairs = calls(seen.runs, 'pair')
  expect(pairs.length).toBe(2)
  expect(pairs[0]?.argv).toEqual(['jev', 'pair', '--name', 'cc-acme-review', '--no-prompt', '--json'])
  expect(pairs[0]?.stdin).toBe(undefined)
  expect(seen.toasts.length).toBe(1)
  expect(seen.toasts[0]?.includes('jev allow ABCD2345')).toBe(true)
})

test('a new pairing code gets its own toast', { plugins: [ACME] }, async ($, on) => {
  resetForTests()
  let code = 'ABCD2345'
  const seen = engine(on, {
    jev: args =>
      args[0] === 'ask'
        ? line({ ok: false, error: { code: 'not-paired', message: '' } }, 4)
        : line({ ok: false, error: { code: 'pending', message: '' }, pair_code: code }, 3),
  })
  await acme($)
  code = 'WXYZ6789'
  await acme($)
  expect(seen.toasts.length).toBe(2)
})

test('key problems toast once per profile', { plugins: [ACME] }, async ($, on) => {
  resetForTests()
  const errors = [
    { code: 'no-key', message: 'm', profile: 'default' },
    { code: 'bad-key', message: 'm', profile: 'default' },
    { code: 'no-key', message: 'm', profile: 'work' },
    { code: 'jev-unauthorized', message: 'm', profile: 'work' },
    { code: 'jev-unauthorized', message: 'm', profile: 'work' },
  ]
  let i = 0
  const seen = engine(on, { jev: () => line({ ok: false, error: errors[i++] }, 6) })
  for (const error of errors) expect(codeOf(await acme($))).toBe(error.code)
  expect(seen.toasts.length).toBe(3)
  expect(seen.toasts[0]?.includes('jev key add --profile default')).toBe(true)
  expect(seen.toasts[1]?.includes('jev key add --profile work')).toBe(true)
  expect(seen.toasts[2]?.includes('refused the key for profile work')).toBe(true)
})

test('other failures pass through with no toast', { plugins: [ACME] }, async ($, on) => {
  resetForTests()
  const seen = engine(on, {
    jev: () => line({ ok: false, error: { code: 'rate-limited', message: 'wait', retry_after: 9 } }, 5),
  })
  expect(await acme($)).toEqual({ ok: false, error: { code: 'rate-limited', message: 'wait', retryAfter: 9 } })
  expect(seen.toasts).toEqual([])
})

test('a missing jev is one toast, and every call returns ok: false', withPath(MISSING), async ($, on) => {
  resetForTests()
  const seen = engine(on)
  expect(codeOf(await acme($))).toBe('jev-missing')
  expect(codeOf(await acme($))).toBe('jev-missing')
  const state = await acme($, 'status')
  expect(state).toEqual({ ok: true, installed: false, version: null, compatible: false, running: false, paired: null })
  expect(seen.toasts.length).toBe(1)
  expect(seen.toasts[0]?.includes('need the jev CLI')).toBe(true)
})

test('an old jev is one toast, and every call returns ok: false', { plugins: [ACME] }, async ($, on) => {
  resetForTests()
  const seen = engine(on, { version: '0.0.9' })
  expect(codeOf(await acme($))).toBe('jev-outdated')
  expect(codeOf(await acme($))).toBe('jev-outdated')
  expect(calls(seen.runs, 'ask')).toEqual([])
  expect(seen.toasts.length).toBe(1)
  expect(seen.toasts[0]?.includes('need jev 0.1.0 or later')).toBe(true)
})

test('a jev that cannot finish is jev-failed, with no throw', { plugins: [ACME] }, async ($, on) => {
  resetForTests()
  // --version answers; the ask then reaches the host, which cannot start MISSING.
  engine(on, { unstartable: 'ask' })
  expect(codeOf(await acme($))).toBe('jev-failed')
})

test('a reply with no answers is bad-answer', { plugins: [ACME] }, async ($, on) => {
  resetForTests()
  engine(on, { jev: () => line({ ok: true, answers: [1] }) })
  expect(codeOf(await acme($))).toBe('bad-answer')
})

test('$.jev.status with jevd stopped runs no jev clients', { plugins: [ACME] }, async ($, on) => {
  resetForTests()
  const seen = engine(on, {
    jev: args => (args[0] === 'status' ? line({ ok: false, error: { code: 'daemon-unavailable', message: 'not running' } }, 8) : line({ ok: true })),
  })
  expect(await acme($, 'status')).toEqual({ ok: true, installed: true, version: '0.1.0', compatible: true, running: false, paired: null })
  expect(calls(seen.runs, 'clients')).toEqual([])
})

test('$.jev.status with jevd running reads the paired clients', { plugins: [ACME] }, async ($, on) => {
  resetForTests()
  let names = ['cc-other']
  engine(on, {
    jev: args =>
      args[0] === 'status'
        ? line({ ok: true, daemon_version: '0.1.0' })
        : line({ ok: true, clients: names.map(name => ({ name, profile: 'default' })) }),
  })
  expect((await acme($, 'status')).paired).toBe(false)
  names = ['cc-other', 'cc-acme-review']
  expect(await acme($, 'status')).toEqual({ ok: true, installed: true, version: '0.1.0', compatible: true, running: true, paired: true })
})
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `claude plugin test plugins/claude-code`
Expected: FAIL. The test file cannot import `./jev`.

- [ ] **Step 3: Write the calls**

`plugins/claude-code/hooks/jev.ts`:

```ts
// The `$.jev` calls: find `jev`, run it, and show each toast once.

import type { EngineInterface } from 'claude-code'

import type { JevAskResult, JevFailure, JevStatus } from '../types'
import {
  MAX_REQUEST_BYTES,
  MIN_JEV,
  type Parsed,
  callerName,
  clientName,
  failure,
  keyToast,
  missingToast,
  olderThan,
  outdatedToast,
  parseVersion,
  pendingToast,
  readReply,
  requestBytes,
  unauthorizedToast,
  versionText,
} from './text'

/** `jev --version` answers at once. */
export const VERSION_TIMEOUT_MS = 10_000
/**
 * One `jev ask`: jev's own 120 s reply timeout, plus its 10 s wait for a
 * jevd that it starts, plus room.
 */
export const ASK_TIMEOUT_MS = 150_000
/** `jev pair`, `jev status`, and `jev clients`. */
export const OTHER_TIMEOUT_MS = 30_000
const TOAST_MS = 15_000

type Found = { ok: true; path: string; version: string }
type NotFound = { ok: false; code: 'jev-missing' | 'jev-outdated'; message: string; version: string | null }

/** The `jev` that answered `--version`, kept for the rest of the process. A failure is not kept, so a `jev` installed later counts. */
let found: Found | null = null
/** The toasts shown in this process, by key. */
const shown = new Set<string>()

export function resetForTests(): void {
  found = null
  shown.clear()
}

export function toastOnce($: EngineInterface, key: string, text: string): void {
  if (shown.has(key)) return
  shown.add(key)
  $.ui.toast(text, { timeoutMs: TOAST_MS })
}

/** Runs `jev --version` once, to find `jev` and check that it is new enough. */
export async function findJev($: EngineInterface, configured: string): Promise<Found | NotFound> {
  if (found !== null) return found
  const path = configured === '' ? 'jev' : configured
  let stdout: string
  let exitCode: number
  try {
    const r = await $.process.run([path, '--version'], { timeoutMs: VERSION_TIMEOUT_MS })
    stdout = r.stdout
    exitCode = r.exitCode
  } catch {
    return { ok: false, code: 'jev-missing', message: `cannot run ${path}`, version: null }
  }
  const version = exitCode === 0 ? parseVersion(stdout) : null
  if (version === null) {
    return { ok: false, code: 'jev-missing', message: `${path} --version printed no jev version`, version: null }
  }
  if (olderThan(version, MIN_JEV)) {
    const text = versionText(version)
    return { ok: false, code: 'jev-outdated', message: `jev ${text} is older than ${versionText(MIN_JEV)}`, version: text }
  }
  found = { ok: true, path, version: versionText(version) }
  return found
}

/** `jev` found and new enough, or a failure after one toast per process. */
export async function ready($: EngineInterface, configured: string): Promise<Found | JevFailure> {
  const jev = await findJev($, configured)
  if (jev.ok) return jev
  toastOnce($, jev.code, jev.code === 'jev-missing' ? missingToast() : outdatedToast(jev.version ?? '?', MIN_JEV))
  return failure(jev.code, jev.message)
}

/** One `jev` run. It never rejects: a run that cannot start or finish is `jev-failed`. */
export async function runJev(
  $: EngineInterface,
  path: string,
  args: readonly string[],
  stdin: string | undefined,
  timeoutMs: number,
): Promise<Parsed> {
  try {
    const r = await $.process.run([path, ...args], { ...(stdin === undefined ? {} : { stdin }), timeoutMs })
    return readReply(r.stdout, r.exitCode)
  } catch (err) {
    return failure('jev-failed', `jev did not finish: ${String(err).slice(0, 200)}`)
  }
}

/** One toast for a key problem, per profile, per process. */
export function notice($: EngineInterface, error: JevFailure['error']): void {
  if (error.code === 'no-key' || error.code === 'bad-key') {
    toastOnce($, `key:${error.profile ?? ''}`, keyToast(error.profile))
  } else if (error.code === 'jev-unauthorized') {
    toastOnce($, `unauthorized:${error.profile ?? ''}`, unauthorizedToast(error.profile))
  }
}

/**
 * `jev pair --no-prompt`: jevd lists the request, and `jev` prints its code.
 * A repeat while the request waits gets the same code, so one toast per code.
 */
export async function pair($: EngineInterface, path: string, plugin: string): Promise<JevFailure> {
  const reply = await runJev($, path, ['pair', '--name', clientName(plugin), '--no-prompt', '--json'], undefined, OTHER_TIMEOUT_MS)
  if (reply.ok) return failure('not-paired', `jev pair approved ${clientName(plugin)} with no person; ask again`)
  const code = reply.error.pairCode
  if (reply.error.code === 'pending' && code !== undefined) toastOnce($, `pending:${code}`, pendingToast(plugin, code))
  return reply
}

/** `$.jev.ask`, spec section 7, "Flow of one ask". */
export async function ask($: EngineInterface, input: unknown, origin: string, configured: string): Promise<JevAskResult> {
  const who = callerName(origin, input)
  if (!who.ok) return who
  const given = (input ?? {}) as { state?: unknown; questions?: unknown }
  let body: string
  try {
    body = JSON.stringify({ state: given.state, questions: given.questions })
  } catch {
    return failure('bad-request', 'state and questions must be JSON values')
  }
  if (requestBytes(body) > MAX_REQUEST_BYTES) {
    return failure('too-large', `the request is over ${MAX_REQUEST_BYTES} bytes`)
  }
  const jev = await ready($, configured)
  if (!jev.ok) return jev
  const reply = await runJev($, jev.path, ['ask', '--client', clientName(who.plugin)], body, ASK_TIMEOUT_MS)
  if (reply.ok) {
    const answers = reply.data.answers
    return answers !== null && typeof answers === 'object' && !Array.isArray(answers)
      ? { ok: true, answers: answers as Extract<JevAskResult, { ok: true }>['answers'] }
      : failure('bad-answer', 'jev printed no answers')
  }
  if (reply.error.code === 'not-paired') return await pair($, jev.path, who.plugin)
  notice($, reply.error)
  return reply
}

/** `$.jev.status`. It never starts jevd: `jev status` does not, and `jev clients` runs only when jevd runs. */
export async function status($: EngineInterface, input: unknown, origin: string, configured: string): Promise<JevStatus> {
  const who = callerName(origin, input)
  if (!who.ok) return who
  const jev = await findJev($, configured)
  if (!jev.ok) {
    toastOnce($, jev.code, jev.code === 'jev-missing' ? missingToast() : outdatedToast(jev.version ?? '?', MIN_JEV))
    return { ok: true, installed: jev.code === 'jev-outdated', version: jev.version, compatible: false, running: false, paired: null }
  }
  const daemon = await runJev($, jev.path, ['status', '--json'], undefined, OTHER_TIMEOUT_MS)
  if (!daemon.ok) return { ok: true, installed: true, version: jev.version, compatible: true, running: false, paired: null }
  const list = await runJev($, jev.path, ['clients', '--json'], undefined, OTHER_TIMEOUT_MS)
  const clients = list.ok ? list.data.clients : undefined
  const paired = Array.isArray(clients)
    ? clients.some(c => c !== null && typeof c === 'object' && (c as { name?: unknown }).name === clientName(who.plugin))
    : null
  return { ok: true, installed: true, version: jev.version, compatible: true, running: true, paired }
}
```

- [ ] **Step 4: Write the hooks**

Replace `plugins/claude-code/hooks/register.ts` with:

```ts
import type { Register } from 'claude-code'

import type { JevFailure } from '../types'
import { ask, status } from './jev'
import { failure } from './text'

/** What a `$.jev` method answers when its hook failed, so no call ever rejects. */
const HOOK_FAILED = { value: failure('internal', 'the jev-daemon hook failed') } as never

export const register: Register = (on, options) => {
  const configured = typeof options.jevPath === 'string' ? options.jevPath.trim() : ''

  // Every plugin that lists jev-daemon under dependencies gets $.jev. The `$`
  // at engine.create carries no nouns yet, so these methods only stand in: a
  // call to $.jev.ask is the event jev.ask, which the hooks below answer.
  on('engine.create', async (_$, e, next) => {
    const built = await next(e)
    const unanswered: JevFailure = failure('internal', 'no jev-daemon hook answered')
    return { ...built, jev: { ask: async () => unanswered, status: async () => unanswered } }
  })

  on('jev.ask', async ($, e, next) => ({ value: await ask($, e, next.origin.plugin, configured) }) as never).catch(
    () => HOOK_FAILED,
  )

  // `$.jev.status()` with no input arrives as an absent e.
  on('jev.status', async ($, e, next) => ({ value: await status($, e, next.origin.plugin, configured) }) as never).catch(
    () => HOOK_FAILED,
  )
}
```

- [ ] **Step 5: Run the tests to see them pass**

Run: `claude plugin test plugins/claude-code`
Expected: `22 pass`, `0 fail`.

Run: `claude plugin validate plugins/claude-code`
Expected: `Validation passed`. The validator lists the hooks `engine.create`, `jev.ask`, and `jev.status`, and the calls `$.process.run` and `$.ui.toast`.

- [ ] **Step 6: Check that the tests guard the rules**

Make each change below in `jev.ts`, run `claude plugin test plugins/claude-code`, and restore the file after each:

| Change | Expected failure |
| --- | --- |
| `if (requestBytes(body) > MAX_REQUEST_BYTES) {` becomes `if (requestBytes(body) < 0) {` | `a request over 1 MiB is too-large, and runs nothing` |
| `  if (shown.has(key)) return` becomes `  if (shown.has('never')) return` | the 4 toast tests: pending, key problems, missing jev, old jev |
| `'--no-prompt', ` is removed from the `pair` arguments | `not-paired starts a pairing; the toast shows once per code` |

- [ ] **Step 7: Commit**

```bash
git add plugins/claude-code/hooks/jev.ts plugins/claude-code/hooks/register.ts plugins/claude-code/hooks/register.test.ts
git commit -m "feat(mod): \$.jev.ask and \$.jev.status through the jev CLI" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: type check, smoke test, and the spec

**Files:**
- Modify: `docs/superpowers/specs/2026-11-02-jev-daemon-design.md`
- Modify: this plan file, only its "Not tested by this plan" section if a step finds more

- [ ] **Step 1: Type-check the mod**

`.claude-plugin/types/` appears only after an interactive load, so write this `tsconfig.json` in your scratchpad, outside the repository. Its `include` names the API types file on this machine, and the mod's `hooks` and `types` folders. Write each path with forward slashes, in full:

```json
{
  "compilerOptions": {
    "target": "es2023", "lib": ["es2023"], "types": [],
    "module": "esnext", "moduleResolution": "bundler",
    "strict": true, "noUncheckedIndexedAccess": true,
    "noEmit": true, "skipLibCheck": true,
    "jsx": "react", "jsxFactory": "h", "jsxFragmentFactory": "Fragment"
  },
  "include": [
    "<Claude Code's API types file, claude-code.d.ts>",
    "<the worktree>/plugins/claude-code/hooks",
    "<the worktree>/plugins/claude-code/types"
  ]
}
```

Run: `npx -y -p typescript@5 tsc -p <that tsconfig.json>`
Expected: exit 0, no output.

- [ ] **Step 2: The smoke test, in a real headless session**

This run uses the real `jev` and `jevd` from this worktree, a temporary `JEV_HOME`, and the real OS keyring for reading only: the flow stops at `pending`, so it writes nothing to the keyring.

1. Run `cargo build -p jev -p jevd --locked`.
2. In your scratchpad, make an empty folder `smokehome`, and a smoke plugin folder `smoke` with these 3 files.

`smoke/.claude-plugin/plugin.json`:

```json
{ "name": "jev-smoke", "version": "0.0.1", "description": "smoke test for jev-daemon", "dependencies": ["jev-daemon"] }
```

`smoke/hooks/hooks.json`:

```json
{ "modules": ["./register.ts"] }
```

`smoke/hooks/register.ts`. Its `/jevsmoke` command calls `$.jev.ask` 2 times, then `$.jev.status`, and returns one line per call:

```ts
import type { Register } from 'claude-code'

export const register: Register = on => {
  on('session.start', async ($, e, next) => {
    const started = await next(e)
    await $.command.register({ name: 'jevsmoke', description: 'smoke' })
    return started
  })
  // The loader refuses `$` bound to a name, so each call casts at its site.
  type Jev = { jev: { ask: (x: unknown) => Promise<unknown>; status: () => Promise<unknown> } }
  on('command.run', { command: 'jevsmoke' }, async $ => {
    const rows: string[] = []
    const questions = { q: { type: 'noul', instructions: 'Is this a smoke test?' } }
    for (let i = 0; i < 2; i++) {
      const t0 = Date.now()
      let out: unknown
      try {
        out = await ($ as never as Jev).jev.ask({ state: 'smoke', questions })
      } catch (err) {
        out = `threw ${String(err)}`
      }
      rows.push(`ask${i} ${Date.now() - t0}ms ${JSON.stringify(out)}`)
    }
    let state: unknown
    try {
      state = await ($ as never as Jev).jev.status()
    } catch (err) {
      state = `threw ${String(err)}`
    }
    rows.push(`status ${JSON.stringify(state)}`)
    return { text: rows.join(' | ') }
  })
}
```

3. Run the session from Git Bash, with this worktree's `target/debug` first on the PATH, the temporary home, and any other plugin that defines `$.jev` off for this run only. Two plugins cannot own `$.jev`. Write each `<...>` path in full, with forward slashes. Write the PATH entry in the Git Bash form, `/c/Users/...`, because Git Bash splits a `C:/...` PATH entry at the colon. Write the other paths in the `C:/...` form:

```bash
PATH="<the worktree, as /c/...>/target/debug:$PATH" JEV_HOME="<scratchpad>/smokehome" MSYS_NO_PATHCONV=1 \
  env -u CLAUDE_PID -u CLAUDE_CODE_SESSION_ID -u CLAUDECODE -u CLAUDE_CODE_CHILD_SESSION \
  claude -p "/jevsmoke" --plugin-dir "<the worktree>/plugins/claude-code" --plugin-dir "<scratchpad>/smoke" \
  --settings '{"enabledPlugins":{"<the other $.jev plugin>":false}}' < /dev/null
```

Expected, in the one output line, in this order:
- `ask0`, with `{"ok":false,"error":{"code":"pending",...,"pairCode":"<8 characters>"}}`. On this machine it took about 3 s, with the `jevd` start.
- `ask1`, with the same `pairCode`, in under 2 s.
- `status`, with `{"ok":true,"installed":true,"version":"0.1.0","compatible":true,"running":true,"paired":false}`.

No call waits for its timeout. A wait of 150 s means the started `jevd` holds the mod's pipe.

4. Stop the daemon: `JEV_HOME="<scratchpad>/smokehome" <the worktree>/target/debug/jev stop`. Check that no `jevd` from this worktree still runs (`tasklist | findstr jevd` on Windows). Then delete `smokehome` and `smoke`.

- [ ] **Step 3: Update the spec**

Make these changes in `docs/superpowers/specs/2026-11-02-jev-daemon-design.md`. Keep every other line as it is.

In section 2, "Messages, protocol version 1", after the line "Every reply has `ok`. A failure carries `error.code` and `error.message`. Clients branch on the code, never on the message.", add:

```
A `no-key`, `bad-key`, or `jev-unauthorized` failure from an `ask` also carries `error.profile`, the profile whose key failed, so a client can name it. A client that does not know the field ignores it.
```

In section 6, "Commands", change the pairing row's command to `jev pair --name <n> [--profile <p>] [--no-prompt]`. Under "Shared behavior", after the bullet "`jev pair` without a terminal exits 3 and prints `pair_code` in its JSON.", add:

```
- `jev pair --no-prompt` never uses the terminal, even when one is there: it exits 3 with `pair_code`, as with no terminal. A caller that runs inside another program's terminal, such as the Claude Code mod, uses it.
```

In section 7, replace the `$.jev` table, the "Flow of one `ask`" list, and the line "The mod finds `jev` on the PATH. The plugin setting `jevPath` overrides that." with:

```
### `$.jev`

| Call | Result |
| --- | --- |
| `$.jev.ask({ state, questions })` | `{ ok: true, answers }` or `{ ok: false, error: { code, message } }` |
| `$.jev.status()` | `{ ok: true, installed, version, compatible, running, paired }`. It never starts `jevd`, and `paired` is `null` when `jevd` does not run. |

Neither call throws. A caller that gets `ok: false` does what it would do without Jev. The host names the calling plugin. An input `plugin` field must name the same plugin, or the call is `bad-request`, so one mod cannot ask as another.

A failure can also carry `retryAfter`, `profile`, and, for `pending`, `pairCode`. The mod adds 3 codes of its own: `jev-missing`, `jev-outdated`, and `jev-failed` (a `jev` run that could not start or finish). Every other code comes from `jev` as it is. When `jev` prints no JSON, the exit code picks the code, by the table in section 6.

### Flow of one `ask`

1. The mod checks the plugin name against `[a-z0-9][a-z0-9-]{0,60}`.
2. The mod checks the request size against `jev`'s 1 MiB cap. Over the cap, it returns `too-large` and runs nothing.
3. The mod runs `jev --version` once per process, and refuses a `jev` older than it needs.
4. The mod runs `jev ask --client cc-<plugin>` through `$.process.run`, with the request JSON on stdin, and parses the reply. A child with no `stdin` option reads end of file at once, and one with `stdin` reads the request, then end of file.
5. On `not-paired`, the mod runs `jev pair --name cc-<plugin> --no-prompt --json`. That returns `pending` and a code, and never prompts on the terminal that Claude Code runs in. While the request waits, a repeat run returns the same code, so repeated asks do not fill the pending list. The mod shows a toast and returns `pending`.
6. After the person runs `jev allow`, the keyring holds the token, and the next `ask` works.

Timeouts: 150 s for `jev ask`, 30 s for `pair`, `status`, and `clients`, and 10 s for `--version`. Time that a hook spends waiting on `$.process.run` does not count against its budget.

The mod finds `jev` on the PATH. The plugin option `jevPath` overrides that. The plugin is named `jev-daemon`. It cannot run with another plugin that also defines `$.jev`.
```

In the "Toasts" table, change the `no-key`, `bad-key` row and the `jev-unauthorized` row so the profile comes from `error.profile`, and add this line under the table:

```
Each toast shows once: per pairing code, per profile for a key problem, and per process for a missing or old `jev`.
```

In section 10, "Limits to document", delete the bullet "A mod can pass another plugin's name to `$.jev.ask`." Replace the bullet "Other mods that hook `process.spawn` see request content in the arguments. They never see a key or a token." with:

```
- Other mods that hook `process.run` see request content on the child's stdin. They never see a key or a token.
- Any process of the user can run `jev ask --client cc-<plugin>` and use that plugin's pairing, as the first bullet says for every CLI client.
```

In section 11, "Mod", replace the first bullet with "Each CLI exit code and error code maps to the right result, with a fake `jev` on `process.run`." and add:

```
- The host's plugin name wins, and a different input `plugin` is `bad-request`.
- The request travels on stdin, as `jev ask --client cc-<plugin>`.
- `jev pair` runs with `--no-prompt`.
- A smoke test in a real headless session, with the real `jev` and `jevd`: 2 asks get `pending` with one code, and `$.jev.status` shows `running` and not `paired`.
```

Run the STE lint on the spec, as the `simple-english-docs` skill says, and fix every ERROR in the lines you wrote.

- [ ] **Step 4: Run every gate once more**

Run the 3 Rust gates, `claude plugin test plugins/claude-code`, and `claude plugin validate plugins/claude-code`.
Expected: all exit 0, and the counts below.

| Suite | Tests |
| --- | --- |
| jev lib | 45 |
| jev tests/binary.rs | 4 |
| jev tests/cli.rs | 30 |
| jev-proto lib | 39 |
| jevd lib | 247, and 1 ignored |
| jevd tests/binary.rs | 3 |
| jevd tests/client.rs | 7 |
| jevd tests/serve.rs | 6 |
| the mod | 22 |

- [ ] **Step 5: Commit**

```bash
git add docs/superpowers/specs/2026-11-02-jev-daemon-design.md
git commit -m "docs: record plan 5 choices in the spec" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Not tested by this plan

- **The toast on screen.** `claude -p` shows no toasts. Check by hand once, in an interactive session with your own toast plugin turned off: an ask from a plugin that is not paired shows the pairing toast.
- **The mod on Linux and macOS.** Every probe and the smoke test ran on Windows. Plan 6's CI runs `claude plugin test` on each OS. Whether a Claude Code child opens the terminal on Unix does not matter, because `--no-prompt` never tries.
- **`tsc` against the 2.1.300 types.** No 2.1.300 types file exists on this machine. Plan 6 must get one for CI, for example: run `/plugin-types` once, and commit a copy outside the plugin folder.
- **A real `jev ask` that gets answers through the mod.** That needs a paired client and a real key, so it spends a request. The CLI flow in plan 4's tests covers `jev ask` itself.
- **The mapping of the host's "failed to start" rejection to `jev-missing`.** The test harness has no real `process.run`, so only the message from the 2026-11-03 probe backs it.

## Notes for plan 6

- CI must run `claude plugin test plugins/claude-code`, `claude plugin validate plugins/claude-code`, and `tsc` with a types file. The prototype's workflow (`jev-service/.github/workflows/ci.yml`) installs Claude Code with Node 22.
- The plugin version (0.1.0) must follow the crate version, per spec section 8.
- Raise `MIN_JEV` in `hooks/text.ts` to the first released version that has `jev pair --no-prompt`, before the first tag.
