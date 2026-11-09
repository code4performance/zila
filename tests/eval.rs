//! `zila eval`, `zila -e` and `zila run -` (spec `eval-command`): code given inline or on
//! standard input runs as `--run` does, and errors name the snippet's own lines.

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

struct Ran {
    out: String,
    err: String,
    code: i32,
}

/// Runs `zila <args>` with `input` on standard input and a private temporary directory.
fn zila(args: &[&str], input: &str) -> Ran {
    let dir = std::env::temp_dir().join(format!("zila-evaltest-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    std::fs::create_dir_all(&dir).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_zila"))
        .args(args)
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
fn a_one_liner_prints_and_succeeds() {
    let r = zila(&["-e", "print(1 + 2)"], "");
    assert_eq!((r.out.as_str(), r.code), ("3\n", 0), "{}", r.err);
    let r = zila(&["eval", "print(1 + 2)"], "");
    assert_eq!((r.out.as_str(), r.code), ("3\n", 0), "{}", r.err);
}

#[test]
fn declarations_and_statements_can_be_mixed() {
    let r = zila(&["eval", "func double(n: int) -> int\n    return n * 2\nend\nprint(double(21))"], "");
    assert_eq!((r.out.as_str(), r.code), ("42\n", 0), "{}", r.err);
}

#[test]
fn a_library_can_be_used() {
    let r = zila(&["eval", "use text\nprint(text.trim(\"  hi  \"))"], "");
    assert_eq!((r.out.as_str(), r.code), ("hi\n", 0), "{}", r.err);
}

#[test]
fn a_failed_try_prints_the_error_and_gives_status_1() {
    let src = "func parse(s: text) -> int or error\n    return error(\"cannot parse '{s}'\")\nend\nlet n = try parse(\"bad\")\nprint(n)";
    let r = zila(&["eval", src], "");
    assert_eq!(r.code, 1);
    // An error that ends `main` is printed by the program, as with `--run`.
    assert_eq!(r.out, "error: cannot parse 'bad'\n", "{}", r.err);
}

#[test]
fn a_snippet_with_its_own_main_is_a_complete_program() {
    let r = zila(&["eval", "func main()\n    print(\"hi\")\nend"], "");
    assert_eq!((r.out.as_str(), r.code), ("hi\n", 0), "{}", r.err);
}

#[test]
fn a_snippet_comes_from_standard_input_with_a_dash() {
    let r = zila(&["eval", "-"], "print(\"from a pipe\")\n");
    assert_eq!((r.out.as_str(), r.code), ("from a pipe\n", 0), "{}", r.err);
}

#[test]
fn a_whole_program_comes_from_standard_input() {
    let src = "package main\n\nfunc main()\n    print(\"whole program\")\nend\n";
    let r = zila(&["run", "-"], src);
    assert_eq!((r.out.as_str(), r.code), ("whole program\n", 0), "{}", r.err);
    let r = zila(&["run", "-"], "func main()\nend\n");
    assert_eq!(r.code, 1, "a program without a package line is not a program");
    assert!(r.err.contains("<stdin>:1:1"), "{}", r.err);
}

#[test]
fn an_error_names_the_snippets_own_line() {
    let r = zila(&["eval", "let a = 1\nlet b: int = \"x\"\nprint(a)"], "");
    assert_eq!(r.code, 1);
    assert_eq!(r.out, "");
    assert!(r.err.contains("<eval>:2:"), "{}", r.err);
    assert!(!r.err.contains("main.zila"), "{}", r.err);
}

#[test]
fn a_syntax_error_gives_the_snippets_line_and_column() {
    let r = zila(&["-e", "print(1)\nprint(1 +)"], "");
    assert_eq!(r.code, 1);
    assert!(r.err.contains("--> <eval>:2:10"), "{}", r.err);
    assert!(r.err.contains("2 | print(1 +)"), "{}", r.err);
}

#[test]
fn run_is_the_same_as_the_flag() {
    let path = format!("{}/examples/hello.zila", env!("CARGO_MANIFEST_DIR"));
    let a2 = zila(&["run", &path], "");
    let b = zila(&[&path, "--run"], "");
    assert_eq!((a2.out.as_str(), a2.code), (b.out.as_str(), b.code));
    assert!(a2.out.starts_with("Hello, Ana!"), "{}{}", a2.out, a2.err);
}

#[test]
fn another_target_is_refused_before_compiling() {
    let other = if cfg!(windows) { "linux" } else { "windows" };
    let r = zila(&["-e", "print(1)", "--target", other], "");
    assert_eq!(r.code, 1);
    assert!(r.err.contains("can only run programs built for this machine"), "{}", r.err);
}

#[test]
fn a_missing_snippet_is_a_usage_error() {
    let r = zila(&["-e"], "");
    assert_ne!(r.code, 0);
    assert!(r.err.contains("-e needs the code to run"), "{}", r.err);
}

#[test]
fn an_inline_filter_reads_piped_lines() {
    let code = "use fs
let r = fs.stdin()
while not try await r.at_end()
    print(\"> {try await r.read_line()}\")
end";
    let r = zila(&["-e", code], "a
b
");
    assert_eq!((r.out.as_str(), r.code), ("> a
> b
", 0), "{}", r.err);
}

#[test]
fn a_program_run_from_standard_input_finds_its_own_input_empty() {
    let src = "package main

use fs

func main() -> error
    let r = fs.stdin()
    print(try await r.at_end())
    return
end
";
    let r = zila(&["run", "-"], src);
    assert_eq!((r.out.as_str(), r.code), ("true
", 0), "{}", r.err);
}
