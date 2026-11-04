# jev-daemon

jev lets local programs ask TypeSafe's Jev model while one background process, `jevd`, holds your TypeSafe keys. A program pairs once, and you approve it with `jev allow` in a terminal. `jevd` then checks each request for shape, rate limit, and secrets before it sends anything.

- `jevd`: the per-user daemon. Programs and the `jev` CLI start it when they need it, and it exits after 2 hours with no use.
- `jev`: the CLI. It manages keys, pairing, and settings, and `jev ask` sends a request.
- `plugins/claude-code`: a Claude Code plugin that gives other plugins `$.jev`.

**Status:** before the first release. No release archive exists yet.

## Build from source

You need Rust 1.85 or newer.

```bash
cargo install --locked --git https://github.com/mattbran87/jev-daemon jev
```

That installs `jev` and `jevd` into the same folder, `~/.cargo/bin`. `jevd` accepts admin commands only from the `jev` next to it, so keep the two together.

## Claude Code plugin

Add the marketplace, then install the plugin. Run these in Claude Code:

```
/plugin marketplace add mattbran87/jev-daemon
/plugin install jev-daemon@jev-daemon
```

The plugin needs `jev` on your `PATH`. Install it first (see above).

## Design

The design is in [docs/superpowers/specs/2026-11-02-jev-daemon-design.md](docs/superpowers/specs/2026-11-02-jev-daemon-design.md).

## License

Apache-2.0. See [LICENSE](LICENSE).
