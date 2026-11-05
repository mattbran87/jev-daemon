//! The parts of `jev-tray` that do not need a tray: the state check, the
//! icon choice, the login entry, the one-copy lock, and the start-up log
//! (the jev-tray spec).

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
