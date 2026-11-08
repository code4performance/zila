//! The `web` library (spec `web-library`): HTTP/1.1 server, router, middleware, JSON helpers,
//! error mapping, client and HTML templates. Every program starts its own servers and clients on
//! 127.0.0.1 (port 0: a free one); nothing reaches the network. Programs are built to native
//! executables and run with a time limit; the Linux half runs through WSL and is skipped without it.
//!
//! Zila Lang text literals treat `{` as an interpolation: JSON inside the programs is written between
//! `‹` and `›` (see `prep`).

use zila::codegen_x64::{build_os, compile_to_linux, CompileOptions};
use zila::{compile_to_native, parse_source};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Once;
use std::time::{Duration, Instant};

static N: AtomicUsize = AtomicUsize::new(0);
static INIT: Once = Once::new();

fn setup() {
    INIT.call_once(|| {
        let dir = std::env::temp_dir().join(format!("zila-web-test-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        zila::comptime::set_cache_dir(&dir);
    });
}

/// Replaces each `‹...›` with the Zila Lang text literal that holds the characters in between.
fn prep(src: &str) -> String {
    let mut out = String::new();
    let mut rest = src;
    while let Some(a) = rest.find('‹') {
        out.push_str(&rest[..a]);
        let after = &rest[a + '‹'.len_utf8()..];
        let b = after.find('›').expect("unterminated ‹");
        out.push('"');
        for c in after[..b].chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '{' => out.push_str("\\{"),
                '}' => out.push_str("\\}"),
                '\n' => out.push_str("\\n"),
                c => out.push(c),
            }
        }
        out.push('"');
        rest = &after[b + '›'.len_utf8()..];
    }
    out.push_str(rest);
    out
}

/// Helpers every program has: one raw request on a fresh connection, and a keep-alive reader.
const PRELUDE: &str = r#"
use web
use json
use net
use errors
use task
use text

// the whole reply to `data` on a fresh connection (the server closes it)
func raw(port: long, data: text) -> text or error
    let c = try await net.connect("127.0.0.1", port)
    try await c.write(data)
    let r = c.reader()
    let all = try await r.read_all()
    await c.close()
    return all
end

func req(port: long, path: text) -> text or error
    return try raw(port, "GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
end

// the part of a raw reply after the blank line
func body_of(reply: text) -> text
    let i = text.index_of(reply, "\r\n\r\n")
    if i < 0
        return reply
    end
    return reply.slice(i + 4, reply.len)
end

func first_line(reply: text) -> text
    let i = text.index_of(reply, "\r\n")
    if i < 0
        return reply
    end
    return reply.slice(0, i)
end

// a one line summary: status and body
func brief(reply: text) -> text
    return first_line(reply) + " | " + body_of(reply)
end

// one response on a kept-alive connection: status line and body
func read_one(r: net.Reader) -> text or error
    let status = try await r.read_line()
    var length = 0
    while true
        let l = try await r.read_line()
        if l.len == 0
            break
        end
        if text.starts_with(text.lower(l), "content-length:")
            length = try text.parse_int(text.trim(l.slice(15, l.len)))
        end
    end
    var body = ""
    if length > 0
        body = try await r.read(length)
    end
    return status + " | " + body
end
"#;

fn program(globals: &str, body: &str) -> String {
    let body: String = body.lines().map(|l| format!("    {}\n", l)).collect();
    prep(&format!("package main\n{}\n{}\nfunc main() -> error\n{}    return\nend\n", PRELUDE, globals, body))
}

fn exe_path(tag: &str, ext: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("zila-web-test-{}-{}-{}{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst), tag, ext))
}

fn build(src: &str) -> Result<std::path::PathBuf, String> {
    setup();
    if let Ok(dir) = std::env::var("ZILA_WEB_DUMP") {
        let _ = std::fs::write(std::path::Path::new(&dir).join(format!("web-test-{}.br", N.load(Ordering::SeqCst))), src);
    }
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = exe_path("w", ".exe");
    match compile_to_native(&file, exe.to_str().unwrap()) {
        Err(e) if e.contains("could not find Visual Studio") => Err("no toolchain".into()),
        r => r.map(|_| exe),
    }
}

/// Runs an executable with a time limit (the program is killed when it takes longer).
fn run_limited(mut cmd: Command, limit: Duration) -> (String, i32) {
    let mut child = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let start = Instant::now();
    loop {
        match child.try_wait().unwrap() {
            Some(_) => break,
            None if start.elapsed() > limit => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("the program did not finish within {:?}", limit);
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    let out = child.wait_with_output().unwrap();
    (String::from_utf8_lossy(&out.stdout).replace('\r', "") + &String::from_utf8_lossy(&out.stderr).replace('\r', ""), out.status.code().unwrap_or(-1))
}

/// Builds and runs a program: its output.
fn run(src: &str) -> String {
    let exe = build(src).unwrap_or_else(|e| panic!("compile error: {}", e));
    let (out, _) = run_limited(Command::new(&exe), Duration::from_secs(120));
    let _ = std::fs::remove_file(&exe);
    out
}

fn compile_error(src: &str) -> String {
    match build(src) {
        Ok(exe) => {
            let _ = std::fs::remove_file(exe);
            panic!("expected a compile error")
        }
        Err(e) => e,
    }
}

fn wsl_ok() -> bool {
    static CHECK: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *CHECK.get_or_init(|| Command::new("wsl.exe").args(["-e", "sh", "-c", "echo ready"]).output().map(|o| String::from_utf8_lossy(&o.stdout).contains("ready")).unwrap_or(false))
}

/// Builds for Linux and runs under WSL with a time limit; `None` without WSL.
fn run_linux(src: &str) -> Option<String> {
    if !wsl_ok() {
        eprintln!("WSL is not available: skipping the Linux half");
        return None;
    }
    setup();
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = exe_path("l", "");
    compile_to_linux(&file, exe.to_str().unwrap(), &CompileOptions::default()).unwrap_or_else(|e| panic!("linux compile error: {}", e));
    let p = exe.to_str().unwrap().replace('\\', "/");
    let wsl = format!("/mnt/{}{}", p[..1].to_ascii_lowercase(), &p[2..]);
    let name = exe.file_name().unwrap().to_str().unwrap().to_string();
    let script = format!("cp '{}' /tmp/{n} && chmod +x /tmp/{n} && timeout 100 /tmp/{n} 2>&1; rm -f /tmp/{n}", wsl, n = name);
    let mut cmd = Command::new("wsl.exe");
    cmd.args(["-e", "sh", "-c", &script]);
    let (out, _) = run_limited(cmd, Duration::from_secs(150));
    let _ = std::fs::remove_file(&exe);
    Some(out)
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

#[test]
fn web_is_rejected_in_a_bare_metal_kernel() {
    let boot = parse_source(&std::fs::read_to_string("os/boot.br").unwrap()).unwrap();
    let kernel = parse_source("package main\nuse web\nfunc console_write(s: text)\nend\nfunc main()\nend\n").unwrap();
    let e = build_os(&boot, &kernel).unwrap_err();
    assert!(e.contains("web") && e.contains("bare-metal"), "{}", e);
}

#[test]
fn web_compiles_for_both_targets() {
    setup();
    let src = program("", "print(\"hi\")");
    let file = parse_source(&src).unwrap();
    let out = std::env::temp_dir().join(format!("zila-web-elf-{}", std::process::id()));
    compile_to_linux(&file, out.to_str().unwrap(), &CompileOptions::default()).unwrap_or_else(|e| panic!("{}", e));
    let _ = std::fs::remove_file(&out);
    if let Ok(exe) = build(&src) {
        let _ = std::fs::remove_file(exe);
    }
}

// ---------------------------------------------------------------------------
// Responses and records
// ---------------------------------------------------------------------------

#[test]
fn constructors_set_status_headers_and_body() {
    let out = run(&program(
        "",
        r#"
let t = web.text(200, "hi")
print("{t.status} {t.headers["Content-Type"]} {t.body}")
let j = web.json_text(201, ‹{"a":1}›)
print("{j.status} {j.headers["Content-Type"]} {j.body}")
let n = web.not_found()
print("{n.status} {n.headers["Content-Type"]} {n.body}")
let r = web.redirect("/there")
print("{r.status} {r.headers["Location"]}")
let h = web.html_page(200, web.raw("<p>x</p>"))
print("{h.status} {h.headers["Content-Type"]} {h.body}")
print(web.reason(404))
"#,
    ));
    assert_eq!(
        out,
        "200 text/plain; charset=utf-8 hi\n201 application/json {\"a\":1}\n404 application/json {\"error\":\"not found\"}\n302 /there\n200 text/html; charset=utf-8 <p>x</p>\nNot Found\n"
    );
}

#[test]
fn a_plain_text_response_has_the_headers_the_server_adds() {
    let out = run(&program(
        "",
        r#"
var app = web.server(port: 0)
app.get("/hi", func(req)
    return web.text(200, "hi")
end)
let up = try app.start()
let reply = try req(up.port, "/hi")
print(reply)
"#,
    ));
    assert_eq!(out, "HTTP/1.1 200 OK\nContent-Type: text/plain; charset=utf-8\nContent-Length: 2\nConnection: close\n\nhi\n");
}

#[test]
fn requests_expose_method_path_query_headers_and_body() {
    let out = run(&program(
        "",
        r#"
var app = web.server(port: 0)
app.get("/echo/:word", func(req)
    return web.text(200, "{req.param("word") ?? "-"}|{req.query_param("a") ?? "-"}|{req.query_param("b") ?? "-"}|{req.header("X-NAME") ?? "-"}|{req.path}|{req.query_param("none") ?? "-"}")
end)
app.post("/body", func(req)
    return web.text(200, "{req.method.name()} {req.body}")
end)
let up = try app.start()
print(brief(try raw(up.port, "GET /echo/a%20b?a=1%202&b=x+y HTTP/1.1\r\nHost: x\r\nx-name: Ana\r\nConnection: close\r\n\r\n")))
print(brief(try raw(up.port, "POST /body HTTP/1.1\r\nHost: x\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello")))
"#,
    ));
    assert_eq!(out, "HTTP/1.1 200 OK | a b|1 2|x y|Ana|/echo/a b|-\nHTTP/1.1 200 OK | POST hello\n");
}

// ---------------------------------------------------------------------------
// Parsing, limits, keep-alive
// ---------------------------------------------------------------------------

#[test]
fn a_chunked_body_is_reassembled() {
    let out = run(&program(
        "",
        r#"
var app = web.server(port: 0)
app.post("/c", func(req)
    return web.text(200, req.body)
end)
let up = try app.start()
print(brief(try raw(up.port, "POST /c HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n")))
print(brief(try raw(up.port, "POST /c HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n3;ext=1\r\nabc\r\n0\r\nTrailer: x\r\n\r\n")))
"#,
    ));
    assert_eq!(out, "HTTP/1.1 200 OK | hello world\nHTTP/1.1 200 OK | abc\n");
}

#[test]
fn bad_requests_get_their_status() {
    let big_line = "a".repeat(9000);
    let many_headers: String = (0..400).map(|i| format!("X-H{}: {}\\r\\n", i, "v".repeat(60))).collect();
    let src = program(
        "",
        &format!(
            r#"
var app = web.server(port: 0)
app.get("/", func(req)
    return web.text(200, "ok")
end)
app.post("/", func(req)
    return web.text(200, "ok")
end)
app.max_body = 100
let up = try app.start()
print(first_line(try raw(up.port, "GET /\r\nHost: x\r\n\r\n")))
print(first_line(try raw(up.port, "GET / HTTP/2\r\nHost: x\r\n\r\n")))
print(first_line(try raw(up.port, "BREW / HTTP/1.1\r\nHost: x\r\n\r\n")))
print(first_line(try raw(up.port, "GET /%zz HTTP/1.1\r\nHost: x\r\n\r\n")))
print(first_line(try raw(up.port, "GET /?a=%4 HTTP/1.1\r\nHost: x\r\n\r\n")))
print(first_line(try raw(up.port, "GET / HTTP/1.1\r\nbad header line\r\n\r\n")))
print(first_line(try raw(up.port, "POST / HTTP/1.1\r\nContent-Length: 3\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n")))
print(first_line(try raw(up.port, "POST / HTTP/1.1\r\nContent-Length: abc\r\n\r\n")))
print(first_line(try raw(up.port, "POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n")))
print(first_line(try raw(up.port, "POST / HTTP/1.1\r\nTransfer-Encoding: gzip\r\n\r\n")))
// declared too large: answered without reading the body
print(first_line(try raw(up.port, "POST / HTTP/1.1\r\nContent-Length: 101\r\n\r\n")))
print(first_line(try raw(up.port, "GET /{big} HTTP/1.1\r\nHost: x\r\n\r\n")))
print(first_line(try raw(up.port, "GET / HTTP/1.1\r\n{headers}\r\n")))
print(first_line(try raw(up.port, "GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")))
"#,
            big = big_line,
            headers = many_headers
        ),
    );
    let out = run(&src);
    assert_eq!(
        out,
        "HTTP/1.1 400 Bad Request\nHTTP/1.1 400 Bad Request\nHTTP/1.1 501 Not Implemented\nHTTP/1.1 400 Bad Request\nHTTP/1.1 400 Bad Request\nHTTP/1.1 400 Bad Request\nHTTP/1.1 400 Bad Request\nHTTP/1.1 400 Bad Request\nHTTP/1.1 400 Bad Request\nHTTP/1.1 501 Not Implemented\nHTTP/1.1 413 Payload Too Large\nHTTP/1.1 414 URI Too Long\nHTTP/1.1 431 Request Header Fields Too Large\nHTTP/1.1 200 OK\n"
    );
}

#[test]
fn keep_alive_rules_and_head() {
    let out = run(&program(
        "",
        r#"
var app = web.server(port: 0)
app.get("/a", func(req)
    return web.text(200, "first")
end)
app.get("/b", func(req)
    return web.text(200, "second")
end)
let up = try app.start()

// two requests on one connection, answered in order
let c = try await net.connect("127.0.0.1", up.port)
let r = c.reader()
try await c.write("GET /a HTTP/1.1\r\nHost: x\r\n\r\n")
print(try read_one(r))
try await c.write("GET /b HTTP/1.1\r\nHost: x\r\n\r\n")
print(try read_one(r))
// both at once (pipelined), the second one closes
try await c.write("GET /a HTTP/1.1\r\nHost: x\r\n\r\nGET /b HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
print(try read_one(r))
print(try read_one(r))
print(try await r.at_end())
await c.close()

// HTTP/1.0 closes after the response unless it asks to keep the connection
let one = try raw(up.port, "GET /a HTTP/1.0\r\n\r\n")
print(text.contains(one, "Connection: close"))
let kept = try await net.connect("127.0.0.1", up.port)
let kr = kept.reader()
try await kept.write("GET /a HTTP/1.0\r\nConnection: keep-alive\r\n\r\n")
print(try read_one(kr))
try await kept.write("GET /b HTTP/1.0\r\n\r\n")
print(try read_one(kr))
print(try await kr.at_end())
await kept.close()

// HEAD: headers of the GET, no body
let head = try raw(up.port, "HEAD /a HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
print(text.contains(head, "Content-Length: 5"))
print(body_of(head).len)
"#,
    ));
    assert_eq!(
        out,
        "HTTP/1.1 200 OK | first\nHTTP/1.1 200 OK | second\nHTTP/1.1 200 OK | first\nHTTP/1.1 200 OK | second\ntrue\ntrue\nHTTP/1.1 200 OK | first\nHTTP/1.1 200 OK | second\ntrue\ntrue\n0\n"
    );
}

#[test]
fn a_stalled_request_gets_408_while_another_client_is_served() {
    let out = run(&program(
        "",
        r#"
var app = web.server(port: 0)
app.get("/ok", func(req)
    return web.text(200, "fine")
end)
app.read_timeout = 400
app.idle_timeout = 300
let up = try app.start()
// half a head, then silence
let slow = try await net.connect("127.0.0.1", up.port)
try await slow.write("GET /ok HTTP/1.1\r\nHost: x\r\nX-Slow: ")
// meanwhile another client is served at once
print(brief(try req(up.port, "/ok")))
let t0 = task.now()
let r = slow.reader()
let reply = try await r.read_all()
print(first_line(reply))
print(task.now() - t0 < 5000)
await slow.close()
// an idle connection that never sends a request is closed silently
let idle = try await net.connect("127.0.0.1", up.port)
let ir = idle.reader()
print("idle: '{try await ir.read_all()}'")
await idle.close()
// an idle kept-alive connection is closed too
let k = try await net.connect("127.0.0.1", up.port)
let kr = k.reader()
try await k.write("GET /ok HTTP/1.1\r\nHost: x\r\n\r\n")
print(try read_one(kr))
print(try await kr.at_end())
await k.close()
"#,
    ));
    assert_eq!(out, "HTTP/1.1 200 OK | fine\nHTTP/1.1 408 Request Timeout\ntrue\nidle: ''\nHTTP/1.1 200 OK | fine\ntrue\n");
}

fn write_timeout_program() -> String {
    program(
        "",
        r#"
var app = web.server(port: 0)
let big = text.repeat("x", 4000000)
app.get("/big", func(req)
    return web.text(200, big)
end)
app.get("/ok", func(req)
    return web.text(200, "fine")
end)
app.write_timeout = 400
app.idle_timeout = 400
app.max_connections = 1
let up = try app.start()
// a client that asks for 4 MB and never reads: the server's write stalls and is given up
let stuck = try await net.connect("127.0.0.1", up.port)
try await stuck.write("GET /big HTTP/1.1\r\nHost: x\r\n\r\n")
await sleep(1500)
// its slot is free again: the write was given up (Windows may take the 4 MB into the kernel's buffers and
// finish it, then the idle timeout frees the slot), and nothing hangs or leaks
print(brief(try req(up.port, "/ok")))
await stuck.close()
"#,
    )
}

#[test]
fn a_write_that_never_finishes_is_given_up_after_write_timeout() {
    let out = run(&write_timeout_program());
    assert_eq!(out, "HTTP/1.1 200 OK | fine\n");
    if let Some(linux) = run_linux(&write_timeout_program()) {
        assert_eq!(linux, "HTTP/1.1 200 OK | fine\n");
    }
}

fn hundred_clients_program() -> String {
    program(
        r#"
async func client(port: long, id: int) -> int
    let c = match await net.connect("127.0.0.1", port)
        ok(c) -> c
        err(e) -> return 0
    end
    let r = c.reader()
    var good = 0
    for k in 0..10
        match await c.write("GET /item/{id}/{k} HTTP/1.1\r\nHost: x\r\n\r\n")
            ok -> good += 0
            err(e) -> return good
        end
        match read_one(r)
            ok(s) ->
                if s == "HTTP/1.1 200 OK | item {id}.{k}"
                    good += 1
                end
            err(e) -> return good
        end
    end
    await c.close()
    return good
end
"#,
        r#"
var app = web.server(port: 0)
app.get("/item/:id/:k", func(req)
    return web.text(200, "item {req.param("id") ?? "?"}.{req.param("k") ?? "?"}")
end)
let up = try app.start()
var fs: []Future<int> = []
for id in 0..100
    fs.add(client(up.port, id))
end
var total = 0
for f in fs
    total += await f
end
print("correct answers: {total}")
"#,
    )
}

#[test]
fn a_hundred_clients_with_ten_requests_each_all_get_the_right_answer() {
    let out = run(&hundred_clients_program());
    assert_eq!(out, "correct answers: 1000\n");
}

fn busy_program() -> String {
    program(
        "",
        r#"
var app = web.server(port: 0)
app.get("/", func(req)
    return web.text(200, "ok")
end)
let up = try app.start()
var conns: []net.Conn = []
for i in 0..1030
    conns.add(try await net.connect("127.0.0.1", up.port))
end
var busy = 0
for i in 1024..1030
    let r = conns[i].reader()
    let reply = try await r.read_all()
    if first_line(reply) == "HTTP/1.1 503 Service Unavailable"
        busy += 1
    end
end
print("busy: {busy}")
// the first 1024 are being served: one of them still works
let c = conns[0]
let r0 = c.reader()
try await c.write("GET / HTTP/1.1\r\nHost: x\r\n\r\n")
print(try read_one(r0))
"#,
    )
}

#[test]
fn connections_beyond_max_connections_get_503() {
    let out = run(&busy_program());
    assert_eq!(out, "busy: 6\nHTTP/1.1 200 OK | ok\n");
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

#[test]
fn routes_match_by_method_then_segments_static_before_parameter() {
    let out = run(&program(
        "",
        r#"
var app = web.server(port: 0)
app.get("/users/:id", func(req)
    return web.text(200, "id={req.param("id") ?? "?"}")
end)
app.get("/users/me", func(req)
    return web.text(200, "me")
end)
app.get("/users/:id/posts/:post", func(req)
    return web.text(200, "post {req.param("post") ?? "?"} of {req.param("id") ?? "?"}")
end)
app.get("/files/*rest", func(req)
    return web.text(200, "rest={req.param("rest") ?? "?"}")
end)
app.get("/files/readme", func(req)
    return web.text(200, "readme")
end)
app.get("/items", func(req)
    return web.text(200, "items")
end)
app.post("/items", func(req)
    return web.text(201, "made")
end)
app.put("/x", func(req)
    return web.text(200, "put")
end)
app.delete("/x", func(req)
    return web.text(200, "delete")
end)
app.patch("/x", func(req)
    return web.text(200, "patch")
end)
let up = try app.start()
for p in ["/users/42", "/users/me", "/users/42/", "/users/7/posts/9", "/files/a/b/c.txt", "/files/readme", "/files/", "/users/a%20b", "/items", "/nothing", "/users"]
    print("{p} -> {brief(try req(up.port, p))}")
end
print(brief(try raw(up.port, "DELETE /items HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")))
let allow = try raw(up.port, "DELETE /items HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
print(text.contains(allow, "Allow: GET, POST, HEAD"))
print(brief(try raw(up.port, "POST /items HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")))
print(brief(try raw(up.port, "PUT /x HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")))
print(brief(try raw(up.port, "PATCH /x HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")))
print(brief(try raw(up.port, "DELETE /x HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")))
print(first_line(try raw(up.port, "HEAD /items HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")))
"#,
    ));
    let expected = [
        "/users/42 -> HTTP/1.1 200 OK | id=42",
        "/users/me -> HTTP/1.1 200 OK | me",
        "/users/42/ -> HTTP/1.1 200 OK | id=42",
        "/users/7/posts/9 -> HTTP/1.1 200 OK | post 9 of 7",
        "/files/a/b/c.txt -> HTTP/1.1 200 OK | rest=a/b/c.txt",
        "/files/readme -> HTTP/1.1 200 OK | readme",
        "/files/ -> HTTP/1.1 200 OK | rest=",
        "/users/a%20b -> HTTP/1.1 200 OK | id=a b",
        "/items -> HTTP/1.1 200 OK | items",
        "/nothing -> HTTP/1.1 404 Not Found | {\"error\":\"not found\"}",
        "/users -> HTTP/1.1 404 Not Found | {\"error\":\"not found\"}",
        "HTTP/1.1 405 Method Not Allowed | {\"error\":\"method not allowed\"}",
        "true",
        "HTTP/1.1 201 Created | made",
        "HTTP/1.1 200 OK | put",
        "HTTP/1.1 200 OK | patch",
        "HTTP/1.1 200 OK | delete",
        "HTTP/1.1 200 OK",
    ];
    assert_eq!(out, expected.join("\n") + "\n");
}

// ---------------------------------------------------------------------------
// Middleware and errors
// ---------------------------------------------------------------------------

#[test]
fn middleware_runs_outermost_first_wraps_404_and_may_short_circuit() {
    let out = run(&program(
        r#"
var trace = ""

func tag(name: text) -> web.Middleware
    return func(req, next)
        trace = trace + name + "-in "
        let r = next(req)
        trace = trace + name + "-out "
        return r
    end
end
"#,
        r#"
var app = web.server(port: 0)
app.use(tag("a"))
app.use(tag("b"))
app.use(func(req, next)
    if req.path == "/secret"
        return web.text(401, "no")
    end
    return next(req)
end)
app.get("/h", func(req)
    trace = trace + "handler "
    return web.text(200, "h")
end)
app.get("/secret", func(req)
    trace = trace + "SECRET "
    return web.text(200, "never")
end)
let up = try app.start()
print(brief(try req(up.port, "/h")))
print(trace)
trace = ""
print(brief(try req(up.port, "/secret")))
print(trace)
trace = ""
print(first_line(try req(up.port, "/missing")))
print(trace)
"#,
    ));
    assert_eq!(
        out,
        "HTTP/1.1 200 OK | h\na-in b-in handler b-out a-out \nHTTP/1.1 401 Unauthorized | no\na-in b-in b-out a-out \nHTTP/1.1 404 Not Found\na-in b-in b-out a-out \n"
    );
}

#[test]
fn the_logger_prints_method_path_status_and_milliseconds() {
    let out = run(&program(
        "",
        r#"
var app = web.server(port: 0)
app.use(web.logger())
app.get("/ping", func(req)
    return web.text(200, "pong")
end)
app.get("/bad", func(req)
    return error(errors.NotFound, "gone")
end)
let up = try app.start()
let a = try req(up.port, "/ping")
let b = try req(up.port, "/nope")
let c = try req(up.port, "/bad")
"#,
    ));
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 3, "{}", out);
    for (line, prefix) in lines.iter().zip(["GET /ping 200 ", "GET /nope 404 ", "GET /bad 404 "]) {
        assert!(line.starts_with(prefix) && line.ends_with("ms"), "{}", out);
    }
}

#[test]
fn errors_become_statuses_by_kind_and_5xx_messages_stay_in_the_log() {
    let out = run(&program(
        r#"
var logged = ""
func fail_with(kind: ErrorKind, message: text) -> web.Handler
    return func(req)
        return error(kind, message)
    end
end
"#,
        r#"
var app = web.server(port: 0)
app.log_to(func(line)
    logged = logged + line + "\n"
end)
app.get("/invalid", fail_with(errors.InvalidInput, "bad input"))
app.get("/unauth", fail_with(errors.Unauthenticated, "who are you"))
app.get("/denied", fail_with(errors.PermissionDenied, "not yours"))
app.get("/missing", fail_with(errors.NotFound, "no customer 7"))
app.get("/exists", fail_with(errors.AlreadyExists, "taken"))
app.get("/down", fail_with(errors.Unavailable, "db down"))
app.get("/slow", fail_with(errors.Timeout, "too slow"))
app.get("/other", fail_with(errors.Other, "disk /var/db failed"))
app.get("/plain", func(req)
    return error("plain failure /secret/path")
end)
let up = try app.start()
for p in ["/invalid", "/unauth", "/denied", "/missing", "/exists", "/down", "/slow", "/other", "/plain"]
    print("{p} {brief(try req(up.port, p))}")
end
print("log:")
print(logged)
"#,
    ));
    let expected = [
        "/invalid HTTP/1.1 400 Bad Request | {\"error\":\"bad input\"}",
        "/unauth HTTP/1.1 401 Unauthorized | {\"error\":\"who are you\"}",
        "/denied HTTP/1.1 403 Forbidden | {\"error\":\"not yours\"}",
        "/missing HTTP/1.1 404 Not Found | {\"error\":\"no customer 7\"}",
        "/exists HTTP/1.1 409 Conflict | {\"error\":\"taken\"}",
        "/down HTTP/1.1 503 Service Unavailable | internal error",
        "/slow HTTP/1.1 504 Gateway Timeout | internal error",
        "/other HTTP/1.1 500 Internal Server Error | internal error",
        "/plain HTTP/1.1 500 Internal Server Error | internal error",
        "log:",
        "web: GET /down: db down",
        "web: GET /slow: too slow",
        "web: GET /other: disk /var/db failed",
        "web: GET /plain: plain failure /secret/path",
        "",
    ];
    assert_eq!(out, expected.join("\n") + "\n");
}

#[test]
fn on_error_replaces_the_mapper() {
    let out = run(&program(
        "",
        r#"
var app = web.server(port: 0)
app.on_error(func(e)
    return web.text(422, "teapot: {e.message}")
end)
app.get("/x", func(req)
    return error(errors.NotFound, "nothing")
end)
let up = try app.start()
print(brief(try req(up.port, "/x")))
"#,
    ));
    assert_eq!(out, "HTTP/1.1 422 Unprocessable Entity | teapot: nothing\n");
}

// ---------------------------------------------------------------------------
// JSON
// ---------------------------------------------------------------------------

const CUSTOMER: &str = r#"
type Customer = record
    id: i32
    name: text
end
"#;

#[test]
fn json_bodies_round_trip_and_bad_ones_are_400_or_415() {
    let out = run(&program(
        CUSTOMER,
        r#"
var app = web.server(port: 0)
app.post("/customers", func(req)
    let c = try web.read_json(Customer, req)
    return web.created(Customer, c)
end)
app.get("/one", func(req)
    return web.json(Customer, 200, Customer { id: 7, name: "Bo" })
end)
let up = try app.start()
let post = "POST /customers HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: "
let good = ‹{"id":1,"name":"Ana"}›
let bad = ‹{"id":"x","name":"Ana"}›
let other = try raw(up.port, post + "{good.len}\r\n\r\n" + good)
print(brief(other))
print(text.contains(other, "Content-Type: application/json"))
print(brief(try raw(up.port, post + "{bad.len}\r\n\r\n" + bad)))
print(brief(try raw(up.port, post + "2\r\n\r\n" + ‹{x›)))
let plain = "POST /customers HTTP/1.1\r\nHost: x\r\nContent-Type: text/plain\r\nConnection: close\r\nContent-Length: {good.len}\r\n\r\n" + good
print(brief(try raw(up.port, plain)))
let none = "POST /customers HTTP/1.1\r\nHost: x\r\nConnection: close\r\nContent-Length: {good.len}\r\n\r\n" + good
print(first_line(try raw(up.port, none)))
print(brief(try req(up.port, "/one")))
"#,
    ));
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "HTTP/1.1 201 Created | {\"id\":1,\"name\":\"Ana\"}");
    assert_eq!(lines[1], "true");
    assert!(lines[2].starts_with("HTTP/1.1 400 Bad Request | {\"error\":\"") && lines[2].contains("$.id"), "{}", out);
    assert!(lines[3].starts_with("HTTP/1.1 400 Bad Request | {\"error\":\""), "{}", out);
    assert!(lines[4].starts_with("HTTP/1.1 415 Unsupported Media Type | {\"error\":\""), "{}", out);
    assert_eq!(lines[5], "HTTP/1.1 415 Unsupported Media Type");
    assert_eq!(lines[6], "HTTP/1.1 200 OK | {\"id\":7,\"name\":\"Bo\"}");
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

/// A raw server that answers each connection with a fixed text (after reading the request head).
const RAW_SERVER: &str = r#"
async func raw_server(l: net.Listener, replies: []text) -> int
    var n = 0
    for reply in replies
        let c = match await l.accept()
            ok(c) -> c
            err(e) -> return n
        end
        let r = c.reader()
        while true
            let line = match await r.read_line()
                ok(s) -> s
                err(e) -> break
            end
            if line.len == 0
                break
            end
        end
        if reply != "<silence>"
            match await c.write(reply)
                ok -> n += 0
                err(e) -> n += 0
            end
            await c.close()
        else
            // never answers: wait for the client to give up
            let rest = match await r.read_all()
                ok(s) -> s
                err(e) -> ""
            end
            await c.close()
        end
        n += 1
    end
    return n
end
"#;

#[test]
fn the_client_reads_content_length_chunked_and_close_delimited_bodies() {
    let out = run(&program(
        RAW_SERVER,
        r#"
var app = web.server(port: 0)
app.get("/hi", func(req)
    return web.text(200, "ok")
end)
app.post("/echo", func(req)
    return web.text(200, "got {req.body} {req.header("content-type") ?? "-"}")
end)
let up = try app.start()
let base = "http://127.0.0.1:{up.port}"
let a = try web.get(base + "/hi")
print("{a.status} {a.body} {a.header("Content-Type") ?? "-"} {a.header("content-length") ?? "-"}")
let b = try web.post(base + "/echo", "data")
print("{b.status} {b.body}")
let c = try web.get(base + "/missing")
print("{c.status} {c.body}")
let h = try web.request(web.Head, base + "/hi", {}, "")
print("{h.status} body='{h.body}'")

// framing the library's own server never produces
let l = try await net.listen(0)
let served = raw_server(l, [
    "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n",
    "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nuntil the end",
    "HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 201 Created\r\nContent-Length: 4\r\n\r\ndone",
    "not http at all\r\n\r\n"
])
let lb = "http://127.0.0.1:{l.port}/"
let d = try web.get(lb)
print("{d.status} {d.body}")
let e = try web.get(lb)
print("{e.status} {e.body}")
let f = try web.get(lb)
print("{f.status} {f.body}")
match web.get(lb)
    ok(x) -> print("unexpected")
    err(x) -> print("error: {x.kind.name}")
end
print(await served)
"#,
    ));
    assert_eq!(
        out,
        "200 ok text/plain; charset=utf-8 2\n200 got data text/plain; charset=utf-8\n404 {\"error\":\"not found\"}\n200 body=''\n200 hello world\n200 until the end\n201 done\nerror: std.InvalidInput\n4\n"
    );
}

#[test]
fn the_client_enforces_max_response_timeout_and_bad_urls() {
    let out = run(&program(
        RAW_SERVER,
        r#"
let big = text.repeat("x", 5000)
let l = try await net.listen(0)
let served = raw_server(l, [
    "HTTP/1.1 200 OK\r\nContent-Length: 5000\r\n\r\n" + big,
    "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n" + big,
    "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1388\r\n" + big + "\r\n0\r\n\r\n",
    "<silence>"
])
let lb = "http://127.0.0.1:{l.port}/"
var opts = web.client_options()
opts.max_response = 1000
for i in 0..3
    match web.request_with(opts, web.Get, lb, {}, "")
        ok(x) -> print("unexpected {x.status}")
        err(x) -> print("too big: {x.kind.name}")
    end
end
opts.timeout = 300
let t0 = task.now()
match web.request_with(opts, web.Get, lb, {}, "")
    ok(x) -> print("unexpected")
    err(x) -> print("{x.kind.name}")
end
print(task.now() - t0 < 5000)
print(await served)
for u in ["ftp://x/", "http://", "http://h:port/", "nonsense"]
    match web.get(u)
        ok(x) -> print("unexpected")
        err(x) -> print("{u}: {x.kind.name}")
    end
end
"#,
    ));
    assert_eq!(
        out,
        "too big: std.Overflow\ntoo big: std.Overflow\ntoo big: std.Overflow\nstd.Timeout\ntrue\n4\nftp://x/: std.InvalidInput\nhttp://: std.InvalidInput\nhttp://h:port/: std.InvalidInput\nnonsense: std.InvalidInput\n"
    );
}

#[test]
fn redirects_are_followed_up_to_five_and_only_to_http_and_https() {
    let out = run(&program(
        "",
        r#"
var app = web.server(port: 0)
app.get("/r1", func(req)
    return web.redirect("/r2")
end)
app.get("/r2", func(req)
    return web.redirect_with(301, "r3")
end)
app.get("/r3", func(req)
    return web.redirect_with(307, "/final?x=1")
end)
app.get("/final", func(req)
    return web.text(200, "final {req.query_param("x") ?? "-"}")
end)
app.get("/loop", func(req)
    return web.redirect("/loop")
end)
app.get("/ftp", func(req)
    return web.redirect("ftp://example.org/file")
end)
app.get("/five", func(req)
    return web.redirect("/b1")
end)
app.get("/b1", func(req)
    return web.redirect("/b2")
end)
app.get("/b2", func(req)
    return web.redirect("/b3")
end)
app.get("/b3", func(req)
    return web.redirect("/b4")
end)
app.get("/b4", func(req)
    return web.redirect("/final")
end)
app.post("/form", func(req)
    return web.redirect_with(303, "/final")
end)
let up = try app.start()
let base = "http://127.0.0.1:{up.port}"
let a = try web.get(base + "/r1")
print("{a.status} {a.body}")
match web.get(base + "/loop")
    ok(x) -> print("unexpected")
    err(x) -> print("loop: {x.message.slice(0, 18)}")
end
match web.get(base + "/ftp")
    ok(x) -> print("unexpected {x.status}")
    err(x) -> print("ftp: {x.kind.name}")
end
// five redirects are still followed (/five, /b1 .. /b4), a sixth is not
let f = try web.get(base + "/five")
print("{f.status} {f.body}")
var tight = web.client_options()
tight.max_redirects = 2
match web.request_with(tight, web.Get, base + "/five", {}, "")
    ok(x) -> print("unexpected")
    err(x) -> print("limit: {x.kind.name}")
end
// 3xx without following when the limit is zero
tight.max_redirects = 0
let s = try web.request_with(tight, web.Get, base + "/r1", {}, "")
print("{s.status} {s.header("location") ?? "-"}")
let g = try web.post(base + "/form", "x")
print("{g.status} {g.body}")
print(web.redirect_allowed("http://a/", "https://b/"))
print(web.redirect_allowed("https://a/", "https://b/"))
print(web.redirect_allowed("https://a/", "http://b/"))
print(web.redirect_allowed("http://a/", "http://b/"))
print(web.redirect_allowed("http://a/", "ftp://b/"))
"#,
    ));
    assert_eq!(
        out,
        "200 final 1\nloop: too many redirects\nftp: std.PermissionDenied\n200 final -\nlimit: std.Other\n302 /r2\n200 final -\ntrue\ntrue\nfalse\ntrue\nfalse\n"
    );
}

#[test]
fn an_https_request_works_when_online_tests_are_enabled() {
    // Reaches a public host: run only with ZILA_ONLINE_TESTS=1.
    if std::env::var("ZILA_ONLINE_TESTS").ok().as_deref() != Some("1") {
        eprintln!("ZILA_ONLINE_TESTS is not set: skipping the real HTTPS request");
        return;
    }
    let out = run(&program(
        "",
        r#"
let r = try web.get("https://example.com/")
print("{r.status} {text.contains(r.body, "Example Domain")}")
"#,
    ));
    assert_eq!(out, "200 true\n");
}

// ---------------------------------------------------------------------------
// Escaping and templates
// ---------------------------------------------------------------------------

#[test]
fn escape_covers_the_five_characters_and_safe_is_not_escaped_twice() {
    let out = run(&program(
        "",
        r#"
print(web.escape("a&b<c>d\"e'f"))
print(web.escape("plain ação"))
let page = web.raw("<b>&amp;</b>")
let t = html
    <p>{page}</p>
    <p>{"<b>&amp;</b>"}</p>
    <p>{web.escape("<i>")}</p>
end
print(t.markup)
print(web.unescape("a%20b+c%C3%A7", true) ?? "bad")
print(web.unescape("a%20b+c", false) ?? "bad")
print(web.unescape("%zz", false) ?? "bad")
"#,
    ));
    assert_eq!(
        out,
        "a&amp;b&lt;c&gt;d&quot;e&#39;f\nplain ação\n<p><b>&amp;</b></p>\n<p>&lt;b&gt;&amp;amp;&lt;/b&gt;</p>\n<p>&amp;lt;i&amp;gt;</p>\na b cç\na b+c\nbad\n"
    );
}

const USERS: &str = r#"
type User = record
    name: text
    admin: bool
end
"#;

#[test]
fn html_blocks_escape_loop_branch_and_compose() {
    let out = run(&program(
        USERS,
        r#"
let name = "<script>alert(1)</script>"
let one = html
    <p>{name}</p>
end
print(one.markup)

let users = [User { name: "Ana", admin: true }, User { name: "<Bo>", admin: false }]
let list = html
    <ul class="{"a\"b"}">
    @for u in users
        @if u.admin
            <li><b>{u.name}</b></li>
        @else
            <li>{u.name}</li>
        @end
    @end
    </ul>
end
print(list.markup)

let inner = html
    <em>{name}</em>
end
let outer = html
    <div>{inner} and {3} {1.5} {true} {'c'} {{braces}} {raw("<hr>")}</div>
    @@at-sign line
end
print(outer.markup)
let none: []User = []
let empty = html
    @for u in none
        <i>{u.name}</i>
    @end
end
print("[{empty.markup}]")
let nested = html
    @for u in users
        @if u.admin
            admin {u.name}
        @else if u.name == "<Bo>"
            bo {u.name}
        @end
    @end
end
print(nested.markup)
let page = web.html_page(200, outer)
print(page.status)
"#,
    ));
    assert_eq!(
        out,
        "<p>&lt;script&gt;alert(1)&lt;/script&gt;</p>\n\
<ul class=\"a&quot;b\">\n<li><b>Ana</b></li>\n<li>&lt;Bo&gt;</li>\n</ul>\n\
<div><em>&lt;script&gt;alert(1)&lt;/script&gt;</em> and 3 1.5 true c {braces} <hr></div>\n@at-sign line\n\
[]\nadmin Ana\nbo &lt;Bo&gt;\n200\n"
    );
}

#[test]
fn a_malformed_template_is_a_compile_error_at_the_block() {
    let cases: [(&str, &str); 6] = [
        ("<p>x</p>\n@for i in [1]\n<i>{i}</i>", "@for without @end"),
        ("@end", "@end without"),
        ("<p>{name</p>", "unclosed"),
        ("@else", "@else without @if"),
        ("@foreach x", "unknown directive"),
        ("<p>}</p>", "unmatched"),
    ];
    for (body, expect) in cases {
        let indented: String = body.lines().map(|l| format!("        {}\n", l)).collect();
        let src = prep(&format!("package main\nuse web\nfunc main()\n    let name = \"n\"\n    let p = html\n{}    end\n    print(p.markup)\nend\n", indented));
        let e = compile_error(&src);
        assert!(e.contains("main.br:5") && e.contains(expect), "{}: {}", expect, e);
    }
}

// ---------------------------------------------------------------------------
// Windows and Linux
// ---------------------------------------------------------------------------

const PARITY: &str = r#"
var app = web.server(port: 0)
app.use(func(req, next)
    let r = next(req)
    return r
end)
app.get("/hello/:name", func(req)
    return web.text(200, "hello {req.param("name") ?? "?"}")
end)
app.post("/customers", func(req)
    let c = try web.read_json(Customer, req)
    return web.created(Customer, c)
end)
app.get("/boom", func(req)
    return error(errors.NotFound, "nothing here")
end)
app.get("/page", func(req)
    let name = req.query_param("n") ?? "x"
    let p = html
        <h1>{name}</h1>
        @for i in [1, 2]
            <li>{i}</li>
        @end
    end
    return web.html_page(200, p)
end)
app.read_timeout = 300
let up = try app.start()
let base = "http://127.0.0.1:{up.port}"
let a = try web.get(base + "/hello/ana")
print("{a.status} {a.body}")
let b = try web.post_json(base + "/customers", ‹{"id":3,"name":"Cy"}›)
print("{b.status} {b.body}")
let c = try web.get(base + "/boom")
print("{c.status} {c.body}")
let d = try web.get(base + "/page?n=%3Cb%3E")
print("{d.status} {d.body}")
let e = try web.get(base + "/nope")
print("{e.status}")
// a stalled request gets 408 and the other clients keep being served
let slow = try await net.connect("127.0.0.1", up.port)
try await slow.write("GET /hello/x HTTP/1.1\r\nHost: x\r\nX: ")
print(brief(try req(up.port, "/hello/zed")))
let sr = slow.reader()
print(first_line(try await sr.read_all()))
// chunked request body
print(brief(try raw(up.port, "POST /customers HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n14\r\n" + ‹{"id":4,"name":"Di"}› + "\r\n0\r\n\r\n")))
"#;

fn parity_program() -> String {
    program(CUSTOMER, PARITY)
}

#[test]
fn windows_and_linux_print_the_same() {
    let src = parity_program();
    let win = run(&src);
    assert!(win.starts_with("200 hello ana\n201 {\"id\":3,\"name\":\"Cy\"}\n404 {\"error\":\"nothing here\"}\n200 <h1>&lt;b&gt;</h1>\n<li>1</li>\n<li>2</li>\n404\n"), "{}", win);
    assert!(win.contains("HTTP/1.1 408 Request Timeout"), "{}", win);
    let Some(linux) = run_linux(&src) else { return };
    assert_eq!(linux, win);
}

#[test]
fn concurrency_limits_and_503_hold_on_linux_too() {
    if let Some(out) = run_linux(&hundred_clients_program()) {
        assert_eq!(out, "correct answers: 1000
");
    }
    if let Some(out) = run_linux(&busy_program()) {
        assert_eq!(out, "busy: 6
HTTP/1.1 200 OK | ok
");
    }
}

// ---------------------------------------------------------------------------
// The examples
// ---------------------------------------------------------------------------

#[test]
fn the_web_example_prints_what_the_docs_say() {
    let src = std::fs::read_to_string("examples/web.br").unwrap();
    let out = run(&src);
    assert_eq!(
        out,
        "200 pong\n200\n<h1>Hello, &lt;b&gt;Ana&lt;/b&gt;</h1>\n<ul>\n<li>pen costs 3</li>\n<li><b>&lt;laptop&gt;</b> costs 900 (expensive)</li>\n</ul>\n"
    );
}

/// One request to 127.0.0.1:8080 from the test itself (loopback only): the whole reply.
fn http(port: u16, request: &str) -> String {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(request.as_bytes()).unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    String::from_utf8_lossy(&out).replace('\r', "")
}

#[test]
fn the_rest_api_example_serves_customers() {
    use std::io::{BufRead, BufReader};
    if std::net::TcpListener::bind(("127.0.0.1", 8080)).is_err() {
        eprintln!("port 8080 is busy: skipping");
        return;
    }
    let src = std::fs::read_to_string("examples/rest_api.br").unwrap();
    let exe = build(&src).unwrap_or_else(|e| panic!("compile error: {}", e));
    let mut child = Command::new(&exe).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
    let mut first = String::new();
    BufReader::new(child.stdout.take().unwrap()).read_line(&mut first).unwrap();
    assert!(first.contains("listening"), "{}", first);
    let post = "POST /customers HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: ";
    let body = "{\"id\":0,\"name\":\"Ana\"}";
    let created = http(8080, &format!("{}{}\r\n\r\n{}", post, body.len(), body));
    let list = http(8080, "GET /customers HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
    let one = http(8080, "GET /customers/1 HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
    let missing = http(8080, "GET /customers/9 HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
    let empty = "{\"id\":0,\"name\":\"\"}";
    let bad = http(8080, &format!("{}{}\r\n\r\n{}", post, empty.len(), empty));
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_file(&exe);
    assert!(created.starts_with("HTTP/1.1 201 Created") && created.ends_with("{\"id\":1,\"name\":\"Ana\"}\n") || created.contains("{\"id\":1,\"name\":\"Ana\"}"), "{}", created);
    assert!(list.starts_with("HTTP/1.1 200 OK") && list.contains("[{\"id\":1,\"name\":\"Ana\"}]"), "{}", list);
    assert!(one.starts_with("HTTP/1.1 200 OK") && one.contains("{\"id\":1,\"name\":\"Ana\"}"), "{}", one);
    assert!(missing.starts_with("HTTP/1.1 404") && missing.contains("no customer 9"), "{}", missing);
    assert!(bad.starts_with("HTTP/1.1 400") && bad.contains("a customer needs a name"), "{}", bad);
}
