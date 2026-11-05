//! jev-tray: one tray icon that shows whether `jevd` runs (the jev-tray
//! spec). The program is in the `jev-tray` library; this binary is in the
//! `jev` package, so it ships with `jev` and `jevd`.

// No console window on Windows in a release build; a debug build keeps one,
// so `cargo run` shows what the program prints.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() {
    jev_tray::app::run();
}
