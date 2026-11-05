//! `jev-tray`: the tray program (in [`app`]), and the parts of it that do
//! not need a tray: the state check, the icon choice, the login entry, the
//! one-copy lock, and the start-up log (the jev-tray spec). The `jev-tray`
//! binary is in the `jev` package, so it ships with `jev` and `jevd`.

pub mod app;
pub mod icon;
pub mod log;
pub mod login;
pub mod one_copy;
pub mod state;

/// `text` as UTF-16 with a zero at the end, for the Windows API.
#[cfg(windows)]
#[doc(hidden)]
pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
