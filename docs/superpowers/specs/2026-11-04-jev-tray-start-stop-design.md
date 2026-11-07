# jev-tray Start and Stop design

`jev-tray` gets one more menu item: **Start jevd** when `jevd` is off, and **Stop jevd** when it is on. A click runs `jev start` or `jev stop`, from the folder that holds `jev-tray`.

This design adds to the tray spec, `2026-11-04-jev-tray-design.md`. Where the two disagree, this document wins. Section 6 lists the lines of the tray spec that change.

## 1. Why the tray runs `jev`

`jevd` accepts admin messages, such as `stop`, only from the `jev` binary next to it (the peer check of the main spec). So `jev-tray` cannot send `stop` itself: `jevd` refuses it. `jev-tray` runs `jev stop` instead.

Start uses the same path, so both actions behave alike, and an error has the same text as in a terminal.

## 2. The menu

| Item | Rule |
| --- | --- |
| Order | **Start jevd** or **Stop jevd**, then **Start at login**, then **Quit**. |
| Label | It follows the last check: on gives **Stop jevd**, off gives **Start jevd**. Before the first check, the state is off, so the label is **Start jevd**. |
| Open | Any click on the icon opens the menu, on all systems. `jev-tray` sets `with_menu_on_left_click(true)` itself, so a change of the `tray-icon` default cannot change this. On Linux, AppIndicator opens the menu on any click. |

## 3. A click

1. The event loop grays out the item. So a second click cannot start a second command while one runs.
2. A worker thread runs `jev start` (label **Start jevd**) or `jev stop` (label **Stop jevd**).
   - The program is `jev` (`jev.exe` on Windows) in the folder of `current_exe`, never a `jev` from `PATH`.
   - The command gets no input. `jev-tray` drops its normal output and keeps its error output.
   - On Windows, the command starts with `CREATE_NO_WINDOW`, so no console window opens.
3. If the command runs longer than 30 seconds, the thread kills it and counts a failure.
4. When the command ends, the thread sends the result to the event loop. The event loop makes the item active again.
5. The watch thread then runs a check at once, not at the next 5-second tick. So the icon and the label change right after the command, not up to 5 seconds later.

`jev stop` succeeds when `jevd` does not run, so a click on **Stop jevd** after `jevd` exits by itself is not an error.

## 4. Errors

A failure is one of these:

| Case | Text |
| --- | --- |
| No `jev` next to `jev-tray` | `cannot find jev next to jev-tray` |
| The command cannot start | The error from the system. |
| A nonzero exit | The last line of the error output that is not empty. If there is none, `jev exited with code <n>`. |
| The 30-second limit | `jev gave no answer in 30 s` |

- Each failure writes one line to `jev-tray.log` in the state folder, as start-up failures do.
- The tooltip shows the failure under the state, for example `jevd is not running` and, on the next line, `Start jevd failed: <text>`. `jev-tray` cuts the tooltip to 127 UTF-16 units, as for a login error.
- The tooltip keeps the failure until the state changes, or until the next click on **Start jevd** or **Stop jevd**. The check that runs at once after the command does not clear it. A failed **Start at login** change replaces it.
- On Linux there is no tooltip. There, the log line and the label that does not change are the only signs of a failure.

## 5. Limits

- **Stop does not last.** After **Stop jevd**, the next request from a plugin or from `jev` starts `jevd` again. The icon then shows on again within 5 seconds.
- **`jevd` can also stop by itself** when someone sets `idle-timeout`. The label follows at the next check.
- **The item needs `jev` in the same folder as `jev-tray`.** `cargo install`, each release archive, and each installer put all three programs in one folder. If you move `jev-tray` alone, the item fails with `cannot find jev next to jev-tray`. The icon and **Start at login** still work.
- **A command that hangs** keeps the item gray for at most 30 seconds.

## 6. Changes to the tray spec

| Place | Old text | New text |
| --- | --- | --- |
| Start | It does nothing else: it never starts, stops, or configures `jevd`. | It also starts and stops `jevd` through `jev`, from its menu. It never configures `jevd`. |
| Section 1, Menu | Two items: **Start at login**, a check item, and **Quit**. | Three items: **Start jevd** or **Stop jevd**, **Start at login**, and **Quit**. See the Start and Stop spec. |
| Section 1, after the table | `jev-tray` never sends an admin message. | `jev-tray` never sends an admin message itself. `jev stop` sends `stop`. |
| Section 5, Release | `jev-tray` needs no particular folder: it does not have to sit next to `jev`. | The icon works in any folder. **Start jevd** and **Stop jevd** need `jev` in the same folder as `jev-tray`. |
| Section 8 | Starting or stopping `jevd` from the menu. | (deleted) |

The README changes to match: the menu has three items, and the limit "Stop does not last" is in it.

## 7. Code

- **`crates/jev-tray/src/control.rs`** (new): the action (`Start` or `Stop`), its label, and the path of `jev` next to a given program. It also holds a function that runs a `Command` with a time limit. The function returns `Ok(())` or the error text. The function takes the `Command` and the limit as inputs, so a test can pass any program and a short limit.
- **`crates/jev-tray/src/app.rs`**:
  - the new menu item, and its label change on each state change;
  - a new message for the result of a command;
  - a channel that wakes the watch thread for a check at once, in place of its `sleep`.
- **`crates/jev-tray/src/state.rs`**: `tooltip` takes one optional error line, with its prefix (`Start at login failed`, `Start jevd failed`, or `Stop jevd failed`), in place of the login error alone.

## 8. Testing

- **The label:** a test checks the action and the label for each state.
- **The path:** a test checks that the path of `jev` is in the folder of the given program, with `.exe` on Windows.
- **The runner,** with a stand-in program in place of `jev`:
  - a command that exits with 0 gives `Ok`;
  - a nonzero exit gives the last error line;
  - a nonzero exit with no error output gives `jev exited with code <n>`;
  - a program that does not exist gives an error;
  - the runner kills a command that runs past a short limit, and gives the limit error.
- **The tooltip:** a test checks each error prefix, and the cut to 127 units.
- **By hand, on Windows:** **Start jevd** and **Stop jevd** from the menu, with no console window, and the label change within 1 second. On macOS and Linux, the same check joins the hand checks of the tray spec.
