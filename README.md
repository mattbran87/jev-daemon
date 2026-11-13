# jev-daemon

jev lets local programs ask TypeSafe's Jev model while one background process, `jevd`, holds your TypeSafe keys. A program pairs once, and you approve it with `jev allow` in a terminal. `jevd` then checks each request for shape, rate limit, and secrets before it sends anything.

- `jevd`: the per-user daemon. Programs and the `jev` CLI start it when they need it, and it keeps running until you stop it. To stop it after a period with no use, run `jev config set idle-timeout 2h`.
- `jev`: the CLI. It manages keys, pairing, and settings, and `jev ask` sends a request.
- `jev-tray`: a tray icon that shows whether `jevd` runs.
- `plugins/claude-code`: a Claude Code plugin that gives other plugins `$.jev`.

**Status:** 0.1.5. See [CHANGELOG.md](CHANGELOG.md).

## Install

Each installer puts `jev`, `jevd`, and `jev-tray` into `~/.cargo/bin`.

On Linux and macOS:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/mattbran87/jev-daemon/releases/latest/download/jev-installer.sh | sh
```

On Windows, in PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/mattbran87/jev-daemon/releases/latest/download/jev-installer.ps1 | iex"
```

The [releases page](https://github.com/mattbran87/jev-daemon/releases) also has an archive for each target: Linux on x86_64 and aarch64 (glibc 2.35 or newer), Windows on x86_64, and macOS on Apple Silicon only.

## Build from source

You need Rust 1.85 or newer.

```bash
cargo install --locked --git https://github.com/mattbran87/jev-daemon jev
```

That installs `jev`, `jevd`, and `jev-tray` into the same folder, `~/.cargo/bin`. `jevd` accepts admin commands only from the `jev` next to it, so keep the two together. Each release archive and each installer also holds all three.

On Linux, the build of `jev-tray` needs GTK 3 and `pkg-config`. On Debian and Ubuntu, install them first:

```bash
sudo apt-get install pkg-config libgtk-3-dev
```

To build only `jev` and `jevd`, with no GTK, add `--no-default-features`. From a clone of the repository:

```bash
cargo install --locked --path crates/jev --no-default-features
```

## Linux keyring

On Linux, `jevd` stores keys only through Secret Service, the desktop keyring on D-Bus. A desktop session usually unlocks its keyring at login, so `jev key add` works at once. A server, WSL, or an SSH-only login can have the keyring service but no unlocked keyring. Then `jev key add` cannot store the key, and `jev status` shows `profiles: none`.

To check, run:

```bash
busctl --user call org.freedesktop.secrets /org/freedesktop/secrets org.freedesktop.Secret.Service ReadAlias s default
```

If it prints `o "/"`, no default keyring exists. If it fails, no keyring service runs: on Debian and Ubuntu, install `gnome-keyring` and `dbus-user-session`, then log in again.

To create the keyring and unlock it, run these 3 lines. The prompt asks for a new password and does not show what you type:

```bash
read -rsp "Keyring password: " pw; echo
printf '%s' "$pw" | gnome-keyring-daemon --replace --unlock --components=secrets
unset pw
```

Then run `jev key add`.

- **After each restart, the keyring is locked.** Run the same 3 lines with the same password. Until you do, `jevd` cannot read your keys.
- **Do not type the password straight into `gnome-keyring-daemon --unlock`.** The Enter key adds a newline, which can become part of the password. The next unlock with the 3 lines then fails.
- **No password recovery exists.** If you forget it, delete `~/.local/share/keyrings/login.keyring`, which also deletes every key in it. Then run the 3 lines again, and add your keys again.
- **On a server, keep the session after you log out:** run `sudo loginctl enable-linger "$USER"`. Without linger, systemd deletes `$XDG_RUNTIME_DIR`, which holds the socket of `jevd`, at your last logout.

## Pair a client

A program must pair once before it can ask Jev. A Claude Code plugin pairs as the client `cc-<plugin>`.

1. The client asks to pair. The Claude Code plugin shows a toast: "In a terminal, run jev allow and approve code `<CODE>`."
2. In a terminal, run `jev allow`. It lists each waiting request: its code, and the name of the client.
3. Type the code from the list. Then pick a profile, and type `y`.

A request waits 10 minutes, also across a restart of `jevd`. When no request waits, `jev allow` says so: ask the client to try again.

## jev-tray

`jev-tray` shows one icon in the system tray. The icon has a check mark when `jevd` runs, and an x when it does not. The tooltip says the same. `jev-tray` checks every 5 seconds. Its menu can also start and stop `jevd`. It never configures `jevd`.

`jev-tray` installs with `jev` (see above). On Linux, `jev-tray` needs GTK 3 and an AppIndicator library when it runs. `jev` and `jevd` do not need them, so they still run on a server with no desktop. On Debian and Ubuntu, install the two libraries for `jev-tray`:

```bash
sudo apt-get install libgtk-3-0 libayatana-appindicator3-1
```

Then run `jev-tray`. Only one copy runs: a second copy exits at once.

Click the icon, with either button, to open its menu. The menu has three items:

- **Start jevd** or **Stop jevd**: the label follows the icon. A click runs `jev start` or `jev stop` from the folder that holds `jev-tray`, so keep `jev` in that folder. If the command fails, the error goes to `jev-tray.log` in the jev state folder. On Windows and macOS, the tooltip also shows it.
- **Start at login**: tick it, and `jev-tray` starts each time you sign in. Untick it to stop this. The entry holds the path of the `jev-tray` that you ticked it from, so tick it again after you move `jev-tray`.
- **Quit**: closes `jev-tray`. `jevd` keeps running.

**Stop jevd** does not last. The next request from a plugin or from `jev` starts `jevd` again.

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

## Licenses

This project is under Apache-2.0. See [LICENSE](LICENSE).

`jev`, `jevd`, and `jev-tray` contain code from other crates. [THIRD-PARTY-LICENSES.md](THIRD-PARTY-LICENSES.md) holds the license and notice text of each one. Each release archive holds this file too.

The file is generated. After a change to the dependencies, refresh it with `cargo-about` 0.9.2:

```bash
cargo install --locked --version 0.9.2 --features cli cargo-about
cargo about generate --frozen --workspace -o THIRD-PARTY-LICENSES.md about.hbs
```

If `cargo about` says that it cannot fetch in offline mode, run `cargo fetch --locked` first. CI fails when the file is out of date.

When you add a dependency, look in its source for a file named `NOTICE` or `NOTICE.*`. Apache-2.0 asks us to pass that text on, and `cargo about` does not collect it. If you find one, add a Notices section to `about.hbs`.
