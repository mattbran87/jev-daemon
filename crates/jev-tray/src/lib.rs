//! The parts of `jev-tray` that do not need a tray: the state check, the
//! icon choice, the login entry, the one-copy lock, and the start-up log
//! (the jev-tray spec).

pub mod icon;
pub mod log;
pub mod login;
pub mod one_copy;
pub mod state;
