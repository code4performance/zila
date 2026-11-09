//! `zila repl` / `zila -i`: an interactive session (spec `repl`).
//!
//! Zila compiles to machine code, so there is no live interpreter to keep state in. The session
//! is the text of the entries accepted so far; each new entry builds and runs the whole session
//! as one program (through `eval::wrap`) and prints only the output after what was already shown.
//! Earlier entries therefore run again, and their side effects repeat: the documented limit.

use crate::eval;
use crate::parse_source;
use crate::run;
use std::io::{BufRead, IsTerminal, Write};

const PROMPT: &str = "zila> ";
const CONTINUE: &str = "...> ";
const LABEL: &str = "<repl>";

/// Words that start a statement or a declaration, so an entry that begins with one is never
/// offered to `print` as an expression.
const STATEMENT_WORDS: &[&str] = &[
    "let", "var", "if", "for", "while", "match", "return", "defer", "spawn", "spawn_thread", "use", "func", "type", "extern", "embed", "pub", "async", "comptime", "break", "continue", "unsafe", "asm", "package", "print",
];

/// What the parser says about an entry.
#[derive(Debug, PartialEq)]
pub enum Check {
    Complete,
    /// An open block or parenthesis: more lines are needed.
    Incomplete,
    /// A syntax error, ready to print.
    Error(String),
}

/// Parses `entry` alone, as `eval` would wrap it, without compiling.
pub fn check(entry: &str) -> Check {
    let w = eval::wrap(entry);
    match parse_source(&w.source) {
        Ok(_) => Check::Complete,
        Err(d) => match w.locate(d.line as usize, d.col as usize) {
            // The error is in text the wrapper generated, or at the end of the input: the entry stops early.
            None => Check::Incomplete,
            Some(_) if d.msg.contains("end of file") => Check::Incomplete,
            Some((line, col)) => Check::Error(format!("error: {}\n  --> {}:{}:{}", d.msg, LABEL, line, col)),
        },
    }
}

/// True when `entry` could be a single expression whose value `print` can show: one line, no
/// statement word first, and no assignment.
pub fn expression_like(entry: &str) -> bool {
    let t = entry.trim();
    if t.is_empty() || t.contains('\n') || t.starts_with("#[") {
        return false;
    }
    let first: String = t.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
    if STATEMENT_WORDS.contains(&first.as_str()) {
        return false;
    }
    !has_assignment(t)
}

/// True when the line has an `=` outside a string that is not part of `==`, `!=`, `<=`, `>=`.
fn has_assignment(line: &str) -> bool {
    let chars: Vec<char> = line.chars().collect();
    let mut in_string = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_string {
            if c == '\\' {
                i += 1;
            } else if c == '"' {
                in_string = false;
            }
        } else if c == '"' {
            in_string = true;
        } else if c == '=' {
            let prev = if i > 0 { chars[i - 1] } else { ' ' };
            let next = chars.get(i + 1).copied().unwrap_or(' ');
            if next == '=' {
                i += 1; // `==`
            } else if !matches!(prev, '!' | '<' | '>' | '=') {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// The part of `output` after the first `shown` bytes. When the output got shorter than what was
/// shown (a program whose output changes between runs) all of it is returned.
pub fn delta(output: &str, shown: usize) -> &str {
    if output.len() < shown {
        return output;
    }
    output.get(shown..).unwrap_or(output)
}

/// What running an entry came to.
enum Outcome {
    /// Built and ended with status 0: the entry joins the session.
    Accepted { stdout: String, stderr: String },
    /// Built and ran, but ended with an error or an abort: its output is shown, the entry is dropped.
    Failed { stdout: String, stderr: String },
    /// Did not compile.
    BuildError(String),
}

/// The accepted entries and how much of the session's output the user has already seen.
#[derive(Default)]
pub struct Session {
    entries: Vec<String>,
    shown: usize,
}

impl Session {
    /// The session's code, one entry after another.
    pub fn code(&self) -> String {
        self.entries.join("\n")
    }

    fn reset(&mut self) {
        self.entries.clear();
        self.shown = 0;
    }

    /// Builds and runs the session plus `entry`.
    fn attempt(&self, entry: &str, optimize: bool, target: &str) -> Outcome {
        let before = self.code();
        let text = if before.is_empty() { entry.to_string() } else { format!("{}\n{}", before, entry) };
        let base = if before.is_empty() { 0 } else { before.lines().count() };
        let w = eval::wrap(&text);
        let file = match parse_source(&w.source) {
            Ok(f) => f,
            Err(d) => {
                let (line, col) = w.locate(d.line as usize, d.col as usize).unwrap_or((1, 1));
                return Outcome::BuildError(format!("error: {}\n  --> {}:{}:{}", d.msg, LABEL, line.saturating_sub(base).max(1), col));
            }
        };
        crate::comptime::register_siblings(std::path::Path::new("repl.zila"), &text);
        match run::run_captured(&file, optimize, target) {
            Err(e) => Outcome::BuildError(format!("compilation error: {}", shift_lines(&w.map_message(&e, LABEL), base))),
            Ok(c) if c.code == 0 => Outcome::Accepted { stdout: c.stdout, stderr: c.stderr },
            Ok(c) => Outcome::Failed { stdout: c.stdout, stderr: c.stderr },
        }
    }

    /// Runs one complete entry, printing what it printed or why it failed.
    fn run_entry(&mut self, entry: &str, optimize: bool, target: &str) {
        let mut outcome = None;
        let mut text = entry.to_string();
        if expression_like(entry) {
            let printed = format!("print({})", entry.trim());
            match self.attempt(&printed, optimize, target) {
                Outcome::BuildError(_) => {}
                other => {
                    outcome = Some(other);
                    text = printed;
                }
            }
        }
        let outcome = outcome.unwrap_or_else(|| self.attempt(entry, optimize, target));
        match outcome {
            Outcome::BuildError(msg) => eprintln!("{}", msg),
            Outcome::Failed { stdout, stderr } => {
                print!("{}", delta(&stdout, self.shown));
                eprint!("{}", stderr);
            }
            Outcome::Accepted { stdout, stderr } => {
                print!("{}", delta(&stdout, self.shown));
                eprint!("{}", stderr);
                self.shown = stdout.len();
                self.entries.push(text);
            }
        }
        let _ = std::io::stdout().flush();
    }
}

/// Rewrites `<repl>:N` in a message so that N counts from the entry's own first line, after
/// `base` lines of earlier entries.
fn shift_lines(msg: &str, base: usize) -> String {
    let key = format!("{}:", LABEL);
    let mut out = String::new();
    let mut rest = msg;
    while let Some(i) = rest.find(&key) {
        out.push_str(&rest[..i + key.len()]);
        let after = &rest[i + key.len()..];
        let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        match digits.parse::<usize>() {
            Ok(n) => {
                out.push_str(&n.saturating_sub(base).max(1).to_string());
                rest = &after[digits.len()..];
            }
            Err(_) => rest = after,
        }
    }
    out.push_str(rest);
    out
}

const HELP: &str = "\
Type a statement, a declaration (func, type, use) or an expression and press Enter.
A block (if, for, func, ...) continues on the next lines until it is complete.
  :help    this text
  :show    the code of the session so far
  :reset   forget everything
  :quit    leave (or press Ctrl-D / Ctrl-Z)
Each entry runs the whole session again and shows only its new output, so the
side effects of earlier entries (files, network, databases) repeat.";

/// Runs the interactive session on standard input and output; returns the exit status.
pub fn run(optimize: bool) -> i32 {
    let target = match run::host_target() {
        Some(t) => t,
        None => {
            eprintln!("error: the REPL is not supported on this platform");
            return 1;
        }
    };
    if let Err(e) = crate::package::install_for(std::path::Path::new("repl.zila")) {
        eprintln!("error: {}", e);
        return 1;
    }
    let tty = std::io::stdin().is_terminal();
    if tty {
        println!("Zila Lang REPL. :help for commands, :quit to leave.");
    }
    let mut session = Session::default();
    let mut pending = String::new();
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    loop {
        if tty {
            print!("{}", if pending.is_empty() { PROMPT } else { CONTINUE });
            let _ = std::io::stdout().flush();
        }
        let mut line = String::new();
        match input.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        // Windows PowerShell starts what it pipes with a byte order mark.
        let line = line.trim_start_matches('\u{feff}').trim_end_matches(['\n', '\r']);
        if pending.is_empty() {
            let t = line.trim();
            if t.is_empty() {
                continue;
            }
            if let Some(cmd) = t.strip_prefix(':') {
                match cmd.trim() {
                    "quit" | "exit" | "q" => return 0,
                    "help" | "h" | "?" => println!("{}", HELP),
                    "show" => println!("{}", session.code()),
                    "reset" => session.reset(),
                    other => eprintln!("unknown command ':{}' (:help lists them)", other),
                }
                continue;
            }
        } else {
            pending.push('\n');
        }
        pending.push_str(line);
        match check(&pending) {
            Check::Incomplete => continue,
            Check::Error(msg) => eprintln!("{}", msg),
            Check::Complete => session.run_entry(&pending, optimize, target),
        }
        pending.clear();
    }
    if tty {
        println!();
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incomplete_entries_ask_for_more() {
        assert_eq!(check("if true"), Check::Incomplete);
        assert_eq!(check("if true\n    print(1)"), Check::Incomplete);
        assert_eq!(check("func f() -> int"), Check::Incomplete);
        assert_eq!(check("print(1"), Check::Incomplete);
        assert_eq!(check("if true\n    print(1)\nend"), Check::Complete);
    }

    #[test]
    fn a_real_syntax_error_is_reported_with_the_entrys_line() {
        match check("print(1)\nprint(1 +)") {
            Check::Error(m) => assert!(m.contains("<repl>:2:10"), "{}", m),
            other => panic!("{:?}", other),
        }
    }

    #[test]
    fn expressions_are_told_from_statements() {
        for yes in ["1 + 2", "name.len", "a == b", "x <= 3", "\"a=b\"", "double(21)"] {
            assert!(expression_like(yes), "{}", yes);
        }
        for no in ["let x = 1", "x = 5", "x += 1", "print(1)", "if x", "func f()", "a\nb", "", "#[test]", "for i in 1..3"] {
            assert!(!expression_like(no), "{}", no);
        }
    }

    #[test]
    fn only_the_new_output_is_shown() {
        assert_eq!(delta("a\nb\n", 2), "b\n");
        assert_eq!(delta("a\n", 2), "");
        assert_eq!(delta("x\n", 5), "x\n");
        assert_eq!(delta("é", 1), "é");
    }

    #[test]
    fn lines_count_from_the_entry() {
        assert_eq!(shift_lines("<repl>:5: boom", 3), "<repl>:2: boom");
        assert_eq!(shift_lines("<repl>:2: boom", 3), "<repl>:1: boom");
    }
}
