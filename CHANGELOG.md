# Changelog

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
