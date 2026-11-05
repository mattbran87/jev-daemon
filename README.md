# jev-daemon

jev lets local programs ask TypeSafe's Jev model while one background process, `jevd`, holds your TypeSafe keys. A program pairs once, and you approve it with `jev allow` in a terminal. `jevd` then checks each request for shape, rate limit, and secrets before it sends anything.

- `jevd`: the per-user daemon. Programs and the `jev` CLI start it when they need it, and it exits after 2 hours with no use.
- `jev`: the CLI. It manages keys, pairing, and settings, and `jev ask` sends a request.
- `jev-tray`: a tray icon that shows whether `jevd` runs.
- `plugins/claude-code`: a Claude Code plugin that gives other plugins `$.jev`.

**Status:** before the first release. No release archive exists yet.

## Build from source

You need Rust 1.85 or newer.

```bash
cargo install --locked --git https://github.com/mattbran87/jev-daemon jev
```

That installs `jev`, `jevd`, and `jev-tray` into the same folder, `~/.cargo/bin`. `jevd` accepts admin commands only from the `jev` next to it, so keep the two together. Each release archive and each installer also holds all three.

On Linux, the build of `jev-tray` needs GTK 3. On Debian and Ubuntu, install the GTK development package first:

```bash
sudo apt-get install libgtk-3-dev
```

To build only `jev` and `jevd`, with no GTK, add `--no-default-features`. From a clone of the repository:

```bash
cargo install --locked --path crates/jev --no-default-features
```

## jev-tray

`jev-tray` shows one icon in the system tray. The icon has a check mark when `jevd` runs, and an x when it does not. The tooltip says the same. `jev-tray` checks every 5 seconds. It never starts, stops, or configures `jevd`.

`jev-tray` installs with `jev` (see above). On Linux, `jev-tray` needs GTK 3 and an AppIndicator library when it runs. `jev` and `jevd` do not need them, so they still run on a server with no desktop. On Debian and Ubuntu, install the two libraries for `jev-tray`:

```bash
sudo apt-get install libgtk-3-0 libayatana-appindicator3-1
```

Then run `jev-tray`. Only one copy runs: a second copy exits at once.

Click the icon to open its menu. The menu has two items:

- **Start at login**: tick it, and `jev-tray` starts each time you sign in. Untick it to stop this. The entry holds the path of the `jev-tray` that you ticked it from, so tick it again after you move `jev-tray`.
- **Quit**: closes `jev-tray`. `jevd` keeps running.

**Start at login** writes one entry: a value under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` on Windows, a file in `~/Library/LaunchAgents` on macOS, and a file in `$XDG_CONFIG_HOME/autostart` (by default `~/.config/autostart`) on Linux.

When `jev-tray` cannot start, it writes the error to `jev-tray.log` in the jev state folder. On Windows, it also shows the error in a message box.

On Linux, the desktop needs a tray host. GNOME has none until you install its AppIndicator extension. With no tray host, `jev-tray` shows nothing. On Linux, the icon is always white, and the panel shows no tooltip.

## Claude Code plugin

Add the marketplace, then install the plugin. Run these in Claude Code:

```
/plugin marketplace add mattbran87/jev-daemon
/plugin install jev-daemon@jev-daemon
```

The plugin needs `jev` on your `PATH`. Install it first (see above).

## Design

The design is in [docs/superpowers/specs/2026-11-02-jev-daemon-design.md](docs/superpowers/specs/2026-11-02-jev-daemon-design.md). The design of `jev-tray` is in [docs/superpowers/specs/2026-11-04-jev-tray-design.md](docs/superpowers/specs/2026-11-04-jev-tray-design.md).

## License

Apache-2.0. See [LICENSE](LICENSE).
