# A type check of the plugin's public types

`claude plugin test` runs the plugin's tests, but it does not check TypeScript types. So a wrong type in `plugins/claude-code/types/index.d.ts` passes CI. The `choice` work (#8) showed this: its new test passed before the types changed. This spec adds a CI job that checks the public types with `tsc`.

## Why only the public types

A full check of the plugin needs the type files that Claude Code writes into `.claude-plugin/types/` when a session loads the mod. These tests show that CI cannot get them:

- `claude plugin validate`, `claude plugin test`, and `claude plugin details` do not write them.
- A headless session with `--bare` refuses to load the mod. Without `--bare`, a session with an invalid API key stops at the key and writes nothing.
- The npm package of Claude Code does not ship them. They live inside the `claude` binary.

A real session in CI needs an API key and costs money on each run. A copy of the generated files in the repo puts about 860 KB of Claude Code's own code, under Anthropic's license, in a public repo. So this check covers `types/index.d.ts`, the contract that other plugins build on. It needs only one name from Claude Code: the module `claude-code` and its `EngineInterface`.

## 1. Files

A new folder, `ci/plugin-types/`, at the repo root. A marketplace install copies only `plugins/claude-code`, so these files never ship with the plugin.

| File | Holds |
| --- | --- |
| `tsconfig.json` | `strict`, `noUncheckedIndexedAccess`, and `noEmit`, as in the generated config. It includes `../../plugins/claude-code/types/index.d.ts` and the 2 files below. |
| `claude-code.d.ts` | A stub: `declare module 'claude-code' { interface EngineInterface {} }`. It stands in for the generated file. |
| `contract.ts` | Type assertions on the contract (section 2). |
| `package.json`, `package-lock.json` | One dev dependency, `typescript`, at an exact version. The lockfile holds its hash, and `npm ci` checks it. |

The version of `typescript` is the latest release that is at least 2 weeks old when the work starts: 7.0.2 on 2026-11-12. The generated types need TypeScript 5.4 or later. If 7.0.2 does not run `tsc -p` as 5.x does, the work uses the latest 5.x release that is at least 2 weeks old and says so.

`.gitignore` already ignores `node_modules/`.

## 2. The assertions

`contract.ts` imports the types from the plugin's `index.d.ts`. Each assertion must compile, or must fail at the line after a `// @ts-expect-error` comment:

- **Questions.** A `noul`, a `score`, and a `choice` question compile with their fields. A `choice` question without `criteria` fails. A `score` question without `criteria` fails.
- **Answers.** After a check of `type === 'choice'`, a `JevAnswer` has `choice` as a `string` and `probabilities` as a `Record<string, number>`. After a check of `type === 'noul'`, it has `noul` as a `number`.
- **Error codes.** `JevErrorCode` takes each named code, such as `'not-paired'` and `'jev-failed'`, and also an unknown string, for a code from a newer `jev`.
- **The engine.** `EngineInterface` has `jev`. `jev.ask` takes a `JevAskInput` and returns a `Promise<JevAskResult>`.

A `@ts-expect-error` line that no longer fails is itself an error, so a type that becomes too loose also fails the check.

## 3. CI

A new job in `.github/workflows/ci.yml`, named `plugin types`:

- It runs on `ubuntu-latest` only, because the result does not depend on the OS.
- It uses the same pinned `actions/checkout` and `actions/setup-node` as the `mod` job, with `persist-credentials: false` and Node 22.
- Its steps: `npm ci` in `ci/plugin-types`, then `npx --no-install tsc -p .` in that folder.

## 4. A check that the check works

During the work, change one type on purpose, for example `choice: number` in `JevAnswer`, and run the check. It must fail. Then undo the change. No commit holds the broken type.

## 5. Docs

- `CHANGELOG.md`: one **Unreleased** line. The programs and the plugin do not change.
- The README does not describe the plugin tests, so it does not change.

## Out of scope

- A type check of `hooks/*.ts`. It needs the generated Claude Code types. `claude plugin test` still covers how the hooks behave at run time.
