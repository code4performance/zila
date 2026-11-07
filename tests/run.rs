//! `brasa-parse --run`: the program is built for this machine, run with the terminal's input and
//! output passed through, and deleted afterwards; its exit status becomes the tool's.

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

struct Ran {
    out: String,
    err: String,
    code: i32,
    /// What was left in the private temporary directory afterwards.
    left: Vec<String>,
}

/// Writes `src` to a file, runs `brasa-parse <file> --run [extra]` with `input` on standard input
/// and a private temporary directory, and collects the result.
fn run(src: &str, extra: &[&str], input: &str) -> Ran {
    let dir = std::env::temp_dir().join(format!("brasa-runtest-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let tmp = dir.join("tmp");
    std::fs::create_dir_all(&tmp).unwrap();
    let file = dir.join("prog.br");
    std::fs::write(&file, src).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_brasa-parse"))
        .arg(&file)
        .arg("--run")
        .args(extra)
        .env("TMP", &tmp)
        .env("TEMP", &tmp)
        .env("TMPDIR", &tmp)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    let o = child.wait_with_output().unwrap();
    let left = std::fs::read_dir(&tmp).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
    let _ = std::fs::remove_dir_all(&dir);
    Ran {
        out: String::from_utf8_lossy(&o.stdout).replace('\r', ""),
        err: String::from_utf8_lossy(&o.stderr).replace('\r', ""),
        code: o.status.code().unwrap_or(-1),
        left,
    }
}

fn main_of(body: &str) -> String {
    let mut s = String::from("package main\n\nfunc main()\n");
    for l in body.lines() {
        s.push_str("    ");
        s.push_str(l);
        s.push('\n');
    }
    s.push_str("end\n");
    s
}

#[test]
fn hello_runs_and_leaves_nothing_behind() {
    let src = std::fs::read_to_string("examples/hello.br").unwrap();
    let r = run(&src, &[], "");
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(r.out.starts_with("Hello, Ana!\n"), "{}", r.out);
    assert!(r.left.is_empty(), "left behind: {:?}", r.left);
}

#[test]
fn an_error_from_main_gives_status_1() {
    let src = "package main

func main() -> error
    print(\"to stdout\")
    return error(\"boom\")
end
";
    let r = run(src, &[], "");
    assert_eq!(r.out, "to stdout
error: boom
");
    assert_eq!(r.code, 1);
    assert!(r.left.is_empty(), "left behind: {:?}", r.left);
}

// The language has no standard-error writer yet, so the program asks the system itself.
#[cfg(windows)]
const WRITE_STDERR: &str = r#"package main

use c

#[link("kernel32")]
extern "system" func GetStdHandle(n: i32) -> usize
extern "system" func WriteFile(handle: usize, buffer: *u8, count: u32, done: *int, overlapped: usize) -> i32

func main()
    print("to stdout")
    var done = 0
    unsafe
        WriteFile(GetStdHandle(-12), c.cstr("to stderr"), 9, &done, 0)
    end
end
"#;

#[cfg(not(windows))]
const WRITE_STDERR: &str = r#"package main

use c
use sys

func main()
    print("to stdout")
    sys.syscall(1, 2, int(c.cstr("to stderr")), 9, 0, 0, 0)
end
"#;

#[test]
fn standard_output_and_error_are_passed_through() {
    let r = run(WRITE_STDERR, &[], "");
    assert_eq!(r.code, 0, "{}{}", r.out, r.err);
    assert_eq!(r.out, "to stdout
");
    assert_eq!(r.err, "to stderr");
}

#[test]
fn an_abort_gives_status_101_and_writes_to_standard_error() {
    let r = run(&main_of("print(\"x\")\nabort(\"stop\")"), &[], "");
    assert_eq!(r.code, 101);
    assert_eq!(r.out, "x
");
    assert_eq!(r.err, "abort: main.br:5: stop
");
    assert!(r.left.is_empty(), "left behind: {:?}", r.left);
}

#[test]
fn a_compile_error_runs_nothing() {
    let r = run(&main_of("print(\"should not run\")\nlet x: int = \"text\""), &[], "");
    assert_eq!(r.code, 1);
    assert!(r.err.contains("compilation error:"), "{}", r.err);
    assert!(!r.out.contains("should not run"), "{}", r.out);
    assert!(r.left.is_empty(), "left behind: {:?}", r.left);
}

#[test]
fn a_target_other_than_the_host_is_rejected() {
    let other = if cfg!(windows) { "linux" } else { "windows" };
    let r = run(&main_of("print(\"should not run\")\nlet x: int = \"text\""), &["--target", other], "");
    assert_eq!(r.code, 1);
    assert!(r.err.contains("--run can only run programs built for this machine"), "{}", r.err);
    assert!(!r.err.contains("compilation error"), "it compiled before the check: {}", r.err);
    assert!(r.left.is_empty(), "left behind: {:?}", r.left);
}

#[cfg(windows)]
const READ_STDIN: &str = r#"package main

use c
use sys

#[link("kernel32")]
extern "system" func GetStdHandle(n: i32) -> usize
extern "system" func ReadFile(handle: usize, buffer: *u8, count: u32, done: *int, overlapped: usize) -> i32

func main()
    let b = sys.buffer(64)
    var got = 0
    unsafe
        let h = GetStdHandle(-10)
        ReadFile(h, b.ptr, 64, &got, 0)
    end
    print("read: {c.from_bytes(b.ptr, got)}")
end
"#;

#[cfg(not(windows))]
const READ_STDIN: &str = r#"package main

use c
use sys

func main()
    let b = sys.buffer(64)
    let got = sys.syscall(0, 0, int(b.ptr), 64, 0, 0, 0)
    print("read: {c.from_bytes(b.ptr, got)}")
end
"#;

#[test]
fn standard_input_reaches_the_program() {
    let r = run(READ_STDIN, &[], "from the pipe");
    assert_eq!(r.code, 0, "{}{}", r.out, r.err);
    assert_eq!(r.out, "read: from the pipe\n");
}
