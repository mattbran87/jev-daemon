# The API key in an object key

**Date:** 2026-11-13
**Status:** design
**Issue:** [#18](https://github.com/mattbran87/jev-daemon/issues/18)

jevd now refuses an `ask` when an object key in `state` or `questions` contains the API key. Before, such a request reached Jev with the key in it. No wire format, file format, or exit code changes.

## Defect

jevd redacts the key in string values only (spec section 4, step 5). It leaves object keys as they are, because 2 keys can become the same text. So an object key that holds the key goes to Jev unredacted. This covers:

- a key of an object at any depth inside `state`;
- an option name in the `criteria` of a `choice` question, because `criteria` is an object;
- a name in the `criteria` of a `score` question, when that `criteria` is an object (jevd leaves the shape of score criteria to Jev);
- any other object key inside `questions`.

A second effect comes after the answer. If an option name holds the key, the answer's `choice` passes the answer check, and then the scrub replaces the key in it. `jev ask --value` then prints `[REDACTED]`, which is not one of the options.

The key leaks only when the caller puts the key itself into an object key. Real names almost never hold an API key.

## Fix

A new check in `Daemon::ask` (`crates/jevd/src/ask.rs`), after the key read and before the rate limit:

- If any object key in `state` or in `questions`, at any depth, contains the key, `ask` returns `bad-request`.
- The message is `an object key in state or questions contains the API key`. It never shows the object key.
- The match is the literal key, the same match as the answer check. The key has no spaces (step 3), so the trimmed key and the key are the same text.
- The check uses `names_key` from `crates/jevd/src/jev.rs`, which becomes `pub(crate)`. The shape check already caps the depth at 64, so the walk is bounded.

The check must come after the key read, because jevd needs the key to compare. It comes before the rate limit, so a refused request uses no slot, as a bad key uses none.

So Jev gets no request, and the second effect cannot happen: an option name can no longer hold the key, so a `choice` answer cannot become `[REDACTED]`. The answer check and the scrub do not change.

Rejected: redact object keys. 2 renamed keys can collide, and a redacted option name makes `--value` print `[REDACTED]`.

## Order of the checks

Spec section 4 gets a new step between **Key** and **Rate limit**:

> **Object keys.** If an object key in `state` or `questions` contains the key, at any depth, the request is `bad-request`. Jev gets nothing, and the request uses no rate-limit slot.

The later steps move down by 1. Step 5 (**Redaction**, now step 6) changes its sentence about dict keys to: "`jevd` leaves dict keys as they are, because 2 keys can become the same text; step 4 refuses a dict key that contains the key."

The table in section 6 about whether Jev got the request does not change: `bad-request` already means no.

## Tests

In `crates/jevd/src/ask.rs`:

- A `state` with an object key that contains the key gets `bad-request`. Jev gets no request, and the client's rate-limit slot stays free.
- A `choice` question with an option name that contains the key gets `bad-request`, and Jev gets no request.
- A `state` with the key in a string value still goes to Jev, with the key redacted.

## Docs

- Spec section 4, as above.
- A new `## Unreleased` section in `CHANGELOG.md`, above `## 0.1.5`, with one line about the new refusal.
- Close #18 after the merge.

## Out of scope

- The encoded forms of the key (for example, percent-encoded) in object keys. The redaction of string values does not cover them either.
- Other secrets in object keys, such as a GitHub token shape. Step 6 covers those in string values only.
