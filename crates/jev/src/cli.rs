//! The command line (spec section 6, "Commands").

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "jev", version, about = "Ask TypeSafe's Jev model through jevd")]
pub struct Cli {
    /// Print one JSON object on stdout instead of text.
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Start jevd when it does not run.
    Start,
    /// Stop jevd. Requests in flight finish first.
    Stop,
    /// Show whether jevd runs, its version, and the profiles.
    Status,
    /// Manage the TypeSafe keys.
    Key {
        #[command(subcommand)]
        command: KeyCommand,
    },
    /// Pair a client whose token the jev command keeps.
    Pair {
        #[arg(long)]
        name: String,
        /// The profile to suggest at the approval.
        #[arg(long)]
        profile: Option<String>,
        /// Never use the terminal: print the pairing code and exit 3, so a
        /// person approves the client with `jev allow`. For a caller, such as
        /// the Claude Code mod, that runs inside another program's terminal.
        #[arg(long)]
        no_prompt: bool,
    },
    /// Approve a waiting pairing request. Needs a terminal.
    Allow { code: Option<String> },
    /// Refuse a waiting pairing request.
    Deny { code: String },
    /// End a client's pairing at once.
    Revoke { name: String },
    /// List the paired clients, or change one client's rate limit.
    Clients {
        #[command(subcommand)]
        command: Option<ClientsCommand>,
    },
    /// Read or change the settings.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Send one request to Jev. Prints JSON.
    ///
    /// The request comes from JSON on stdin, from --request, or from --state
    /// with --noul, --score, or --choice. With question flags or --request, stdin must
    /// be a terminal or closed; jev reads it to the end.
    Ask(AskArgs),
}

#[derive(Debug, Subcommand)]
pub enum KeyCommand {
    /// Store a key in the OS keyring.
    Add {
        #[arg(long, default_value = "default")]
        profile: String,
        /// Read the key from stdin instead of a hidden prompt.
        #[arg(long)]
        stdin: bool,
        /// Send one real request to check that TypeSafe accepts the key.
        /// It spends one request, or two when TypeSafe answers 429 or 529,
        /// because jev retries once.
        #[arg(long)]
        check: bool,
    },
    /// Delete a profile and its key.
    Remove {
        #[arg(long)]
        profile: String,
        /// Also revoke the clients that use the profile.
        #[arg(long)]
        force: bool,
    },
    /// List the profiles.
    List,
}

#[derive(Debug, Subcommand)]
pub enum ClientsCommand {
    /// Set a client's rate limit, `<count>/<seconds>`, or `default`.
    Limit { name: String, limit: String },
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Print one setting's value.
    Get { name: String },
    /// Change one setting. A change that loosens a limit needs a terminal
    /// to confirm it.
    Set { name: String, value: String },
    /// Print every setting and its value.
    List,
}

#[derive(Debug, Args)]
pub struct AskArgs {
    /// The client whose token to use. Default: the JEV_CLIENT variable.
    #[arg(long)]
    pub client: Option<String>,
    /// The state text, or @<file> for a file's text. A state that starts
    /// with @ cannot be given here; put it in a file, or use --request.
    #[arg(long)]
    pub state: Option<String>,
    /// A noul question, <id>=<question>. Repeatable.
    #[arg(long)]
    pub noul: Vec<String>,
    /// A score question, <id>=<question>. Repeatable. Give its levels with
    /// --level.
    #[arg(long)]
    pub score: Vec<String>,
    /// A choice question, <id>=<question>. Repeatable. Give its options with
    /// --option.
    #[arg(long)]
    pub choice: Vec<String>,
    /// One option of choice question <id>, <id>=<name>. Repeatable, 1 to 255
    /// per question.
    #[arg(long)]
    pub option: Vec<String>,
    /// One level of score question <id>, <id>=<text>, from the lowest to the
    /// highest. Repeatable, 1 to 10 per question.
    #[arg(long)]
    pub level: Vec<String>,
    /// The whole request as JSON, for callers whose stdin is closed.
    #[arg(long)]
    pub request: Option<String>,
    /// Print only this answer's number.
    #[arg(long)]
    pub value: Option<String>,
}
