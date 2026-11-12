# Changelog

## Unreleased

- The design spec and the plugin's `JevErrorCode` comment say, for each error code of a failed ask, whether Jev got the request: no, yes, or maybe. The programs do not change.
- jevd accepts `choice` questions. A choice question needs `criteria` with 1 to 255 options, and the answer's `choice` must be one of them. An older `jevd` refuses a choice question with `bad-request`; run `jev stop`, and the next request starts the new `jevd`.
- `jev ask` has 3 new flags: `--choice <id>=<question>`, `--option <id>=<name>`, and `--level <id>=<text>`. `--level` gives the levels of a `--score` question, from the lowest to the highest.
- `jev ask --value` prints the option name of a choice answer, not a number.
- `jev ask --score` with no `--level` now exits 2 and calls nothing. Before, it reached Jev, and Jev refused it because the API needs `criteria`.
- The plugin's types describe `choice` questions and answers.
- CI checks the plugin's public types (`types/index.d.ts`) with TypeScript. The check needs no Claude Code session, so it does not cover `hooks/*.ts`. The programs and the plugin do not change.

## 0.1.3

- With `JEV_HOME` set, `jev` and `jevd` keep keys and client tokens under the keyring service `jev-` plus a 16-digit hash of the folder. Before, they used the service `jev`, so `jev key add` under a test `JEV_HOME` replaced the real key. Without `JEV_HOME`, nothing changes.
- A `JEV_HOME` folder that held keys before this change looks empty after the update. Run `jev key add` again, and pair each client again.
- The entries under a `JEV_HOME` service stay in the keyring after you delete the folder. To delete them, run `jev key remove` with the same `JEV_HOME` before you delete the folder.

## 0.1.2

- Each release archive holds `THIRD-PARTY-LICENSES.md`. It gives the license and notice text of each crate that `jev`, `jevd`, and `jev-tray` contain. The archives of 0.1.0 and 0.1.1 hold no license text except this project's `LICENSE`.
- CI fails when `THIRD-PARTY-LICENSES.md` is out of date.
- The programs and the Claude Code plugin do not change.

Known limits:

- Nobody has checked `jev-tray` by hand on macOS or on a Linux desktop yet. CI builds and tests both systems, but it has no real tray.
- The macOS binaries are not signed or notarized.
- **Stop jevd** does not last: a plugin's next request starts `jevd` again.
- `jev ask --score` has no way to give `criteria`. The plugin's types list `criteria` as needed on a score question. To ask a score question, give the whole request as JSON on stdin or with `--request`.

## 0.1.1

- Waiting pairing requests survive a `jevd` restart ([#5](https://github.com/mattbran87/jev-daemon/issues/5)). `jevd` saves them to `pending.json` in the jev state folder. The file holds no token and no request ID. A request still lasts 10 minutes.
- The pairing toast of the Claude Code plugin leads with `jev allow`, and it ends with the code. It stays 60 seconds, up from 15 ([#6](https://github.com/mattbran87/jev-daemon/issues/6)). Each toast of the plugin now stays 60 seconds.
- `jev allow` says what to do next when no request waits, and when no waiting request has the code.

Known limits:

- Nobody has checked `jev-tray` by hand on macOS or on a Linux desktop yet. CI builds and tests both systems, but it has no real tray.
- The macOS binaries are not signed or notarized.
- **Stop jevd** does not last: a plugin's next request starts `jevd` again.
- `jev ask --score` has no way to give `criteria`. The plugin's types list `criteria` as needed on a score question. To ask a score question, give the whole request as JSON on stdin or with `--request`.

## 0.1.0

The first release.

- `jevd`, the per-user daemon. It holds your TypeSafe keys, and it checks each request for shape, rate limit, and secrets before it sends anything. Programs and `jev` start it when they need it, and it keeps running until you stop it. To stop it after a period with no use, run `jev config set idle-timeout 2h`.
- `jev`, the CLI. It manages keys, pairing, and settings, and `jev ask` sends a request. A program pairs once, and you approve it with `jev allow`.
- `jev-tray`, a tray icon that shows whether `jevd` runs. Its menu has **Start jevd** or **Stop jevd**, **Start at login**, and **Quit**.
- `plugins/claude-code`, a Claude Code plugin that gives other plugins `$.jev`.

Each archive and each installer holds `jev`, `jevd`, and `jev-tray`. The targets are Linux (x86_64 and aarch64, glibc 2.35 or newer), Windows (x86_64), and macOS (Apple Silicon only).

Known limits:

- Nobody has checked `jev-tray` by hand on macOS or on a Linux desktop yet. CI builds and tests both systems, but it has no real tray.
- The macOS binaries are not signed or notarized.
- **Stop jevd** does not last: a plugin's next request starts `jevd` again.
- A `jevd` restart deletes every waiting pairing code ([#5](https://github.com/mattbran87/jev-daemon/issues/5)). The client must ask again for a new code.
- The pairing toast closes before you can read the code ([#6](https://github.com/mattbran87/jev-daemon/issues/6)). To see the code again, run `jev pair --name <client> --no-prompt --json`.
- `jev ask --score` has no way to give `criteria`. The plugin's types list `criteria` as needed on a score question. To ask a score question, give the whole request as JSON on stdin or with `--request`.
