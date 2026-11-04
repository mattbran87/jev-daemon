# jev-daemon plan 6: distribution and CI

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add CI on Windows, Linux, and macOS, a release build with `dist` that puts `jev` and `jevd` in one archive, a `cargo-deny` check, and the public GitHub repo, and check every workflow on GitHub before any tag.

**Architecture:** The `jev` package owns both binaries, so `dist` sees one app and each archive holds `jev` and `jevd` next to each other, as the peer check needs. A CI workflow runs the Rust gates, an MSRV check, the mod's tests, and `cargo deny`. `dist` writes the release workflow; a pull request builds every archive with no release. Plan 6 stops before the first tag.

**Tech Stack:** Rust 1.99.1 in CI (MSRV 1.85), `dist` 0.33.1, `cargo-deny` 0.20.2, GitHub Actions, Claude Code 2.1.300 for the mod.

**Spec:** `docs/superpowers/specs/2026-11-02-jev-daemon-design.md`, sections 8 and 12. **Plans 1 to 5** sit on `main`.

---

## Before you start

- Work in the worktree that the executing skill makes. Run every command from its root.
- Stage explicit paths. Never run `git add -A`.
- End every commit message with your Co-Authored-By line, as a separate `-m` argument.
- Run each gate on its own, never in a pipe that hides its exit status: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`. Run no two cargo commands at the same time.
- After Task 1, `cargo test` builds `jevd` itself. Before Task 1, build it first with `cargo build -p jevd --locked`.
- Write every file with LF line endings. On Windows, Python's text mode writes CRLF; use `newline='\n'` or binary mode. Check with `git ls-files --eol <file>`: the index column must say `i/lf`.
- Each patch below applies with `git apply <file>`. Save the patch text to a file in your scratchpad first, byte for byte, with LF line endings. If `git apply` refuses a patch, stop and report it: do not edit by hand.
- This machine has `dist` 0.33.1 (`dist --version`) and `cargo-deny` 0.20.2 (`cargo deny --version`) in `~/.cargo/bin`. If either is missing, stop and report it: do not install anything.
- No test sends a request to `api.typesafe.ai`.

## Decisions the person made on 2026-11-04

- The repo is public: `mattbran87/jev-daemon` on GitHub. The release archives and installers then download with no sign-in.
- No crate goes to crates.io in v1. `cargo install --git` still works.
- The Linux build uses glibc (`gnu`), not static musl.
- The rate limit default stays 120 requests per 60 s for v0.1.0. Measuring a real client stays an open item.

## What the checks showed

These facts come from runs on this machine on 2026-11-04.

| Fact | What it changes |
| --- | --- |
| `dist plan` on the old layout makes 2 apps, `jev` and `jevd`, each with its own archive and installers. | The spec needs both binaries in one archive. Task 1 moves `jevd`'s binary into the `jev` package; `dist plan` then shows one app with both binaries in every archive. |
| `dist` picks a native runner for every target: `ubuntu-22.04`, `ubuntu-22.04-arm`, `macos-14`, `macos-15-intel`, `windows-2022`. | `aws-lc-sys` never needs a C cross compiler. The Linux archives need glibc 2.35 or newer, from Ubuntu 22.04. |
| `dist` 0.33.1 writes `actions/checkout@v6`, `upload-artifact@v7`, and `download-artifact@v8` by tag. Its `github-action-commits` setting pins each one to a commit. | Every action in both workflows is pinned to a commit SHA, as spec section 8 asks. |
| `cargo deny check` passes licenses, bans, and sources. Advisories fail on 3 "unmaintained" notices: `zvariant`, `zbus_names`, and `zvariant_derive` (RUSTSEC-2026-0370, -0371, -0372, issued 2026-10-17). They are not vulnerabilities. They reach us on Linux only, through `keyring` 3.6, `secret-service` 4, and `zbus` 4. | `deny.toml` ignores the 3 IDs, each with its reason. The fix is `zbus` 6, which `keyring` 3.6 does not use; `keyring` 4 needs Rust 1.88. |
| `cargo +1.85 check --workspace --locked` passes. With `--all-targets` it fails, because `wiremock` 0.6.5, a test-only dependency, uses let-chains (Rust 1.88). | The MSRV job checks the shipped code only. |
| `/plugin-types` works only in an interactive session, and its file is Claude Code's own text. | CI runs `claude plugin test` and `claude plugin validate`, not `tsc`. `tsc` stays a local check, as in plan 5. |
| `claude plugin validate .` on the repo root accepts `.claude-plugin/marketplace.json`. | CI validates the marketplace too. |

## Choices this plan makes

- Versions at least two weeks old on 2026-11-04: `dist` 0.33.1 (2026-10-21, not 0.34.0 from 2026-11-02), `cargo-deny` 0.20.2, Rust 1.99.1 (2026-10-13), and the actions below.
- Actions and their commits:

| Action | Tag | Commit |
| --- | --- | --- |
| `actions/checkout` (CI) | v7.0.1 | `3d3c42e5aac5ba805825da76410c181273ba90b1` |
| `actions/checkout` (release, through `dist`) | v6.1.0 | `d23441a48e516b6c34aea4fa41551a30e30af803` |
| `actions/setup-node` | v7.1.0 | `949feb2413d6458794dcd2491c4babbbce0c15c1` |
| `actions/upload-artifact` | v7.0.2 | `cf430e030ddbb5b0abf93d22962f4752f3646cd9` |
| `actions/download-artifact` | v8.0.2 | `9000827ccba6bdab643e8b6fd33ac0654aef8333` |
| `Swatinem/rust-cache` | v2.9.3 | `d1527c8f67ef9af357290475d4af4b0fab8f12e0` |
| `EmbarkStudios/cargo-deny-action` | v2.1.1 | `3c6349835b2b7b196a839186cb8b78e02f7b5f25` |

- `dist` installs both binaries into `$CARGO_HOME/bin`, the same folder `cargo install` uses, so the peer check holds either way.
- A pull request runs the release workflow in `upload` mode: it builds every archive and uploads them to the run, with no release. Task 7 uses this to check the release build.
- `MIN_JEV` in the mod stays 0.1.0: v0.1.0 is the first release, and it has `jev pair --no-prompt`. A new test keeps `MIN_JEV` no newer than the crates.
- Plan 6 adds a short `README.md`. The full docs come in a later plan (spec section 9).
- Plan 6 stops before the first tag. The tag `v0.1.0` waits for the docs and for the person's go-ahead.

## Files

```
crates/jev/src/bin/jevd.rs           moved from crates/jevd/src/main.rs (Task 1)
crates/jev/tests/jevd_binary.rs      moved from crates/jevd/tests/binary.rs (Task 1)
crates/jev/tests/jevd_client.rs      moved from crates/jevd/tests/client.rs (Task 1)
crates/jev/tests/common/mod.rs       moved from crates/jevd/tests/common/mod.rs (Task 1)
crates/jev/Cargo.toml, tests/binary.rs, Cargo.lock    Task 1 edits
crates/jev/tests/versions.rs         one version for the crates and the plugin (Task 2)
deny.toml                            cargo-deny settings (Task 3)
.github/workflows/ci.yml             the CI workflow (Task 4)
Cargo.toml                           [profile.dist] (Task 5)
dist-workspace.toml                  dist settings (Task 5)
.github/workflows/release.yml        written by `dist generate` (Task 5)
README.md                            a short README (Task 6)
docs/superpowers/specs/2026-11-02-jev-daemon-design.md   sections 1, 8, 12 (Task 6)
```

---

### Task 1: the `jev` package owns both binaries

`dist` makes one app per package. With `jevd`'s binary in the `jevd` package, the release has two archives, and a person who installs one gets no peer. This task moves the binary and the 2 test files that run it into the `jev` package. The `jevd` package stays a library. `jev` already depends on it, so `crates/jev/src/bin/jevd.rs` compiles as it is. The binary's name stays `jevd`.

**Files:**
- Move: `crates/jevd/src/main.rs` to `crates/jev/src/bin/jevd.rs`; `crates/jevd/tests/binary.rs` to `crates/jev/tests/jevd_binary.rs`; `crates/jevd/tests/client.rs` to `crates/jev/tests/jevd_client.rs`; `crates/jevd/tests/common/mod.rs` to `crates/jev/tests/common/mod.rs`
- Modify: `crates/jev/Cargo.toml`, `crates/jev/tests/binary.rs`, `Cargo.lock`

- [ ] **Step 1: Move the files**

```bash
mkdir -p crates/jev/src/bin crates/jev/tests/common
git mv crates/jevd/src/main.rs crates/jev/src/bin/jevd.rs
git mv crates/jevd/tests/binary.rs crates/jev/tests/jevd_binary.rs
git mv crates/jevd/tests/client.rs crates/jev/tests/jevd_client.rs
git mv crates/jevd/tests/common/mod.rs crates/jev/tests/common/mod.rs
```

- [ ] **Step 2: Apply the edits**

Save this patch as `t1-edits.patch` and apply it with `git apply t1-edits.patch`. It gives `jev`'s tests the `test-util` feature of tokio and, on Unix, `libc`, which the moved tests use. It adds `libc` to the lock file. And it makes `jev`'s own binary test use `CARGO_BIN_EXE_jevd`, so it no longer asks for a separate `jevd` build.

```diff
diff --git a/Cargo.lock b/Cargo.lock
index d974c87..9d24226 100644
--- a/Cargo.lock
+++ b/Cargo.lock
@@ -1139,6 +1139,7 @@ dependencies = [
  "clap",
  "jev-proto",
  "jevd",
+ "libc",
  "rpassword",
  "serde_json",
  "tempfile",
diff --git a/crates/jev/Cargo.toml b/crates/jev/Cargo.toml
index 0b6432b..ba2149e 100644
--- a/crates/jev/Cargo.toml
+++ b/crates/jev/Cargo.toml
@@ -18,5 +18,8 @@ zeroize = "1.9"
 
 [dev-dependencies]
 tempfile = "3"
-tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
+tokio = { version = "1", features = ["macros", "rt-multi-thread", "test-util"] }
 wiremock = "0.6"
+
+[target.'cfg(unix)'.dev-dependencies]
+libc = "0.2"
diff --git a/crates/jev/tests/binary.rs b/crates/jev/tests/binary.rs
index 08aa420..3f82029 100644
--- a/crates/jev/tests/binary.rs
+++ b/crates/jev/tests/binary.rs
@@ -1,10 +1,8 @@
 //! The real `jev` binary, with the real `jevd` next to it in the target
 //! folder, under a new `JEV_HOME`. No command here touches the keyring.
 //!
-//! These tests need `jevd` built first: `cargo test --workspace` builds it,
-//! and `cargo test -p jev` alone does not, so run `cargo build -p jevd`
-//! first. Without it, the tests that start a daemon fail at once with
-//! "jevd... is missing".
+//! The `jev` package owns both binaries, so cargo builds `jevd` for these
+//! tests, next to `jev`, as a release archive holds them.
 
 use std::path::{Path, PathBuf};
 use std::process::{Command, Output, Stdio};
@@ -17,15 +15,16 @@ use jev_proto::Paths;
 use serde_json::Value;
 
 const JEV: &str = env!("CARGO_BIN_EXE_jev");
+const JEVD: &str = env!("CARGO_BIN_EXE_jevd");
 /// How long one `jev` command may take, with its pipes closed.
 const DEADLINE: Duration = Duration::from_secs(30);
 
 fn jevd_next_to_jev() -> PathBuf {
-    let jevd = Path::new(JEV).with_file_name(format!("jevd{}", std::env::consts::EXE_SUFFIX));
-    assert!(
-        jevd.exists(),
-        "{} is missing; run cargo build -p jevd (cargo test --workspace builds it)",
-        jevd.display()
+    let jevd = PathBuf::from(JEVD);
+    assert_eq!(
+        jevd.parent(),
+        Path::new(JEV).parent(),
+        "cargo puts jevd next to jev"
     );
     jevd
 }
```

- [ ] **Step 3: Build and test with no separate `jevd` build**

Run: `cargo clean -p jevd`, then `cargo test --workspace --locked`.
Expected: PASS, with these counts:

| Suite | Tests |
| --- | --- |
| jev lib | 45 |
| jev tests/binary.rs | 4 |
| jev tests/cli.rs | 31 |
| jev tests/jevd_binary.rs | 3 |
| jev tests/jevd_client.rs | 7 |
| jev-proto lib | 39 |
| jevd lib | 247, and 1 ignored |
| jevd tests/serve.rs | 6 |

Then check that `target/debug` holds both `jev` and `jevd` (`jev.exe` and `jevd.exe` on Windows).

- [ ] **Step 4: Run the gates and commit**

Run the 3 gates. Expected: all exit 0. Check that no `jevd` still runs (`tasklist | findstr jevd` on Windows).

```bash
git add crates/jev/Cargo.toml crates/jev/tests/binary.rs Cargo.lock crates/jev/src/bin/jevd.rs crates/jev/tests/jevd_binary.rs crates/jev/tests/jevd_client.rs crates/jev/tests/common/mod.rs crates/jevd/src/main.rs crates/jevd/tests/binary.rs crates/jevd/tests/client.rs crates/jevd/tests/common/mod.rs
git commit -m "refactor: the jev package owns both binaries, so one archive holds jev and jevd"
```

---

### Task 2: one version for the crates and the plugin

**Files:**
- Create: `crates/jev/tests/versions.rs`

- [ ] **Step 1: Write the test**

`crates/jev/tests/versions.rs`:

```rust
//! One version for the crates and the Claude Code plugin (spec section 8).

use serde_json::Value;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn read(path: &str) -> String {
    std::fs::read_to_string(format!("{ROOT}/{path}"))
        .unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn version(text: &str) -> (u64, u64, u64) {
    let parts: Vec<u64> = text.split('.').map(|part| part.parse().unwrap()).collect();
    assert_eq!(parts.len(), 3, "{text} is not major.minor.patch");
    (parts[0], parts[1], parts[2])
}

#[test]
fn the_plugin_version_is_the_crate_version() {
    let manifest: Value =
        serde_json::from_str(&read("plugins/claude-code/.claude-plugin/plugin.json")).unwrap();
    assert_eq!(manifest["version"], env!("CARGO_PKG_VERSION"));
}

#[test]
fn the_mod_needs_no_jev_newer_than_this_one() {
    let text = read("plugins/claude-code/hooks/text.ts");
    let line = text
        .lines()
        .find(|line| line.starts_with("export const MIN_JEV"))
        .expect("text.ts declares MIN_JEV");
    let inside = &line[line.find('[').unwrap() + 1..line.find(']').unwrap()];
    let min = version(&inside.replace(", ", ".").replace(',', "."));
    assert!(
        min <= version(env!("CARGO_PKG_VERSION")),
        "MIN_JEV {min:?} is newer than jev {}",
        env!("CARGO_PKG_VERSION")
    );
}
```

- [ ] **Step 2: Run it**

Run: `cargo test --locked -p jev --test versions`
Expected: PASS, 2 tests.

- [ ] **Step 3: Check that it guards the rule**

In `plugins/claude-code/.claude-plugin/plugin.json`, change `"version": "0.1.0"` to `"version": "0.1.1"`. Run Step 2's command. Expected: FAIL in `the_plugin_version_is_the_crate_version`. Restore the file with `git checkout -- plugins/claude-code/.claude-plugin/plugin.json`.

In `plugins/claude-code/hooks/text.ts`, change `MIN_JEV: Version = [0, 1, 0]` to `MIN_JEV: Version = [0, 2, 0]`. Run Step 2's command. Expected: FAIL in `the_mod_needs_no_jev_newer_than_this_one`. Restore the file the same way.

- [ ] **Step 4: Commit**

Run `cargo fmt --all --check` and `cargo clippy --workspace --all-targets --locked -- -D warnings`. Expected: both exit 0.

```bash
git add crates/jev/tests/versions.rs
git commit -m "test: one version for the crates and the Claude Code plugin"
```

---

### Task 3: `cargo-deny`

**Files:**
- Create: `deny.toml`

- [ ] **Step 1: See the advisories fail with no settings**

Run: `cargo deny check advisories`
Expected: FAIL, with 3 `unmaintained` errors: `zvariant`, `zbus_names`, and `zvariant_derive`. The check fetches the RustSec database from GitHub first.

- [ ] **Step 2: Write the settings**

`deny.toml`:

```toml
# cargo-deny: licenses, security advisories, and sources (spec section 8).
# CI runs `cargo deny check` on each push and pull request.

[graph]
# The release targets only, so a crate that only another platform uses does not count.
targets = [
    "x86_64-pc-windows-msvc",
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
]

[advisories]
version = 2
yanked = "deny"
# Each ignored advisory needs a reason here.
ignore = [
    # zvariant, zbus_names, and zvariant_derive: "unmaintained" notices, not
    # vulnerabilities (2026-10-17). They reach us on Linux only: keyring 3.6 ->
    # secret-service 4 -> zbus 4. The fix is zbus 6, which keyring 3.6 does not
    # use, and keyring 4 needs Rust 1.88, above the 1.85 MSRV. Look again when
    # the MSRV moves.
    { id = "RUSTSEC-2026-0370", reason = "zvariant unmaintained; via keyring 3.6, Linux only" },
    { id = "RUSTSEC-2026-0371", reason = "zbus_names unmaintained; via keyring 3.6, Linux only" },
    { id = "RUSTSEC-2026-0372", reason = "zvariant_derive unmaintained; via keyring 3.6, Linux only" },
]

[licenses]
version = 2
# Every license in the tree on 2026-11-04. A new one fails CI until someone adds it here.
allow = [
    "Apache-2.0",
    "Apache-2.0 WITH LLVM-exception",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "CC0-1.0",
    "CDLA-Permissive-2.0",
    "ISC",
    "MIT",
    "MIT-0",
    "Unicode-3.0",
    "Unlicense",
]
confidence-threshold = 0.93

[bans]
# Two versions of one crate (windows-sys, for example) are common and not a defect.
multiple-versions = "warn"
wildcards = "deny"
allow-wildcard-paths = true

[sources]
unknown-registry = "deny"
unknown-git = "deny"
allow-registry = ["https://github.com/rust-lang/crates.io-index"]
```

- [ ] **Step 3: Run every check**

Run: `cargo deny check`
Expected: exit 0, and the summary `advisories ok, bans ok, licenses ok, sources ok`. One `warning[duplicate]` about `core-foundation` is expected, because `multiple-versions` is `warn`.

- [ ] **Step 4: Check that the license list guards the rule**

In `deny.toml`, delete the line `"ISC",`. Run `cargo deny check licenses`. Expected: FAIL, with `error[rejected]: failed to satisfy license requirements` and "license is not explicitly allowed". Restore the line.

- [ ] **Step 5: Commit**

```bash
git add deny.toml
git commit -m "ci: cargo-deny for licenses, advisories, and sources"
```

---

### Task 4: the CI workflow

**Files:**
- Create: `.github/workflows/ci.yml`

The workflow runs on each push to `main` and on each pull request. It has 4 jobs:
- **rust**, on Ubuntu, Windows, and macOS: fmt, clippy, and the tests, on Rust 1.99.1.
- **msrv**: `cargo +1.85 check --workspace --locked`.
- **mod**, on the 3 systems: Claude Code 2.1.300 validates the marketplace and the plugin, then runs the mod's tests.
- **deny**: `cargo deny check`.

- [ ] **Step 1: Write the workflow**

`.github/workflows/ci.yml`:

```yaml
name: ci

on:
  push:
    branches: [main]
  pull_request:

permissions:
  contents: read

# A newer push to the same branch cancels the run that is still going.
concurrency:
  group: ci-${{ github.ref }}
  cancel-in-progress: true

defaults:
  run:
    # bash on every runner, so a failing command in a multi-line step fails the step on Windows too.
    shell: bash

env:
  CARGO_TERM_COLOR: always
  # CI pins its toolchain, so a new Rust release cannot add a lint that fails a commit that passed.
  RUST_VERSION: "1.99.1"
  # The workspace's rust-version. The shipped code must build with it; the tests need a newer Rust.
  MSRV: "1.85"

jobs:
  rust:
    name: rust (${{ matrix.os }})
    runs-on: ${{ matrix.os }}
    timeout-minutes: 45
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, windows-latest, macos-latest]
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1  # v7.0.1
        with:
          persist-credentials: false
      - name: Install Rust
        run: |
          rustup toolchain install "$RUST_VERSION" --profile minimal --component rustfmt,clippy
          rustup default "$RUST_VERSION"
      - uses: Swatinem/rust-cache@d1527c8f67ef9af357290475d4af4b0fab8f12e0  # v2.9.3
      - name: Format
        run: cargo fmt --all --check
      - name: Lint
        run: cargo clippy --workspace --all-targets --locked -- -D warnings
      - name: Test
        run: cargo test --workspace --locked

  msrv:
    name: msrv (Rust 1.85, no tests)
    runs-on: ubuntu-latest
    timeout-minutes: 30
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1  # v7.0.1
        with:
          persist-credentials: false
      - name: Install Rust
        run: rustup toolchain install "$MSRV" --profile minimal
      - uses: Swatinem/rust-cache@d1527c8f67ef9af357290475d4af4b0fab8f12e0  # v2.9.3
      - name: Check
        # No --all-targets: wiremock, a test-only dependency, needs Rust 1.88.
        run: cargo +"$MSRV" check --workspace --locked

  mod:
    name: mod (${{ matrix.os }})
    runs-on: ${{ matrix.os }}
    timeout-minutes: 20
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, windows-latest, macos-latest]
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1  # v7.0.1
        with:
          persist-credentials: false
      - uses: actions/setup-node@949feb2413d6458794dcd2491c4babbbce0c15c1  # v7.1.0
        with:
          # Claude Code 2.1.300 needs Node 22 or later.
          node-version: "22"
      - name: Install Claude Code
        run: npm install -g @anthropic-ai/claude-code@2.1.300
      - name: Validate the marketplace and the plugin
        run: |
          claude plugin validate .
          claude plugin validate plugins/claude-code
      - name: The mod's tests
        run: claude plugin test plugins/claude-code

  deny:
    name: cargo deny
    runs-on: ubuntu-latest
    timeout-minutes: 15
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1  # v7.0.1
        with:
          persist-credentials: false
      - uses: EmbarkStudios/cargo-deny-action@3c6349835b2b7b196a839186cb8b78e02f7b5f25  # v2.1.1
        with:
          command: check
```

- [ ] **Step 2: Check it locally as far as this machine can**

GitHub runs the workflow in Task 7. Here, check:
1. The file parses as YAML: `python -c "import sys, yaml; yaml.safe_load(open(sys.argv[1]))" .github/workflows/ci.yml` exits 0. If `yaml` is not installed, skip this check and say so in your report; do not install it.
2. Every `uses:` line names a 40-character commit, with its tag in a comment: `grep -n "uses:" .github/workflows/ci.yml`.
3. Each job's commands pass here, run by hand: the 3 Rust gates; `cargo +1.85 check --workspace --locked` if `rustup toolchain list` shows Rust 1.85; `claude plugin validate .`, `claude plugin validate plugins/claude-code`, and `claude plugin test plugins/claude-code` (38 pass); and `cargo deny check`.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: Rust gates on 3 systems, the MSRV check, the mod's tests, and cargo deny"
```

---

### Task 5: the release build with `dist`

**Files:**
- Modify: `Cargo.toml`
- Create: `dist-workspace.toml`, `.github/workflows/release.yml` (written by `dist generate`)

- [ ] **Step 1: Add the `dist` profile**

Save this patch as `t5-cargo.patch` and apply it with `git apply t5-cargo.patch`:

```diff
diff --git a/Cargo.toml b/Cargo.toml
index 89653d0..173eb4a 100644
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -8,3 +8,8 @@ edition = "2021"
 license = "Apache-2.0"
 rust-version = "1.85"
 repository = "https://github.com/mattbran87/jev-daemon"
+
+# The profile that 'dist' will build with
+[profile.dist]
+inherits = "release"
+lto = "thin"
```

- [ ] **Step 2: Write the `dist` settings**

`dist-workspace.toml`:

```toml
[workspace]
members = ["cargo:."]

# Config for 'dist'
[dist]
# The preferred dist version to use in CI (Cargo.toml SemVer syntax)
cargo-dist-version = "0.33.1"
# CI backends to support
ci = "github"
# The installers to generate for each app
installers = ["shell", "powershell"]
# Target platforms to build apps for (Rust target-triple syntax)
targets = ["aarch64-apple-darwin", "aarch64-unknown-linux-gnu", "x86_64-apple-darwin", "x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc"]
# Path that installers should place binaries in
install-path = "CARGO_HOME"
# Where to host releases
hosting = "github"
# Whether to install an updater program
install-updater = false
# A pull request builds every archive and uploads them to its workflow run, with
# no release, so the release build is tested before a tag (plan 6).
pr-run-mode = "upload"

# Each action that the release workflow uses, pinned to a commit (spec section 8).
[dist.github-action-commits]
"actions/checkout" = "d23441a48e516b6c34aea4fa41551a30e30af803" # v6.1.0
"actions/upload-artifact" = "cf430e030ddbb5b0abf93d22962f4752f3646cd9" # v7.0.2
"actions/download-artifact" = "9000827ccba6bdab643e8b6fd33ac0654aef8333" # v8.0.2
```

- [ ] **Step 3: Write the release workflow**

Run: `dist generate`
Expected: `generated Github CI to ...\.github\workflows\release.yml`.

Check the result:
1. `sha256sum .github/workflows/release.yml` prints `8e547171f2deb074e72906573a6470cd55d4685797b11934367130760d0c6060`. That is the file the plan's checks produced from the same settings. If the hash differs, stop and report `dist --version` and the hash. Do not edit `release.yml` by hand: the release workflow checks that the file matches the settings.
2. Every `uses:` line names a commit: `grep -n "uses:" .github/workflows/release.yml` shows only the 3 pinned commits from "Choices this plan makes".
3. After `git add`, `git ls-files --eol .github/workflows/release.yml` shows `i/lf`.

- [ ] **Step 4: Check the archives**

Run: `dist plan`
Expected: one app, `jev 0.1.0`, with `jev-installer.sh`, `jev-installer.ps1`, and 5 archives. Each archive lists 2 binaries, `[bin] jev` and `[bin] jevd` (`jev.exe` and `jevd.exe` in the Windows `.zip`), plus `[misc] LICENSE`. No app named `jevd` appears.

- [ ] **Step 5: Run the gates and commit**

Run the 3 gates. The new `[profile.dist]` changes no other build. Expected: all exit 0.

```bash
git add Cargo.toml dist-workspace.toml .github/workflows/release.yml
git commit -m "release: dist builds one archive per target with jev and jevd"
```

---

### Task 6: the README and the spec

**Files:**
- Create: `README.md`
- Modify: `docs/superpowers/specs/2026-11-02-jev-daemon-design.md`

- [ ] **Step 1: Write the README**

`README.md`:

~~~markdown
# jev-daemon

jev lets local programs ask TypeSafe's Jev model while one background process, `jevd`, holds your TypeSafe keys. A program pairs once, and you approve it with `jev allow` in a terminal. `jevd` then checks each request for shape, rate limit, and secrets before it sends anything.

- `jevd`: the per-user daemon. Programs and the `jev` CLI start it when they need it, and it exits after 2 hours with no use.
- `jev`: the CLI. It manages keys, pairing, and settings, and `jev ask` sends a request.
- `plugins/claude-code`: a Claude Code plugin that gives other plugins `$.jev`.

**Status:** before the first release. No release archive exists yet.

## Build from source

You need Rust 1.85 or newer.

```bash
cargo install --locked --git https://github.com/mattbran87/jev-daemon jev
```

That installs `jev` and `jevd` into the same folder, `~/.cargo/bin`. `jevd` accepts admin commands only from the `jev` next to it, so keep the two together.

## Design

The design is in [docs/superpowers/specs/2026-11-02-jev-daemon-design.md](docs/superpowers/specs/2026-11-02-jev-daemon-design.md).

## License

Apache-2.0. See [LICENSE](LICENSE).
~~~

- [ ] **Step 2: Update the spec**

Make these changes in `docs/superpowers/specs/2026-11-02-jev-daemon-design.md`. Keep every other line as it is.

In section 1, "Repo layout", change 2 rows:
- `crates/jevd`: the kind becomes `library`. Add to its job: "Its binary is built from the `jev` package."
- `crates/jev`: the kind becomes `library and the binaries jev and jevd`. Its job becomes: "The CLI, and the `jevd` binary (`src/bin/jevd.rs`), so one release archive holds both."

In section 8, "Release", change these rows and add 2:

```
| Tool | `dist` 0.33.1, run by `.github/workflows/release.yml` on each version tag. Its settings sit in `dist-workspace.toml`; `dist generate` writes the workflow. |
| Archive | One app, the `jev` package, owns both binaries, so each archive holds `jev` and `jevd` in one folder. The peer check depends on that. |
| Installers | Shell and PowerShell, from `dist`. They install both binaries into `$CARGO_HOME/bin`. `cargo install --locked --git https://github.com/mattbran87/jev-daemon jev` also works and installs both. |
| Linux | glibc (`gnu`) builds on `ubuntu-22.04` and `ubuntu-22.04-arm`, so they need glibc 2.35 or newer. No musl build in v1. Every target builds on a native runner. |
| crates.io | Nothing is published in v1. |
| Pull requests | The release workflow builds every archive on a pull request and uploads them to the run, with no release (`pr-run-mode = "upload"`). |
```

In section 8, "CI on each push and pull request", replace the list with:

```
- Windows, Linux, and macOS: `cargo fmt --check`, `cargo clippy` with warnings as errors, `cargo test --locked`, on Rust 1.99.1. CI pins its toolchain, so a new Rust release cannot fail a commit that passed.
- The MSRV, Rust 1.85: `cargo check --workspace --locked`, with no test targets, because `wiremock`, a test-only dependency, needs Rust 1.88.
- The mod, on the 3 systems, with Claude Code 2.1.300: `claude plugin validate` on the marketplace and the plugin, and `claude plugin test`. `tsc` runs locally only: `/plugin-types` works only in an interactive session, and its file is Claude Code's own text, so the repo does not hold it.
- `cargo deny` for licenses, security advisories, bans, and sources, with `deny.toml`. It ignores 3 "unmaintained" notices for the `zbus` 4 crates (RUSTSEC-2026-0370, -0371, -0372), each with its reason, until the MSRV allows `keyring` 4.
- Every action pinned to a commit SHA, in both workflows. `Cargo.lock` committed.
- A test in `crates/jev/tests/versions.rs` checks that the plugin's version is the crates' version, and that the mod's `MIN_JEV` is no newer.
```

In section 12, change 3 bullets:
- "Linux keyring backend": replace its last sentence, from "so plan 6 still picks", with: "Plan 6 picked a glibc build on native runners, so no cross compiler is needed."
- "Crate names": add: "Plan 6 publishes nothing in v1, so no name is claimed yet. If a later release publishes, `jevctl` is free for the CLI crate (checked 2026-11-04), and the binary stays `jev`."
- "Rate limit default": add: "v0.1.0 keeps `120/60`; the measurement stays open for a later release."

Run the STE lint on the spec, as the `simple-english-docs` skill says, and fix every ERROR in the lines you wrote.

- [ ] **Step 3: Commit**

```bash
git add README.md docs/superpowers/specs/2026-11-02-jev-daemon-design.md
git commit -m "docs: README, and record plan 6 choices in the spec"
```

---

### Task 7: the GitHub repo, and both workflows on GitHub

This task acts outside the machine: it creates the public repo and pushes to it. The person chose a public repo on 2026-11-04. Do nothing beyond the steps below, and never push a tag.

- [ ] **Step 1: Check the account and that the repo does not exist yet**

Run: `gh auth status`. Expected: signed in as `mattbran87`, with the `repo` and `workflow` scopes.
Run: `gh repo view mattbran87/jev-daemon`. Expected: "Could not resolve to a Repository". If the repo exists, stop and report it.

- [ ] **Step 2: Create the repo and push `main`**

`main` holds plans 1 to 5, with no workflow yet, so this push starts no run.

Run from the main checkout, not the worktree:

```bash
gh repo create mattbran87/jev-daemon --public --description "jev: a per-user daemon and CLI that hold TypeSafe keys and send checked requests to Jev" --source . --remote origin
git push -u origin main
```

Expected: the repo exists, and `git ls-remote origin main` shows the same commit as `git rev-parse main`.

- [ ] **Step 3: Push the plan 6 branch and open a pull request**

From the worktree:

```bash
git push -u origin HEAD
gh pr create --base main --title "Distribution and CI (plan 6)" --body "CI on Windows, Linux, and macOS; one release archive with jev and jevd; cargo-deny. The release workflow runs in upload mode on this pull request, with no release."
```

- [ ] **Step 4: Watch both workflows**

Run: `gh pr checks --watch`. It ends when every check finishes.
Expected: every check passes: `rust` on 3 systems, `msrv`, `mod` on 3 systems, `cargo deny`, and the release workflow's `plan`, its 5 build jobs, and its `build-global-artifacts` job.

If a check fails:
1. Read its log: `gh run view <run id> --log-failed`.
2. Fix the cause in the worktree, with a test where a test can show it. Run the local gates. Commit, and push the branch again.
3. Repeat until every check passes. Report each failure and its fix.

Do not merge the pull request: the person decides that after the final review.

- [ ] **Step 5: Check one archive**

Run: `gh run download <the release run's id> --name artifacts-build-local-x86_64-pc-windows-msvc --dir <scratchpad>/archive`. If the name differs, list the artifacts with `gh run view <id>` and pick the Windows one.
Expected: `jev-x86_64-pc-windows-msvc.zip` holds `jev.exe`, `jevd.exe`, and `LICENSE`. Unzip it into the scratchpad. Run `jev --version` and `jevd --version` from it: `jev 0.1.0` and `jevd 0.1.0`. Delete the downloaded files afterward.

- [ ] **Step 6: Record the result**

Write the run IDs, the checks, and any fixes in your report. Nothing to commit, unless Step 4 needed fixes.

---

## Not tested by this plan

- **The tag and the published release.** Plan 6 stops before `v0.1.0`. The first tag runs the release workflow's `host` and `announce` jobs, which no pull request runs.
- **The installers.** A pull request builds `jev-installer.sh` and `jev-installer.ps1`, but they download from a release, which does not exist yet.
- **macOS signing.** The Apple Silicon archive gets the linker's ad hoc signature only. A check by hand on a real Mac after the first release.
- **`tsc` in CI.** See "What the checks showed".
- **The real OS keyring on Linux and macOS.** The 1 ignored test covers it, and CI runs no keyring service.

## Notes for the next plans

- The docs plan (spec section 9) writes `install.md` with the installer commands and the SmartScreen and Keychain warnings.
- Before the tag: the docs, a `CHANGELOG.md` entry for 0.1.0 (`dist` uses it for the release notes), and the person's go-ahead.
- When the MSRV reaches 1.88: `keyring` 4, and drop the 3 ignored advisories in `deny.toml`.
