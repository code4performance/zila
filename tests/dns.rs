//! Asynchronous name lookup (`net.lookup`) and its use by `connect` and `send_to`. Only `localhost`
//! and malformed names are used, so the tests never need the outside network. Programs are compiled
//! to native executables and run (skipped when the Visual Studio build tools are missing).

use zila::{compile_to_native, parse_source};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

fn run(src: &str) -> Option<String> {
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = std::env::temp_dir().join(format!("zila-dns-test-{}-{}.exe", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    match compile_to_native(&file, exe.to_str().unwrap()) {
        Ok(()) => {}
        Err(e) if e.contains("could not find Visual Studio") || e.contains("vcvars64") => {
            eprintln!("no native toolchain: skipping");
            return None;
        }
        Err(e) => panic!("compile error: {}", e),
    }
    let out = Command::new(&exe).output().unwrap();
    let _ = std::fs::remove_file(&exe);
    assert_eq!(out.status.code(), Some(0), "exit code; output: {}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    Some(String::from_utf8_lossy(&out.stdout).replace('\r', ""))
}

fn program(extra: &str, body: &str) -> String {
    let indented: String = body.lines().map(|l| format!("    {}\n", l)).collect();
    format!("package main\n\nuse net\nuse task\n\n{}\nfunc main() -> error\n{}    return\nend\n", extra, indented)
}

#[test]
fn a_name_a_numeric_address_and_a_bad_name() {
    let src = program(
        "",
        "print(try await net.lookup(\"localhost\"))\nprint(try await net.lookup(\"10.1.2.3\"))\nmatch await net.lookup(\"a b c\")\n    ok(ip) -> print(\"unexpected {ip}\")\n    err(e) -> print(\"error: {e.message}\")\nend\n",
    );
    let Some(out) = run(&src) else { return };
    assert_eq!(out, "127.0.0.1\n10.1.2.3\nerror: cannot resolve 'a b c'\n");
}

#[test]
fn other_tasks_run_while_lookups_are_in_flight() {
    let extra = "var ticks = 0\n\nasync func ticker()\n    while true\n        await sleep(2)\n        ticks += 1\n    end\nend\n\nasync func look(name: text) -> text or error\n    return try await net.lookup(name)\nend\n";
    let src = program(
        extra,
        "let t = ticker()\nlet f1 = look(\"localhost\")\nlet f2 = look(\"localhost\")\nfor i in 0..5\n    print(try await net.lookup(\"localhost\"))\nend\nprint(try await f1)\nprint(try await f2)\nvar rounds = 0\nwhile ticks == 0 and rounds < 5000\n    let again = try await net.lookup(\"localhost\")\n    rounds += 1\nend\nprint(\"ticker ran {ticks > 0}\")\n",
    );
    let Some(out) = run(&src) else { return };
    assert_eq!(out, "127.0.0.1\n127.0.0.1\n127.0.0.1\n127.0.0.1\n127.0.0.1\n127.0.0.1\n127.0.0.1\nticker ran true\n");
}

#[test]
fn connect_by_name_works_like_connect_by_address() {
    let extra = "async func serve(l: net.Listener) -> error\n    let conn = try await l.accept()\n    let msg = try await conn.read(64)\n    try await conn.write(\"echo: {msg}\")\n    await conn.close()\n    return\nend\n";
    let src = program(
        extra,
        "let l = try await net.listen(0)\nlet server = serve(l)\nlet c = try await net.connect(\"localhost\", l.port)\ntry await c.write(\"hello\")\nprint(try await c.read(64))\ntry await server\n",
    );
    let Some(out) = run(&src) else { return };
    assert_eq!(out, "echo: hello\n");
}

#[test]
fn udp_send_to_a_name() {
    let src = program(
        "",
        "let a = try await net.udp(0)\nlet b = try await net.udp(0)\ntry await a.send_to(\"localhost\", b.port, \"by name\")\nlet p = try await b.recv()\nprint(\"[{p.data}] from {p.host}\")\n",
    );
    let Some(out) = run(&src) else { return };
    assert_eq!(out, "[by name] from 127.0.0.1\n");
}

#[test]
fn connect_and_send_to_report_names_that_cannot_be_resolved() {
    let src = program(
        "",
        "match await net.connect(\"a b c\", 80)\n    ok(c) -> print(\"unexpected\")\n    err(e) -> print(\"connect: {e.message}\")\nend\nlet s = try await net.udp(0)\nmatch await s.send_to(\"a b c\", 80, \"x\")\n    ok -> print(\"unexpected\")\n    err(e) -> print(\"send_to: {e.message}\")\nend\n",
    );
    let Some(out) = run(&src) else { return };
    assert_eq!(out, "connect: cannot resolve 'a b c'\nsend_to: cannot resolve 'a b c'\n");
}

#[test]
fn a_lookup_can_have_a_deadline() {
    let src = program("", "print(try await timeout(2000, net.lookup(\"localhost\")))\n");
    let Some(out) = run(&src) else { return };
    assert_eq!(out, "127.0.0.1\n");
}

#[test]
fn lookups_do_not_leak() {
    let src = program(
        "",
        "let warm = try await net.lookup(\"localhost\")\nmatch await net.lookup(\"a b c\")\n    ok(x) -> print(\"unexpected\")\n    err(e) -> print(\"warm: {e.message}\")\nend\nlet before = live_objects()\nvar n = 0\nfor i in 0..500\n    let ip = try await net.lookup(\"localhost\")\n    n += ip.len\n    match await net.lookup(\"a b c\")\n        ok(x) -> n += 1\n        err(e) -> n += 0\n    end\nend\nprint(\"grew {live_objects() - before} {n > 0}\")\n",
    );
    let Some(out) = run(&src) else { return };
    assert_eq!(out, "warm: cannot resolve 'a b c'\ngrew 0 true\n");
}
