//! UDP sockets (`net.udp`, `send_to`, `recv`, `close`) on the loopback interface. Programs are
//! compiled to native executables and run (skipped when the Visual Studio build tools are
//! missing). Every test binds port 0, so tests cannot collide.

use brasa::{compile_to_native, parse_source};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

/// Runs a program and returns its output; `None` when there is no native toolchain.
fn run(src: &str) -> Option<String> {
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = std::env::temp_dir().join(format!("brasa-udp-test-{}-{}.exe", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
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

/// A `main() -> error` with the given body, after `extra` declarations.
fn program(extra: &str, body: &str) -> String {
    let indented: String = body.lines().map(|l| format!("    {}\n", l)).collect();
    format!("package main\n\nuse net\nuse task\n\n{}\nfunc main() -> error\n{}    return\nend\n", extra, indented)
}

#[test]
fn port_zero_picks_a_free_port_and_a_port_in_use_is_an_error() {
    let src = program(
        "",
        "let a = try await net.udp(0)\nlet b = try await net.udp(0)\nprint(\"{a.port > 0} {b.port > 0} {a.port != b.port}\")\nmatch await net.udp(a.port)\n    ok(s) -> print(\"unexpected\")\n    err(e) -> print(\"error: {e.message}\")\nend\nprint(a.port)\n",
    );
    let Some(out) = run(&src) else { return };
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "true true true", "{}", out);
    let port = lines[2];
    assert!(lines[1].starts_with(&format!("error: cannot bind UDP port {}: ", port)), "{}", out);
}

#[test]
fn request_and_reply_between_two_sockets() {
    let extra = "async func serve(s: net.UdpSocket) -> error\n    let p = try await s.recv()\n    print(\"server got [{p.data}] from {p.host}, sender port known {p.port > 0}\")\n    try await s.send_to(p.host, p.port, \"pong: {p.data}\")\n    return\nend\n";
    let src = program(
        extra,
        "let srv = try await net.udp(0)\nlet t = serve(srv)\nlet cli = try await net.udp(0)\ntry await cli.send_to(\"127.0.0.1\", srv.port, \"ping\")\nlet reply = try await cli.recv()\nprint(\"client got [{reply.data}] from the server port {reply.port == srv.port}\")\ntry await t\n",
    );
    let Some(out) = run(&src) else { return };
    assert_eq!(
        out,
        "server got [ping] from 127.0.0.1, sender port known true\nclient got [pong: ping] from the server port true\n"
    );
}

#[test]
fn the_sender_port_is_the_clients_port() {
    let src = program(
        "",
        "let srv = try await net.udp(0)\nlet cli = try await net.udp(0)\ntry await cli.send_to(\"127.0.0.1\", srv.port, \"x\")\nlet p = try await srv.recv()\nprint(\"{p.port == cli.port} {p.host}\")\n",
    );
    let Some(out) = run(&src) else { return };
    assert_eq!(out, "true 127.0.0.1\n");
}

#[test]
fn each_receive_is_one_datagram() {
    let src = program(
        "",
        "let a = try await net.udp(0)\nlet b = try await net.udp(0)\ntry await a.send_to(\"127.0.0.1\", b.port, \"a\")\ntry await a.send_to(\"127.0.0.1\", b.port, \"bb\")\ntry await a.send_to(\"127.0.0.1\", b.port, \"ccc\")\nlet p1 = try await b.recv()\nlet p2 = try await b.recv()\nlet p3 = try await b.recv()\nprint(\"[{p1.data}] [{p2.data}] [{p3.data}]\")\n",
    );
    let Some(out) = run(&src) else { return };
    assert_eq!(out, "[a] [bb] [ccc]\n");
}

#[test]
fn an_empty_datagram_is_a_packet() {
    let src = program(
        "",
        "let a = try await net.udp(0)\nlet b = try await net.udp(0)\ntry await a.send_to(\"127.0.0.1\", b.port, \"\")\nlet p = try await b.recv()\nprint(\"[{p.data}] {p.data.len}\")\n",
    );
    let Some(out) = run(&src) else { return };
    assert_eq!(out, "[] 0\n");
}

#[test]
fn a_large_datagram_arrives_whole() {
    let src = program(
        "",
        "var big = \"\"\nfor i in 0..6000\n    big = big + \"0123456789\"\nend\nlet a = try await net.udp(0)\nlet b = try await net.udp(0)\ntry await a.send_to(\"127.0.0.1\", b.port, big)\nlet p = try await b.recv()\nprint(\"{p.data.len} {p.data == big}\")\n",
    );
    let Some(out) = run(&src) else { return };
    assert_eq!(out, "60000 true\n");
}

#[test]
fn a_datagram_larger_than_the_buffer_is_an_error_and_the_socket_keeps_working() {
    let src = program(
        "",
        "var long = \"0123456789\"\nfor i in 0..3\n    long = long + long\nend\nlet a = try await net.udp(0)\nlet b = try await net.udp(0)\ntry await a.send_to(\"127.0.0.1\", b.port, long)\ntry await a.send_to(\"127.0.0.1\", b.port, \"short\")\nmatch await b.recv(10)\n    ok(p) -> print(\"unexpected {p.data.len}\")\n    err(e) -> print(\"error: {e.message}\")\nend\nlet p = try await b.recv()\nprint(\"next: {p.data}\")\n",
    );
    let Some(out) = run(&src) else { return };
    assert_eq!(out, "error: datagram larger than 10 bytes\nnext: short\n");
}

#[test]
fn a_receive_has_a_deadline_with_timeout_and_other_tasks_run() {
    let extra = "var ticks = 0\n\nasync func ticker()\n    while true\n        await sleep(5)\n        ticks += 1\n    end\nend\n";
    let src = program(
        extra,
        "let s = try await net.udp(0)\nlet t = ticker()\nmatch await timeout(60, s.recv())\n    ok(p) -> print(\"unexpected\")\n    err(e) -> print(\"error: {e.message}\")\nend\nprint(\"ticker ran {ticks > 3}\")\n",
    );
    let Some(out) = run(&src) else { return };
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines[0].starts_with("error: ") && lines[0].contains("timed out"), "{}", out);
    assert_eq!(lines[1], "ticker ran true", "{}", out);
}

#[test]
fn a_closed_socket_reports_it_and_its_port_can_be_bound_again() {
    let src = program(
        "",
        "let s = try await net.udp(0)\nlet port = s.port\nawait s.close()\nmatch await s.recv()\n    ok(p) -> print(\"unexpected\")\n    err(e) -> print(\"recv: {e.message}\")\nend\nmatch await s.send_to(\"127.0.0.1\", 9, \"x\")\n    ok -> print(\"unexpected\")\n    err(e) -> print(\"send: {e.message}\")\nend\nawait s.close()\nlet again = try await net.udp(port)\nprint(\"rebound {again.port == port}\")\n",
    );
    let Some(out) = run(&src) else { return };
    assert_eq!(out, "recv: the socket is closed\nsend: the socket is closed\nrebound true\n");
}

#[test]
fn sending_to_a_closed_port_does_not_break_later_receives() {
    let src = program(
        "",
        "let a = try await net.udp(0)\nlet dead = try await net.udp(0)\nlet dead_port = dead.port\nawait dead.close()\ntry await a.send_to(\"127.0.0.1\", dead_port, \"anyone?\")\nawait sleep(60)\nlet b = try await net.udp(0)\ntry await b.send_to(\"127.0.0.1\", a.port, \"hello\")\nlet p = try await a.recv()\nprint(\"got {p.data}\")\n",
    );
    let Some(out) = run(&src) else { return };
    assert_eq!(out, "got hello\n");
}

#[test]
fn sockets_do_not_leak() {
    let extra = "func round(i: int) -> int\n    return i\nend\n";
    let src = program(
        extra,
        "let warm = try await net.udp(0)\ntry await warm.send_to(\"127.0.0.1\", warm.port, \"warm\")\nlet first = try await warm.recv()\nawait warm.close()\nlet before = live_objects()\nvar total = 0\nfor i in 0..1000\n    let s = try await net.udp(0)\n    try await s.send_to(\"127.0.0.1\", s.port, \"x{i}\")\n    let p = try await s.recv()\n    total += p.data.len\n    await s.close()\nend\nprint(\"grew {live_objects() - before} {total > 0}\")\n",
    );
    let Some(out) = run(&src) else { return };
    assert_eq!(out, "grew 0 true\n");
}

#[test]
fn the_udp_example_runs() {
    let src = std::fs::read_to_string("examples/udp.br").unwrap();
    let Some(out) = run(&src) else { return };
    assert!(out.contains("echoed: hello udp") && out.contains("3 datagrams"), "{}", out);
}
