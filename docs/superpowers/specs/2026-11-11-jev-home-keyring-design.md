# A separate keyring service under `JEV_HOME`

Issue #7. `JEV_HOME` moves every jev file and the channel into one folder. But `jev` and `jevd` store keys (`profile:<name>`) and client tokens (`client:<name>`) in the OS keyring under one fixed service, `jev` (`crates/jevd/src/keystore.rs`). So `jev key add` under a test `JEV_HOME` replaces the person's real key, with no notice. This spec gives each `JEV_HOME` folder its own keyring service.

The automated tests do not have this problem. Every test that adds a key or pairs a client uses `MemoryKeyring`, and the tests that run the real binaries never touch the keyring. The risk is a person who sets `JEV_HOME` by hand, for example to try a build next to the installed jev.

## 1. The service name

`Paths` gets one new method, `keyring_service()`:

| `JEV_HOME` | Service |
| --- | --- |
| Not set | `jev`, the same as today |
| Set | `jev-` and 16 lower-case hex digits: the FNV-1a hash of the folder |

The hash is the one that names the Windows pipe today, over a key for the folder:

- **Windows.** The key stays as it is: the UTF-16 units of the absolute folder, with ASCII letters in lower case, `/` as `\`, and no trailing separator. The pipe name does not change.
- **Unix.** The key is the bytes of the absolute folder path, with no trailing `/`. The key keeps letter case, because Unix paths keep it.

`fnv1a` and the folder key leave their `#[cfg(windows)]` blocks, so every platform uses one hash function. `Paths::under` already makes the folder absolute, so a relative `JEV_HOME` and its absolute form give the same service.

## 2. The keyring

`OsKeyring` changes from a unit struct to a struct that holds its service: `OsKeyring::new(service)`. `entry()` uses that service. The constant `SERVICE` stays as the name of the default service, and `keyring_service()` returns it when `JEV_HOME` is not set.

Two places build an `OsKeyring`, and both pass `paths.keyring_service()`:

- `jev`, in `Env::from_os` (`crates/jev/src/env.rs`).
- `jevd`, in `serve` (`crates/jev/src/bin/jevd.rs`).

`jevd` gets `JEV_HOME` from the `jev` that starts it (`client::start_daemon` passes `paths.home`), so both programs use the same service. A person who starts `jevd` by hand must set the same `JEV_HOME` for both, as the `HOME_VAR` comment already says.

`MemoryKeyring` and every test that uses it do not change.

## 3. What a person sees

- **No `JEV_HOME`:** no change. Keys and tokens stay under the service `jev`.
- **`JEV_HOME` set:** keys and client tokens go to that folder's own service. The real entries under `jev` are never read or written.
- **A `JEV_HOME` folder that held keys before this change** looks empty after the update. Its `pairings.json` still lists the clients, but `jev ask` finds no token for them. The person must run `jev key add` again and pair each client again. This includes Claude Code plugin clients, because `jev ask` reads every client's token from the keyring.
- **Old entries stay.** Entries that a test folder wrote stay in the keyring after the person deletes the folder. To delete them, run `jev key remove` with the same `JEV_HOME`, before you delete the folder. Nothing deletes them automatically.

## 4. Docs

- The `HOME_VAR` comment in `crates/jev-proto/src/paths.rs` names the keyring service, and says that its entries outlive the folder.
- The `JEV_HOME` row in `docs/superpowers/specs/2026-11-02-jev-daemon-design.md` says the same.
- `CHANGELOG.md` gets an **Unreleased** entry with the change and the re-add step from section 3.
- The README does not mention `JEV_HOME`, so it does not change.

The version and the release are not part of this spec. The person decides when the next release ships.

## 5. Tests

- `keyring_service()` returns `jev` with no `JEV_HOME`.
- With `JEV_HOME`, it returns `jev-` and 16 lower-case hex digits.
- 2 different folders give 2 different services.
- On Windows, another spelling of the same folder (letter case, `/` for `\`, a trailing `\`) gives the same service. On Unix, a trailing `/` gives the same service.
- On Windows, the pipe name does not change: a test pins one known folder to its known pipe hash, computed before the change.
- The ignored real-keyring test in `keystore.rs` uses `OsKeyring::new("jev-test")`, so a hand run never touches the service `jev`.

## 6. Out of scope

- A command that lists or deletes the entries of old test services.
- A move of existing entries from `jev` to a `JEV_HOME` service. Such a copy would put the real key in the test service, which defeats the isolation.
- The Claude Code plugin and `jev-tray`. Neither one touches the keyring.
