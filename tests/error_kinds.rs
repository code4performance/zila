//! `error` has a kind, a message and a cause: the open `ErrorKind` type, the `errors` package, the
//! constructors and `context`, and the kinds the standard library returns. Programs are built for
//! Windows and run; the ones that touch the operating system also run on Linux under WSL (those
//! checks are skipped, with a message, when it is not usable).

use brasa::codegen_x64::{compile_to_linux, compile_to_native, CompileOptions};
use brasa::parse_source;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("brasa-kinds-test-{}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst), name))
}

/// The program built for Windows and run: (output, exit status). `None` without the toolchain.
fn windows(src: &str) -> Option<(String, i32)> {
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = temp("prog.exe");
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
    Some((String::from_utf8_lossy(&out.stdout).replace('\r', ""), out.status.code().unwrap_or(-1)))
}

fn wsl_path(p: &std::path::Path) -> String {
    let s = p.to_str().unwrap().replace('\\', "/");
    let mut chars = s.chars();
    match (chars.next(), chars.next()) {
        (Some(d), Some(':')) => format!("/mnt/{}{}", d.to_ascii_lowercase(), &s[2..]),
        _ => s,
    }
}

fn wsl_works() -> bool {
    static CHECK: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *CHECK.get_or_init(|| match Command::new("wsl.exe").args(["-e", "sh", "-c", "echo ready"]).output() {
        Ok(o) => String::from_utf8_lossy(&o.stdout).contains("ready"),
        Err(_) => false,
    })
}

/// The program built for Linux and run under WSL: (output, exit status). `None` without WSL.
fn linux(src: &str) -> Option<(String, i32)> {
    if !wsl_works() {
        eprintln!("WSL is not available: skipping the Linux run");
        return None;
    }
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = temp("prog");
    compile_to_linux(&file, exe.to_str().unwrap(), &CompileOptions::default()).unwrap_or_else(|e| panic!("compile error: {}", e));
    let name = exe.file_name().unwrap().to_str().unwrap().to_string();
    let script = format!("cp '{}' /tmp/{n} && chmod +x /tmp/{n} && /tmp/{n}; echo \"__EXIT:$?\"; rm -f /tmp/{n}", wsl_path(&exe), n = name);
    let out = Command::new("wsl.exe").args(["-e", "sh", "-c", &script]).output().unwrap();
    let _ = std::fs::remove_file(&exe);
    let text = String::from_utf8_lossy(&out.stdout).replace('\r', "");
    let (body, code) = text.rsplit_once("__EXIT:").unwrap_or_else(|| panic!("no exit marker in: {}", text));
    Some((body.to_string(), code.trim().parse().unwrap()))
}

/// Runs `src` on Windows and checks the output; where WSL works, the same program on Linux must
/// print the same.
fn expect(src: &str, want: &str) {
    if let Some((out, code)) = windows(src) {
        assert_eq!((out.as_str(), code), (want, 0), "Windows");
    }
    if let Some((out, code)) = linux(src) {
        assert_eq!((out.as_str(), code), (want, 0), "Linux");
    }
}

/// Windows only, for programs that do not touch the operating system.
fn expect_win(src: &str, want: &str) {
    if let Some((out, code)) = windows(src) {
        assert_eq!((out.as_str(), code), (want, 0));
    }
}

/// The message of the compile error of a program that must not compile.
fn compile_error(src: &str) -> String {
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = temp("bad.exe");
    match compile_to_native(&file, exe.to_str().unwrap()) {
        Ok(()) => panic!("the program compiled"),
        Err(e) => e,
    }
}

// ---- the structure of an error ----

#[test]
fn reading_the_parts() {
    expect_win(
        r#"package main
use errors
func main()
    let e = error(errors.NotFound, "no such user")
    print(e.kind == errors.NotFound, e.message, e.cause == nil)
    print(e.kind.name)
end
"#,
        "true no such user true\nstd.NotFound\n",
    );
}

#[test]
fn existing_code_keeps_working() {
    expect_win(
        r#"package main
use errors
func parse() -> int or error
    return error("cannot parse 'x'")
end
func main()
    match parse()
        ok(v) -> print("ok")
        err(e) -> print(e.message, e.kind == errors.Other)
    end
end
"#,
        "cannot parse 'x' true\n",
    );
}

#[test]
fn error_without_use_errors_and_use_errors_alone() {
    // no `use errors`: error("msg") still works and has a kind with a name
    expect_win(
        r#"package main
func fail() -> error
    return error("plain")
end
func main()
    match fail()
        ok -> print("ok")
        err(e) -> print(e.message, e.kind.name)
    end
end
"#,
        "plain std.Other\n",
    );
    // `use errors` with no other `use`
    expect_win(
        r#"package main
use errors
func main()
    print(errors.Closed.name, errors.Io.name, errors.Unsupported.name)
end
"#,
        "std.Closed std.Io std.Unsupported\n",
    );
}

// ---- open kinds ----

#[test]
fn a_library_defines_its_own_kind_and_namespaces_never_collide() {
    expect_win(
        r#"package main
use errors
let Corrupt: ErrorKind = errors.kind("mylib.Corrupt") ?? errors.Other
let A: ErrorKind = errors.kind("a.NotFound") ?? errors.Other
let B: ErrorKind = errors.kind("b.NotFound") ?? errors.Other
func read() -> error
    return error(Corrupt, "bad header")
end
func main()
    match read()
        ok -> print("ok")
        err(e) -> print(e.kind == Corrupt, e.kind.name, e.kind == errors.Other)
    end
    print(A == B, A == errors.NotFound, B == errors.NotFound, A == (errors.kind("a.NotFound") ?? errors.Other))
end
"#,
        "true mylib.Corrupt false\nfalse false false true\n",
    );
}

#[test]
fn a_bad_kind_name_and_the_std_namespace_are_input_errors() {
    expect_win(
        r#"package main
use errors
func main()
    match errors.kind("no namespace")
        ok(k) -> print("unexpected")
        err(e) -> print(e.kind == errors.InvalidInput, e.message)
    end
    match errors.kind("std.Mine")
        ok(k) -> print("unexpected")
        err(e) -> print(e.kind == errors.InvalidInput)
    end
    match errors.kind("a.b.c")
        ok(k) -> print("unexpected")
        err(e) -> print(e.kind == errors.InvalidInput)
    end
    match errors.kind("a.")
        ok(k) -> print("unexpected")
        err(e) -> print(e.kind == errors.InvalidInput)
    end
    match errors.kind("1a.b")
        ok(k) -> print("unexpected")
        err(e) -> print(e.kind == errors.InvalidInput)
    end
    print(errors.Timeout.name)
end
"#,
        "true 'no namespace' is not a kind name: write package.Name\ntrue\ntrue\ntrue\ntrue\nstd.Timeout\n",
    );
}

#[test]
fn a_text_is_not_a_kind() {
    let e = compile_error(
        r#"package main
func main()
    let e = error("NotFound", "msg")
    print(e.message)
end
"#,
    );
    assert!(e.contains("ErrorKind") && e.contains("errors.kind("), "{}", e);
    let e = compile_error(
        r#"package main
use errors
func main()
    let t: text = errors.NotFound
    print(t)
end
"#,
    );
    assert!(e.contains("expected text, found ErrorKind"), "{}", e);
}

// ---- wrapping ----

#[test]
fn context_keeps_the_kind_and_chains_causes() {
    expect_win(
        r#"package main
use errors
func load() -> error
    return error(errors.NotFound, "a.json: not found")
end
func main()
    match load()
        ok -> print("ok")
        err(e) ->
            let w = e.context("loading the config")
            print(w.kind == errors.NotFound, w.message)
            print(w.full_message)
            print(e.message, e.cause == nil)
            let c = w.cause
            print(c?.message)
            let w2 = w.context("starting")
            print(w2.full_message)
            let root = w2.cause?.cause
            print(root?.message, root?.cause == nil)
            print(w2.is(errors.NotFound), w2.is(errors.Timeout))
    end
end
"#,
        "true loading the config\nloading the config: a.json: not found\na.json: not found true\na.json: not found\nstarting: loading the config: a.json: not found\na.json: not found true\ntrue false\n",
    );
}

#[test]
fn an_explicit_cause_may_have_a_different_kind() {
    expect_win(
        r#"package main
use errors
let Corrupt: ErrorKind = errors.kind("mylib.Corrupt") ?? errors.Other
func main()
    let inner = error(errors.UnexpectedEof, "end of file")
    let e = error(Corrupt, "bad header", inner)
    print(e.kind == Corrupt, e.is(Corrupt), e.is(errors.UnexpectedEof), e.is(errors.NotFound))
    let c = e.cause
    print(c?.message, c?.kind == errors.UnexpectedEof)
    print(e.full_message)
end
"#,
        "true true true false\nend of file true\nbad header: end of file\n",
    );
}

#[test]
fn the_cause_stays_valid_after_its_error_goes_away() {
    expect_win(
        r#"package main
use errors
func inner() -> error?
    let e = error(errors.Timeout, "slow").context("outer")
    return e.cause
end
func main()
    let c = inner()
    print(c?.message, c?.kind.name)
end
"#,
        "slow std.Timeout\n",
    );
}

// ---- try ----

#[test]
fn try_carries_the_error_unchanged_through_every_level() {
    expect_win(
        r#"package main
use errors
let Corrupt: ErrorKind = errors.kind("mylib.Corrupt") ?? errors.Other
func c() -> int or error
    return error(Corrupt, "deep", error(errors.Timeout, "root"))
end
func b() -> int or error
    let v = try c()
    return v
end
func a() -> int or error
    let v = try b()
    return v
end
func main()
    match a()
        ok(v) -> print("ok")
        err(e) ->
            print(e.kind == Corrupt, e.message, e.full_message)
            let k = e.cause?.kind == errors.Timeout
            print(k)
    end
    let d = a() ?? 7
    print(d)
end
"#,
        "true deep deep: root\ntrue\n7\n",
    );
}

#[test]
fn a_missing_file_is_not_found_through_two_functions() {
    let src = r#"package main
use fs
use errors
async func b() -> text or error
    let t = try await fs.read("no-such-directory/missing.txt")
    return t
end
async func a() -> text or error
    let t = try await b()
    return t
end
func main()
    match await a()
        ok(t) -> print("unexpected")
        err(e) -> print(e.kind == errors.NotFound, e.kind.name, e.message.len > 0)
    end
end
"#;
    expect(src, "true std.NotFound true\n");
}

// ---- memory ----

#[test]
fn creating_wrapping_and_discarding_errors_leaks_nothing() {
    expect_win(
        r#"package main
use errors
let Corrupt: ErrorKind = errors.kind("mylib.Corrupt") ?? errors.Other
func wrap(e: error) -> error?
    return e.context("again")
end
func main()
    let before = live_objects()
    var i = 0
    while i < 100000
        let q = error(Corrupt, "a").context("b").context("c")
        let w = wrap(q)
        let m = w.full_message
        let k = w.kind
        let c = w.cause
        let z = error("plain")
        i += 1
    end
    print(live_objects() == before)
end
"#,
        "true\n",
    );
}

// ---- the standard library ----

#[test]
fn a_connection_refused_and_a_busy_port() {
    let src = r#"package main
use net
use errors
func main() -> error
    match await net.connect("127.0.0.1", 1)
        ok(c) -> print("unexpected")
        err(e) -> print(e.kind == errors.ConnectionRefused)
    end
    let l = try await net.listen(0)
    match await net.listen(l.port)
        ok(x) -> print("unexpected")
        err(e) -> print(e.kind == errors.AddrInUse)
    end
    match await net.connect("no-such-host.invalid", 80)
        ok(c) -> print("unexpected")
        err(e) -> print(e.kind == errors.NotFound or e.kind == errors.Io)
    end
    return
end
"#;
    expect(src, "true\ntrue\ntrue\n");
}

#[test]
fn a_refused_tls_connection_has_a_kind() {
    let src = r#"package main
use tls
use errors
func main() -> error
    match await tls.connect("127.0.0.1", 1)
        ok(c) -> print("unexpected")
        err(e) -> print(e.kind == errors.ConnectionRefused)
    end
    return
end
"#;
    expect(src, "true\n");
}

#[test]
fn end_of_input_is_an_unexpected_eof() {
    let src = r#"package main
use fs
use errors
func main() -> error
    let w = try await fs.create("kinds-eof-test.tmp")
    try await w.write("one\n")
    try await w.close()
    let r = try await fs.open("kinds-eof-test.tmp")
    print(try await r.read_line())
    match await r.read_line()
        ok(t) -> print("unexpected")
        err(e) -> print(e.kind == errors.UnexpectedEof, e.message)
    end
    await r.close()
    return
end
"#;
    // the file name is relative: the test runs in the current directory, and removes it
    let ok = std::panic::catch_unwind(|| expect(src, "one\ntrue end of file\n"));
    let _ = std::fs::remove_file("kinds-eof-test.tmp");
    if let Err(e) = ok {
        std::panic::resume_unwind(e);
    }
}

#[test]
fn parser_input_is_invalid() {
    expect_win(
        r#"package main
use text
use errors
func main()
    match text.parse_int("4x2")
        ok(v) -> print("unexpected")
        err(e) -> print(e.kind == errors.InvalidInput, e.message)
    end
    match text.parse_float("abc")
        ok(v) -> print("unexpected")
        err(e) -> print(e.kind == errors.InvalidInput)
    end
end
"#,
        "true not a whole number: \"4x2\"\ntrue\n",
    );
}

#[test]
fn operating_system_codes_map_to_kinds_and_keep_the_message() {
    // Windows codes
    expect_win(
        r#"package main
use sys
use errors
func main()
    let e = sys.error_for(2, "x")
    print(e.kind == errors.NotFound, e.message == sys.describe(2, "x"))
    print(sys.error_for(5, "x").kind == errors.PermissionDenied)
    print(sys.error_for(10061, "x").kind == errors.ConnectionRefused)
    print(sys.error_for(10048, "x").kind == errors.AddrInUse)
    print(sys.error_for(995, "x").kind == errors.Cancelled)
    print(sys.error_for(99999, "x").kind == errors.Io)
end
"#,
        "true true\ntrue\ntrue\ntrue\ntrue\ntrue\n",
    );
    // Linux error numbers (a negative result of `syscall` is accepted as it is)
    let src = r#"package main
use sys
use errors
func main()
    let e = sys.error_for(2, "x")
    print(e.kind == errors.NotFound, e.message == sys.describe(2, "x"))
    print(sys.error_for(-13, "x").kind == errors.PermissionDenied)
    print(sys.error_for(111, "x").kind == errors.ConnectionRefused)
    print(sys.error_for(98, "x").kind == errors.AddrInUse)
    print(sys.error_for(38, "x").kind == errors.Unsupported)
    print(sys.error_for(99999, "x").kind == errors.Io)
end
"#;
    if let Some((out, code)) = linux(src) {
        assert_eq!((out.as_str(), code), ("true true\ntrue\ntrue\ntrue\ntrue\ntrue\n", 0), "Linux");
    }
}

#[test]
fn closed_sockets_and_oversized_datagrams_have_kinds() {
    let src = r#"package main
use net
use errors
func main() -> error
    let a = try await net.udp(0)
    let b = try await net.udp(0)
    var big = ""
    for i in 0..7000
        big = big + "0123456789"
    end
    match await a.send_to("127.0.0.1", b.port, big)
        ok -> print("unexpected")
        err(e) -> print(e.kind == errors.InvalidInput)
    end
    await a.close()
    match await a.send_to("127.0.0.1", b.port, "x")
        ok -> print("unexpected")
        err(e) -> print(e.kind == errors.Closed)
    end
    return
end
"#;
    expect(src, "true\ntrue\n");
}

#[test]
fn a_timeout_and_a_cancelled_operation_have_kinds() {
    // `timeout` through a socket: the failure is a Timeout (the message is unchanged)
    let src = r#"package main
use net
use errors
func main() -> error
    let s = try await net.udp(0)
    match await timeout(60, s.recv())
        ok(p) -> print("unexpected")
        err(e) -> print(e.kind == errors.Timeout, e.message)
    end
    return
end
"#;
    expect(src, "true timed out
");
}

#[test]
fn the_errors_example_runs() {
    let src = std::fs::read_to_string("examples/errors.br").unwrap();
    expect_win(&src, "no settings file, using defaults\nstd.Timeout\nno answer: connection lost\nconnection lost\n75\n-1\nstd.Overflow: integer overflow: 9223372036854775807 + 1\n");
}
