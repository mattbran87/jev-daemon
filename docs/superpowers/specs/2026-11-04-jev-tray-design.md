# jev-tray design

`jev-tray` is a small program that shows one icon in the system tray. The icon tells you whether `jevd` runs. It does nothing else: it never starts, stops, or configures `jevd`.

This design adds to the main spec, `2026-11-02-jev-daemon-design.md`. Where the two disagree on `jev-tray`, this document wins.

## 1. Behavior

| Item | Rule |
| --- | --- |
| On | `jevd` answers `status` with a success reply. The icon shows a check mark. |
| Off | Anything else: no channel, no reply in time, or an error reply. The icon shows an x. |
| Tooltip | `jevd runs` when on, `jevd is not running` when off. |
| Menu | Two items: **Start at login**, a check item, and **Quit**. |
| Start state | Off, until the first check finishes. |
| One copy | A second `jev-tray` exits at once with success, and shows nothing. |

`jev-tray` never sends an admin message. It needs no pairing and no key.

## 2. Reading the state

- Every 5 seconds, `jev-tray` calls `jev_proto::client::Connection::connect_if_running`, then sends `status`.
- `connect_if_running` never starts `jevd`. `status` is not an admin message, so `jevd`'s peer check does not apply.
- Each check closes its connection when it ends, so `jev-tray` holds no `max-connections` slot between checks.
- A check that takes more than 5 seconds counts as off.
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

| System | How the icon matches the bar |
| --- | --- |
| macOS | The black PNG is a template image, so the menu bar recolors it for light and dark mode. |
| Windows | `jev-tray` reads `SystemUsesLightTheme` under `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize`. A light taskbar gets the black icon; a dark one, the white icon. It reads the value again at each check, so a theme change shows within 5 seconds. |
| Linux | The white icon, always. Linux panels do not report their theme reliably, and most panels are dark. |

## 4. Start at login

**Start at login** is off until you tick it. Ticking it writes one entry; unticking it deletes that entry. At start, `jev-tray` reads the entry, so the check item always matches the system.

| System | Entry |
| --- | --- |
| Windows | The string value `jev-tray` under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, holding the quoted full path of `jev-tray.exe`. |
| macOS | `~/Library/LaunchAgents/com.github.mattbran87.jev-tray.plist`, with `RunAtLoad` true and the full path of `jev-tray`. |
| Linux | `~/.config/autostart/jev-tray.desktop` (or under `$XDG_CONFIG_HOME/autostart`), with `Exec=` the full path of `jev-tray`. |

- The entry holds the path of the running `jev-tray`, from `current_exe`.
- If the entry exists but names another path, the check item shows ticked. Ticking it again writes the current path.
- A failed write or delete leaves the check item as it was and shows the error in the tooltip until the next check.

## 5. Platforms and packaging

- **Code:** a new crate, `crates/jev-tray`, with one binary, `jev-tray`. It depends on `jev-proto`, not on `jevd`.
- **Tray and menu:** `tray-icon` 0.24.2 and its `muda` menus, with `tao` for the event loop. `tray-icon` 0.25 and later need Rust 1.90, above the workspace MSRV of 1.85.
- **Windows:** the binary uses the `windows` subsystem, so it opens no console window.
- **macOS:** the app has no Dock icon; it sets the activation policy to accessory.
- **Linux:** `tray-icon` needs GTK 3 and an AppIndicator library to build and to run. A desktop with no tray host, such as GNOME without its AppIndicator extension, shows nothing; the README says so.
- **One copy:** `jev-tray` locks `jev-tray.lock` in the runtime folder from `jev_proto::paths`. If another `jev-tray` holds the lock, it exits with success.

### Release

- `jev-tray` is its own `dist` app, with its own archives for the same 4 targets as `jev`.
- The `jev` archive does not change, so `jev` and `jevd` still run on a Linux server with no GTK.
- `jev-tray` needs no particular folder: it does not have to sit next to `jev`.

### CI

- Linux builds of `jev-tray` need `libgtk-3-dev`, `libxdo-dev`, and `libayatana-appindicator3-dev`. The Linux `rust` job and the Linux `jev-tray` release builds install them first.
- The workspace tests run `jev-tray`'s unit tests on all three systems.
- `cargo deny` checks the new dependencies' licenses. A license outside the allow list fails CI, and the list grows only by a reviewed change.

## 6. Testing

- **The state rule:** a test runs an in-process `jevd` and checks the state in four cases: running, stopped, no channel, and a reply that never comes.
- **The login entries:** tests write and delete each system's entry under a temporary home, and check its text. The Windows test uses a test key under `HKCU\Software\jev-tray-test`, never the real `Run` key.
- **The theme choice:** a test feeds both values of `SystemUsesLightTheme` and checks the icon choice.
- **The icon files:** a test checks each PNG's size.
- **By hand, once per system:** that the icon appears in a real tray, switches within 5 seconds of `jev start` and `jev stop`, follows a theme change, and that **Start at login** survives a sign-out. WSL has no tray, so the Linux check needs a Linux desktop.

## 7. Limits

- A Linux desktop with no tray host shows no icon.
- On Linux, the icon is white on every panel, so it is hard to see on a light panel.
- `jev-tray` shows only whether `jevd` answers. It does not show keys, clients, or pending pairings.
- Moving `jev-tray` after you tick **Start at login** leaves the entry pointing at the old path until you tick it again.

## 8. Out of scope

- Starting or stopping `jevd` from the menu.
- Showing pending pairings, clients, or profiles.
- Notifications.
- An installer that ticks **Start at login** for you.
