# jev-daemon — design

Date: 2026-11-02. Status: approved in brainstorm, section by section.

## Summary

jev-daemon is the Rust v1 of jev. One background process per user, `jevd`, holds the person's TypeSafe keys and sends requests to TypeSafe's Jev model for any local caller: scripts, desktop apps, editor extensions, other LLM tools, and Claude Code plugins. A caller must pair once, and a person must approve the pairing on a terminal. `jevd` then checks each request for shape, token, rate limit, and secrets before it sends anything.

The Python prototype, jev-service, stays private. It serves as the reference for the checks, the tests, and the docs. Its 0.1.0 never ships, so no public user builds on a contract that v1 breaks.

## Decisions

| # | Decision |
| --- | --- |
| 1 | Goal: a standalone service for any caller, plus the performance and memory safety of Rust. Not tied to Claude Code. |
| 2 | One per-user daemon, `jevd`. Clients and the CLI start it on demand. No login service. |
| 3 | Consent: a client pairs once and gets a revocable token. No `sends` text. |
| 4 | Approval surface in v1: interactive `jev allow` on a terminal. One-step `jev pair` on a terminal also approves. No OS dialog. The Claude Code mod is not an approval surface. |
| 5 | Keys live in the OS keyring, as named profiles. Each client binds to one profile, picked by the person at approval. |
| 6 | Key commands: `jev key add`, `jev key remove`, `jev key list`. No account sign-in exists. |
| 7 | No idle exit by default: `jevd` uses little memory and no CPU while it waits. The person can set a period. |
| 8 | `jevd` caches each key after its first use and wipes it after 2 hours with no use of that key. The cache period cannot be `off`. |
| 9 | One request in flight per connection. Parallel work uses more connections. |
| 10 | Admin messages are accepted only from the exact full path of the `jev` binary next to `jevd`. |
| 11 | Rate limit: 120 requests per 60 seconds per client by default, with a per-client override. No per-profile limit. |
| 12 | Changes that loosen protection need a terminal and a typed `y`. Changes that tighten it run anywhere. |
| 13 | `jev ask` takes JSON on stdin or flags for simple calls. |
| 14 | Claude Code plugins pair one client per plugin, named `cc-<plugin>`. |
| 15 | Release: prebuilt binaries on GitHub Releases, built by `dist`. No paid code signing in v1. First tag `v0.1.0`. |
| 16 | Docs: the prototype's Zensical setup, with the built site committed in `site/`. |
| 17 | New repo `jev-daemon`. Author email `mattbran87@users.noreply.github.com`. |
| 18 | `jevd` commits a new client, and revokes an old client of the same name, only when the new token reaches its storage. For a CLI client, `jev allow` writes the token to the keyring itself. |
| 19 | A client reads an error code it does not know as an unknown failure, never as an unreadable reply. |

### Rejected options

- **A per-session bridge, as in the prototype.** It ties jev to Claude Code sessions. Decision 1 rules it out.
- **The `sends` text.** The client declares it, and `jevd` cannot check it. It catches honest changes only.
- **A native OS dialog for approval.** It needs GUI code per OS, fails on a headless machine, and a computer-use agent can click it.
- **The mod as an approval surface.** The mod runs inside the Claude Code process, so its approval fails the peer check. A peer check on the Claude Code binary is fragile: its path changes with the install method, and an npm install runs as `node`.
- **Binding consent to the caller's executable.** Every Node script shows up as `node`, and every Python script as `python`.
- **Many requests per connection, matched by ID.** It gives no speed gain, because the Jev HTTPS call dominates each request. It can come later as protocol version 2.
- **HMAC on every message.** With a user-only channel and a pipe-squatting check, HMAC adds nothing. Its secret would sit in a file that any same-user process can read.
- **A shell guard in the mod.** It protects Claude Code only, and it sees only the outer command. Decision 12 covers every agent.

## 1. Architecture

### Repo layout

| Path | Kind | Job |
| --- | --- | --- |
| `crates/jev-proto` | library | Wire types, the protocol version, and a small client that connects and starts `jevd` when needed. No tokio or reqwest dependency. |
| `crates/jevd` | library | Socket server, pairing store, checks, keyring read, Jev HTTPS client. Its binary is built from the `jev` package. |
| `crates/jev` | library and the binaries jev, jevd, and jev-tray | The CLI, the `jevd` binary (`src/bin/jevd.rs`), and the `jev-tray` binary (`src/bin/jev-tray.rs`), so one release archive holds all three. The `tray` feature, on by default, builds `jev-tray`. |
| `crates/jev-tray` | library | The tray icon that shows whether `jevd` runs. The `jev-tray` binary in `crates/jev` calls it. It depends on `jev-proto`, not on `jevd`. Its design is in `2026-11-04-jev-tray-design.md`. |
| `plugins/claude-code` | TypeScript mod | `$.jev` for other mods, and toasts |

### Storage

| Item | Location |
| --- | --- |
| TypeSafe keys | OS keyring, service `jev`, account `profile:<name>` |
| Tokens of CLI clients | OS keyring, service `jev`, account `client:<name>` |
| Settings and profile names | TOML file in the platform configuration directory. Written by the CLI. Never holds a key. |
| Pairings | JSON file in the platform configuration directory. Written only by `jevd`. Holds token hashes, never tokens. |
| Waiting pairing requests | `pending.json` in the platform state directory. Written only by `jevd`. Holds no token and no request ID. See `2026-11-07-pairing-restart-design.md`. |
| Socket and PID file | Platform runtime directory |
| Logs | Platform state directory. They record daemon events (start, stop, idle exit, signals, failures), and never hold a key, a token, or request text. One line per daemon event. Control characters, line separators, and bidi characters become spaces. On Unix the log file has mode 0600. |

The `keyring` crate cannot list entries on every OS, so profile names also go in the settings file.

## 2. Daemon and transport

### Lifecycle

1. **Start.** A client or `jev start` launches `jevd`, detached from the terminal: no console window on Windows, a new session on Unix.
2. **Bind.** `jevd` creates the socket or pipe. If another daemon already holds it, the new one exits at once, with success. A client that started `jevd` retries the connection for up to 10 seconds before it reports `daemon-unavailable`.
3. **Load.** `jevd` reads the settings file and the pairing file. It reads no key yet.
4. **Serve.** One tokio task handles each connection.
5. **Idle exit.** After `idle-timeout` with no request, `jevd` stops.
6. **Stop.** On `jev stop`, idle exit, logoff, or a termination signal, `jevd` refuses new connections. It lets requests in flight finish, for at most 45 seconds. A new connection in this period gets `daemon-unavailable` ("jevd is stopping") after `jevd` reads its first line. Then `jevd` wipes all cached keys, deletes the socket file, and exits. On Unix it empties the PID file and never deletes it. On Windows it deletes the PID file. A detached `jevd` on Windows gets no console events, so it ends with the session.

### Key cache

- `jevd` reads a profile's key from the keyring at that profile's first request, then keeps it in memory.
- `jevd` wipes the key after `key-cache` with no use of that key. It also wipes keys on `reload`, on `jev key remove`, and on exit.
- The cache limits keyring prompts, such as a locked Linux keyring or a macOS Keychain access prompt, to one per profile per cache period.
- `jevd` reads a key off the cache lock, on a blocking thread, and only one read per profile runs at a time.
- A timer wipes a key at most 60 seconds after its period ends. Each lookup also checks the period, so `jevd` never uses a key past it.
- The key-cache and idle timers use a monotonic clock. On Linux and macOS, this clock does not count system sleep, so after a laptop sleeps, each period continues where it stopped. On Windows, the clock can count the sleep.

### Transport

| Item | Design |
| --- | --- |
| Channel | Linux: `$XDG_RUNTIME_DIR/jev/jevd.sock`. macOS: `$TMPDIR/jev/jevd.sock`. Both folders mode 0700. Windows: `\\.\pipe\jev-<user SID>`, with an access list for the current user only. |
| Pipe squatting | `jevd` creates the pipe with `FILE_FLAG_FIRST_PIPE_INSTANCE`, and with an access list that names the user as owner and gives only that user access. The client opens the pipe with `SECURITY_IDENTIFICATION`, so a squatter cannot impersonate the client. Then the client checks that the current user owns the pipe. |
| Framing | One JSON object per line. A request line holds at most 1 MiB, newline not counted; a longer line gets `too-large`, and the connection closes. `jev_proto::MAX_LINE` holds this cap. The client reads reply lines of at most 2 MiB. A longer reply is `internal`. |
| Concurrency | One request in flight per connection. `max-connections` caps connections. A connection over the cap gets `daemon-unavailable` ("too many connections") after `jevd` reads its first line, and then closes. |
| Idle connection | `jevd` closes a connection after 60 seconds with no message. This timer runs only while `jevd` waits for a message, never while a request runs. A reply write that takes more than 10 seconds also closes the connection. |
| Version | The first message is `hello`, with the protocol version. `jevd` refuses a version it does not support. |
| Peer check | `jevd` takes the full path of the `jev` next to it at start. It accepts admin messages only from a peer process with that exact path, through the OS peer process ID. On Linux and macOS, `jevd` compares the peer's path in the form that the kernel gives. On Windows, it compares the canonical path. |
| Single writer | Only `jevd` writes the pairing file. `jev key add` writes the keyring directly, then sends `reload`, so a key never passes through the socket. |
| Single instance | Unix: an exclusive `flock` on `jevd.pid` before the bind. `jevd` never deletes this file, so the lock is always on one file. Windows: the first pipe instance. A second daemon exits with success. |
| `JEV_HOME` | A test hook. When set, every file and the channel go under this folder. The `JEV_HOME` folder must be private to the user. On Unix, only its `run` folder is checked. On Windows, the pipe name includes a 16-digit hash of the folder. The hash ignores ASCII case and trailing separators. Keys and client tokens go to the keyring service `jev-` plus a 16-digit hash of the folder, in place of `jev`. On Unix, that hash keeps letter case. These keyring entries stay after the person deletes the folder. |
| Client | `hello` waits 5 seconds for its reply. A call waits 120 seconds. `connect_or_start` starts `jevd` only when there is no channel. It does not start one after an error that a daemon sent, a busy pipe, a failed owner check, or a `hello` timeout. It waits up to 10 seconds for the new daemon. If that `jevd` exits with a failure at start, the client reports its exit status. The client gives `JEV_HOME` from its `Paths`, as an absolute path, to the `jevd` that it starts. `Connection::connect_if_running` never starts `jevd`: no channel gives no connection, and every other failure is an error. On Windows, the client clears the inherit flag on its own standard handles before it starts jevd, so a caller that reads its output through a pipe does not wait for jevd. The client sets the flag again after the spawn. |

### Messages, protocol version 1

| Message | Who may send | Fields in | Fields out |
| --- | --- | --- | --- |
| `hello` | Any client | `protocol` | `protocol`, `daemon_version` |
| `status` | Any client | none | Daemon version, uptime, profile names. No secrets. |
| `pair` | Any client | `name`, `profile` (optional) | `request_id`, `code` |
| `pair-status` | Any client | `request_id` | `state`, plus `token` once when `approved`. The `approved` reply commits the client. |
| `ask` | A paired client | `token`, `state`, `questions` | `answers` |
| `pending` | `jev` only | none | The waiting requests: code, name, suggested profile |
| `approve` | `jev` only | `code`, `profile` | client name, plus `request_id` and `token` for a CLI request |
| `deny` | `jev` only | `code` | none |
| `revoke` | `jev` only | `name` | none |
| `clients` | `jev` only | none | name, profile, created, last used, limit override |
| `set-limit` | `jev` only | `name`, `limit` (`<count>/<seconds>`), or no `limit` for the default | none |
| `reload` | `jev` only | none | none |
| `stop` | `jev` only | none | none |

Every reply has `ok`. A failure carries `error.code` and `error.message`. Clients branch on the code, never on the message.

A `no-key`, `bad-key`, or `jev-unauthorized` failure from an `ask` also carries `error.profile`, the profile whose key failed, so a client can name it. A client that does not know the field ignores it.

A client that gets an error code it does not know treats the reply as a failure with no special handling. It never treats the reply as unreadable, and it keeps `error.message` and `retry_after`.

- In `jev-proto`, the received code is `WireCode`: a known `ErrorCode`, or `Unknown` with the raw code.
- A raw code that does not match `[a-z0-9-]{1,64}` reads as `Unknown("malformed")`, so a bad reply cannot put control characters into CLI output.
- `ErrorCode` stays a closed enum, not `#[non_exhaustive]`. A new code then makes the compiler flag each `match` in `jev`. `jevd` sends only known codes.
- A client that gets a `pair-status` state it does not know stops polling and treats the state as a failure.

`jevd` and `jev` never log a `pair-status` or `approve` reply, because either one can carry a token.

## 3. Pairing

### Two kinds of client

| Kind | Who holds the token | Example |
| --- | --- | --- |
| Socket client | The client, in its own storage. The docs recommend the OS keyring. | A desktop app, an editor extension |
| CLI client | The `jev` CLI, in the keyring under `client:<name>` | A shell script, a one-file Python script, a shared `my-scripts` client, a Claude Code plugin |

A client does not have to be an application. A client is whatever the person chooses to approve.

### Flow for a socket client

1. The client sends `pair` with `name` and an optional `profile`.
2. `jevd` returns `request_id` and `code`, and holds the request as pending.
3. The client shows the code and tells the person to run `jev allow`.
4. The person runs `jev allow`, types the code, picks the profile, and confirms. `jevd` saves nothing yet.
5. The client polls `pair-status`. It gets `pending`, then `approved` with the token. `jevd` returns the token once only. `denied` and `expired` end the request.
6. The `approved` reply commits the client: `jevd` saves it, and revokes an old client of the same name. If the save fails, `pair-status` returns `internal` and keeps the request, so the client can poll again.

### Flow for a CLI client

`jevd` marks a `pair` as a CLI request when the peer check shows that it came from the `jev` binary. For a CLI request, the token never waits in `jevd` for collection:

1. `approve` returns `request_id` and `token` to `jev`. `jevd` saves nothing yet.
2. `jev` reads the old `client:<name>` entry, then writes the new token to the keyring.
3. `jev` sends `pair-status` with the `request_id`. That commits the client, as in step 6 of the socket flow.
4. If the commit fails, `jev` writes the old keyring entry back and reports the failure.

The keyring write comes before the commit. In the other order, a locked keyring would leave the old client revoked and the new token lost. If `jev` stops between steps 2 and 3, the keyring holds a token that `jevd` does not know. The next `jev ask` then gets `not-paired`, and the person pairs again.

- `jev` takes the token and the `request_id` from the `approve` reply as they are. It checks the token against jevd's format: `jevt_`, then 64 lowercase hex digits. It checks the `request_id` against 32 lowercase hex digits. A value that does not match is `internal`, and `jev` never shows it.
- In step 4, `jev` writes the old entry back only while the keyring still holds the token of this run. So the newer token of another run stays.
- A commit that finds the approval canceled or expired is `internal`.
- If the write-back in step 4 also fails, the message names both errors. It tells the person to run `jev pair --name <name>` again.
- If the transport fails at the commit, `jev` cannot know if `jevd` committed the client. The message tells the person to pair again if `jevd` committed it.

- **On a terminal:** `jev pair --name <n> [--profile <p>]` shows the prompt and runs these steps in one process.
- **Without a terminal:** `jev pair` prints the code and exits with the `pending` code. The person runs `jev allow`, which runs these steps. The next `jev ask --client <name>` finds the token in the keyring. `jev` stores no `request_id` between runs.

### Values

| Item | Value |
| --- | --- |
| Code | 8 characters, from an alphabet with no look-alike characters |
| Request ID | 128-bit random. Only the client that sent `pair` gets it. |
| Token | 256-bit random, prefix `jevt_`. `jevd` stores only its SHA-256. |
| Pending expiry | 10 minutes. Waiting requests are also saved to `pending.json`, and survive a restart. Approved requests that are not collected live in memory only. |
| Pending requests at once | 16. Waiting requests and approved requests that are not committed both count. |
| Full pending list | The next `pair` gets `rate-limited` |
| Repeat CLI request | A `pair` from `jev` for a name that already has a waiting request from `jev` gets that request's `request_id` and `code`, or a new `request_id` after a `jevd` restart (see the [pairing-restart spec](2026-11-07-pairing-restart-design.md)). Its expiry does not restart. A request from a socket client is never shared. |
| Unknown request ID | `pair-status` reports `expired`, the same as a real expiry |
| Collection window | 10 minutes from approval, for a socket client. After it, `pair-status` reports `expired`, and `jevd` commits nothing. |
| Name rule | Lowercase letters `a-z`, digits, and hyphens only. 1 to 64 characters. The first character cannot be a hyphen. A hyphen may be the last character. |
| Client names | Unique. An approval for an existing name replaces that client at the commit. Until the commit, the old client keeps working. The approval screen warns about the replace, and says that `jev revoke` stops the old client at once. |
| Approval not committed | A second approval for the same name cancels the first. `jev revoke <name>` cancels approvals that are not committed, for that name. `jev key remove --force` revokes each client of the profile by name, then sends `reload`. An approval whose profile is no longer in the settings expires at its commit. |
| Token expiry | None in v1. `jev revoke` ends a pairing. |
| Last-used time | Written at most once a minute per client |

## 4. Checks

`jevd` runs these in order on each `ask`. A request that fails a check stops there.

1. **Shape.** `state` present, nesting depth at most 64 levels. `questions`: 1 to 20 entries. IDs match `[A-Za-z0-9_-]{1,64}`. Type `noul`, `score`, or `choice`. A `choice` needs `criteria`: an object with 1 to 255 options by name.
2. **Token.** The token's SHA-256 matches a client in the pairing file.
3. **Key.** From the cache or the keyring. Printable ASCII, no spaces, 1 to 512 characters. A control character in a header makes an HTTP library put the key in its error text. A request whose key fails here uses no rate-limit slot, so a client keeps its slots while the person fixes the key.
4. **Rate limit.** A sliding window per client, kept in memory. A refused request gets `rate-limited` with `retry_after` in seconds. An idle exit or a restart resets the counts. Just before `jevd` takes a slot, it checks the token again, so `jev revoke` stops a request that waits on a keyring read.
5. **Redaction.** Every string in `state` and `questions`: the key itself, PEM private keys, `Bearer` and `Basic` values, `sk-` and `tsk-` keys, GitHub, AWS, and Slack token shapes, and the value after a name such as `password=` or `api_key:`. `jevd` leaves dict keys as they are, because 2 keys can become the same text. With `redact-strict on`, `jevd` also redacts string values under secret-looking keys.
6. **Send.** `https://api.typesafe.ai/v1/systemone`, model `jev-latest`. 2 attempts of 15 seconds each, with a 1-second pause. Retries only on 429 and 529. Redirects never followed, because a redirect would resend the `Authorization` header to the new host. A 401 or a 403 is `jev-unauthorized`. Any other status that is not 200, a redirect included, is `jev-http-error` with the status. A 200 body over its cap is `bad-answer`; an error body over its cap gives no detail. Only an `https` endpoint is used. When an error body cannot be read for a reason other than a timeout, the status still counts.
7. **Answer check.** Each question has an answer of its own type. `noul` is a finite number from 0 to 1. `score` and `confidence` are finite numbers. A `choice` answer's `choice` is a key of the question's `criteria`, and its `confidence` is a finite number from 0 to 1. Response capped at 1 MiB, error body at 4 KiB. An answer deeper than 64 levels, or one with an object key that contains the key, is `bad-answer`. `jevd` replaces the key and its encoded forms in the answer's string values.

`jevd` redacts every error `detail`, including the JSON-escaped and percent-encoded forms of the key, turns control and bidi characters into spaces, and then cuts it to 300 characters.

The values in steps 1, 3, 5, 6, and 7 come from the prototype's `jevservice/names.py`, `redact.py`, and `typesafe.py`. Rust's `regex` crate runs in linear time, so the prototype's workarounds for Python's backtracking go away.

## 5. Settings

| Setting | Default | Loosening change (needs a terminal and a typed `y`) |
| --- | --- | --- |
| `idle-timeout` | `off` | A longer period, or `off` |
| `key-cache` | `2h` (`off` not allowed) | A longer period |
| `rate-limit` | `120/60` | A higher count, or a higher average rate |
| `redact-strict` | `off` | `off` |
| `max-connections` | `32` | None. It limits resources, not security. |

Per-client limit overrides follow the same rule as `rate-limit`. A tightening change runs anywhere, so a script can lower a limit, revoke a client, or turn strict redaction on. A refused loosening change exits with code 9.

The rate limit default replaces the prototype's `60/600`, which was an unmeasured placeholder. The short window means a refused client waits at most 60 seconds.

## 6. CLI

### Commands

| Group | Command | Needs a terminal |
| --- | --- | --- |
| Daemon | `jev start`, `jev stop`, `jev status` | No |
| Keys | `jev key add [--profile <p>] [--stdin] [--check]` | Yes, unless `--stdin` |
| | `jev key remove --profile <p> [--force]` | No |
| | `jev key list` | No |
| Pairing | `jev pair --name <n> [--profile <p>] [--no-prompt]` | One-step mode only |
| | `jev allow [<code>]` | Yes |
| | `jev deny <code>`, `jev revoke <name>`, `jev clients` | No |
| | `jev clients limit <name> <count>/<seconds>\|default` | For a loosening change |
| Settings | `jev config get\|set\|list` | For a loosening change |
| Use | `jev ask` | No |

`jev key remove` refuses while clients still use the profile. With `--force`, it deletes the profile and revokes those clients.

`jev key add` sends nothing by default. With `--check`, it sends one small real request and stores the key only if TypeSafe accepts it; that spends one request. TypeSafe documents no key-check endpoint.

- With `--check`, `jev key add` spends one request, or two when TypeSafe answers 429 or 529, because the Jev client tries again one time.
- With `--check`, a `bad-key` or `jev-unauthorized` failure also carries `error.profile` in its JSON.
- `jev key add` reads the settings before it reads the key. So a broken settings file stops the command before the person types a key.
- `jev` keeps a key that it reads in a buffer that it wipes when it drops the buffer (`Zeroizing`), until it stores the key.

### `jev ask`

- **Input.** JSON on stdin, or flags: `--state <text>` or `--state @<file>`, and repeatable `--noul <id>=<question>`, `--score <id>=<question>`, and `--choice <id>=<question>`. Each `--score` needs 1 to 10 `--level <id>=<text>`, from the lowest level to the highest. Each `--choice` needs 1 to 255 `--option <id>=<name>`; an option from a flag has no description. An `--option` or `--level` that names no question of its type, an empty name or level, or a name given twice on one question, exits with code 2. `--request <json>` passes JSON as an argument, for a caller that gives no stdin. Question flags plus a piped stdin exit with code 2. A terminal stdin with no question flags and no `--request` exits with code 2. Its failure JSON tells the three ways to give the request; `jev` prints no usage text.
- **One source.** With question flags or `--request`, stdin must be a terminal or closed. When stdin is not a terminal, `jev` reads it to the end. Any data there other than white space and a byte order mark at the start, or more than 1 MiB, exits with code 2. A caller whose stdin is an open pipe that never closes waits.
- **Size.** A request line over `MAX_LINE` (1 MiB) is `too-large` (exit 9). `jev` checks the size before it connects, so it does not start or wake `jevd` for such a request. `jev` reads at most 1 MiB of JSON from stdin, and at most 1 MiB from `--state @<file>`. More is `too-large`.
- **Byte order mark.** `jev` skips a byte order mark at the start of the JSON on stdin.
- **Question IDs.** `jev` checks each ID in the question flags with jevd's rule, `[A-Za-z0-9_-]{1,64}`. A bad ID, or the same ID two times, exits with code 2.
- **Client.** `--client <name>` or `JEV_CLIENT`. No default client.
- **Output.** Always JSON on stdout, for success and failure. `--value <id>` prints only that answer's value: a number, or the option name of a choice. On failure with `--value`, stdout stays empty and stderr gets the error code.
- **`--value`.** An ID that names no question in the request exits with code 2. `jev` takes the value from the field that the answer's `type` names: the number in `noul` or `score`, or the option name in `choice`. So `--value` prints a name, not a number, for a choice question. Control and bidi characters in the name print as spaces. Any other answer is `bad-answer` (exit 7).
- **Bad command line.** When clap refuses the command line of `jev ask`, or of a command with `--json`, `jev` also prints one failure JSON line on stdout. Its code is `bad-request`. The exit code is 2.

### Shared behavior

- Human-readable text by default, `--json` for scripts.
- Every command that talks to `jevd` starts it when no daemon runs, except `jev stop` and `jev status`.
- Prompts read the terminal device (`/dev/tty`, or `CONIN$` on Windows), never stdin.
- Crates: `clap` for arguments, `rpassword` for the hidden key prompt.
- Prompts, lists, and warnings go to the terminal device. Stdout holds only the result, so `--json` output stays one JSON object: the reply data with `"ok": true`, or `{"ok": false, "error": {...}}`.
- A command that needs a terminal and has none exits 9 (`forbidden`). So does a loosening change that the person does not confirm with `y`.
- `jev pair` without a terminal exits 3 and prints `pair_code` in its JSON.
- `jev pair --no-prompt` never uses the terminal, even when one is there: it exits 3 with `pair_code`, as with no terminal. A caller that runs inside another program's terminal, such as the Claude Code mod, uses it.
- `jev pair` checks `--name` with jevd's name rule before it connects. A bad name exits with code 2, as a bad `jev ask --client` does, and starts no daemon.
- `jev stop` with no daemon succeeds.
- `jev config set` and `jev key add` send `reload` only when a daemon runs. They save the file first. `reload_if_running` skips the reload only when no daemon has a channel, because a new `jevd` reads the file at its start. If a daemon runs but is busy, full, or does not answer, the command fails with "saved; the running jevd did not reload: ..." and the code of the reload error. `Connection::connect_if_running` in `jev-proto` makes this split.
- `jev` cleans control and bidi characters from every message it prints from jevd.
- Every JSON line on stdout writes control, bidi, and other format characters as `\uXXXX` escapes. The data stays the same, and the line is safe on a terminal.
- `jev` cleans its human text: these characters become spaces. A field from jevd becomes one line.
- On Windows, `jev` writes only ASCII text to the terminal device, because `CONIN$` and `CONOUT$` use the console code page.

### Exit codes

| Code | Meaning |
| --- | --- |
| 0 | Success |
| 1 | Unexpected error, or an error code this `jev` does not know. The JSON on stdout shows the raw code. |
| 2 | Bad command line |
| 3 | `pending` |
| 4 | `not-paired`, `revoked` |
| 5 | `rate-limited` |
| 6 | `no-key`, `bad-key`, `jev-unauthorized` |
| 7 | `jev-http-error`, `jev-timeout`, `jev-unreachable`, `bad-answer` |
| 8 | `daemon-unavailable`, `protocol-unsupported` |
| 9 | `forbidden`, `bad-request`, `too-large` |

### Did Jev get the request?

A client that tells its user what it sent needs to know which failures come before the call to Jev. The error code of a failed `ask` answers this:

| Jev got the request | Codes |
| --- | --- |
| No | `bad-request`, `too-large`, `forbidden`, `pending`, `not-paired`, `revoked`, `no-key`, `bad-key`, `rate-limited`, `protocol-unsupported`, `internal` |
| Yes | `jev-unauthorized`, `jev-http-error`, `bad-answer` |
| Maybe | `jev-timeout`, `jev-unreachable`, `daemon-unavailable` |

- jevd checks the shape, the token, the key, and the rate limit, in that order, before it calls Jev. So a "No" code proves that Jev got nothing. A bad command line (exit 2) also sends nothing.
- `jev-timeout` and `jev-unreachable` can come after Jev got the request: the send or the read of the reply breaks off.
- `daemon-unavailable` from a failed connect comes before the send. From a reply timeout or a closed connection, it comes after jevd got the request, and jevd may have called Jev. The code does not tell the two cases apart, so a client must treat it as "Maybe".
- jevd retries a 429 or 529 reply once. So with `jev-http-error`, Jev may have got the request twice.
- A failed Jev call still uses a rate-limit slot.
- `bad-answer` from `jev ask --value` comes after a reply, so Jev got the request.

## 7. Claude Code mod

The mod holds no key, no token, and no socket code. It has no `/jev allow` and no shell guard.

### `$.jev`

| Call | Result |
| --- | --- |
| `$.jev.ask({ state, questions })` | `{ ok: true, answers }` or `{ ok: false, error: { code, message } }` |
| `$.jev.status()` | `{ ok: true, installed, version, compatible, running, paired }`. It never starts `jevd`, and `paired` is `null` when `jevd` does not run, or when the mod could not read the client list. |

Neither call throws. A caller that gets `ok: false` does what it would do without Jev. The host names the calling plugin. An input `plugin` field must name the same plugin, or the call is `bad-request`, so one mod cannot ask as another.

A failure can also carry `retryAfter`, `profile`, and, for `pending`, `pairCode`. The mod adds 3 codes of its own: `jev-missing`, `jev-outdated`, and `jev-failed`. Every other code comes from `jev` as it is.

- `jev-missing`: the `jev` binary does not start, `jev --version` prints no jev version, or `jevPath` is not an absolute path.
- `jev-failed`: a `jev` run does not finish, or the mod cannot read its reply. A reply with no readable JSON, a JSON value with the wrong shape, or an error code that does not match `[a-z0-9][a-z0-9-]{0,63}` is always `jev-failed`. Its message names the exit code. A `--version` run that does not finish is `jev-failed`, with no toast.
- The mod cleans the message that it passes on: control, bidi, and invisible format characters become spaces. It cuts the message to 300 code points.
- Did Jev get the request? For a code from `jev`, see "Did Jev get the request?" in section 6. With `jev-missing` and `jev-outdated`, Jev got nothing, because the mod runs no `jev ask`. With `jev-failed`, Jev may have got the request, because the `jev ask` run can stop or print an unreadable reply after the send.

`$.jev.status` shows a missing or old `jev` in its fields (`installed`, `compatible`), never as `jev-missing` or `jev-outdated`. Its only failures are `bad-request` and `internal`.

- Only `daemon-unavailable` from `jev status` gives `running: false`.
- If `jevd` stops between the two runs, `jev clients` can start it again.
- `installed: true` with `version: null` means that the version check did not finish.

### Flow of one `ask`

1. The mod checks the plugin name against `[a-z0-9][a-z0-9-]{0,60}`.
2. The mod checks the request size against `jev`'s 1 MiB cap. Over the cap, it returns `too-large` and runs nothing.
3. The mod runs `jev --version` once per process, and refuses a `jev` older than it needs.
4. The mod runs `jev ask --client cc-<plugin>` through `$.process.run`, with the request JSON on stdin, and parses the reply. A child with no `stdin` option reads end of file at once, and one with `stdin` reads the request, then end of file.
5. On `not-paired`, the mod runs `jev pair --name cc-<plugin> --no-prompt --json`. That returns `pending` and a code, and never prompts on the terminal that Claude Code runs in. While the request waits, a repeat run returns the same code, so repeated asks do not fill the pending list. The mod shows a toast and returns `pending`.
6. After the person runs `jev allow`, the keyring holds the token, and the next `ask` works.

Timeouts: 150 s for `jev ask`, 30 s for `pair`, `status`, and `clients`, and 10 s for `--version`. Time that a hook spends waiting on `$.process.run` does not count against its budget.

### Toasts

All toasts use Claude Code's own `$.ui.toast`, as the prototype does (`register.ts:170`). The mod needs no other plugin.

| Event | Toast | How often |
| --- | --- | --- |
| `pending` | "In a terminal, run `jev allow` and approve code `<CODE>`": the action first, the code last. With no valid code, only "Run `jev allow` in a terminal". It stays 60 seconds, as every toast does. | Once per code |
| `no-key`, `bad-key` | "Run `jev key add --profile <p>`", with `<p>` from `error.profile` | Once per profile per process |
| `jev-unauthorized` | "Jev refused the key for profile `<p>`. Run `jev key add --profile <p>`.", with `<p>` from `error.profile` | Once per profile per process |
| `jev` not found | Install instructions | Once per process |
| `jev` older than the plugin needs | Update instructions | Once per process |

Each toast shows once: per pairing code, per profile for a key problem, and per process for a missing or old `jev`.

The mod finds `jev` on the PATH. The plugin option `jevPath` overrides that, and must be an absolute path. A bare `jev` comes from the PATH: the host refuses a binary that it finds only in the session's folder. The plugin's name is `jev-daemon`. It cannot run with another plugin that also defines `$.jev`.

## 8. Distribution and CI

### Release

| Item | Design |
| --- | --- |
| Tool | `dist` 0.33.1, run by `.github/workflows/release.yml` on each version tag. Its settings sit in `dist-workspace.toml`; `dist generate` writes the workflow. |
| Targets | `x86_64-pc-windows-msvc`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `aarch64-apple-darwin` |
| macOS | Apple Silicon (M1 and later) only. There is no Intel Mac archive. |
| Archive | One app, the `jev` package, owns all three binaries, so each archive holds `jev`, `jevd`, and `jev-tray` in one folder. The peer check depends on `jev` and `jevd` in one folder. |
| jev-tray | A binary of the `jev` app, so there is no second app. On Linux, only `jev-tray` links GTK. `jev` and `jevd` do not, so they still run on a Linux server with no GTK. The `jev` `Cargo.toml` names 2 Linux packages in `[package.metadata.dist.dependencies.apt]`: `libgtk-3-dev` and `libayatana-appindicator3-dev`. The Linux release runners install them before the build. |
| Installers | Shell and PowerShell, from `dist`. They install the three binaries into `$CARGO_HOME/bin`. `cargo install --locked --git https://github.com/mattbran87/jev-daemon jev` also works and installs all three. On Linux, that build needs `libgtk-3-dev`. With `--no-default-features`, it builds only `jev` and `jevd`, with no GTK. |
| Linux | glibc (`gnu`) builds on `ubuntu-22.04` and `ubuntu-22.04-arm`, so they need glibc 2.35 or newer. No musl build in v1. Every target builds on a native runner. |
| crates.io | Nothing is published in v1. |
| Pull requests | The release workflow builds every archive on a pull request and uploads them to the run, with no release (`pr-run-mode = "upload"`). |
| Action pins | `[dist.github-action-commits]` in `dist-workspace.toml` pins each action of the release workflow to a commit, `swatinem/rust-cache` too. One exception: the workflow downloads the `dist` binary by its version, with no hash check. `dist` has no setting for a hash. The release workflow can also run rustup's installer by `curl` when a runner has no Rust (none of GitHub's runners do). That is a second unpinned download, like the `dist` one. |
| Token | The generated `release.yml` gives the whole workflow `contents: write`, pull request runs included. `dist` sets this. So a branch in this repo gets a write token in its pull request runs. Fork pull requests do not. |
| Signing | None paid in v1. The macOS binaries are not signed or notarized. The Apple Silicon build gets the linker's ad hoc signature. The shell and PowerShell installers are the supported way to install. On macOS, a binary from a browser download needs `xattr -d com.apple.quarantine` before it runs. On Windows, it gets a SmartScreen warning. |
| Versions | One version for the crates and the plugin. The protocol version changes only with the wire format. A new error code changes the protocol version only if an older client would act wrongly when it treats the code as unknown. First tag `v0.1.0`. |
| Plugin | A marketplace file at the repo root |

### CI on each push and pull request

- Windows, Linux, and macOS: `cargo fmt --check`, `cargo clippy` with warnings as errors, `cargo test --locked`, on Rust 1.99.1. CI pins its toolchain, so a new Rust release cannot fail a commit that passed.
- The Linux `rust` job and the `msrv` job install `libgtk-3-dev` and `libayatana-appindicator3-dev` first. `jev-tray` needs GTK 3 to build, and it loads the AppIndicator library when it runs.
- The macOS jobs run on `macos-15`, an Apple Silicon runner. GitHub retired `macos-14`.
- The MSRV, Rust 1.85: `cargo check --workspace --locked`, with no test targets, because `wiremock`, a test-only dependency, needs Rust 1.88.
- The mod, on the 3 systems, with Claude Code 2.1.300: `claude plugin validate` on the marketplace and the plugin, and `claude plugin test`. `tsc` runs locally only: `/plugin-types` works only in an interactive session, and its file is Claude Code's own text, so the repo does not hold it.
- `cargo deny` for licenses, security advisories, bans, and sources, with `deny.toml`. It ignores 3 "unmaintained" notices for the `zbus` 4 crates (RUSTSEC-2026-0370, -0371, -0372), each with its reason, until the MSRV allows `keyring` 4. It also ignores the "unmaintained" notice for `proc-macro-error` (RUSTSEC-2024-0370), a build-time crate that `jev-tray` gets on Linux through `gtk` 0.18, until the MSRV allows `tray-icon` 0.25. A license exception allows `MPL-2.0` for the crate `option-ext` only. `jev-tray` also gets that crate on Linux.
- The `deny` job runs 2 checks: `advisories`, and `bans licenses sources`. A new advisory can appear with no change in the repo. So on a pull request, a failed `advisories` step does not fail the job. On `main`, it fails the job.
- `.github/workflows/advisories.yml` runs `cargo deny check advisories` each day, and fails when a new advisory appears. GitHub turns off a scheduled workflow after 60 days with no repo activity.
- `deny.toml` bans `openssl` and `openssl-sys`, because the binaries use rustls.
- On a pull request, a newer push cancels the CI run that is still going. On `main`, each commit gets its own concurrency group (`github.sha`), so no run cancels another.
- Every action pinned to a commit SHA, in every workflow. `Cargo.lock` committed.
- A test in `crates/jev/tests/versions.rs` checks that the plugin's version is the crates' version, and that the mod's `MIN_JEV` is no newer.

## 9. Docs

Zensical, as in the prototype. The built site sits in `site/` and opens from disk with no web server. The Markdown pages also read on GitHub. The prototype's site tests port over. Every page follows Simplified Technical English.

| Page | Content |
| --- | --- |
| `index.md` | What jev does, who it is for, install in 3 commands |
| `install.md` | Installers per OS, `jev key add`, the SmartScreen and Keychain warnings, uninstall |
| `cli.md` | Every command, the exit codes, the `jev ask` flags |
| `config.md` | Every setting, its default, and which changes need a terminal |
| `security.md` | The threat model, pairing, redaction, and every limit in section 10 |
| `building-clients.md` | Shell and Python examples through `jev ask`, the pairing flow, the error codes |
| `protocol.md` | The wire format and the version rules |
| `claude-code.md` | `$.jev`, the `cc-<plugin>` clients, the toasts, the spawn limit |
| `how-it-works.md` | Architecture and data flow diagrams |

## 10. Threat model

The boundary is the user account. jev protects against:

- An honest client that would use a key without the person's knowledge.
- An agent that approves its own client, or loosens a limit, by accident.
- Other users on the same machine, and callers on other machines.
- A runaway caller that spends a key's quota.
- Secrets with known shapes in request content.

jev does not protect against malware that runs as the same user. Such code can read the keyring and use a key directly.

### Limits to document

- A token that the CLI holds works for any process of that user that knows the client name.
- A process can replace the `jev` binary, or drive a pseudo-terminal, and then fake an approval. Both take a deliberate act.
- An agent can edit the settings file by hand and loosen a limit. The terminal rule only removes the easy path.
- On Linux and macOS, a process that connects and then runs `jev` in its place, before jevd accepts the connection, passes the peer check.
- Other mods that hook `process.run` see request content on the child's stdin. They never see a key or a token.
- Any process of the user can run `jev ask --client cc-<plugin>` and use that plugin's pairing, as the first bullet says for every CLI client.
- Redaction uses patterns, so a secret with an unknown shape gets through.
- Extra fields in Jev's answer reach the client. The answer check tests the fields it reads, not the absence of others.
- Without paid signing, Windows SmartScreen warns on first run, and the macOS Keychain can ask again for `jevd`'s access after each update. The cause is the ad hoc signature, which changes with each build.
- On a domain-joined Windows machine with roaming profiles, Credential Manager can copy jev's keys and client tokens to other machines where the same user signs in. The `keyring` crate stores them with `CRED_PERSIST_ENTERPRISE`.
- `jevd` uses the system and environment proxy settings. A proxy cannot read the key over HTTPS, but a TLS-inspecting proxy whose root certificate the OS trusts can.
- `jevd` matches a peer by its process ID. An ID that the OS reuses at once after its process ends can match the wrong process, for a few milliseconds.
- `jevd` builds its HTTPS client once. A change to the proxy settings takes effect after a restart.
- Two `jev allow` runs for one name at once can leave a revoked token in the keyring, in some orders. For example: run A reads the old token O, and run B writes its token and commits. Then A writes over B's token, A's commit expires, and A puts O back. Another order: B writes, A writes over it, B commits, and A's commit fails. In both orders, the client must pair again.
- `jev config set`, `jev clients limit`, and `jev key remove` read the current value and then write the new one, with no lock. Two runs at once can lose one update, and the loosening check can compare with an old value.
- On Windows, a `jevd` that `jev` starts gets none of the caller's standard handles, because they are not inheritable during the spawn. Other inheritable handles of the caller still reach `jevd`. On Unix, the caller's file descriptors 3 and up without close-on-exec reach `jevd`.

## 11. Testing

### Daemon and transport

- Two daemons started at once: exactly one survives.
- Idle exit and key-cache wipe, with a test clock.
- A same-user process that is not `jev` sends `approve`: `forbidden`.
- A copy of `jev` in another folder sends `approve`: `forbidden`.
- A pipe created first by another process: the client refuses it (Windows only).
- A line over the size cap: `too-large`, and the daemon keeps running.
- Stop with a request in flight: the request finishes, then the daemon exits.

### Pairing

- Full socket-client flow: `pair-status` returns the token once only, and that reply commits the client.
- A failed save at collection: `pair-status` returns `internal`, and a later poll gets the token.
- Full CLI flow: `approve` returns the token, the keyring write comes before the commit, and a failed commit restores the old keyring entry.
- A repeat `pair` from `jev` for a waiting name returns the same code, and its expiry does not restart.
- A wrong `request_id` returns nothing useful.
- `jevd` refuses the 17th pending request. A request expires after 10 minutes, with a test clock.
- `jev allow` and one-step `jev pair` refuse to run without a terminal.
- An approval for an existing name: the old token works until the commit, and gets `revoked` after it.
- A second approval for the same name, `jev revoke`, and `jev key remove --force` each cancel an approval that is not committed.
- A reply with an unknown error code parses, and `jev` exits 1 with the raw code in its JSON.
- `revoke` stops a token on its next request.
- The pairing file never holds a token in plain text.

### Checks

- The prototype's redaction tests, including the 1 MiB linear-time cases.
- Strict redaction: off by default, on only through the setting.
- `jevd` refuses the 121st request in the window, `retry_after` stays at or below 60, and an override changes the count.
- A fake Jev server returns 401, 429 twice, 529, a redirect, a 2 MiB body, and a malformed answer. Each case gives the right code, and the redirect is never followed.
- The key never appears in a reply, a log line, or an error detail, including when the HTTP library fails.
- A mock keyring in tests, so CI never touches a real credential store.

### CLI

- Flags and stdin JSON for the same question give byte-identical requests.
- Flags plus piped stdin: exit code 2.
- `--value` with a missing ID: exit code 2. With a failed call: empty stdout and the class exit code.
- One test per row of the exit code table.
- Each loosening change without a terminal: exit code 9. Each tightening change without a terminal: success.
- `jev key add --stdin` stores the key, and the key never appears in output or logs.

### Mod

- Each `jev` error code reaches the caller as it is, and a reply with no readable JSON is `jev-failed`, with a fake `jev` on `process.run`.
- The host's plugin name wins, and a different input `plugin` is `bad-request`.
- The request travels on stdin, as `jev ask --client cc-<plugin>`.
- `jev pair` runs with `--no-prompt`.
- A smoke test in a real headless session, with the real `jev` and `jevd`: 2 asks get `pending` with one code, and `$.jev.status` shows `running` and not `paired`.
- `not-paired`, then `pending`: one toast per pairing code, per process.
- A request over the size limit: `too-large`, and runs nothing.
- A missing `jev`: one toast, and `ok: false` with no throw.
- A real session with the user's own toast plugin turned off: the toast still shows.

## 12. Checks for the plan

These items need a check before the plan fixes the design that depends on them:

- **Linux keyring backend.** Done in plan 2: `keyring` 3.6 with `async-secret-service`, `crypto-rust`, and `async-io`. It uses the pure-Rust `zbus`, so the build needs no system `libdbus` and no OpenSSL. `keyring` 4 needs Rust 1.88, above the MSRV. TLS uses `reqwest`'s `rustls` with `aws-lc-rs`, whose `aws-lc-sys` compiles C code. Plan 6 picked a glibc build on native runners, so the build needs no cross compiler.
- **Crate names.** Checked on 2026-11-02: `jevd` and `jev-proto` are free on crates.io, and `jev` is taken. Plan 6 decides the published name of the CLI crate. The binary can stay `jev`. Plan 6 publishes nothing in v1, so it claims no name yet. If a later release publishes, `jevctl` is free for the CLI crate (checked 2026-11-04), and the binary stays `jev`.
- **Windows pipe access list.** Done in plan 3: tokio's `ServerOptions::create_with_security_attributes_raw` with a descriptor from the SDDL `O:<SID>D:P(A;;GA;;;<SID>)`, built with `windows-sys`. No `interprocess` crate.
- **TypeSafe key check.** Checked in plan 4: TypeSafe documents no key-check endpoint. `jev key add --check` spends one request, and only when the person asks for it.
- **Rate limit default.** Measure the busiest real client, such as jev-adhd, and check TypeSafe's pricing and server limits. Keep or change `120/60` before the first release. Release v0.1.0 keeps `120/60`. The measurement stays open for a later release.
- **Mod socket access.** The mod spawns the CLI because no test so far shows that the mod API can open a Unix socket or a named pipe. Leave this as is unless a check proves otherwise.

## Out of scope for v1

- Keys for services other than TypeSafe.
- A native OS dialog, a tray app, or any other GUI.
- A login service that starts `jevd` at login.
- Paid code signing and notarization.
- Package managers: winget, Homebrew, Linux packages.
- Client libraries beyond `jev-proto`, such as a Python package.
- Many requests per connection (protocol version 2).
- Token expiry.
- A per-profile rate limit.
- Moving existing plugins onto jev-daemon. That follows the first release.
