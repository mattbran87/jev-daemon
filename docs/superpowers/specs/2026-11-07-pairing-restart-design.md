# Waiting pairing requests survive a restart (0.1.1)

This spec fixes issues #5 and #6 for jev-daemon 0.1.1. Where it differs from the main spec, [2026-11-02-jev-daemon-design.md](2026-11-02-jev-daemon-design.md), this spec wins. Section 6 lists the lines of the main spec that change.

## 1. The problems

- **#5:** `jevd` holds waiting pairing requests in memory only. A restart deletes every one. A person who then types a code from an earlier toast gets `no waiting request has that code`. A socket client gets `expired`.
- **#6:** the Claude Code plugin's toast shows for 15 seconds, and it leads with the code. A person can lose the code before they type it.

Two facts shape the fix:

- `jev pair` from the `jev` binary, for a name that already has a waiting request from `jev`, gets back that request's code (`crates/jevd/src/pairing.rs`, `Pairings::start`). So if a waiting request survives the restart, the plugin's next `jev pair` gets the **same** code, and the plugin shows no second toast.
- `jev allow` with no code lists every waiting request with its code, and asks which one to approve. So a person never needs to copy the code from the toast.

## 2. The waiting-request file

`jevd` keeps its waiting requests in `pending.json` in the state folder, next to `jevd.log`. Only `jevd` writes the file. It uses the same atomic write as the pairings file.

The file holds one entry per waiting request:

| Field | Value |
| --- | --- |
| `id_sha256` | The SHA-256 of the request ID, as lowercase hex. Never the request ID itself. |
| `code` | The 8-character code. |
| `name` | The client name. |
| `profile` | The suggested profile, or `null`. |
| `cli` | `true` when the request came from the `jev` binary. |
| `deadline` | The wall-clock expiry, in Unix seconds: 10 minutes after the request started. |

The file holds no token and no request ID. A request that a person approved, but whose token the client has not collected, is not in the file: its token must never reach disk. It stays in memory only, as in 0.1.0.

## 3. Writes and reads

**Writes.** `jevd` writes the whole file after each change to the set of waiting requests:

- a new request;
- an approval or a denial, because the request stops waiting;
- the removal of expired requests, when that removal drops at least one waiting request.

A repeat `jev pair` that gets back an existing request changes nothing, so it writes nothing.

**A failed write** does not fail the request that caused it. `jevd` writes one line to its log and keeps the request in memory. The next change writes the whole file again.

**Reads.** `jevd` reads the file once, at start, after the pairings file.

- It drops each entry whose `deadline` has passed.
- It caps each `deadline` at 10 minutes from the load, so a clock moved back cannot keep a request alive longer than 10 minutes.
- It keeps at most 16 entries, the limit on waiting requests. It drops the rest, those with the earliest deadlines first.
- It drops an entry with a name, a profile, or a code that is not valid, and an entry whose code another entry already uses.

**A file that `jevd` cannot use does not stop it.** If the file cannot be read, is not JSON, or has an unknown field or a field of the wrong type, `jevd` writes one line to its log, deletes the file, and starts with no waiting requests. This is the opposite of the pairings file, which refuses to load when it is damaged. A lost waiting request costs one new pairing, so `jevd` must never fail to start because of it.

## 4. Requests after a restart

- **A socket client** polls `pair-status` with its request ID. `jevd` finds the request by the SHA-256 of that ID. The client keeps polling as if no restart happened.
- **A request from `jev`**, such as the plugin's: `jev` never polls with the ID that `pair` returns. After a load, `jevd` does not know the plaintext ID, so it gives each reloaded request from `jev` a new random ID. `approve` returns that new ID to `jev allow`, which commits with it, as in 0.1.0.
- **A repeat `jev pair`** for the same name finds the reloaded request, and gets the same code and the new ID. It takes no new slot.
- **A person** who types the code into `jev allow` finds the request, because the file stores the code.

The 10-minute limit, the limit of 16 waiting requests, and the rule that tokens never reach disk do not change. A request still disappears when the state folder is cleared, and when `jevd` crashes between a change and its write.

## 5. The toast and the errors

**The toast** leads with the action. The code comes last, so the person can match it to a line in the `jev allow` list:

> jev: the plugin `<plugin>` asks to use Jev. In a terminal, run `jev allow` and approve code `<CODE>`.

- The toast stays for 60 seconds (`TOAST_MS`), up from 15. Claude Code's toast has only a `timeoutMs` option, so it cannot stay until someone closes it.
- The plugin still shows each toast once per code.
- The toast with no code does not change.

**The `jev allow` errors** say what to do next:

| Case | 0.1.0 | 0.1.1 |
| --- | --- | --- |
| No request waits | `no pairing request waits` | `no pairing request waits; a request lasts 10 minutes, so ask the client to try again` |
| Unknown code | `no waiting request has that code` | `no waiting request has that code; run jev allow with no code to see the waiting requests` |

The exit codes do not change.

## 6. Changes to other documents

| Document | Change |
| --- | --- |
| Main spec, section 1, the Storage table | Add `pending.json`: the waiting pairing requests, written only by `jevd`, with no token and no request ID. |
| Main spec, section 3, the "Pending expiry" row of the Values table | "Pending requests live in memory only" becomes: waiting requests are also saved to `pending.json` and survive a restart; approved requests that are not collected live in memory only. |
| Main spec, section 7, the toast table | The `pending` toast leads with "run `jev allow`", ends with the code, and stays 60 seconds. |
| README | A short **Pair a client** section: the client asks, you run `jev allow`, you pick the code from the list. |
| CHANGELOG | A 0.1.1 entry. It removes #5 and #6 from the known limits. |

## 7. Tests

**`jevd`**, with the test clock the pairing tests use:

1. A waiting request survives a reload, with the same code, name, profile, and deadline.
2. A socket client's request ID still works after a reload.
3. A `jev pair` for the same name after a reload gets the same code and takes no new slot.
4. `jev allow` approves a reloaded request by its code.
5. An approved request that was not collected is gone after a reload, and the pairings file has no new entry.
6. An expired entry is dropped at load. A deadline more than 10 minutes ahead is cut to 10 minutes from the load.
7. More than 16 entries load as 16. A bad entry and a repeated code are dropped.
8. A damaged file, a file with an unknown field, and a file that cannot be read each give a start with no waiting requests and one log line, and the file is deleted.
9. `pending.json` never holds a request ID or a token: the test searches the file text for both.
10. A failed write of `pending.json` does not fail the pairing request.

**`jev`:** the 2 new error texts.

**The plugin:** the new toast text, and the 60-second timeout.

**End to end**, in `crates/jev/tests`: start `jevd`, run `jev pair --no-prompt`, stop `jevd` with `jev stop`, start it again, and approve the request with `jev allow <code>`.

## 8. Release

0.1.1 closes #5 and #6. The version changes to 0.1.1 in the workspace `Cargo.toml` and in `plugins/claude-code/.claude-plugin/plugin.json`. The tag `v0.1.1` follows the merge of the pull request, after the person approves it.

**Out of scope:** issues #7 and #8, a `criteria` flag for `jev ask --score`, and a live test against the real TypeSafe API.
