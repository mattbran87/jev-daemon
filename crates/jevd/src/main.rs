//! jevd: binds the channel, loads the files, and serves (spec section 2,
//! "Lifecycle"). Clients start it detached; it also runs in a terminal.

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use jev_proto::Paths;
use jevd::daemon::{Config, Daemon};
use jevd::jev_http::JevClient;
use jevd::keystore::OsKeyring;
use jevd::listen::{bind, BindError};
use jevd::peer::{jev_next_to_me, AdminCheck};
use jevd::{log, run};

fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("jevd {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    // The log is not set up yet, so this failure can only go to stderr.
    let paths = match Paths::from_env() {
        Ok(paths) => paths,
        Err(error) => return fail(&format!("cannot find the jev folders: {error}")),
    };
    log::init(&paths.log_file());
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => return fail(&format!("cannot start the runtime: {error}")),
    };
    let code = runtime.block_on(serve(paths));
    // A keyring prompt can hold a blocking thread. The keys are already
    // wiped, so do not wait long for it.
    runtime.shutdown_timeout(Duration::from_secs(1));
    code
}

async fn serve(paths: Paths) -> ExitCode {
    let listener = match bind(&paths) {
        Ok(listener) => listener,
        Err(error @ BindError::Running) => {
            log::event(&format!("{error}; this one exits"));
            return ExitCode::SUCCESS;
        }
        Err(error) => return fail(&error.to_string()),
    };
    let jev = match JevClient::new() {
        Ok(jev) => jev,
        Err(problem) => {
            listener.close();
            return fail(&problem);
        }
    };
    let config = Config {
        settings_file: paths.settings_file(),
        pairing_file: paths.pairing_file(),
        store: Arc::new(OsKeyring),
        jev,
    };
    let daemon = match Daemon::new(config) {
        Ok(daemon) => daemon,
        Err(problem) => {
            listener.close();
            return fail(&problem);
        }
    };
    let jev_path = jev_next_to_me().unwrap_or_default();
    if !jev_path.exists() {
        log::event("jev was not found next to jevd; admin messages will be refused");
    }
    let admin = AdminCheck::new(&jev_path);
    log::event("start");
    run::run(daemon, listener, admin).await;
    log::event("stop");
    ExitCode::SUCCESS
}

fn fail(problem: &str) -> ExitCode {
    log::event(&format!("start failed: {problem}"));
    eprintln!("jevd: {problem}");
    ExitCode::FAILURE
}
