# `choice` questions

Issue #8, second suggestion. jevd accepts only the question types `noul` and `score` (`QUESTION_TYPES` in `crates/jevd/src/shape.rs`). TypeSafe also offers `choice`: the model picks one option from a set that the client defines. Today a client that needs a choice must send one `noul` question for each option. This spec adds `choice` to jevd, to `jev ask`, and to the plugin types.

The same work fixes a gap in `jev ask --score`. The API needs `criteria` on a score question, but no `jev ask` flag can give it. So a score question built from flags reaches Jev, and Jev refuses it.

## 1. The API

From the TypeSafe API reference (`https://docs.typesafe.ai/api.md`, schemas `ChoiceQuestion`, `ChoiceAnswer`, and `ScoreQuestion`):

- **Choice question.** `type` is `choice`. The API needs `criteria`: an object with 1 to 255 keys. Each key is an option name. Each value describes when that option applies: a string, an object, an array, or `null`. "A choice without a description is interpreted by its name alone." `instructions` is optional.
- **Choice answer.** `type`, `choice`, `confidence`, and `probabilities`, all required. `choice` is the option with the highest probability. `confidence` runs from 0 to 1. `probabilities` holds one number for each option.
- **Score question.** The API needs `criteria`: an array of 1 to 10 level descriptions, from the low end of the scale to the high end. The position of each level gives its score, from 0.

Example answer from the reference:

```json
{"type": "choice", "choice": "calm", "confidence": 0.7,
 "probabilities": {"angry": 0.15, "calm": 0.8, "excited": 0.05}}
```

## 2. jevd

**Shape check** (`crates/jevd/src/shape.rs`, spec section 4, step 1):

- `QUESTION_TYPES` becomes `["noul", "score", "choice"]`.
- A `choice` question must have `criteria`: an object with 1 to 255 keys. Without it, jevd cannot check the answer. So a choice question without valid `criteria` is `bad-request`, and Jev gets nothing.
- jevd does not check `criteria` on a `score` question. Jev refuses a score question without one, so jevd keeps today's behavior there.
- The `bad-request` message names the 3 types and the `criteria` rule.

**Answer check** (`crates/jevd/src/answer.rs`, spec section 4, step 7):

- `choice` must be a string that is a key of the question's `criteria`.
- `confidence` must be a finite number from 0 to 1.
- Any other choice answer is `bad-answer`.
- `probabilities` passes through unchecked, as extra fields do today. jevd checks only what `jev` reads.

Today the answer check passes every type that it does not know (`_ => true`). That arm stays, but `choice` no longer reaches it.

## 3. `jev ask`

Three new flags, each repeatable:

| Flag | Meaning |
| --- | --- |
| `--choice <id>=<question>` | A choice question. `<question>` becomes `instructions`. |
| `--option <id>=<name>` | One option of choice question `<id>`. It adds `"<name>": null` to that question's `criteria`. |
| `--level <id>=<text>` | One level of score question `<id>`. It adds `<text>` to that question's `criteria`, in the order given, low to high. |

For example:

```bash
jev ask --client app --state "I have waited 3 days and nobody answers." \
  --choice mood="What is the user's mood?" \
  --option mood=angry --option mood=calm --option mood=excited \
  --value mood
```

An option from a flag has no description. A description needs the JSON request, on stdin or with `--request`.

**Flag errors.** `jev` checks the flags before it connects. Each of these is a usage error, exit 2, and Jev gets nothing:

- a `--choice` with no `--option`
- a `--score` with no `--level`
- an `--option` whose id names no `--choice` question
- a `--level` whose id names no `--score` question
- the same option name twice on one question
- more than 255 options on one question, or more than 10 levels

`--option` and `--level` follow the same rules as `--noul` and `--score`: they cannot go with `--request` or a piped stdin. The usage messages that list the question flags add `--choice`.

**`--value`.** On a choice answer, `--value <id>` prints the option name, for example `angry`. The name is one of the request's own `criteria` keys, because jevd checks it. So the output is never free text from the model. On `noul` and `score` answers, `--value` prints a number, as today. A choice answer with no string `choice` is `bad-answer`, exit 7.

So `--value` no longer prints only numbers. The spec and the `--help` text say so.

## 4. Plugin types

In `plugins/claude-code/types/index.d.ts`:

- `JevQuestion` adds `{ type: 'choice'; instructions?: string; criteria: Record<string, unknown>; [field: string]: unknown }`.
- `JevAnswer` adds `{ type: 'choice'; choice: string; confidence: number; probabilities: Record<string, number>; [field: string]: unknown }`.
- The comment on `JevQuestion` describes `choice`.

The types only grow, so the plugin stays compatible. The mod code passes questions and answers through, so it does not change.

## 5. Compatibility

- **A change that breaks a script.** `jev ask --score s=...` with no `--level` now exits 2 before it connects. Today the same command reaches Jev, and Jev refuses it because `criteria` is missing. So the command never worked, but its exit code changes to 2, and it now fails with no call to Jev. The CHANGELOG says so.
- **An older jevd.** A new `jev` that sends a choice question to an older `jevd` gets `bad-request`. This happens only when an old `jevd` still runs after the person updates `jev`. `jev stop` ends the old `jevd`, and the next request starts the new one.
- **An older `jev` with a new plugin.** The plugin passes a choice question to `jev ask` as JSON on stdin, and `jev` passes it to `jevd`. An older `jevd` refuses it with `bad-request`, as above.

## 6. Docs

- The main design spec: the shape rule and the answer check in section 4, and the `jev ask` input and `--value` rules in section 6.
- `CHANGELOG.md`: an **Unreleased** entry for `choice`, the new flags, and the `--score` change.
- The README does not list the `jev ask` flags, so it does not change.

## 7. Tests

**jevd:**

- The shape check accepts a choice question with 1 option and with 255 options.
- The shape check refuses a choice question with no `criteria`, with `criteria` as an array, with 0 options, and with 256 options.
- The answer check accepts a valid choice answer.
- The answer check refuses a `choice` that is not a `criteria` key, a `choice` that is not a string, a missing `confidence`, and a `confidence` of 1.5.

**`jev ask`:**

- A request built from `--choice` and `--option` flags equals the same request as JSON.
- A request built from `--score` and `--level` flags keeps the order of the levels.
- Each flag error in section 3 exits 2 and connects to nothing.
- `--value` on a choice answer prints the option name.

**Plugin:**

- A type test for a choice question and a choice answer.

## Out of scope

- `box` questions. The API needs a special header for them, and this work did not research them.
- Descriptions for options and levels given by flag. The JSON request covers them.
- `criteria` on `noul` questions. The API accepts it, and the JSON request passes it through today.
