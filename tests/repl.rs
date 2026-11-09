//! `zila repl` (spec `repl`): an interactive session, driven here with piped input, where no
//! prompts are written and the output is only what the entries printed.

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

struct Ran {
    out: String,
    err: String,
    code: i32,
}

/// Runs `zila repl` with `input` on standard input in a private directory.
fn repl(input: &str) -> Ran {
    let dir = std::env::temp_dir().join(format!("zila-repltest-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    std::fs::create_dir_all(&dir).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_zila"))
        .arg("repl")
        .current_dir(&dir)
        .env("TMP", &dir)
        .env("TEMP", &dir)
        .env("TMPDIR", &dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    let o = child.wait_with_output().unwrap();
    let left: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(left.is_empty(), "left behind: {:?}", left);
    Ran { out: String::from_utf8_lossy(&o.stdout).replace('\r', ""), err: String::from_utf8_lossy(&o.stderr).replace('\r', ""), code: o.status.code().unwrap_or(-1) }
}

#[test]
fn a_statement_prints_once_and_no_prompt_is_written_when_piped() {
    let r = repl("print(\"hi\")\n");
    assert_eq!((r.out.as_str(), r.code), ("hi\n", 0), "{}", r.err);
}

#[test]
fn a_piped_script_runs_in_order() {
    let r = repl("print(1)\nprint(2)\n");
    assert_eq!((r.out.as_str(), r.code), ("1\n2\n", 0), "{}", r.err);
}

#[test]
fn variables_and_functions_stay_for_later_entries() {
    let r = repl("let x = 20\nprint(x + 1)\nfunc double(n: int) -> int\n    return n * 2\nend\nprint(double(21))\n");
    assert_eq!((r.out.as_str(), r.code), ("21\n42\n", 0), "{}", r.err);
}

#[test]
fn the_output_of_earlier_entries_is_not_repeated() {
    let r = repl("print(\"a\")\nprint(\"b\")\nprint(\"c\")\n");
    assert_eq!(r.out, "a\nb\nc\n", "{}", r.err);
}

#[test]
fn an_expression_prints_its_value_and_a_void_call_prints_once() {
    let r = repl("1 + 2\nlet name = \"zila\"\nname.len\nprint(\"x\")\nfunc hello()\n    print(\"from hello\")\nend\nhello()\n");
    assert_eq!(r.out, "3\n4\nx\nfrom hello\n", "{}", r.err);
    assert_eq!(r.code, 0);
}

#[test]
fn a_block_continues_on_the_next_lines() {
    let r = repl("if true\n    print(\"yes\")\nend\nprint(\"after\")\n");
    assert_eq!((r.out.as_str(), r.code), ("yes\nafter\n", 0), "{}", r.err);
}

#[test]
fn a_failed_entry_is_reported_and_discarded() {
    let r = repl("let n: int = \"a\"\nprint(1)\nn\n");
    assert_eq!(r.out, "1\n", "{}", r.err);
    assert!(r.err.contains("expected int, found text"), "{}", r.err);
    assert!(r.err.contains("<repl>:1"), "{}", r.err);
    assert!(r.err.contains("undefined identifier 'n'"), "{}", r.err);
    assert_eq!(r.code, 0);
}

#[test]
fn a_syntax_error_names_the_entrys_own_line() {
    let r = repl("print(1 +)\nprint(2)\n");
    assert_eq!(r.out, "2\n", "{}", r.err);
    assert!(r.err.contains("<repl>:1:10"), "{}", r.err);
}

#[test]
fn an_entry_that_ends_in_an_error_is_discarded_after_showing_its_output() {
    let r = repl("print(\"before\")\nfunc fail() -> int or error\n    return error(\"boom\")\nend\nlet v = try fail()\nprint(\"after\")\n");
    assert!(r.out.contains("before\n"), "{}{}", r.out, r.err);
    assert!(r.out.contains("error: boom"), "{}{}", r.out, r.err);
    assert!(r.out.ends_with("after\n"), "{}{}", r.out, r.err);
    assert_eq!(r.out.matches("before").count(), 1, "{}", r.out);
}

#[test]
fn reset_forgets_everything() {
    let r = repl("let x = 1\n:reset\nprint(x)\nprint(\"ok\")\n");
    assert_eq!(r.out, "ok\n", "{}", r.err);
    assert!(r.err.contains("undefined identifier 'x'"), "{}", r.err);
}

#[test]
fn show_prints_the_session_code() {
    let r = repl("let x = 1\n:show\n");
    assert_eq!(r.out, "let x = 1\n", "{}", r.err);
}

#[test]
fn help_lists_the_commands_and_an_unknown_one_is_reported() {
    let r = repl(":help\n:bogus\nprint(1)\n");
    assert!(r.out.contains(":quit") && r.out.contains(":reset") && r.out.contains(":show"), "{}", r.out);
    assert!(r.out.ends_with("1\n"), "{}", r.out);
    assert!(r.err.contains("unknown command ':bogus'"), "{}", r.err);
}

#[test]
fn quit_and_the_end_of_input_both_end_the_session_with_status_0() {
    let r = repl("print(1)\n:quit\nprint(2)\n");
    assert_eq!((r.out.as_str(), r.code), ("1\n", 0), "{}", r.err);
    let r = repl("print(1)\n");
    assert_eq!(r.code, 0);
    let r = repl(":exit\n");
    assert_eq!((r.out.as_str(), r.code), ("", 0));
}

#[test]
fn a_library_can_be_imported_and_used_later() {
    let r = repl("use text\ntext.trim(\"  hi  \")\n");
    assert_eq!((r.out.as_str(), r.code), ("hi\n", 0), "{}", r.err);
}
