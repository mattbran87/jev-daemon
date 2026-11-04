//! The person's terminal (spec section 6, "Shared behavior"). Prompts read
//! the terminal device, `/dev/tty` or `CONIN$`, never stdin, so a script or
//! an agent that writes to stdin cannot answer them.

use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Write};
use std::sync::{Arc, Mutex};

use zeroize::Zeroizing;

use crate::out::clean_lines;

pub trait Terminal: Send {
    /// True when the terminal device opens. An agent or a script with no
    /// terminal gets false.
    fn available(&self) -> bool;
    /// Shows `prompt` and reads one line, without its line end.
    fn ask(&mut self, prompt: &str) -> io::Result<String>;
    /// As `ask`, with no echo, for a key. The answer is wiped from memory
    /// when it is dropped.
    fn ask_hidden(&mut self, prompt: &str) -> io::Result<Zeroizing<String>>;
    /// Shows one line on the terminal, not on stdout. The text goes through
    /// `out::clean_lines`, which keeps only the line breaks that jev itself
    /// writes. So a field from jevd or Jev must go through `out::clean`
    /// first, or its own line breaks reach the screen.
    fn say(&mut self, text: &str) -> io::Result<()>;
}

/// The real terminal device. Every line it shows goes through
/// `out::clean_lines` first.
///
/// On Windows it writes and reads bytes through `CONOUT$` and `CONIN$`,
/// which use the console code page, not UTF-8. So terminal text must stay
/// ASCII; all of jev's prompts are ASCII.
pub struct DeviceTerminal;

#[cfg(unix)]
fn device() -> io::Result<(std::fs::File, std::fs::File)> {
    let input = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")?;
    let output = input.try_clone()?;
    Ok((input, output))
}

#[cfg(windows)]
fn device() -> io::Result<(std::fs::File, std::fs::File)> {
    let input = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("CONIN$")?;
    let output = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("CONOUT$")?;
    Ok((input, output))
}

/// Opens the terminal device. Any failure is `NotFound`, as for
/// `ScriptedTerminal::none`.
fn open() -> io::Result<(std::fs::File, std::fs::File)> {
    device().map_err(|error| io::Error::new(io::ErrorKind::NotFound, error))
}

impl Terminal for DeviceTerminal {
    fn available(&self) -> bool {
        open().is_ok()
    }

    fn ask(&mut self, prompt: &str) -> io::Result<String> {
        let (input, mut output) = open()?;
        output.write_all(clean_lines(prompt).as_bytes())?;
        output.flush()?;
        let mut line = String::new();
        if BufReader::new(input).read_line(&mut line)? == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        Ok(line.trim_end_matches(['\r', '\n']).to_string())
    }

    fn ask_hidden(&mut self, prompt: &str) -> io::Result<Zeroizing<String>> {
        rpassword::prompt_password(clean_lines(prompt)).map(Zeroizing::new)
    }

    fn say(&mut self, text: &str) -> io::Result<()> {
        let (_, mut output) = open()?;
        writeln!(output, "{}", clean_lines(text))
    }
}

/// A terminal for tests: no terminal, or one with answers given ahead. It
/// records what it shows after `out::clean_lines`, as a person sees it.
#[derive(Clone, Default)]
pub struct ScriptedTerminal {
    present: bool,
    answers: Arc<Mutex<VecDeque<String>>>,
    shown: Arc<Mutex<Vec<String>>>,
}

impl ScriptedTerminal {
    /// No terminal, as for an agent.
    pub fn none() -> ScriptedTerminal {
        ScriptedTerminal::default()
    }

    /// A terminal whose person types `answers`, one per prompt.
    pub fn answering(answers: &[&str]) -> ScriptedTerminal {
        ScriptedTerminal {
            present: true,
            answers: Arc::new(Mutex::new(answers.iter().map(|a| a.to_string()).collect())),
            shown: Arc::default(),
        }
    }

    /// Every prompt and line shown so far.
    pub fn shown(&self) -> Vec<String> {
        self.shown.lock().unwrap().clone()
    }

    fn next(&mut self, prompt: &str) -> io::Result<String> {
        if !self.present {
            return Err(io::ErrorKind::NotFound.into());
        }
        self.shown.lock().unwrap().push(clean_lines(prompt));
        self.answers
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| io::ErrorKind::UnexpectedEof.into())
    }
}

impl Terminal for ScriptedTerminal {
    fn available(&self) -> bool {
        self.present
    }

    fn ask(&mut self, prompt: &str) -> io::Result<String> {
        self.next(prompt)
    }

    fn ask_hidden(&mut self, prompt: &str) -> io::Result<Zeroizing<String>> {
        self.next(prompt).map(Zeroizing::new)
    }

    fn say(&mut self, text: &str) -> io::Result<()> {
        if !self.present {
            return Err(io::ErrorKind::NotFound.into());
        }
        self.shown.lock().unwrap().push(clean_lines(text));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scripted_terminal_answers_in_order_and_records_prompts() {
        let mut terminal = ScriptedTerminal::answering(&["a", "b"]);
        assert!(terminal.available());
        assert_eq!(terminal.ask("first? ").unwrap(), "a");
        terminal.say("note").unwrap();
        assert_eq!(terminal.ask_hidden("second? ").unwrap().as_str(), "b");
        assert!(terminal.ask("third? ").is_err());
        assert_eq!(terminal.shown(), ["first? ", "note", "second? ", "third? "]);
    }

    #[test]
    fn no_terminal_answers_nothing() {
        let mut terminal = ScriptedTerminal::none();
        assert!(!terminal.available());
        assert!(terminal.ask("x? ").is_err());
        assert!(terminal.say("x").is_err());
    }

    #[test]
    fn no_terminal_shows_no_prompt() {
        let mut terminal = ScriptedTerminal::none();
        let error = terminal.ask_hidden("key? ").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(terminal.shown().is_empty());
    }

    #[test]
    fn say_cleans_each_line_and_keeps_the_line_breaks() {
        let mut terminal = ScriptedTerminal::answering(&[]);
        terminal.say("a\x1b[2J\u{202e}b\r\nc\u{200b}d").unwrap();
        assert_eq!(terminal.shown(), ["a [2J b\nc d"]);
    }
}
