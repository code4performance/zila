//! Buffered I/O: the `bufio` buffers (native and interpreted), line and byte readers and
//! buffered writers over files, and readers and writers over TCP connections. Programs are
//! compiled to native executables and run (skipped when the Visual Studio build tools are
//! missing).

use brasa::{compile_to_native, parse_source};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

fn native(src: &str) -> Option<String> {
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = std::env::temp_dir().join(format!("brasa-bio-test-{}-{}.exe", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    match compile_to_native(&file, exe.to_str().unwrap()) {
        Ok(()) => {}
        Err(e) if e.contains("could not find Visual Studio") => {
            eprintln!("no native toolchain: skipping: {}", e);
            return None;
        }
        Err(e) => panic!("compile error: {}", e),
    }
    let out = Command::new(&exe).output().unwrap();
    let _ = std::fs::remove_file(&exe);
    assert_eq!(out.status.code(), Some(0), "exit code; output: {}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    Some(String::from_utf8_lossy(&out.stdout).replace('\r', ""))
}

fn interpreted(src: &str) -> String {
    let file = std::env::temp_dir().join(format!("brasa-bio-interp-{}-{}.br", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    std::fs::write(&file, src).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_brasa-parse")).arg(&file).arg("--run").output().unwrap();
    let _ = std::fs::remove_file(&file);
    assert!(out.status.success(), "interpreter failed: {}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).replace('\r', "")
}

fn temp_path(name: &str) -> String {
    let p = std::env::temp_dir().join(format!("brasa-bio-test-{}-{}", std::process::id(), name));
    p.to_str().unwrap().replace('\\', "/")
}

/// Runs a program that works on `PATH`, then removes the file.
fn with_file(name: &str, src: &str) -> Option<String> {
    let path = temp_path(name);
    let out = native(&src.replace("PATH", &path));
    let _ = std::fs::remove_file(&path);
    out
}

// ---- the buffers ----

const BUFFERS: &str = r#"package main

use bufio

func main()
    let b = bufio.input()
    b.feed("ab")
    print(b.has_line())
    b.feed("c\nd")
    print(b.has_line())
    print(b.take_line())
    print(b.has_line())
    b.feed("e\r\n\r\nlast")
    print("[{b.take_line()}]")
    print("[{b.take_line()}]")
    print(b.has_line())
    print("[{b.take_line()}]")
    print(b.has_line())
    b.finish()
    print(b.has_line())
    print("[{b.take_line()}]")
    print(b.has_line())
    print(b.unread())
    let c = bufio.input()
    c.feed("héllo wörld")
    print("[{c.take(3)}]")
    print("[{c.take_all()}]")
    print("[{c.take(5)}]")
    let o = bufio.output()
    o.add("a")
    o.add("")
    o.add("bc")
    print(o.len())
    print(o.take())
    print(o.len())
end
"#;

#[test]
fn buffers_cut_pieces_into_lines_and_agree_with_the_interpreter() {
    let want = "false\ntrue\nabc\nfalse\n[de]\n[]\nfalse\n[]\nfalse\ntrue\n[last]\nfalse\n0\n[hé]\n[llo wörld]\n[]\n3\nabc\n0\n";
    assert_eq!(interpreted(BUFFERS), want, "interpreter output");
    if let Some(out) = native(BUFFERS) {
        assert_eq!(out, want, "native output");
    }
}

#[test]
fn the_last_line_waits_for_finish() {
    let src = "package main\n\nuse bufio\n\nfunc main()\n    let b = bufio.input()\n    b.feed(\"tail\")\n    print(b.has_line())\n    b.finish()\n    print(b.has_line())\n    print(b.take_line())\n    print(b.has_line())\nend\n";
    let want = "false\ntrue\ntail\nfalse\n";
    assert_eq!(interpreted(src), want);
    if let Some(out) = native(src) {
        assert_eq!(out, want);
    }
}

// ---- files ----

#[test]
fn lines_of_every_ending_and_the_end_of_file() {
    let src = r#"package main
use fs

func main() -> error
    let w = try await fs.create("PATH")
    try await w.write("one\ntwo\r\n\nla")
    try await w.write("st")
    try await w.close()
    let r = try await fs.open("PATH")
    while not try await r.at_end()
        let line = try await r.read_line()
        print("[{line}]")
    end
    match await r.read_line()
        ok(t) -> print("unexpected")
        err(e) -> print("error: {e.message}")
    end
    await r.close()
    return
end
"#;
    let Some(out) = with_file("lines.txt", src) else { return };
    assert_eq!(out, "[one]\n[two]\n[]\n[last]\nerror: end of file\n");
}

#[test]
fn an_empty_file_is_at_its_end_and_a_missing_file_names_the_path() {
    let src = r#"package main
use fs

func main() -> error
    let w = try await fs.create("PATH")
    try await w.close()
    let r = try await fs.open("PATH")
    print(try await r.at_end())
    match await r.read_line()
        ok(t) -> print("unexpected")
        err(e) -> print("error: {e.message}")
    end
    print("[{try await r.read_all()}]")
    match await fs.open("PATH.missing")
        ok(t) -> print("unexpected")
        err(e) -> print("failed: {e.message}")
    end
    return
end
"#;
    let path = temp_path("empty.txt");
    let Some(out) = with_file("empty.txt", src) else { return };
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(&lines[..3], ["true", "error: end of file", "[]"], "{}", out);
    assert!(lines[3].starts_with("failed: ") && lines[3].contains(&format!("{}.missing", path)) && lines[3].to_lowercase().contains("cannot find"), "{}", out);
}

#[test]
fn long_and_multibyte_lines_come_back_whole() {
    let src = r#"package main
use fs

func main() -> error
    var big = "0123456789abcdef"
    for i in 0..14
        big = big + big
    end
    let w = try await fs.create("PATH")
    try await w.write_line(big)
    try await w.write_line("ação ação")
    try await w.close()
    let r = try await fs.open("PATH")
    let first = try await r.read_line()
    print("{first == big} {first.len}")
    print(try await r.read_line())
    return
end
"#;
    let Some(out) = with_file("long.txt", src) else { return };
    assert_eq!(out, "true 262144\nação ação\n");
}

#[test]
fn bytes_and_lines_can_be_mixed_on_one_reader() {
    let src = r#"package main
use fs

func main() -> error
    let w = try await fs.create("PATH")
    try await w.write("one\ntwo\r\n\nlast\n")
    try await w.close()
    let r = try await fs.open("PATH")
    print("[{try await r.read(3)}]")
    print("[{try await r.read_line()}]")
    print("[{try await r.read_line()}]")
    let rest = try await r.read_all()
    print("rest {rest.len}")
    print("[{try await r.read(10)}]")
    print(try await r.at_end())
    return
end
"#;
    let Some(out) = with_file("mixed.txt", src) else { return };
    assert_eq!(out, "[one]\n[]\n[two]\nrest 6\n[]\ntrue\n");
}

#[test]
fn big_files_move_in_pieces_of_64_kib() {
    let src = r#"package main
use fs

func main() -> error
    let w = try await fs.create("PATH")
    for i in 0..100000
        try await w.write_line("line {i}")
    end
    print("before close {w.writes}")
    try await w.close()
    print("writes {w.writes}")
    let r = try await fs.open("PATH")
    var count = 0
    var sum = 0
    var first = ""
    var last = ""
    while not try await r.at_end()
        let line = try await r.read_line()
        if count == 0
            first = line
        end
        last = line
        count += 1
        sum += line.len
    end
    print("lines {count} {sum} {first} {last}")
    print("reads {r.reads}")
    return
end
"#;
    let Some(out) = with_file("big.txt", src) else { return };
    let size: usize = (0..100000).map(|i| format!("line {}\n", i).len()).sum();
    let sum: usize = (0..100000).map(|i| format!("line {}", i).len()).sum();
    let max = (size + 65535) / 65536 + 1;
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[2], format!("lines 100000 {} line 0 line 99999", sum), "{}", out);
    let writes: usize = lines[1].trim_start_matches("writes ").parse().unwrap();
    let before: usize = lines[0].trim_start_matches("before close ").parse().unwrap();
    let reads: usize = lines[3].trim_start_matches("reads ").parse().unwrap();
    assert!(writes >= 1 && writes <= max && before < writes, "writes {} (max {}), before close {}", writes, max, before);
    assert!(reads >= 1 && reads <= max, "reads {} (max {})", reads, max);
}

#[test]
fn nothing_is_written_before_the_flush_and_append_adds_at_the_end() {
    let src = r#"package main
use fs

func main() -> error
    let w = try await fs.create("PATH")
    try await w.write("a\n")
    print("writes {w.writes}")
    try await w.flush()
    print("writes {w.writes}")
    try await w.flush()
    print("writes {w.writes}")
    try await w.close()
    let a = try await fs.append("PATH")
    try await a.write("b")
    try await a.close()
    print(try await fs.read("PATH"))
    let again = try await fs.append("PATH")
    try await again.write_line("!")
    try await again.close()
    let text = try await fs.read("PATH")
    print("{text.len}")
    return
end
"#;
    let Some(out) = with_file("append.txt", src) else { return };
    assert_eq!(out, "writes 0\nwrites 1\nwrites 1\na\nb\n5\n");
}

#[test]
fn readers_and_writers_do_not_leak() {
    let src = r#"package main
use fs

func main() -> error
    let w = try await fs.create("PATH")
    try await w.write("first\nsecond\n")
    try await w.close()
    let warm = try await fs.open("PATH")
    let before = live_objects()
    var total = 0
    for i in 0..1500
        let r = try await fs.open("PATH")
        total += (try await r.read_line()).len
        total += (try await r.read_all()).len
        await r.close()
        let a = try await fs.append("PATH.out")
        try await a.write_line("x")
        try await a.close()
    end
    print("grew {live_objects() - before} {total > 0}")
    return
end
"#;
    let path = temp_path("leak.txt");
    let Some(out) = with_file("leak.txt", src) else { return };
    let _ = std::fs::remove_file(format!("{}.out", path));
    assert_eq!(out, "grew 0 true\n");
}

// ---- connections ----

#[test]
fn a_line_protocol_over_a_connection() {
    let src = r#"package main
use net
use task

async func serve(l: net.Listener) -> error
    let conn = try await l.accept()
    let r = conn.reader()
    let w = conn.writer()
    var count = 0
    while not try await r.at_end()
        let line = try await r.read_line()
        count += 1
        try await w.write_line("got {line}")
        try await w.flush()
    end
    try await w.write_line("bye {count}")
    try await w.close()
    await conn.close()
    return
end

func main() -> error
    let l = try await net.listen(0)
    let server = serve(l)
    let c = try await net.connect("127.0.0.1", l.port)
    let w = c.writer()
    let r = c.reader()
    try await w.write("hel")
    try await w.write("lo\nwor")
    print("sent {w.writes}")
    try await w.flush()
    print(try await r.read_line())
    try await w.write_line("ld")
    try await w.write_line("three")
    try await w.flush()
    print(try await r.read_line())
    print(try await r.read_line())
    print("sent {w.writes}")
    await c.close()
    try await server
    return
end
"#;
    let Some(out) = native(src) else { return };
    assert_eq!(out, "sent 0\ngot hello\ngot world\ngot three\nsent 2\n");
}

#[test]
fn lines_split_across_packets_and_the_end_of_the_connection() {
    let src = r#"package main
use net
use task

async func serve(l: net.Listener) -> error
    let conn = try await l.accept()
    let r = conn.reader()
    print(try await r.read_line())
    print(try await r.read_line())
    print("[{try await r.read_all()}]")
    match await r.read_line()
        ok(t) -> print("unexpected")
        err(e) -> print("error: {e.message}")
    end
    await conn.close()
    return
end

func main() -> error
    let l = try await net.listen(0)
    let server = serve(l)
    let c = try await net.connect("127.0.0.1", l.port)
    let w = c.writer()
    try await w.write("ab")
    try await w.flush()
    await sleep(30)
    try await w.write("c\nd")
    try await w.flush()
    await sleep(30)
    try await w.write("e\nrest")
    try await w.flush()
    await c.close()
    try await server
    return
end
"#;
    let Some(out) = native(src) else { return };
    assert_eq!(out, "abc\nde\n[rest]\nerror: end of file\n");
}

#[test]
fn the_lines_example_runs() {
    let src = std::fs::read_to_string("examples/lines.br").unwrap();
    let Some(out) = native(&src) else { return };
    let _ = std::fs::remove_file("lines-example.txt");
    assert!(out.contains("lines: 3") && out.contains("longest: ação"), "{}", out);
}
