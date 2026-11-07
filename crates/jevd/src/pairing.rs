//! Pairing (spec section 3): pending requests in memory, paired clients in a
//! JSON file that holds token hashes, never tokens. Only jevd writes the file.
//!
//! The requests that wait for a person also go to a pending file, so they
//! survive a restart (the pairing-restart spec). It holds no token and no
//! request ID. An approved request that is not committed stays in memory only.
//!
//! A client is committed (saved, and able to ask) only when its token reaches
//! its storage (spec decision 18). For a socket client, that is the first
//! `pair-status` after the approval. For a CLI client, `approve` gives the
//! token to `jev`, which writes it to the keyring and then sends `pair-status`.

use crate::files::write_atomic;
use crate::units::Limit;
use jev_proto::{valid_name, ErrorCode};
use rand::rngs::OsRng;
use rand::{Rng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// No 0, O, 1, I, or L, so a code reads aloud and types without mistakes.
pub const CODE_ALPHABET: &[u8] = b"23456789ABCDEFGHJKMNPQRSTUVWXYZ";
pub const CODE_LENGTH: usize = 8;
/// How long a request waits for a person, and how long an approval waits for
/// its commit. The second window starts at the approval.
pub const PENDING_EXPIRY: Duration = Duration::from_secs(600);
/// Waiting requests and approvals that are not committed both hold a slot.
pub const MAX_PENDING: usize = 16;
pub const TOKEN_PREFIX: &str = "jevt_";
/// The lowercase hex digits after `TOKEN_PREFIX` in a token: 32 random bytes.
pub const TOKEN_HEX_LENGTH: usize = 64;
/// The lowercase hex digits of a request id: 16 random bytes.
pub const REQUEST_ID_HEX_LENGTH: usize = 32;
/// The last-used time moves at most once per this many seconds per client.
const LAST_USED_STEP: u64 = 60;

/// One paired client, as stored in the pairing file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Client {
    pub name: String,
    pub profile: String,
    pub token_sha256: String,
    /// Unix seconds.
    pub created: u64,
    /// Unix seconds.
    #[serde(default)]
    pub last_used: Option<u64>,
    /// A per-client rate limit, `<count>/<seconds>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<String>,
}

impl Client {
    /// The per-client override, if one is set and readable.
    pub fn limit(&self) -> Option<Limit> {
        self.limit.as_deref().and_then(|text| text.parse().ok())
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    clients: Vec<Client>,
    /// Hashes of revoked and replaced tokens, so jevd can answer `revoked`.
    #[serde(default)]
    revoked: Vec<String>,
}

/// The pending file: the requests that wait for a person.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PendingFile {
    #[serde(default)]
    requests: Vec<SavedRequest>,
}

/// One waiting request in the pending file.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedRequest {
    /// The SHA-256 of the request ID, as lowercase hex. Never the ID itself.
    id_sha256: String,
    code: String,
    name: String,
    profile: Option<String>,
    /// True when the request came from the `jev` binary.
    cli: bool,
    /// The end of the wait, in Unix seconds.
    deadline: u64,
}

/// Where the waiting requests go, and the two clocks at the load, so that a
/// deadline in memory can be written in Unix seconds.
struct PendingStore {
    path: PathBuf,
    loaded: Instant,
    loaded_unix: u64,
}

impl PendingStore {
    /// The Unix time of `at`.
    fn unix_at(&self, at: Instant) -> u64 {
        match at.checked_duration_since(self.loaded) {
            Some(after) => self.loaded_unix.saturating_add(after.as_secs()),
            None => self
                .loaded_unix
                .saturating_sub(self.loaded.duration_since(at).as_secs()),
        }
    }
}

/// What `pair-status` reports.
#[derive(Clone, PartialEq, Eq)]
pub enum PairState {
    Pending,
    /// The client is committed. A socket client gets its token here, once
    /// only. A CLI client gets `None`: `approve` gave its token to `jev`.
    Approved(Option<String>),
    Denied,
    /// Also the answer for an unknown request ID.
    Expired,
}

/// Written by hand so the token never reaches a log or a panic message.
impl std::fmt::Debug for PairState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PairState::Pending => f.write_str("Pending"),
            PairState::Approved(_) => f.write_str("Approved(<redacted>)"),
            PairState::Denied => f.write_str("Denied"),
            PairState::Expired => f.write_str("Expired"),
        }
    }
}

impl PairState {
    pub fn as_str(&self) -> &'static str {
        match self {
            PairState::Pending => "pending",
            PairState::Approved(_) => "approved",
            PairState::Denied => "denied",
            PairState::Expired => "expired",
        }
    }
}

/// A pending request, as `jev allow` lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingView {
    pub code: String,
    pub name: String,
    pub profile: Option<String>,
}

/// What `approve` returns. Nothing is saved yet.
#[derive(Debug)]
pub struct Approval {
    pub name: String,
    /// True when a client of that name is committed now. The commit replaces it.
    pub replaces: bool,
    /// For a request from `jev`: the request ID and the token, for `jev` to store.
    pub cli: Option<CliToken>,
}

/// The token of an approved CLI request, and the request ID that commits it.
pub struct CliToken {
    pub request_id: String,
    pub token: String,
}

/// Written by hand so the token never reaches a log or a panic message.
impl std::fmt::Debug for CliToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CliToken(<redacted>)")
    }
}

enum Stage {
    Waiting,
    /// Approved, not committed. `token` is `None` for a CLI request.
    Approved {
        profile: String,
        token_sha256: String,
        token: Option<String>,
    },
    Denied,
}

struct Pending {
    code: String,
    name: String,
    profile: Option<String>,
    /// The request ID of a request from the `jev` binary: a repeat `start`
    /// and `approve` give it back. `None` for a socket client, which alone
    /// holds its ID.
    cli_id: Option<String>,
    /// The end of the wait, or of the collection window once approved.
    deadline: Instant,
    stage: Stage,
}

impl Pending {
    fn holds_slot(&self) -> bool {
        !matches!(self.stage, Stage::Denied)
    }

    fn approved_profile(&self) -> Option<&str> {
        match &self.stage {
            Stage::Approved { profile, .. } => Some(profile),
            _ => None,
        }
    }
}

pub struct Pairings {
    path: PathBuf,
    clients: BTreeMap<String, Client>,
    revoked: BTreeSet<String>,
    /// By the SHA-256 of the request ID, so a request loaded from the pending
    /// file, which holds no request ID, is found the same way.
    pending: HashMap<String, Pending>,
    /// `None` until `load_pending`: until then, nothing writes a pending file.
    pending_store: Option<PendingStore>,
}

/// The SHA-256 of a token, as lowercase hex.
pub fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

/// The SHA-256 of a request ID, as lowercase hex.
fn hash_request_id(request_id: &str) -> String {
    hex::encode(Sha256::digest(request_id.as_bytes()))
}

fn random_hex(bytes: usize) -> String {
    let mut buffer = vec![0u8; bytes];
    OsRng.fill_bytes(&mut buffer);
    hex::encode(buffer)
}

fn new_code() -> String {
    (0..CODE_LENGTH)
        .map(|_| CODE_ALPHABET[OsRng.gen_range(0..CODE_ALPHABET.len())] as char)
        .collect()
}

/// A typed code, ready to compare: spaces trimmed, letters uppercased.
pub fn normalize_code(code: &str) -> String {
    code.trim().to_ascii_uppercase()
}

fn valid_code(code: &str) -> bool {
    code.len() == CODE_LENGTH && code.bytes().all(|b| CODE_ALPHABET.contains(&b))
}

/// Deletes a pending file that jevd cannot use, and returns the line for the log.
fn discard(path: &Path, problem: &str) -> String {
    let _ = std::fs::remove_file(path);
    format!(
        "cannot use {}: {problem}; jevd deletes it and starts with no waiting pairing requests",
        path.display()
    )
}

impl Pairings {
    /// Read the pairing file, or start empty when it does not exist.
    pub fn load(path: &Path) -> Result<Pairings, String> {
        let file: File = match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text)
                .map_err(|error| format!("{}: {error}", path.display()))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => File::default(),
            Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
        };
        // Fail closed: a name or a token hash that appears twice is a damaged file.
        let mut clients = BTreeMap::new();
        let mut hashes = BTreeSet::new();
        for client in file.clients {
            if !hashes.insert(client.token_sha256.clone()) {
                return Err(format!("{}: a token hash appears twice", path.display()));
            }
            if clients.contains_key(&client.name) {
                return Err(format!(
                    "{}: the client {} appears twice",
                    path.display(),
                    client.name
                ));
            }
            // Fail closed: a limit that cannot be read must not fall back to the global default.
            if let Some(text) = &client.limit {
                if text.parse::<Limit>().is_err() {
                    return Err(format!(
                        "{}: the client {} has a limit that cannot be read",
                        path.display(),
                        client.name
                    ));
                }
            }
            clients.insert(client.name.clone(), client);
        }
        Ok(Pairings {
            path: path.to_path_buf(),
            clients,
            revoked: file.revoked.into_iter().collect(),
            pending: HashMap::new(),
            pending_store: None,
        })
    }

    /// Reads the pending file, once, at start, and writes the waiting
    /// requests to it from now on. `now` and `now_unix` are the same moment
    /// on the two clocks. A file that jevd cannot use never stops it: the
    /// file is deleted, and the returned line goes to the log.
    pub fn load_pending(&mut self, path: &Path, now: Instant, now_unix: u64) -> Option<String> {
        self.pending_store = Some(PendingStore {
            path: path.to_path_buf(),
            loaded: now,
            loaded_unix: now_unix,
        });
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
            Err(error) => return Some(discard(path, &error.to_string())),
        };
        let file: PendingFile = match serde_json::from_str(&text) {
            Ok(file) => file,
            // The place only: serde's own text can quote the file.
            Err(error) => {
                let problem = format!(
                    "not a pending file (line {}, column {})",
                    error.line(),
                    error.column()
                );
                return Some(discard(path, &problem));
            }
        };
        let mut codes = BTreeSet::new();
        // Each entry with the seconds it has left, at most 10 minutes, so a
        // clock moved back cannot keep a request alive longer.
        let mut kept: Vec<(u64, SavedRequest)> = Vec::new();
        for saved in file.requests {
            if saved.deadline <= now_unix {
                continue;
            }
            let valid = valid_name(&saved.name)
                && saved.profile.as_deref().is_none_or(valid_name)
                && valid_code(&saved.code);
            if !valid || !codes.insert(saved.code.clone()) {
                continue;
            }
            let left = (saved.deadline - now_unix).min(PENDING_EXPIRY.as_secs());
            kept.push((left, saved));
        }
        // The latest deadlines stay, and the earliest go first.
        kept.sort_by_key(|(left, _)| std::cmp::Reverse(*left));
        kept.truncate(MAX_PENDING);
        for (left, saved) in kept {
            // jevd does not know the ID of a request from `jev` any more, and
            // `jev` never polls with it, so it gets a new one.
            let cli_id = saved.cli.then(|| random_hex(REQUEST_ID_HEX_LENGTH / 2));
            let key = cli_id.as_deref().map_or(saved.id_sha256, hash_request_id);
            if self.pending.contains_key(&key) {
                continue;
            }
            self.pending.insert(
                key,
                Pending {
                    code: saved.code,
                    name: saved.name,
                    profile: saved.profile,
                    cli_id,
                    deadline: now + Duration::from_secs(left),
                    stage: Stage::Waiting,
                },
            );
        }
        // Write at once, so a dropped entry and a cut deadline stay so at the
        // next start.
        self.save_pending();
        None
    }

    /// Writes the waiting requests to the pending file, after a load. A
    /// failed write fails nothing: it goes to the log, and the next change
    /// writes the whole file again.
    fn save_pending(&self) {
        let Some(store) = &self.pending_store else {
            return;
        };
        let mut requests: Vec<SavedRequest> = self
            .pending
            .iter()
            .filter(|(_, pending)| matches!(pending.stage, Stage::Waiting))
            .map(|(key, pending)| SavedRequest {
                id_sha256: key.clone(),
                code: pending.code.clone(),
                name: pending.name.clone(),
                profile: pending.profile.clone(),
                cli: pending.cli_id.is_some(),
                deadline: store.unix_at(pending.deadline),
            })
            .collect();
        requests.sort_by(|a, b| a.code.cmp(&b.code));
        let written = serde_json::to_string_pretty(&PendingFile { requests })
            .map_err(|error| error.to_string())
            .and_then(|text| write_atomic(&store.path, &text).map_err(|error| error.to_string()));
        if let Err(problem) = written {
            crate::log::event(&format!("cannot save {}: {problem}", store.path.display()));
        }
    }

    fn waiting_count(&self) -> usize {
        self.pending
            .values()
            .filter(|pending| matches!(pending.stage, Stage::Waiting))
            .count()
    }

    /// Write a new client list and revoked list to the file, and only then put
    /// them in memory. A failed save leaves memory as it was, so memory and disk agree.
    fn save(
        &mut self,
        clients: BTreeMap<String, Client>,
        revoked: BTreeSet<String>,
    ) -> Result<(), ErrorCode> {
        let file = File {
            clients: clients.values().cloned().collect(),
            revoked: revoked.iter().cloned().collect(),
        };
        let text = serde_json::to_string_pretty(&file).map_err(|_| ErrorCode::Internal)?;
        write_atomic(&self.path, &text).map_err(|_| ErrorCode::Internal)?;
        self.clients = clients;
        self.revoked = revoked;
        Ok(())
    }

    /// Drops expired requests. Writes the pending file when a waiting
    /// request was among them.
    fn purge(&mut self, now: Instant) {
        let waiting = self.waiting_count();
        self.pending.retain(|_, pending| now < pending.deadline);
        if self.waiting_count() != waiting {
            self.save_pending();
        }
    }

    /// The key of the waiting request with `code`.
    fn waiting_key(&self, code: &str) -> Option<String> {
        let code = normalize_code(code);
        self.pending
            .iter()
            .find(|(_, pending)| pending.code == code && matches!(pending.stage, Stage::Waiting))
            .map(|(key, _)| key.clone())
    }

    /// How many approvals that are not committed `which` picks.
    fn count_approvals(&self, which: impl Fn(&Pending) -> bool) -> usize {
        self.pending
            .values()
            .filter(|pending| pending.approved_profile().is_some() && which(pending))
            .count()
    }

    /// Drops every approval that is not committed and that `which` picks.
    /// Returns how many it dropped.
    fn cancel_approvals(&mut self, which: impl Fn(&Pending) -> bool) -> usize {
        let before = self.pending.len();
        self.pending
            .retain(|_, pending| pending.approved_profile().is_none() || !which(pending));
        before - self.pending.len()
    }

    /// Start a pairing. Returns the request ID and the code. `cli` is true
    /// when the request came from the `jev` binary. A request from `jev` for
    /// a name that already has a waiting request from `jev` gets that
    /// request's ID and code back, and its expiry does not restart. The first
    /// request's suggested profile is kept.
    pub fn start(
        &mut self,
        name: &str,
        profile: Option<&str>,
        cli: bool,
        now: Instant,
    ) -> Result<(String, String), ErrorCode> {
        if !valid_name(name) || profile.is_some_and(|profile| !valid_name(profile)) {
            return Err(ErrorCode::BadRequest);
        }
        self.purge(now);
        if cli {
            let repeat = self
                .pending
                .values()
                .find_map(|pending| match &pending.cli_id {
                    Some(id) if pending.name == name && matches!(pending.stage, Stage::Waiting) => {
                        Some((id.clone(), pending.code.clone()))
                    }
                    _ => None,
                });
            if let Some(repeat) = repeat {
                return Ok(repeat);
            }
        }
        let held = self
            .pending
            .values()
            .filter(|pending| pending.holds_slot())
            .count();
        if held >= MAX_PENDING {
            return Err(ErrorCode::RateLimited);
        }
        let mut code = new_code();
        while self.pending.values().any(|pending| pending.code == code) {
            code = new_code();
        }
        let request_id = random_hex(REQUEST_ID_HEX_LENGTH / 2);
        self.pending.insert(
            hash_request_id(&request_id),
            Pending {
                code: code.clone(),
                name: name.to_string(),
                profile: profile.map(str::to_string),
                cli_id: cli.then(|| request_id.clone()),
                deadline: now + PENDING_EXPIRY,
                stage: Stage::Waiting,
            },
        );
        self.save_pending();
        Ok((request_id, code))
    }

    /// The state of a request. `Approved` commits the client: it saves the
    /// client, and revokes an old client of the same name. `Approved` and
    /// `Denied` end the request. A failed save is `internal` and keeps the
    /// request, so the client can poll again. An approval whose profile is
    /// not in `profiles` any more is dropped, and reports `expired`.
    pub fn status(
        &mut self,
        request_id: &str,
        profiles: &[String],
        now: Instant,
        now_unix: u64,
    ) -> Result<PairState, ErrorCode> {
        self.purge(now);
        let key = hash_request_id(request_id);
        let Some(pending) = self.pending.get(&key) else {
            return Ok(PairState::Expired);
        };
        let (profile, token_sha256, token) = match &pending.stage {
            Stage::Waiting => return Ok(PairState::Pending),
            Stage::Denied => {
                self.pending.remove(&key);
                return Ok(PairState::Denied);
            }
            Stage::Approved { profile, .. } if !profiles.contains(profile) => {
                self.pending.remove(&key);
                return Ok(PairState::Expired);
            }
            Stage::Approved {
                profile,
                token_sha256,
                token,
            } => (profile.clone(), token_sha256.clone(), token.clone()),
        };
        let name = pending.name.clone();
        let mut clients = self.clients.clone();
        let mut revoked = self.revoked.clone();
        if let Some(old) = clients.remove(&name) {
            revoked.insert(old.token_sha256);
        }
        clients.insert(
            name.clone(),
            Client {
                name,
                profile,
                token_sha256,
                created: now_unix,
                last_used: None,
                limit: None,
            },
        );
        self.save(clients, revoked)?;
        self.pending.remove(&key);
        Ok(PairState::Approved(token))
    }

    /// The requests that wait for a person, sorted by code.
    pub fn pending_list(&mut self, now: Instant) -> Vec<PendingView> {
        self.purge(now);
        let mut list: Vec<PendingView> = self
            .pending
            .values()
            .filter(|pending| matches!(pending.stage, Stage::Waiting))
            .map(|pending| PendingView {
                code: pending.code.clone(),
                name: pending.name.clone(),
                profile: pending.profile.clone(),
            })
            .collect();
        list.sort_by(|a, b| a.code.cmp(&b.code));
        list
    }

    /// Approve the request with `code`, bound to `profile`. Saves nothing:
    /// `status` commits the client. An approval of the same name that is not
    /// committed gets cancelled. The collection window starts now.
    pub fn approve(
        &mut self,
        code: &str,
        profile: &str,
        now: Instant,
    ) -> Result<Approval, ErrorCode> {
        if !valid_name(profile) {
            return Err(ErrorCode::BadRequest);
        }
        self.purge(now);
        let key = self.waiting_key(code).ok_or(ErrorCode::BadRequest)?;
        let name = self.pending[&key].name.clone();
        self.cancel_approvals(|pending| pending.name == name);
        let token = format!("{TOKEN_PREFIX}{}", random_hex(TOKEN_HEX_LENGTH / 2));
        let pending = self.pending.get_mut(&key).expect("found above");
        let cli_id = pending.cli_id.clone();
        pending.stage = Stage::Approved {
            profile: profile.to_string(),
            token_sha256: hash_token(&token),
            token: cli_id.is_none().then(|| token.clone()),
        };
        pending.deadline = now + PENDING_EXPIRY;
        // The request stops waiting. Its token never reaches the file.
        self.save_pending();
        Ok(Approval {
            replaces: self.clients.contains_key(&name),
            name,
            cli: cli_id.map(|request_id| CliToken { request_id, token }),
        })
    }

    pub fn deny(&mut self, code: &str, now: Instant) -> Result<(), ErrorCode> {
        self.purge(now);
        let key = self.waiting_key(code).ok_or(ErrorCode::BadRequest)?;
        self.pending.get_mut(&key).expect("found above").stage = Stage::Denied;
        self.save_pending();
        Ok(())
    }

    fn lookup(&self, token: &str) -> Result<&Client, ErrorCode> {
        let hash = hash_token(token);
        if self.revoked.contains(&hash) {
            return Err(ErrorCode::Revoked);
        }
        self.clients
            .values()
            .find(|client| client.token_sha256 == hash)
            .ok_or(ErrorCode::NotPaired)
    }

    /// The client that owns `token` now: `revoked` or `not-paired` when none.
    /// Unlike `authenticate`, it never writes.
    pub fn current(&self, token: &str) -> Result<Client, ErrorCode> {
        self.lookup(token).cloned()
    }

    /// The client that owns `token`. Moves its last-used time at most once a minute.
    pub fn authenticate(&mut self, token: &str, now_unix: u64) -> Result<Client, ErrorCode> {
        let client = self.lookup(token)?;
        let fresh =
            matches!(client.last_used, Some(t) if now_unix.saturating_sub(t) < LAST_USED_STEP);
        if fresh {
            return Ok(client.clone());
        }
        let name = client.name.clone();
        let mut clients = self.clients.clone();
        clients.get_mut(&name).expect("found above").last_used = Some(now_unix);
        // The last-used time is a convenience. A failed write must not fail the
        // request. It leaves the old time in memory, as on disk.
        let _ = self.save(clients, self.revoked.clone());
        Ok(self.clients[&name].clone())
    }

    /// Revoke the client `name`, and cancel its approvals that are not
    /// committed. `bad-request` when there is neither. A failed save
    /// cancels nothing.
    pub fn revoke(&mut self, name: &str) -> Result<(), ErrorCode> {
        let uncommitted = self.count_approvals(|pending| pending.name == name);
        let mut clients = self.clients.clone();
        let mut revoked = self.revoked.clone();
        let Some(client) = clients.remove(name) else {
            if uncommitted == 0 {
                return Err(ErrorCode::BadRequest);
            }
            self.cancel_approvals(|pending| pending.name == name);
            return Ok(());
        };
        revoked.insert(client.token_sha256);
        self.save(clients, revoked)?;
        self.cancel_approvals(|pending| pending.name == name);
        Ok(())
    }

    /// Revoke every client bound to `profile`, and cancel the approvals on it
    /// that are not committed. Returns the names of the revoked clients, sorted.
    pub fn revoke_profile(&mut self, profile: &str) -> Result<Vec<String>, ErrorCode> {
        let names: Vec<String> = self
            .clients
            .values()
            .filter(|client| client.profile == profile)
            .map(|client| client.name.clone())
            .collect();
        let mut clients = self.clients.clone();
        let mut revoked = self.revoked.clone();
        for name in &names {
            let client = clients.remove(name).expect("listed above");
            revoked.insert(client.token_sha256);
        }
        self.save(clients, revoked)?;
        self.cancel_approvals(|pending| pending.approved_profile() == Some(profile));
        Ok(names)
    }

    /// Set a client's rate limit, or clear it with None.
    pub fn set_limit(&mut self, name: &str, limit: Option<Limit>) -> Result<(), ErrorCode> {
        let mut clients = self.clients.clone();
        let client = clients.get_mut(name).ok_or(ErrorCode::BadRequest)?;
        client.limit = limit.map(|limit| limit.to_string());
        self.save(clients, self.revoked.clone())
    }

    /// Every paired client, sorted by name.
    pub fn clients(&self) -> Vec<Client> {
        self.clients.values().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: u64 = 1_800_000_000;

    fn store() -> (tempfile::TempDir, Pairings) {
        let dir = tempfile::tempdir().unwrap();
        let pairings = Pairings::load(&dir.path().join("pairings.json")).unwrap();
        (dir, pairings)
    }

    /// The profiles that the settings file knows, in these tests.
    fn known() -> Vec<String> {
        vec!["default".to_string(), "work".to_string()]
    }

    /// Pair a socket client `name` on `profile` all the way, and return its token.
    fn pair_on(pairings: &mut Pairings, name: &str, profile: &str, now: Instant) -> String {
        let (request_id, code) = pairings.start(name, None, false, now).unwrap();
        pairings.approve(&code, profile, now).unwrap();
        match pairings.status(&request_id, &known(), now, T).unwrap() {
            PairState::Approved(Some(token)) => token,
            other => panic!("expected approved, got {other:?}"),
        }
    }

    fn pair(pairings: &mut Pairings, name: &str, now: Instant) -> String {
        pair_on(pairings, name, "default", now)
    }

    #[test]
    fn codes_and_tokens_have_the_spec_shape() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let (request_id, code) = pairings.start("app", None, false, now).unwrap();
        assert_eq!(request_id.len(), 32);
        assert_eq!(code.len(), CODE_LENGTH);
        assert!(code.bytes().all(|b| CODE_ALPHABET.contains(&b)));
        for look_alike in ["0", "O", "1", "I", "L"] {
            assert!(!std::str::from_utf8(CODE_ALPHABET)
                .unwrap()
                .contains(look_alike));
        }
        let token = pair(&mut pairings, "other", now);
        assert!(token.starts_with(TOKEN_PREFIX));
        assert_eq!(token.len(), TOKEN_PREFIX.len() + 64);
    }

    #[test]
    fn the_token_comes_back_once_only() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let (request_id, code) = pairings.start("app", Some("work"), false, now).unwrap();
        assert_eq!(
            pairings.status(&request_id, &known(), now, T),
            Ok(PairState::Pending)
        );
        assert_eq!(
            pairings.pending_list(now),
            [PendingView {
                code: code.clone(),
                name: "app".into(),
                profile: Some("work".into())
            }]
        );
        let approval = pairings.approve(&code.to_lowercase(), "work", now).unwrap();
        assert_eq!(approval.name, "app");
        assert!(!approval.replaces);
        assert!(approval.cli.is_none(), "a socket client collects its token");
        assert!(matches!(
            pairings.status(&request_id, &known(), now, T),
            Ok(PairState::Approved(Some(_)))
        ));
        assert_eq!(
            pairings.status(&request_id, &known(), now, T),
            Ok(PairState::Expired)
        );
        assert!(pairings.pending_list(now).is_empty());
    }

    #[test]
    fn a_wrong_request_id_reports_expired() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        pairings.start("app", None, false, now).unwrap();
        assert_eq!(
            pairings.status("0123456789abcdef0123456789abcdef", &known(), now, T),
            Ok(PairState::Expired)
        );
    }

    #[test]
    fn deny_ends_the_request() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let (request_id, code) = pairings.start("app", None, false, now).unwrap();
        pairings.deny(&code, now).unwrap();
        assert_eq!(
            pairings.status(&request_id, &known(), now, T),
            Ok(PairState::Denied)
        );
        assert_eq!(
            pairings.status(&request_id, &known(), now, T),
            Ok(PairState::Expired)
        );
        assert_eq!(pairings.deny(&code, now), Err(ErrorCode::BadRequest));
    }

    #[test]
    fn the_17th_pending_request_is_refused() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        for i in 0..MAX_PENDING {
            pairings
                .start(&format!("app{i}"), None, false, now)
                .unwrap();
        }
        assert_eq!(
            pairings.start("one-more", None, false, now),
            Err(ErrorCode::RateLimited)
        );
    }

    #[test]
    fn a_pending_request_expires_after_ten_minutes() {
        let (_dir, mut pairings) = store();
        let t0 = Instant::now();
        let (request_id, code) = pairings.start("app", None, false, t0).unwrap();
        let almost = t0 + PENDING_EXPIRY - Duration::from_secs(1);
        assert_eq!(
            pairings.status(&request_id, &known(), almost, T),
            Ok(PairState::Pending)
        );
        let late = t0 + PENDING_EXPIRY;
        assert_eq!(
            pairings.status(&request_id, &known(), late, T),
            Ok(PairState::Expired)
        );
        assert_eq!(
            pairings.approve(&code, "default", late).err(),
            Some(ErrorCode::BadRequest)
        );
    }

    #[test]
    fn bad_names_are_refused() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        assert_eq!(
            pairings.start("Bad Name", None, false, now),
            Err(ErrorCode::BadRequest)
        );
        assert_eq!(
            pairings.start("app", Some("Bad"), false, now),
            Err(ErrorCode::BadRequest)
        );
        let (_, code) = pairings.start("app", None, false, now).unwrap();
        assert_eq!(
            pairings.approve(&code, "Bad", now).err(),
            Some(ErrorCode::BadRequest)
        );
    }

    #[test]
    fn current_looks_a_token_up_and_never_writes() {
        let (dir, mut pairings) = store();
        let now = Instant::now();
        let token = pair(&mut pairings, "app", now);
        let file = dir.path().join("pairings.json");
        let before = std::fs::read(&file).unwrap();
        assert_eq!(pairings.current(&token).unwrap().name, "app");
        assert_eq!(pairings.current("jevt_wrong"), Err(ErrorCode::NotPaired));
        assert_eq!(std::fs::read(&file).unwrap(), before, "no write");
        assert_eq!(pairings.clients()[0].last_used, None);
        pairings.revoke("app").unwrap();
        assert_eq!(pairings.current(&token), Err(ErrorCode::Revoked));
    }

    #[test]
    fn a_token_authenticates_its_client() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let token = pair(&mut pairings, "app", now);
        let client = pairings.authenticate(&token, T).unwrap();
        assert_eq!(
            (client.name.as_str(), client.profile.as_str()),
            ("app", "default")
        );
        assert_eq!(
            pairings.authenticate("jevt_wrong", T),
            Err(ErrorCode::NotPaired)
        );
    }

    #[test]
    fn an_approval_saves_nothing_until_the_commit() {
        let (dir, mut pairings) = store();
        let now = Instant::now();
        let (request_id, code) = pairings.start("app", None, false, now).unwrap();
        pairings.approve(&code, "default", now).unwrap();
        assert!(pairings.clients().is_empty());
        assert!(!dir.path().join("pairings.json").exists());
        let Ok(PairState::Approved(Some(token))) = pairings.status(&request_id, &known(), now, T)
        else {
            panic!("expected the token");
        };
        assert_eq!(pairings.authenticate(&token, T).unwrap().name, "app");
        assert!(dir.path().join("pairings.json").exists());
    }

    #[test]
    fn the_old_client_works_until_the_commit_and_is_revoked_after_it() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let old = pair(&mut pairings, "app", now);
        let (request_id, code) = pairings.start("app", None, false, now).unwrap();
        let approval = pairings.approve(&code, "default", now).unwrap();
        assert!(approval.replaces);
        assert_eq!(pairings.authenticate(&old, T).unwrap().name, "app");
        let Ok(PairState::Approved(Some(new))) = pairings.status(&request_id, &known(), now, T)
        else {
            panic!("expected the token");
        };
        assert_eq!(pairings.authenticate(&old, T), Err(ErrorCode::Revoked));
        assert_eq!(pairings.authenticate(&new, T).unwrap().name, "app");
        assert_eq!(pairings.clients().len(), 1);
    }

    #[test]
    fn a_cli_request_gets_its_token_at_approval() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let (request_id, code) = pairings.start("script", None, true, now).unwrap();
        let approval = pairings.approve(&code, "default", now).unwrap();
        let cli = approval.cli.expect("a CLI request gets the token");
        assert_eq!(cli.request_id, request_id);
        assert!(cli.token.starts_with(TOKEN_PREFIX));
        assert_eq!(
            pairings.authenticate(&cli.token, T),
            Err(ErrorCode::NotPaired),
            "not committed yet"
        );
        assert_eq!(
            pairings.status(&request_id, &known(), now, T),
            Ok(PairState::Approved(None)),
            "the commit does not send the token again"
        );
        assert_eq!(pairings.authenticate(&cli.token, T).unwrap().name, "script");
    }

    #[test]
    fn a_repeat_cli_request_gets_the_same_request() {
        let (_dir, mut pairings) = store();
        let t0 = Instant::now();
        let first = pairings.start("script", None, true, t0).unwrap();
        let later = t0 + Duration::from_secs(300);
        assert_eq!(
            pairings.start("script", Some("work"), true, later).unwrap(),
            first
        );
        assert_eq!(pairings.pending_list(later).len(), 1);
        assert_eq!(
            pairings.status(&first.0, &known(), t0 + PENDING_EXPIRY, T),
            Ok(PairState::Expired),
            "the expiry does not restart"
        );
    }

    #[test]
    fn only_a_waiting_cli_request_is_shared() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let socket = pairings.start("app", None, false, now).unwrap();
        let cli = pairings.start("app", None, true, now).unwrap();
        assert_ne!(cli, socket, "a socket request is never given to jev");
        assert_ne!(pairings.start("app", None, false, now).unwrap(), socket);
        pairings.approve(&cli.1, "default", now).unwrap();
        assert_ne!(
            pairings.start("app", None, true, now).unwrap(),
            cli,
            "an approved request is not waiting"
        );
    }

    #[test]
    fn an_approval_holds_its_slot_until_the_commit() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let mut started = Vec::new();
        for i in 0..MAX_PENDING {
            started.push(
                pairings
                    .start(&format!("app{i}"), None, false, now)
                    .unwrap(),
            );
        }
        pairings.approve(&started[0].1, "default", now).unwrap();
        assert!(pairings
            .pending_list(now)
            .iter()
            .all(|view| view.code != started[0].1));
        assert_eq!(
            pairings.start("one-more", None, false, now),
            Err(ErrorCode::RateLimited)
        );
        pairings.status(&started[0].0, &known(), now, T).unwrap();
        assert!(pairings.start("one-more", None, false, now).is_ok());
    }

    #[test]
    fn a_denied_request_frees_its_slot() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let mut codes = Vec::new();
        for i in 0..MAX_PENDING {
            codes.push(
                pairings
                    .start(&format!("app{i}"), None, false, now)
                    .unwrap()
                    .1,
            );
        }
        pairings.deny(&codes[0], now).unwrap();
        assert!(pairings.start("one-more", None, false, now).is_ok());
    }

    #[test]
    fn a_second_approval_for_a_name_cancels_the_first() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let first = pairings.start("app", None, false, now).unwrap();
        let second = pairings.start("app", None, false, now).unwrap();
        pairings.approve(&first.1, "default", now).unwrap();
        pairings.approve(&second.1, "work", now).unwrap();
        assert_eq!(
            pairings.status(&first.0, &known(), now, T),
            Ok(PairState::Expired)
        );
        assert!(matches!(
            pairings.status(&second.0, &known(), now, T),
            Ok(PairState::Approved(Some(_)))
        ));
        assert_eq!(pairings.clients()[0].profile, "work");
    }

    #[test]
    fn revoke_cancels_an_approval_that_is_not_committed() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let (request_id, code) = pairings.start("app", None, false, now).unwrap();
        pairings.approve(&code, "default", now).unwrap();
        assert_eq!(pairings.revoke("app"), Ok(()));
        assert_eq!(
            pairings.status(&request_id, &known(), now, T),
            Ok(PairState::Expired)
        );
        assert!(pairings.clients().is_empty());
        assert_eq!(pairings.revoke("app"), Err(ErrorCode::BadRequest));
    }

    #[test]
    fn revoke_profile_cancels_approvals_on_it() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let on_it = pairings.start("a", None, false, now).unwrap();
        let other = pairings.start("b", None, false, now).unwrap();
        pairings.approve(&on_it.1, "default", now).unwrap();
        pairings.approve(&other.1, "work", now).unwrap();
        assert!(pairings.revoke_profile("default").unwrap().is_empty());
        assert_eq!(
            pairings.status(&on_it.0, &known(), now, T),
            Ok(PairState::Expired)
        );
        assert!(matches!(
            pairings.status(&other.0, &known(), now, T),
            Ok(PairState::Approved(Some(_)))
        ));
    }

    #[test]
    fn an_approval_whose_profile_went_away_expires() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let (request_id, code) = pairings.start("app", None, false, now).unwrap();
        pairings.approve(&code, "default", now).unwrap();
        assert_eq!(
            pairings.status(&request_id, &["work".to_string()], now, T),
            Ok(PairState::Expired)
        );
        assert!(pairings.clients().is_empty());
    }

    #[test]
    fn a_failed_save_at_the_commit_keeps_the_request() {
        let (dir, mut pairings) = store();
        let path = dir.path().join("pairings.json");
        let now = Instant::now();
        let old = pair(&mut pairings, "app", now);
        let (request_id, code) = pairings.start("app", None, false, now).unwrap();
        pairings.approve(&code, "default", now).unwrap();
        break_saves(&path);
        assert_eq!(
            pairings.status(&request_id, &known(), now, T),
            Err(ErrorCode::Internal)
        );
        assert_eq!(pairings.authenticate(&old, T).unwrap().name, "app");
        std::fs::remove_dir_all(&path).unwrap();
        assert!(matches!(
            pairings.status(&request_id, &known(), now, T),
            Ok(PairState::Approved(Some(_)))
        ));
        assert_eq!(pairings.authenticate(&old, T), Err(ErrorCode::Revoked));
    }

    #[test]
    fn revoke_stops_a_token() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let token = pair(&mut pairings, "app", now);
        pairings.revoke("app").unwrap();
        assert_eq!(pairings.authenticate(&token, T), Err(ErrorCode::Revoked));
        assert_eq!(pairings.revoke("app"), Err(ErrorCode::BadRequest));
    }

    #[test]
    fn revoke_profile_revokes_every_client_on_it() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let a = pair(&mut pairings, "a", now);
        pair(&mut pairings, "b", now);
        pair_on(&mut pairings, "c", "work", now);
        assert_eq!(pairings.revoke_profile("default").unwrap(), ["a", "b"]);
        assert_eq!(pairings.authenticate(&a, T), Err(ErrorCode::Revoked));
        assert_eq!(pairings.clients().len(), 1);
    }

    #[test]
    fn limits_set_and_clear() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        pair(&mut pairings, "app", now);
        let limit: Limit = "10/60".parse().unwrap();
        pairings.set_limit("app", Some(limit)).unwrap();
        assert_eq!(pairings.clients()[0].limit(), Some(limit));
        pairings.set_limit("app", None).unwrap();
        assert_eq!(pairings.clients()[0].limit(), None);
        assert_eq!(pairings.set_limit("nope", None), Err(ErrorCode::BadRequest));
    }

    #[test]
    fn last_used_moves_at_most_once_a_minute() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let token = pair(&mut pairings, "app", now);
        assert_eq!(pairings.authenticate(&token, T).unwrap().last_used, Some(T));
        assert_eq!(
            pairings.authenticate(&token, T + 59).unwrap().last_used,
            Some(T)
        );
        assert_eq!(
            pairings.authenticate(&token, T + 60).unwrap().last_used,
            Some(T + 60)
        );
    }

    #[test]
    fn the_file_holds_hashes_and_survives_a_reload() {
        let (dir, mut pairings) = store();
        let now = Instant::now();
        let token = pair(&mut pairings, "app", now);
        let path = dir.path().join("pairings.json");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains(&token));
        assert!(text.contains(&hash_token(&token)));
        let mut reloaded = Pairings::load(&path).unwrap();
        assert_eq!(reloaded.authenticate(&token, T).unwrap().name, "app");
        assert!(
            reloaded.pending_list(now).is_empty(),
            "the pairing file holds no pending request"
        );
    }

    #[test]
    fn the_constants_match_the_spec() {
        assert_eq!(CODE_LENGTH, 8);
        assert_eq!(CODE_ALPHABET, b"23456789ABCDEFGHJKMNPQRSTUVWXYZ");
        assert_eq!(TOKEN_PREFIX, "jevt_");
        assert_eq!(PENDING_EXPIRY, Duration::from_secs(600));
        assert_eq!(MAX_PENDING, 16);
    }

    #[test]
    fn codes_use_every_letter_of_the_alphabet() {
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..1000 {
            seen.extend(new_code().bytes());
        }
        assert_eq!(seen.len(), CODE_ALPHABET.len());
    }

    #[test]
    fn an_expired_code_cannot_be_approved_or_denied() {
        // Two stores: each call purges, and that would hide a call that does not.
        let t0 = Instant::now();
        let late = t0 + PENDING_EXPIRY;
        let (_dir, mut pairings) = store();
        let (_, code) = pairings.start("a", None, false, t0).unwrap();
        assert_eq!(
            pairings.approve(&code, "default", late).err(),
            Some(ErrorCode::BadRequest)
        );
        let (_dir, mut pairings) = store();
        let (_, code) = pairings.start("b", None, false, t0).unwrap();
        assert_eq!(pairings.deny(&code, late), Err(ErrorCode::BadRequest));
    }

    #[test]
    fn expired_requests_leave_the_list_and_free_their_slots() {
        let (_dir, mut pairings) = store();
        let t0 = Instant::now();
        for i in 0..MAX_PENDING {
            pairings.start(&format!("app{i}"), None, false, t0).unwrap();
        }
        let late = t0 + PENDING_EXPIRY;
        assert!(pairings.pending_list(late).is_empty());
        let (_dir, mut pairings) = store();
        for i in 0..MAX_PENDING {
            pairings.start(&format!("app{i}"), None, false, t0).unwrap();
        }
        assert!(pairings.start("one-more", None, false, late).is_ok());
    }

    #[test]
    fn a_code_works_once_only() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let (_, code) = pairings.start("a", None, false, now).unwrap();
        pairings.approve(&code, "default", now).unwrap();
        assert_eq!(
            pairings.approve(&code, "default", now).err(),
            Some(ErrorCode::BadRequest)
        );
        assert_eq!(pairings.deny(&code, now), Err(ErrorCode::BadRequest));
        let (_, code) = pairings.start("b", None, false, now).unwrap();
        pairings.deny(&code, now).unwrap();
        assert_eq!(
            pairings.approve(&code, "default", now).err(),
            Some(ErrorCode::BadRequest)
        );
    }

    #[test]
    fn a_typed_code_may_have_spaces_around_it() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let (_, code) = pairings.start("app", None, false, now).unwrap();
        assert!(pairings
            .approve(&format!("  {code}\n"), "default", now)
            .is_ok());
    }

    #[test]
    fn the_pending_list_is_sorted_by_code() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        for i in 0..MAX_PENDING {
            pairings
                .start(&format!("app{i}"), None, false, now)
                .unwrap();
        }
        let codes: Vec<String> = pairings
            .pending_list(now)
            .into_iter()
            .map(|view| view.code)
            .collect();
        let mut sorted = codes.clone();
        sorted.sort();
        assert_eq!(codes, sorted);
    }

    #[test]
    fn the_commit_records_the_created_time() {
        let (_dir, mut pairings) = store();
        pair(&mut pairings, "app", Instant::now());
        assert_eq!(pairings.clients()[0].created, T);
    }

    #[test]
    fn changes_survive_a_reload() {
        let (dir, mut pairings) = store();
        let path = dir.path().join("pairings.json");
        let now = Instant::now();
        let gone = pair(&mut pairings, "gone", now);
        let a = pair(&mut pairings, "a", now);
        pair_on(&mut pairings, "app", "work", now);
        // Reload after each change: every save writes the whole store, so a
        // later save would hide an earlier change that did not save.
        let limit: Limit = "10/60".parse().unwrap();
        pairings.set_limit("app", Some(limit)).unwrap();
        let mut pairings = Pairings::load(&path).unwrap();
        assert_eq!(
            pairings
                .clients()
                .iter()
                .find(|c| c.name == "app")
                .unwrap()
                .limit(),
            Some(limit)
        );
        pairings.revoke("gone").unwrap();
        let mut pairings = Pairings::load(&path).unwrap();
        assert_eq!(pairings.authenticate(&gone, T), Err(ErrorCode::Revoked));
        pairings.revoke_profile("default").unwrap();
        let mut pairings = Pairings::load(&path).unwrap();
        assert_eq!(pairings.authenticate(&a, T), Err(ErrorCode::Revoked));
    }

    #[test]
    fn last_used_survives_a_reload() {
        let (dir, mut pairings) = store();
        let token = pair(&mut pairings, "app", Instant::now());
        pairings.authenticate(&token, T + 5).unwrap();
        let reloaded = Pairings::load(&dir.path().join("pairings.json")).unwrap();
        assert_eq!(reloaded.clients()[0].last_used, Some(T + 5));
    }

    #[test]
    fn a_bad_file_is_an_error_not_an_empty_store() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pairings.json");
        std::fs::write(&path, "not json").unwrap();
        assert!(Pairings::load(&path).is_err());
        assert!(
            Pairings::load(dir.path()).is_err(),
            "a folder cannot be read as the file"
        );
    }

    #[test]
    fn an_approved_token_can_be_collected_for_a_full_window_after_approval() {
        let (_dir, mut pairings) = store();
        let t0 = Instant::now();
        let (late_id, late_code) = pairings.start("late", None, false, t0).unwrap();
        let (gone_id, gone_code) = pairings.start("gone", None, false, t0).unwrap();
        let approved_at = t0 + Duration::from_secs(599);
        pairings
            .approve(&late_code, "default", approved_at)
            .unwrap();
        pairings
            .approve(&gone_code, "default", approved_at)
            .unwrap();
        let collect = t0 + Duration::from_secs(601);
        assert!(matches!(
            pairings.status(&late_id, &known(), collect, T),
            Ok(PairState::Approved(_))
        ));
        assert_eq!(
            pairings.status(&gone_id, &known(), approved_at + PENDING_EXPIRY, T),
            Ok(PairState::Expired)
        );
        assert_eq!(
            pairings.clients().len(),
            1,
            "nothing commits after the window"
        );
    }

    /// Put a folder that is not empty where the pairing file goes, so every save fails.
    fn break_saves(path: &Path) {
        std::fs::remove_file(path).unwrap();
        std::fs::create_dir(path).unwrap();
        std::fs::write(path.join("inside"), "x").unwrap();
    }

    #[test]
    fn a_failed_save_changes_nothing() {
        let (dir, mut pairings) = store();
        let path = dir.path().join("pairings.json");
        let now = Instant::now();
        let old = pair(&mut pairings, "app", now);
        pairings
            .set_limit("app", Some("10/60".parse().unwrap()))
            .unwrap();
        let (request_id, code) = pairings.start("app", None, false, now).unwrap();
        pairings.approve(&code, "default", now).unwrap();
        let before = pairings.clients();
        break_saves(&path);

        assert!(pairings.revoke("app").is_err());
        assert_eq!(pairings.authenticate(&old, T).unwrap().name, "app");
        // A status call that fails to save proves the approval is still there.
        assert_eq!(
            pairings.status(&request_id, &known(), now, T),
            Err(ErrorCode::Internal)
        );

        assert!(pairings.revoke_profile("default").is_err());
        assert_eq!(pairings.authenticate(&old, T).unwrap().name, "app");
        assert_eq!(
            pairings.status(&request_id, &known(), now, T),
            Err(ErrorCode::Internal)
        );

        assert!(pairings.set_limit("app", None).is_err());
        assert_eq!(
            pairings.clients(),
            before,
            "no limit or last-used change without a save"
        );
        assert_eq!(
            pairings.authenticate(&old, T + 600).unwrap().last_used,
            before[0].last_used
        );
        assert_eq!(pairings.clients(), before);
        std::fs::remove_dir_all(&path).unwrap();
        assert!(matches!(
            pairings.status(&request_id, &known(), now, T),
            Ok(PairState::Approved(Some(_)))
        ));
    }

    #[test]
    fn debug_output_never_holds_a_token() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let (request_id, code) = pairings.start("app", None, false, now).unwrap();
        pairings.approve(&code, "default", now).unwrap();
        let state = pairings.status(&request_id, &known(), now, T).unwrap();
        assert!(matches!(state, PairState::Approved(Some(_))));
        assert_eq!(format!("{state:?}"), "Approved(<redacted>)");
        assert!(!format!("{state:#?}").contains(TOKEN_PREFIX));
        assert_eq!(format!("{:?}", PairState::Pending), "Pending");
        assert_eq!(format!("{:?}", PairState::Denied), "Denied");
        assert_eq!(format!("{:?}", PairState::Expired), "Expired");
        let (_, code) = pairings.start("script", None, true, now).unwrap();
        let approval = pairings.approve(&code, "default", now).unwrap();
        assert!(!format!("{approval:?}").contains(TOKEN_PREFIX));
        assert!(format!("{approval:?}").contains("CliToken(<redacted>)"));
    }

    #[test]
    fn the_file_refuses_unknown_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pairings.json");
        std::fs::write(&path, r#"{"clients": [], "revoked": [], "extra": 1}"#).unwrap();
        assert!(Pairings::load(&path).is_err());
    }

    #[test]
    fn the_file_refuses_a_limit_it_cannot_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pairings.json");
        let file = |limit: &str| {
            format!(
                r#"{{"clients": [{{"name": "a", "profile": "default", "token_sha256": "aa", "created": 1, "limit": "{limit}"}}]}}"#
            )
        };
        std::fs::write(&path, file("10/60")).unwrap();
        assert!(Pairings::load(&path).is_ok());
        std::fs::write(&path, file("fast")).unwrap();
        let error = Pairings::load(&path).err().expect("a bad limit is refused");
        assert!(error.contains("the client a"));
    }

    #[test]
    fn the_file_refuses_a_name_or_a_hash_twice() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pairings.json");
        let client = |name: &str, hash: &str| {
            format!(
                r#"{{"name": "{name}", "profile": "default", "token_sha256": "{hash}", "created": 1}}"#
            )
        };
        let good = format!(
            r#"{{"clients": [{}, {}]}}"#,
            client("a", "aa"),
            client("b", "bb")
        );
        std::fs::write(&path, good).unwrap();
        assert!(Pairings::load(&path).is_ok());
        let same_name = format!(
            r#"{{"clients": [{}, {}]}}"#,
            client("a", "aa"),
            client("a", "bb")
        );
        std::fs::write(&path, same_name).unwrap();
        assert!(Pairings::load(&path).is_err());
        let same_hash = format!(
            r#"{{"clients": [{}, {}]}}"#,
            client("a", "aa"),
            client("b", "aa")
        );
        std::fs::write(&path, same_hash).unwrap();
        assert!(Pairings::load(&path).is_err());
    }

    #[test]
    fn a_client_with_an_unknown_key_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pairings.json");
        let text = r#"{"clients": [{"name": "a", "profile": "default", "token_sha256": "aa", "created": 1, "limt": "1/60"}]}"#;
        std::fs::write(&path, text).unwrap();
        assert!(
            Pairings::load(&path).is_err(),
            "a typo such as limt must not drop a limit silently"
        );
    }

    #[test]
    fn a_repeat_cli_request_needs_no_free_slot() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let first = pairings.start("script", None, true, now).unwrap();
        for i in 1..MAX_PENDING {
            pairings
                .start(&format!("app{i}"), None, false, now)
                .unwrap();
        }
        assert_eq!(
            pairings.start("other", None, false, now),
            Err(ErrorCode::RateLimited)
        );
        assert_eq!(pairings.start("script", None, true, now).unwrap(), first);
    }

    #[test]
    fn a_failed_save_at_a_cli_commit_keeps_the_request() {
        let (dir, mut pairings) = store();
        let path = dir.path().join("pairings.json");
        let now = Instant::now();
        pair(&mut pairings, "other", now);
        let (request_id, code) = pairings.start("script", None, true, now).unwrap();
        let approval = pairings.approve(&code, "default", now).unwrap();
        let cli = approval.cli.unwrap();
        break_saves(&path);
        assert_eq!(
            pairings.status(&request_id, &known(), now, T),
            Err(ErrorCode::Internal)
        );
        std::fs::remove_dir_all(&path).unwrap();
        assert_eq!(
            pairings.status(&request_id, &known(), now, T),
            Ok(PairState::Approved(None))
        );
        assert_eq!(pairings.authenticate(&cli.token, T).unwrap().name, "script");
    }

    #[test]
    fn revoke_of_a_waiting_request_is_refused_and_leaves_it() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let (request_id, code) = pairings.start("app", None, false, now).unwrap();
        assert_eq!(pairings.revoke("app"), Err(ErrorCode::BadRequest));
        assert!(pairings.approve(&code, "default", now).is_ok());
        assert!(matches!(
            pairings.status(&request_id, &known(), now, T),
            Ok(PairState::Approved(Some(_)))
        ));
    }

    #[test]
    fn a_cli_approval_expires_a_window_after_the_approval() {
        let t0 = Instant::now();
        let approved_at = t0 + Duration::from_secs(300);
        let (_dir, mut pairings) = store();
        let (request_id, code) = pairings.start("script", None, true, t0).unwrap();
        pairings.approve(&code, "default", approved_at).unwrap();
        let almost = approved_at + PENDING_EXPIRY - Duration::from_secs(1);
        assert_eq!(
            pairings.status(&request_id, &known(), almost, T),
            Ok(PairState::Approved(None))
        );
        let (_dir, mut pairings) = store();
        let (request_id, code) = pairings.start("script", None, true, t0).unwrap();
        pairings.approve(&code, "default", approved_at).unwrap();
        assert_eq!(
            pairings.status(&request_id, &known(), approved_at + PENDING_EXPIRY, T),
            Ok(PairState::Expired)
        );
    }

    // The pending file (the pairing-restart spec, section 7).

    fn pending_path(dir: &tempfile::TempDir) -> PathBuf {
        dir.path().join("state").join("pending.json")
    }

    /// The store in `dir`, as jevd loads it at `now`, the Unix time `unix`.
    fn reload(dir: &tempfile::TempDir, now: Instant, unix: u64) -> Pairings {
        let mut pairings = Pairings::load(&dir.path().join("pairings.json")).unwrap();
        assert_eq!(pairings.load_pending(&pending_path(dir), now, unix), None);
        pairings
    }

    /// The codes in the pending file, sorted.
    fn codes_in_file(dir: &tempfile::TempDir) -> Vec<String> {
        let text = std::fs::read_to_string(pending_path(dir)).unwrap();
        let file: serde_json::Value = serde_json::from_str(&text).unwrap();
        let mut codes: Vec<String> = file["requests"]
            .as_array()
            .unwrap()
            .iter()
            .map(|request| request["code"].as_str().unwrap().to_string())
            .collect();
        codes.sort();
        codes
    }

    /// One socket request as the pending file holds it.
    fn saved(code: &str, name: &str, deadline: u64) -> serde_json::Value {
        serde_json::json!({
            "id_sha256": hash_request_id(code),
            "code": code,
            "name": name,
            "profile": null,
            "cli": false,
            "deadline": deadline,
        })
    }

    fn write_pending(dir: &tempfile::TempDir, requests: Vec<serde_json::Value>) {
        let path = pending_path(dir);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let text = serde_json::json!({ "requests": requests }).to_string();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn a_waiting_request_survives_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let t0 = Instant::now();
        let mut pairings = reload(&dir, t0, T);
        let (_, code) = pairings.start("app", Some("work"), false, t0).unwrap();
        let t1 = t0 + Duration::from_secs(100);
        let mut reloaded = reload(&dir, t1, T + 100);
        assert_eq!(
            reloaded.pending_list(t1),
            [PendingView {
                code,
                name: "app".into(),
                profile: Some("work".into())
            }]
        );
        let almost = t0 + PENDING_EXPIRY - Duration::from_secs(1);
        assert_eq!(reloaded.pending_list(almost).len(), 1);
        assert!(
            reloaded.pending_list(t0 + PENDING_EXPIRY).is_empty(),
            "the deadline stays 10 minutes after the start"
        );
    }

    #[test]
    fn a_socket_client_polls_with_its_request_id_after_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let now = Instant::now();
        let mut pairings = reload(&dir, now, T);
        let (request_id, code) = pairings.start("app", None, false, now).unwrap();
        let mut reloaded = reload(&dir, now, T);
        assert_eq!(
            reloaded.status(&request_id, &known(), now, T),
            Ok(PairState::Pending)
        );
        reloaded.approve(&code, "default", now).unwrap();
        assert!(matches!(
            reloaded.status(&request_id, &known(), now, T),
            Ok(PairState::Approved(Some(_)))
        ));
    }

    #[test]
    fn a_repeat_cli_request_after_a_reload_gets_the_same_code_and_no_new_slot() {
        let dir = tempfile::tempdir().unwrap();
        let now = Instant::now();
        let mut pairings = reload(&dir, now, T);
        let (old_id, code) = pairings.start("script", None, true, now).unwrap();
        for i in 1..MAX_PENDING {
            pairings
                .start(&format!("app{i}"), None, false, now)
                .unwrap();
        }
        let mut reloaded = reload(&dir, now, T);
        let (new_id, again) = reloaded.start("script", None, true, now).unwrap();
        assert_eq!(again, code);
        assert_ne!(new_id, old_id, "jevd does not know the old ID after a load");
        assert_eq!(
            reloaded.start("script", None, true, now).unwrap(),
            (new_id, code),
            "the new ID stays"
        );
        assert_eq!(
            reloaded.start("other", None, false, now),
            Err(ErrorCode::RateLimited)
        );
    }

    #[test]
    fn jev_allow_approves_a_reloaded_cli_request_by_its_code() {
        let dir = tempfile::tempdir().unwrap();
        let now = Instant::now();
        let mut pairings = reload(&dir, now, T);
        let (_, code) = pairings.start("script", None, true, now).unwrap();
        let mut reloaded = reload(&dir, now, T);
        let approval = reloaded.approve(&code, "default", now).unwrap();
        let cli = approval.cli.expect("a CLI request gets the token");
        assert_eq!(
            reloaded.status(&cli.request_id, &known(), now, T),
            Ok(PairState::Approved(None))
        );
        assert_eq!(reloaded.authenticate(&cli.token, T).unwrap().name, "script");
    }

    #[test]
    fn an_approval_that_was_not_collected_is_gone_after_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let now = Instant::now();
        let mut pairings = reload(&dir, now, T);
        pair(&mut pairings, "other", now);
        let pairing_file = dir.path().join("pairings.json");
        let before = std::fs::read(&pairing_file).unwrap();
        let (request_id, code) = pairings.start("app", None, false, now).unwrap();
        pairings.approve(&code, "default", now).unwrap();
        let mut reloaded = reload(&dir, now, T);
        assert_eq!(
            reloaded.status(&request_id, &known(), now, T),
            Ok(PairState::Expired)
        );
        assert!(reloaded.pending_list(now).is_empty());
        assert_eq!(
            std::fs::read(&pairing_file).unwrap(),
            before,
            "the pairings file has no new entry"
        );
    }

    #[test]
    fn a_load_drops_an_expired_entry_and_cuts_a_far_deadline() {
        let dir = tempfile::tempdir().unwrap();
        write_pending(
            &dir,
            vec![
                saved("AAAAAAAA", "old", T),
                saved("BBBBBBBB", "far", T + 3600),
            ],
        );
        let t0 = Instant::now();
        let mut pairings = reload(&dir, t0, T);
        assert_eq!(
            codes_in_file(&dir),
            ["BBBBBBBB"],
            "the load writes the file"
        );
        let text = std::fs::read_to_string(pending_path(&dir)).unwrap();
        let file: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(file["requests"][0]["deadline"], T + 600);
        let almost = t0 + PENDING_EXPIRY - Duration::from_secs(1);
        assert_eq!(pairings.pending_list(almost).len(), 1);
        assert!(pairings.pending_list(t0 + PENDING_EXPIRY).is_empty());
    }

    #[test]
    fn a_load_keeps_the_16_latest_valid_entries() {
        let dir = tempfile::tempdir().unwrap();
        let code = |i: usize| (CODE_ALPHABET[i] as char).to_string().repeat(CODE_LENGTH);
        let mut requests: Vec<serde_json::Value> = (0..MAX_PENDING + 2)
            .map(|i| saved(&code(i), &format!("app{i}"), T + 100 + i as u64))
            .collect();
        requests.push(saved("XXXXXXXX", "Bad Name", T + 500));
        let mut bad_profile = saved("YYYYYYYY", "bad-profile", T + 500);
        bad_profile["profile"] = "Bad".into();
        requests.push(bad_profile);
        requests.push(saved("ABC", "short-code", T + 500));
        requests.push(saved("ABCDEFG0", "zero-in-code", T + 500));
        requests.push(saved(&code(MAX_PENDING + 1), "copy", T + 590));
        write_pending(&dir, requests);
        let now = Instant::now();
        let mut pairings = reload(&dir, now, T);
        let mut names: Vec<String> = pairings
            .pending_list(now)
            .into_iter()
            .map(|view| view.name)
            .collect();
        names.sort();
        let mut expected: Vec<String> = (2..MAX_PENDING + 2).map(|i| format!("app{i}")).collect();
        expected.sort();
        assert_eq!(names, expected);
    }

    #[test]
    fn a_file_that_jevd_cannot_use_is_deleted_and_logged() {
        let unknown_field = r#"{"requests": [{"id_sha256": "aa", "code": "AAAAAAAA", "name": "a", "profile": null, "cli": false, "deadline": 1900000000, "token": "x"}]}"#;
        let wrong_type = r#"{"requests": [{"id_sha256": "aa", "code": "AAAAAAAA", "name": "a", "profile": null, "cli": false, "deadline": "soon"}]}"#;
        let cases: [&[u8]; 5] = [
            b"not json",
            br#"{"requests": [], "extra": 1}"#,
            unknown_field.as_bytes(),
            wrong_type.as_bytes(),
            // Not UTF-8, so it cannot be read as text.
            &[0xff, 0xfe, 0x00],
        ];
        for contents in cases {
            let dir = tempfile::tempdir().unwrap();
            let path = pending_path(&dir);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, contents).unwrap();
            let now = Instant::now();
            let mut pairings = Pairings::load(&dir.path().join("pairings.json")).unwrap();
            let problem = pairings
                .load_pending(&path, now, T)
                .expect("one line for the log");
            assert!(problem.contains("pending.json"), "{problem}");
            assert!(!problem.contains('\n'), "{problem}");
            assert!(
                !problem.contains("soon"),
                "no text from the file: {problem}"
            );
            assert!(pairings.pending_list(now).is_empty());
            assert!(!path.exists(), "the file is deleted: {problem}");
            assert!(pairings.start("app", None, false, now).is_ok());
        }
    }

    #[test]
    fn the_pending_file_never_holds_a_request_id_or_a_token() {
        let dir = tempfile::tempdir().unwrap();
        let now = Instant::now();
        let mut pairings = reload(&dir, now, T);
        let (socket_id, socket_code) = pairings.start("app", None, false, now).unwrap();
        let (cli_id, cli_code) = pairings.start("script", None, true, now).unwrap();
        let text = std::fs::read_to_string(pending_path(&dir)).unwrap();
        assert!(!text.contains(&socket_id) && !text.contains(&cli_id));
        assert!(text.contains(&hash_request_id(&socket_id)));
        pairings.approve(&socket_code, "default", now).unwrap();
        let approval = pairings.approve(&cli_code, "default", now).unwrap();
        let cli = approval.cli.unwrap();
        pairings.start("waits", None, false, now).unwrap();
        let text = std::fs::read_to_string(pending_path(&dir)).unwrap();
        assert!(!text.contains(&socket_id) && !text.contains(&cli.request_id));
        assert!(!text.contains(&cli.token) && !text.contains(TOKEN_PREFIX));
    }

    #[test]
    fn a_failed_write_of_the_pending_file_fails_no_request() {
        let dir = tempfile::tempdir().unwrap();
        let now = Instant::now();
        let mut pairings = reload(&dir, now, T);
        let path = pending_path(&dir);
        // A folder that is not empty, where the file goes, so every write fails.
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("inside"), "x").unwrap();
        let (request_id, _) = pairings.start("app", None, false, now).unwrap();
        assert_eq!(
            pairings.status(&request_id, &known(), now, T),
            Ok(PairState::Pending)
        );
        std::fs::remove_dir_all(&path).unwrap();
        pairings.start("other", None, false, now).unwrap();
        let mut reloaded = reload(&dir, now, T);
        assert_eq!(
            reloaded.pending_list(now).len(),
            2,
            "the next change writes the whole file"
        );
    }

    #[test]
    fn each_change_to_the_waiting_requests_writes_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let t0 = Instant::now();
        let mut pairings = reload(&dir, t0, T);
        let (_, a) = pairings.start("a", None, false, t0).unwrap();
        let (_, b) = pairings.start("b", None, false, t0).unwrap();
        let later = t0 + Duration::from_secs(60);
        let (_, c) = pairings.start("c", None, false, later).unwrap();
        let mut all = vec![a.clone(), b.clone(), c.clone()];
        all.sort();
        assert_eq!(codes_in_file(&dir), all);
        pairings.approve(&a, "default", t0).unwrap();
        let mut left = vec![b.clone(), c.clone()];
        left.sort();
        assert_eq!(codes_in_file(&dir), left);
        pairings.deny(&b, t0).unwrap();
        assert_eq!(codes_in_file(&dir), [c]);
        pairings.pending_list(later + PENDING_EXPIRY);
        assert!(codes_in_file(&dir).is_empty(), "an expiry writes too");
    }

    #[test]
    fn a_repeat_cli_request_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let now = Instant::now();
        let mut pairings = reload(&dir, now, T);
        let first = pairings.start("script", None, true, now).unwrap();
        std::fs::remove_file(pending_path(&dir)).unwrap();
        assert_eq!(pairings.start("script", None, true, now).unwrap(), first);
        assert!(!pending_path(&dir).exists());
    }
}
