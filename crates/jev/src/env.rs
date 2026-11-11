//! Everything a command touches outside its own code: the folders, the
//! keyring, the terminal, the standard streams, and the Jev client for
//! `jev key add --check`. Tests put in-memory parts in its place.

use std::io::{self, IsTerminal, Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use jev_proto::Paths;
use jevd::jev_http::JevClient;
use jevd::keystore::{KeyStore, OsKeyring};
use zeroize::Zeroizing;

use crate::term::{DeviceTerminal, Terminal};

/// Standard input.
pub trait Input: Send {
    fn is_terminal(&self) -> bool;
    /// Reads to the end, for text such as `jev ask`'s JSON request, through
    /// `read_capped`: more than `limit` bytes gives `Ok(None)`.
    fn read_all(&mut self, limit: usize) -> io::Result<Option<String>>;
    /// Reads to the end, for a secret such as `jev key add --stdin`'s key,
    /// through `read_secret_capped` with `SECRET_ROOM`: more gives
    /// `Ok(None)`. The text is wiped from memory when it is dropped.
    fn read_secret(&mut self) -> io::Result<Option<Zeroizing<String>>>;
}

/// Reads UTF-8 text to the end, but never more than `limit` bytes plus one:
/// more than `limit` bytes gives `Ok(None)`.
pub fn read_capped(reader: impl Read, limit: usize) -> io::Result<Option<String>> {
    let mut bytes = Vec::new();
    reader.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Ok(None);
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// Room for a key. A secret is read into a buffer of this size plus one,
/// which never grows, so no unwiped copy stays behind.
pub const SECRET_ROOM: usize = 4096;

/// Reads UTF-8 text to the end into wiped memory, but never more than
/// `limit` bytes plus one: more than `limit` bytes gives `Ok(None)`.
pub fn read_secret_capped(
    mut reader: impl Read,
    limit: usize,
) -> io::Result<Option<Zeroizing<String>>> {
    let mut bytes = Zeroizing::new(vec![0u8; limit + 1]);
    let mut length = 0;
    while length < bytes.len() {
        match reader.read(&mut bytes[length..]) {
            Ok(0) => break,
            Ok(count) => length += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    if length > limit {
        return Ok(None);
    }
    let text = std::str::from_utf8(&bytes[..length])
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let mut secret = Zeroizing::new(String::with_capacity(length));
    secret.push_str(text);
    Ok(Some(secret))
}

pub struct StdInput;

impl Input for StdInput {
    fn is_terminal(&self) -> bool {
        io::stdin().is_terminal()
    }

    fn read_all(&mut self, limit: usize) -> io::Result<Option<String>> {
        read_capped(io::stdin(), limit)
    }

    fn read_secret(&mut self) -> io::Result<Option<Zeroizing<String>>> {
        read_secret_capped(io::stdin(), SECRET_ROOM)
    }
}

/// Standard input for tests: a terminal (`None`) or this text.
pub struct TextInput(pub Option<String>);

impl Input for TextInput {
    fn is_terminal(&self) -> bool {
        self.0.is_none()
    }

    fn read_all(&mut self, limit: usize) -> io::Result<Option<String>> {
        read_capped(self.0.as_deref().unwrap_or_default().as_bytes(), limit)
    }

    fn read_secret(&mut self) -> io::Result<Option<Zeroizing<String>>> {
        read_secret_capped(
            self.0.as_deref().unwrap_or_default().as_bytes(),
            SECRET_ROOM,
        )
    }
}

/// An output stream that tests can read back.
#[derive(Clone, Default)]
pub struct Buffer(Arc<Mutex<Vec<u8>>>);

impl Buffer {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

impl Write for Buffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Looks up one environment variable.
pub type Vars = Box<dyn Fn(&str) -> Option<String> + Send>;
/// Builds the client for `jev key add --check`.
pub type JevFactory = Box<dyn Fn() -> Result<JevClient, String> + Send>;

pub struct Env {
    pub paths: Paths,
    /// The `jevd` binary that `connect_or_start` starts.
    pub jevd: PathBuf,
    pub store: Arc<dyn KeyStore>,
    pub terminal: Box<dyn Terminal>,
    pub stdin: Box<dyn Input>,
    pub out: Box<dyn Write + Send>,
    pub err: Box<dyn Write + Send>,
    /// The environment variables, such as `JEV_CLIENT`.
    pub vars: Vars,
    /// Builds the client for `jev key add --check`.
    pub jev_client: JevFactory,
}

impl Env {
    /// The real environment: the OS keyring, the terminal device, the
    /// standard streams, and the `jevd` next to this `jev`.
    pub fn from_os() -> Result<Env, String> {
        let paths =
            Paths::from_env().map_err(|error| format!("cannot find the jev folders: {error}"))?;
        let me = std::env::current_exe()
            .map_err(|error| format!("cannot find this program: {error}"))?;
        let folder = me.parent().map(PathBuf::from).unwrap_or_default();
        let service = paths.keyring_service();
        Ok(Env {
            paths,
            jevd: folder.join(format!("jevd{}", std::env::consts::EXE_SUFFIX)),
            store: Arc::new(OsKeyring::new(service)),
            terminal: Box::new(DeviceTerminal),
            stdin: Box::new(StdInput),
            out: Box::new(io::stdout()),
            err: Box::new(io::stderr()),
            vars: Box::new(|name| std::env::var(name).ok()),
            jev_client: Box::new(JevClient::new),
        })
    }
}
