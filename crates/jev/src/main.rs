//! The `jev` binary. All the work is in the library's `run`.

use std::process::ExitCode;

fn exit(code: i32) -> ExitCode {
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}

fn main() -> ExitCode {
    let (mut out, mut err) = (std::io::stdout(), std::io::stderr());
    // Parse first, so --help, --version, and a bad command line work even
    // when jev cannot find its folders.
    let cli = match jev::parse(std::env::args_os().collect(), &mut out, &mut err) {
        Ok(cli) => cli,
        Err(code) => return exit(code),
    };
    let mut env = match jev::env::Env::from_os() {
        Ok(env) => env,
        Err(problem) => return exit(jev::report_no_env(&cli, &problem, &mut out, &mut err)),
    };
    exit(jev::run_cli(&cli, &mut env))
}
