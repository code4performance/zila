//! `fs.stdin()` (spec `stdin-reader`): a reader over standard input with the methods of a file
//! reader, run through `zila run` with the input piped, redirected from a file or produced slowly.

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

static N: AtomicUsize = AtomicUsize::new(0);

struct Ran {
    out: String,
    err: String,
    code: i32,
}

fn dir() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("zila-stdintest-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn collect(o: std::process::Output) -> Ran {
    Ran { out: String::from_utf8_lossy(&o.stdout).replace('\r', ""), err: String::from_utf8_lossy(&o.stderr).replace('\r', ""), code: o.status.code().unwrap_or(-1) }
}

fn command(d: &std::path::Path, src: &str) -> Command {
    let file = d.join("prog.zila");
    std::fs::write(&file, src).unwrap();
    let mut c = Command::new(env!("CARGO_BIN_EXE_zila"));
    c.arg("run").arg(&file).env("TMP", d).env("TEMP", d).env("TMPDIR", d);
    c
}

/// Runs `src` with `input` piped to its standard input.
fn run(src: &str, input: &[u8]) -> Ran {
    let d = dir();
    let mut child = command(&d, src).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    let r = collect(child.wait_with_output().unwrap());
    let _ = std::fs::remove_dir_all(&d);
    r
}

const LINES: &str = r#"package main

use fs

func main() -> error
    let r = fs.stdin()
    while not try await r.at_end()
        let line = try await r.read_line()
        print("[{line}]")
    end
    print("done")
    return
end
"#;

#[test]
fn lines_come_without_their_endings_and_the_last_one_needs_no_newline() {
    let r = run(LINES, b"a\nb\r\nc");
    assert_eq!((r.out.as_str(), r.code), ("[a]\n[b]\n[c]\ndone\n", 0), "{}", r.err);
}

#[test]
fn read_all_returns_everything() {
    let src = "package main\n\nuse fs\n\nfunc main() -> error\n    let r = fs.stdin()\n    let all = try await r.read_all()\n    print(\"{all.len}:{all}\")\n    return\nend\n";
    let r = run(src, b"hello");
    assert_eq!((r.out.as_str(), r.code), ("5:hello\n", 0), "{}", r.err);
}

#[test]
fn a_redirected_file_of_200000_bytes_arrives_whole() {
    let src = "package main\n\nuse fs\n\nfunc main() -> error\n    let r = fs.stdin()\n    let all = try await r.read_all()\n    print(all.len)\n    return\nend\n";
    let d = dir();
    let input = d.join("input.bin");
    std::fs::write(&input, vec![b'x'; 200_000]).unwrap();
    let o = command(&d, src).stdin(std::fs::File::open(&input).unwrap()).stdout(Stdio::piped()).stderr(Stdio::piped()).output().unwrap();
    let r = collect(o);
    let _ = std::fs::remove_dir_all(&d);
    assert_eq!((r.out.as_str(), r.code), ("200000\n", 0), "{}", r.err);
}

#[test]
fn empty_input_is_an_ordinary_end() {
    let r = run(LINES, b"");
    assert_eq!((r.out.as_str(), r.code), ("done\n", 0), "{}", r.err);
    let src = "package main\n\nuse fs\n\nfunc main() -> error\n    let r = fs.stdin()\n    print(\"[{try await r.read(10)}]\")\n    let line = try await r.read_line()\n    print(line)\n    return\nend\n";
    let r = run(src, b"");
    assert_eq!(r.code, 1);
    assert_eq!(r.out, "[]\nerror: end of file\n", "{}", r.err);
}

#[test]
fn a_slow_producer_is_waited_for() {
    let d = dir();
    let mut child = command(&d, LINES).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(b"first\n").unwrap();
    stdin.flush().unwrap();
    std::thread::sleep(Duration::from_millis(400));
    stdin.write_all(b"second\n").unwrap();
    drop(stdin);
    let r = collect(child.wait_with_output().unwrap());
    let _ = std::fs::remove_dir_all(&d);
    assert_eq!((r.out.as_str(), r.code), ("[first]\n[second]\ndone\n", 0), "{}", r.err);
}

#[test]
fn closing_a_reader_does_not_close_standard_input() {
    // The first reader buffers the small input and is closed; the second one must read from the
    // same standard input without an error (it finds the end, the first reader took the data).
    let src = "package main\n\nuse fs\n\nfunc main() -> error\n    let a = fs.stdin()\n    print(try await a.read_line())\n    await a.close()\n    let b = fs.stdin()\n    print(try await b.at_end())\n    return\nend\n";
    let r = run(src, b"one\n");
    assert_eq!((r.out.as_str(), r.code), ("one\ntrue\n", 0), "{}", r.err);
}

#[test]
fn utf8_text_arrives_intact_and_survives_text_functions() {
    let src = "package main\n\nuse fs\nuse text\n\nfunc main() -> error\n    let r = fs.stdin()\n    let line = try await r.read_line()\n    print(\"{line} {text.upper(line)} {line.char_count()}\")\n    return\nend\n";
    let r = run(src, "a\u{e7}\u{e3}o\n".as_bytes());
    assert_eq!((r.out.as_str(), r.code), ("a\u{e7}\u{e3}o A\u{c7}\u{c3}O 4\n", 0), "{}", r.err);
}

