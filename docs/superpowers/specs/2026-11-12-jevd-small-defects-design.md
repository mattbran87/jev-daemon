# Three small jevd defects

**Date:** 2026-11-12
**Status:** built

This change fixes 3 small defects in jevd's pairing code. None of them changes the wire protocol, a file format, or an exit code.

## 1. The message for an unknown code

**Defect.** `crates/jevd/src/daemon.rs:297` and `:304` answer a failed approve or deny with the text "no waiting request has that code". The text names no next step. `jev allow` hides this, because it prints its own message, `NO_SUCH_CODE` in `crates/jev/src/pairing.rs`. A reader of the raw reply, such as another client, sees only the bare text.

A second gap: `Pairings::approve` also returns `bad-request` when the profile name fails `valid_name`. `daemon.rs` then gives the same message about the code, which is wrong for that case. `daemon.rs` checks that the profile is a known profile first, so this path needs a bad profile in the settings file. It is rare, but its message still names the wrong cause.

**Fix.**
- jevd's message for an unknown code becomes the same text as `NO_SUCH_CODE`: "no waiting request has that code; run jev allow with no code to see the waiting requests". One constant in jevd holds it, and both `approve` and `deny` use it.
- `daemon.rs` checks the profile name with `valid_name` before it calls `Pairings::approve`. A bad name gets its own `bad-request` message that names the profile. `Pairings::approve` keeps its own check, so its contract does not change.

## 2. A bad `id_sha256` in `pending.json`

**Defect.** The load of `pending.json` (`Pairings::load_pending` in `crates/jevd/src/pairing.rs`) checks the name, the profile, and the code of each entry. It does not check `id_sha256`. An entry with `""`, or any text that is not a SHA-256, loads as a waiting request under that key. No request ID hashes to that key, so the client that asked can never collect its token. The request still shows in `jev allow`, and a person can approve it, but the approval reaches nobody.

**Fix.** The load drops an entry whose `id_sha256` is not exactly 64 lowercase hex digits, the same way it drops an entry with a bad name. An entry with `cli: true` skips this check, because jevd gives it a new request ID and a new key at the load.

## 3. The order after the 10-minute cap

**Defect.** The load caps each entry's time left at `PENDING_EXPIRY` (600 seconds), so a clock moved back cannot keep a request alive longer. It then sorts the entries by time left, the most first, and keeps at most `MAX_PENDING` (16). After a clock change, many entries can all have 600 seconds left. They tie, and the stable sort keeps them in file order. jevd writes the file sorted by code, so the alphabet decides which entries stay.

**Fix.** The load sorts by the saved deadline, the latest first. The time left grows with the deadline and never shrinks, so this order matches the old order wherever no tie occurs. Where the cap makes a tie, the requests with the latest saved deadlines stay. Those are the newest requests.

## Tests

- **1:** the reply to an approve and to a deny with an unknown code holds the new message. An approve with a profile name that fails `valid_name` gets a message that names the profile, not the code.
- **2:** a non-`cli` entry with `id_sha256` of `""` drops at the load. So does an entry with 63 hex digits, and an entry with uppercase hex. A valid entry next to them stays. A `cli` entry with a bad `id_sha256` stays.
- **3:** 17 entries, each with a deadline more than 10 minutes away and each with a different deadline. The 16 with the latest deadlines stay. The one with the earliest deadline drops, even when its code sorts first.

## Docs

One line under **Unreleased** in `CHANGELOG.md`. The main design spec does not describe these details, so it does not change.

## Out of scope

- A change to `jev`'s own messages.
- Any other check on `pending.json` fields.
