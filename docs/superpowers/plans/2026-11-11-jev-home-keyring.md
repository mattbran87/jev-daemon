# A separate keyring service under `JEV_HOME`: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** With `JEV_HOME` set, `jev` and `jevd` keep keys and client tokens under the keyring service `jev-<16 hex digits>`, so a test folder never touches the real entries under `jev`.

**Architecture:** `Paths` (in `jev-proto`) already knows the `JEV_HOME` folder and hashes it for the Windows pipe name. It gets one new method, `keyring_service()`, which uses the same hash on every platform. `OsKeyring` (in `jevd`) holds a service name, and both programs build it from `paths.keyring_service()`.

**Tech stack:** Rust 2021 workspace, the `keyring` 3 crate, `cargo test`, `cargo clippy`, `cargo fmt`.

**Spec:** `docs/superpowers/specs/2026-11-11-jev-home-keyring-design.md`.

**Branch:** `fix/jev-home-keyring` (it already holds the spec commit).

**Commits:** use this identity and trailer for every commit in this plan:

```bash
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "<subject>" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

The repository is public. Never put private names, paths, or IDs in code, docs, or commit messages.

---

## Files

| File | Change |
| --- | --- |
| `crates/jev-proto/src/paths.rs` | `KEYRING_SERVICE`, `Paths::keyring_service()`, a shared `fnv1a`, `home_hash` for each platform, the `HOME_VAR` comment, tests |
| `crates/jevd/src/keystore.rs` | `SERVICE` comes from `jev_proto::paths::KEYRING_SERVICE`; `OsKeyring` holds its service; the ignored test uses `jev-test` |
| `crates/jev/src/env.rs` | `Env::from_os` builds `OsKeyring::new(paths.keyring_service())` |
| `crates/jev/src/bin/jevd.rs` | `serve` builds `OsKeyring::new(paths.keyring_service())` |
| `docs/superpowers/specs/2026-11-02-jev-daemon-design.md` | the `JEV_HOME` row names the keyring service |
| `CHANGELOG.md` | an **Unreleased** entry |

---

### Task 1: Pin the Windows pipe name before the change

The hash code moves in Task 2. This test proves that the move does not change the pipe name of an existing `JEV_HOME` folder. It passes before Task 2 and must still pass after it.

The value `2b614bce24bfacb3` is FNV-1a over the UTF-16 little-endian bytes of `c:\jev-pin`, which is the folder key of `C:\jev-pin`.

**Files:**
- Modify: `crates/jev-proto/src/paths.rs` (the `tests` module, after `the_pipe_name_ignores_case_and_trailing_slashes`)

- [ ] **Step 1: Add the pin test**

Put this test right after `the_pipe_name_ignores_case_and_trailing_slashes`:

```rust
    #[cfg(windows)]
    #[test]
    fn the_pipe_name_of_a_known_home_does_not_change() {
        let paths = Paths::under(Path::new(r"C:\jev-pin")).unwrap();
        let expected = format!(
            r"\\.\pipe\jev-{}-2b614bce24bfacb3",
            crate::os::user_sid().unwrap()
        );
        assert_eq!(paths.socket, PathBuf::from(expected));
    }
```

- [ ] **Step 2: Run it on the current code**

Run: `cargo test -p jev-proto --locked the_pipe_name_of_a_known_home_does_not_change`
Expected: PASS on Windows. On Linux and macOS, the test does not exist, and `0 passed` is correct.

If it fails, stop. The pinned value is wrong, and Task 2 has nothing to check against. Report the actual pipe name.

- [ ] **Step 3: Commit**

```bash
git add crates/jev-proto/src/paths.rs
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "test(jev-proto): pin the pipe name of a known JEV_HOME" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: `Paths::keyring_service()`

**Files:**
- Modify: `crates/jev-proto/src/paths.rs`

- [ ] **Step 1: Write the failing tests**

Add these tests to the `tests` module, after `two_homes_get_two_channels`:

```rust
    #[test]
    fn no_home_uses_the_default_keyring_service() {
        let paths = Paths::from_vars(vars(&[
            ("HOME", "/home/u"),
            ("APPDATA", r"C:\r"),
            ("LOCALAPPDATA", r"C:\l"),
        ]))
        .unwrap();
        assert_eq!(paths.keyring_service(), "jev");
        assert_eq!(KEYRING_SERVICE, "jev");
    }

    #[test]
    fn a_home_gets_its_own_keyring_service() {
        let service = Paths::under(Path::new("/a")).unwrap().keyring_service();
        let digits = service.strip_prefix("jev-").expect("starts with jev-");
        assert_eq!(digits.len(), 16, "{service}");
        assert!(
            digits.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
            "{service}"
        );
    }

    #[test]
    fn two_homes_get_two_keyring_services() {
        let a = Paths::under(Path::new("/a")).unwrap();
        let b = Paths::under(Path::new("/b")).unwrap();
        assert_ne!(a.keyring_service(), b.keyring_service());
    }

    #[cfg(windows)]
    #[test]
    fn the_keyring_service_ignores_case_and_trailing_slashes() {
        let service = |home: &str| Paths::under(Path::new(home)).unwrap().keyring_service();
        assert_eq!(service(r"C:\x"), service(r"c:\X"));
        assert_eq!(service(r"C:\x"), service(r"C:\x\"));
        assert_eq!(service(r"C:\x"), service("C:/x/"));
        assert_eq!(service(r"C:\jev-pin"), "jev-2b614bce24bfacb3");
    }

    #[cfg(unix)]
    #[test]
    fn the_keyring_service_ignores_a_trailing_slash() {
        let service = |home: &str| Paths::under(Path::new(home)).unwrap().keyring_service();
        assert_eq!(service("/x"), service("/x/"));
        assert_ne!(service("/x"), service("/X"));
        // FNV-1a over the bytes of "/jev-pin".
        assert_eq!(service("/jev-pin"), "jev-9978e02a32614065");
    }
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p jev-proto --locked keyring_service`
Expected: FAIL to compile, with `no method named keyring_service` and `cannot find value KEYRING_SERVICE`.

- [ ] **Step 3: Add `KEYRING_SERVICE`**

Put this right after the `HOME_VAR` constant:

```rust
/// The OS keyring service for keys and client tokens when `JEV_HOME` is not
/// set. `Paths::keyring_service` gives the service for a `JEV_HOME` folder.
pub const KEYRING_SERVICE: &str = "jev";
```

- [ ] **Step 4: Add `keyring_service()`**

Put this method in `impl Paths`, right after `pid_file`:

```rust
    /// The OS keyring service for keys and client tokens: `jev`, or under
    /// `JEV_HOME`, `jev-` and the 16-digit hash of the folder. A test folder
    /// then never reads or writes the real entries.
    pub fn keyring_service(&self) -> String {
        match &self.home {
            None => KEYRING_SERVICE.to_string(),
            Some(home) => format!("jev-{:016x}", home_hash(home)),
        }
    }
```

- [ ] **Step 5: Share the hash between the platforms**

In `Paths::under`, change the pipe name to use `home_hash`:

```rust
        #[cfg(windows)]
        let socket = PathBuf::from(format!(
            r"\\.\pipe\jev-{}-{:016x}",
            crate::os::user_sid()?,
            home_hash(home)
        ));
```

Replace the whole `fnv1a` function, with its comment and its `#[cfg(windows)]` line, by this code:

```rust
/// A short, stable name for a `JEV_HOME` folder: FNV-1a, 64 bits, over the
/// folder key. Windows hashes the two bytes of each UTF-16 unit of
/// `home_key`, low byte first. Not a security measure; the pipe's access
/// list and the keyring's own access rules are.
#[cfg(windows)]
fn home_hash(home: &Path) -> u64 {
    fnv1a(home_key(home).iter().flat_map(|unit| unit.to_le_bytes()))
}

/// Unix hashes the bytes of the folder path, without trailing `/`. Unix
/// paths keep letter case, so the hash keeps it too.
#[cfg(unix)]
fn home_hash(home: &Path) -> u64 {
    use std::os::unix::ffi::OsStrExt;
    let mut bytes = home.as_os_str().as_bytes();
    while bytes.len() > 1 && bytes.ends_with(b"/") {
        bytes = &bytes[..bytes.len() - 1];
    }
    fnv1a(bytes.iter().copied())
}

fn fnv1a(bytes: impl IntoIterator<Item = u8>) -> u64 {
    bytes
        .into_iter()
        .fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        })
}
```

`home_key` stays `#[cfg(windows)]` and does not change.

- [ ] **Step 6: Run the tests to see them pass**

Run: `cargo test -p jev-proto --locked`
Expected: PASS. The pin test from Task 1, `the_pipe_name_of_a_known_home_does_not_change`, still passes. On Windows, the Unix test does not run; CI runs it on Linux and macOS.

- [ ] **Step 7: Run clippy and fmt**

Run: `cargo fmt --all` then `cargo clippy -p jev-proto --all-targets --locked -- -D warnings`
Expected: no warnings.

- [ ] **Step 8: Commit**

```bash
git add crates/jev-proto/src/paths.rs
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "feat(jev-proto): a keyring service for each JEV_HOME folder" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: `OsKeyring` uses the service from `Paths`

**Files:**
- Modify: `crates/jevd/src/keystore.rs`
- Modify: `crates/jev/src/env.rs` (`Env::from_os`)
- Modify: `crates/jev/src/bin/jevd.rs` (`serve`)

- [ ] **Step 1: Change the ignored real-keyring test first**

In `crates/jevd/src/keystore.rs`, in `the_os_keyring_round_trips`, change:

```rust
        let store = OsKeyring;
```

to:

```rust
        // A test service, so a hand run never touches the entries under `jev`.
        let store = OsKeyring::new("jev-test");
```

- [ ] **Step 2: Check that it fails to compile**

Run: `cargo test -p jevd --locked --no-run`
Expected: FAIL to compile, with `no function or associated item named new found for struct OsKeyring`.

- [ ] **Step 3: Give `OsKeyring` its service**

Change the `SERVICE` constant to:

```rust
/// The service when `JEV_HOME` is not set. `Paths::keyring_service` picks
/// the service that `jev` and `jevd` use.
pub const SERVICE: &str = jev_proto::paths::KEYRING_SERVICE;
```

Replace the `OsKeyring` struct and the `entry` function with:

```rust
/// The real keyring: Windows Credential Manager, the macOS Keychain, or the
/// Secret Service on Linux, under one service name. A call can block on a
/// keyring prompt, so async code runs it on a blocking thread.
pub struct OsKeyring {
    service: String,
}

impl OsKeyring {
    /// The keyring entries under `service`. Give it `Paths::keyring_service`.
    pub fn new(service: impl Into<String>) -> OsKeyring {
        OsKeyring {
            service: service.into(),
        }
    }

    fn entry(&self, account: &str) -> Result<keyring::Entry, StoreError> {
        keyring::Entry::new(&self.service, account).map_err(|error| store_error(&error))
    }
}
```

In `impl KeyStore for OsKeyring`, change each of the 3 calls `entry(account)?` to `self.entry(account)?`.

- [ ] **Step 4: Pass the service in `jev`**

In `crates/jev/src/env.rs`, in `Env::from_os`, change:

```rust
            store: Arc::new(OsKeyring),
```

to:

```rust
            store: Arc::new(OsKeyring::new(service)),
```

`paths` moves into the struct on the line `paths,`, before the `store` field, so get the service first. Put this line right before `Ok(Env {`:

```rust
        let service = paths.keyring_service();
```

- [ ] **Step 5: Pass the service in `jevd`**

In `crates/jev/src/bin/jevd.rs`, in `serve`, change:

```rust
        store: Arc::new(OsKeyring),
```

to:

```rust
        store: Arc::new(OsKeyring::new(paths.keyring_service())),
```

- [ ] **Step 6: Find every other user**

Run: `git grep -n "OsKeyring" -- crates`
Expected: only `keystore.rs`, `crates/jev/src/env.rs`, and `crates/jev/src/bin/jevd.rs`. Each use outside the struct definition calls `OsKeyring::new`. If another file uses the bare `OsKeyring`, stop and report it. The spec names only these 2 programs.

- [ ] **Step 7: Run the whole workspace**

Run: `cargo fmt --all`, then `cargo clippy --workspace --all-targets --locked -- -D warnings`, then `cargo test --workspace --locked`
Expected: no warnings, and every test passes. The count of ignored tests stays at 1.

- [ ] **Step 8: Commit**

```bash
git add crates/jevd/src/keystore.rs crates/jev/src/env.rs crates/jev/src/bin/jevd.rs
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "fix: keep JEV_HOME keys and tokens out of the real keyring" -m "Closes #7." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Docs

Use the `simple-english:simple-english-docs` skill for these edits. Each is a partial edit, so match the words each file already uses.

**Files:**
- Modify: `crates/jev-proto/src/paths.rs` (the `HOME_VAR` comment)
- Modify: `docs/superpowers/specs/2026-11-02-jev-daemon-design.md` (the `JEV_HOME` row)
- Modify: `CHANGELOG.md`

- [ ] **Step 1: The `HOME_VAR` comment**

Replace the comment above `pub const HOME_VAR` with:

```rust
/// When set, every jev file and the channel go under this folder. This is a
/// test hook: tests use it, so they never meet a real daemon. The `JEV_HOME`
/// folder must be private to the user (on Unix, `check_runtime_dir` checks
/// only its `run` folder), and `jevd` and every client must see the same folder.
/// Keys and client tokens go to the OS keyring under the folder's own
/// service (`Paths::keyring_service`). Those entries outlive the folder: run
/// `jev key remove` with the same `JEV_HOME` before you delete it.
```

- [ ] **Step 2: The spec row**

In `docs/superpowers/specs/2026-11-02-jev-daemon-design.md`, replace the `JEV_HOME` row with:

```markdown
| `JEV_HOME` | A test hook. When set, every file and the channel go under this folder. The `JEV_HOME` folder must be private to the user. On Unix, only its `run` folder is checked. On Windows, the pipe name includes a 16-digit hash of the folder. The hash ignores ASCII case and trailing separators. Keys and client tokens go to the keyring service `jev-` plus a 16-digit hash of the folder, in place of `jev`. On Unix, that hash keeps letter case. These keyring entries stay after the person deletes the folder. |
```

- [ ] **Step 3: The CHANGELOG**

In `CHANGELOG.md`, put this section right after the `# Changelog` line, before `## 0.1.2`:

```markdown
## Unreleased

- With `JEV_HOME` set, `jev` and `jevd` keep keys and client tokens under the keyring service `jev-` plus a 16-digit hash of the folder. Before, they used the service `jev`, so `jev key add` under a test `JEV_HOME` replaced the real key. Without `JEV_HOME`, nothing changes.
- A `JEV_HOME` folder that held keys before this change looks empty after the update. Run `jev key add` again, and pair each client again.
- The entries under a `JEV_HOME` service stay in the keyring after you delete the folder. To delete them, run `jev key remove` with the same `JEV_HOME` before you delete the folder.
```

- [ ] **Step 4: Run the style linter on the 2 Markdown files**

Run, with `<base>` as the base directory of the `simple-english-docs` skill:
`python "<base>/scripts/ste_lint.py" CHANGELOG.md docs/superpowers/specs/2026-11-02-jev-daemon-design.md`
Expected: no ERROR on the lines this task changed. Hits on other lines existed before and stay.

- [ ] **Step 5: Check the line ends**

Run: `git ls-files --eol CHANGELOG.md docs/superpowers/specs/2026-11-02-jev-daemon-design.md crates/jev-proto/src/paths.rs`
Expected: each file keeps the `i/` value it had before. `core.autocrlf` is `true`.

- [ ] **Step 6: Commit**

```bash
git add crates/jev-proto/src/paths.rs docs/superpowers/specs/2026-11-02-jev-daemon-design.md CHANGELOG.md
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "docs: JEV_HOME has its own keyring service" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Final check

- [ ] **Step 1: The CI commands, in order**

Run:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo check --workspace --locked
```

Expected: no format change, no warnings, every test passes, 1 ignored.

- [ ] **Step 2: Check the branch**

Run: `git log --oneline main..HEAD` and `git status --short`
Expected: 5 commits (the spec, then Tasks 1 to 4) and a clean tree. `plugins/claude-code/tsconfig.json` no longer shows, because `.gitignore` ignores it.

- [ ] **Step 3: A hand check on Windows, only with the person's approval**

This check writes to the real Windows Credential Manager, under a test service only. Ask the person first. Use the debug build in `target\debug`.

Preview the entries before the check:

```powershell
cmdkey /list | Select-String 'jev'
```

Read that output, and note every entry that exists now. Then run:

```powershell
$env:JEV_HOME = "$env:TEMP\jev-hand-check"
'tsk-not-a-real-key' | .\target\debug\jev.exe key add --stdin --profile handcheck
cmdkey /list | Select-String 'jev'
.\target\debug\jev.exe key remove --profile handcheck --force
.\target\debug\jev.exe stop
Remove-Item Env:JEV_HOME
```

Expected: after the add, one new entry names both `profile:handcheck` and a `jev-` service with 16 hex digits. No entry under the plain service `jev` changes. After the remove, the list matches the preview again. If `jev key add` refuses the fake key, the check still shows which service it used; report the output.

Push, the pull request, and the merge wait for the person's approval.
