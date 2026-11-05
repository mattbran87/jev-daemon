# jev-tray design

`jev-tray` is a small program that shows one icon in the system tray. The icon tells you whether `jevd` runs. It does nothing else: it never starts, stops, or configures `jevd`.

This design adds to the main spec, `2026-11-02-jev-daemon-design.md`. Where the two disagree on `jev-tray`, this document wins.

## 1. Behavior

| Item | Rule |
| --- | --- |
| On | `jevd` answers `status` with a success reply. The icon shows a check mark. |
| Off | Anything else: no channel, no reply in time, or an error reply. The icon shows an x. |
| Tooltip | `jevd runs` when on, `jevd is not running` when off. Windows and macOS show it. Linux does not: `tray-icon` has no tooltips on Linux, so there only the icon shows the state. |
| Menu | Two items: **Start at login**, a check item, and **Quit**. |
| Start state | Off, until the first check finishes. |
| One copy | A second `jev-tray` exits at once with success, and shows nothing. |
| Start-up failure | `jev-tray` opens no console. So when it cannot start, it writes one line, `<unix seconds> <message>`, to `jev-tray.log` in the state folder. On Windows, it also shows a message box. A failed read of the login entry also writes one line, once. |

`jev-tray` never sends an admin message. It needs no pairing and no key.

## 2. Reading the state

- Every 5 seconds, `jev-tray` calls `jev_proto::client::Connection::connect_if_running`, then sends `status`.
- `connect_if_running` never starts `jevd`. `status` is not an admin message, so `jevd`'s peer check does not apply.
- Each check closes its connection when it ends, so `jev-tray` holds no `max-connections` slot between checks.
- A check that takes more than 5 seconds counts as off.
- `jev-tray` runs one check at a time, through a `Checker`. While a check still runs, the next tick counts as off and starts nothing. So a check that hangs (for example, after someone stops `jevd` with `SIGSTOP`) never adds threads.
- The icon and tooltip change only when the state changes.

## 3. The icon

The icon is an outlined square with a `j` on its right side. The mark on the left of the `j` gives the state: a check mark for on, an x for off. The states differ by shape alone, so color-blind people can tell them apart.

The source is SVG, on a 16 by 16 grid, in one color:

```svg
<!-- both states -->
<rect x="1.75" y="1.75" width="12.5" height="12.5" rx="2" fill="none" stroke="currentColor" stroke-width="1.5"/>
<path fill="currentColor" d="M10.6 3.8h1.8v1.8h-1.8z M10.6 6.6h1.8v4.7a2.1 2.1 0 0 1-2.1 2.1H8.6v-1.7h1.6a.4.4 0 0 0 .4-.4z"/>
<!-- on -->
<path d="M3.8 8.4l1.6 1.7 3-3.6" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>
<!-- off -->
<path d="M4.2 6.6l3.2 3.2 M7.4 6.6l-3.2 3.2" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
```

- `crates/jev-tray/icons/` holds the two SVG files and PNGs rendered from them at 16, 20, 24, 32, and 48 pixels, in black and in white.
- A developer renders the PNGs once and commits them. The build needs no SVG renderer.
- A test checks that each committed PNG has the size its name gives.
- At 16 pixels, the edges of the icon are soft, because the border and the `j` sit on half pixels. This look is a deliberate choice.

| System | How the icon matches the bar |
| --- | --- |
| macOS | The black PNG is a template image, so the menu bar recolors it for light and dark mode. |
| Windows | `jev-tray` reads `SystemUsesLightTheme` under `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize`. A light taskbar gets the black icon; a dark one, the white icon. When the value is missing, the taskbar counts as dark. It reads the value again at each check, so a theme change shows within 5 seconds. |
| Linux | The white icon, always. Linux panels do not report their theme reliably, and most panels are dark. |

## 4. Start at login

**Start at login** is off until you tick it. Ticking it writes one entry; unticking it deletes that entry. At start, `jev-tray` reads the entry, so the check item matches the system when `jev-tray` starts. `jev-tray` does not see a change that you make outside `jev-tray` while it runs.

| System | Entry |
| --- | --- |
| Windows | The string value `jev-tray` under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, holding the quoted full path of `jev-tray.exe`. |
| macOS | `~/Library/LaunchAgents/com.github.mattbran87.jev-tray.plist`, with `RunAtLoad` true and the full path of `jev-tray`. |
| Linux | `~/.config/autostart/jev-tray.desktop` (or under `$XDG_CONFIG_HOME/autostart`), with `Exec=` the full path of `jev-tray`. |

- The entry holds the path of the running `jev-tray`, from `current_exe`.
- If the entry exists but names another path, the check item shows ticked. Ticking it again writes the current path.
- A failed write or delete leaves the check item as it was and shows the error in the tooltip until the next check.

## 5. Platforms and packaging

- **Code:** a new library crate, `crates/jev-tray`. It holds the program, in `jev_tray::app::run`. It depends on `jev-proto`, not on `jevd`.
- **Binary:** the `jev` package holds the `jev-tray` binary, `crates/jev/src/bin/jev-tray.rs`. It only calls `jev_tray::app::run`. The `jev` package gets `jev-tray` through its `tray` feature, which is on by default. The binary needs that feature (`required-features`).
- **Tray and menu:** `tray-icon` 0.24.2 and its `muda` menus, with `tao` for the event loop. `tray-icon` 0.25 and later need Rust 1.90, above the workspace MSRV of 1.85. `tao` and `tray-icon` use no default features, so `jev-tray` turns off the `libxdo` feature of `tray-icon`.
- **Windows:** the binary uses the `windows` subsystem, so it opens no console window.
- **macOS:** the app has no Dock icon; it sets the activation policy to accessory.
- **Linux:** `tray-icon` needs GTK 3 to build and to run. It loads an AppIndicator library when it runs. A desktop with no tray host, such as GNOME without its AppIndicator extension, shows nothing; the README says so.
- **One copy:** `jev-tray` locks `jev-tray.lock` in the runtime folder from `jev_proto::paths`. If another `jev-tray` holds the lock, it exits with success.
  - On Windows, the lock is a `LockFileEx` lock on one byte of the file. `jev-tray` opens the file with read and write sharing. So when another program has the file open (for example, an antivirus scan), the only copy does not exit.
  - On Unix, the lock is `flock`.

### Release

- `jev-tray` is a binary of the `jev` app. So the `jev` archive for each of the 4 targets holds `jev`, `jevd`, and `jev-tray`, and the `jev` installers install all three.
- On Linux, only `jev-tray` links GTK. `jev` and `jevd` do not, so they still run on a Linux server with no GTK. On such a server, `jev-tray` does not start.
- `cargo install` with `--no-default-features` builds only `jev` and `jevd`, with no GTK.
- `jev-tray` needs no particular folder: it does not have to sit next to `jev`.

### CI

- On Linux, `jev-tray` needs 2 packages. It needs `libgtk-3-dev` to build. It loads the library from `libayatana-appindicator3-dev` when it runs, and CI installs that package to match a desktop.
- The Linux `rust` job and the `msrv` job install both packages first.
- The `jev` package names both packages in `[package.metadata.dist.dependencies.apt]`, so the Linux release runners install them before the build.
- The `jev` and `jevd` binaries do not link GTK: on Linux, `ldd` shows no `libgtk` for them. Only `ldd jev-tray` shows `libgtk-3`.
- The workspace tests run `jev-tray`'s unit tests on all three systems.
- `cargo deny` checks the new dependencies' licenses. A license outside the allow list fails CI, and the list grows only by a reviewed change.
- `jev-tray` adds 2 entries to `deny.toml`. Both are for Linux only:
  - A license exception allows `MPL-2.0` for the crate `option-ext` only. The global allow list does not hold `MPL-2.0`. `jev-tray` gets `option-ext` through `dirs` 6 from `tray-icon`.
  - `deny.toml` ignores the advisory RUSTSEC-2024-0370. It says that `proc-macro-error` has no maintainer. `jev-tray` uses that crate at build time only, through `glib-macros` 0.18 (from `gtk` 0.18).
  - When the MSRV allows `tray-icon` 0.25, look at both entries again.

## 6. Testing

- **The state rule:** a test runs an in-process `jevd` and checks the state in four cases: running, stopped, no channel, and a reply that never comes.
- **The login entries:** tests write and delete each system's entry under a temporary home, and check its text. The Windows test uses a test key under `HKCU\Software` whose name starts with `jev-tray-test`, never the real `Run` key.
- **The theme choice:** a test feeds both values of `SystemUsesLightTheme` and checks the icon choice.
- **The icon files:** a test checks each PNG's size.
- **By hand, once per system:** that the icon appears in a real tray, switches within 5 seconds of `jev start` and `jev stop`, follows a theme change, and that **Start at login** survives a sign-out. WSL has no tray, so the Linux check needs a Linux desktop.

## 7. Limits

- A Linux desktop with no tray host shows no icon.
- On Linux, the icon is white on every panel, so it is hard to see on a light panel.
- `tray-icon` shows no tooltip on Linux, so on Linux only the icon shows the state. A failed **Start at login** change shows only as the check item that flips back.
- `jev-tray` shows only whether `jevd` answers. It does not show keys, clients, or pending pairings.
- Moving `jev-tray` after you tick **Start at login** leaves the entry pointing at the old path until you tick it again.

## 8. Out of scope

- Starting or stopping `jevd` from the menu.
- Showing pending pairings, clients, or profiles.
- Notifications.
- An installer that ticks **Start at login** for you.
