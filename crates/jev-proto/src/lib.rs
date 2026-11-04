//! Wire types shared by jevd, the jev CLI, and third-party clients.

pub mod client;
mod error;
mod message;
mod names;
#[cfg(windows)]
pub mod os;
pub mod paths;

pub use error::{ErrorCode, WireCode, WireError};
pub use message::{Reply, Request, MAX_LINE, PROTOCOL_VERSION};
pub use names::valid_name;
pub use paths::Paths;
