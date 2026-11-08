//! async/await, futures, timers: programs are compiled to native executables
//! and run. Skipped when the Visual Studio build tools are not installed.

use zila::{compile_to_native, parse_source};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

/// Compiles and runs a program; `None` when there is no native toolchain.
fn run(src: &str) -> Option<(String, i32)> {
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = std::env::temp_dir().join(format!("zila-async-test-{}-{}.exe", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
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
    Some((String::from_utf8_lossy(&out.stdout).replace('\r', ""), out.status.code().unwrap_or(-1)))
}

#[test]
fn async_call_starts_before_await_and_yields_its_value() {
    let src = r#"package main
use task
async func double(n: int) -> int
    task.yield()
    return n * 2
end
async func greet(name: text) -> text
    return "hello {name}"
end
func main() -> error
    let a = double(21)
    print("started")
    print("double = {await a}")
    print(await greet("Ana"))
    return
end
"#;
    let Some((out, code)) = run(src) else { return };
    assert_eq!(out, "started\ndouble = 42\nhello Ana\n");
    assert_eq!(code, 0);
}

#[test]
fn errors_flow_through_await() {
    let src = r#"package main
async func parse(s: text) -> int or error
    if s == "bad"
        return error("cannot parse '{s}'")
    end
    return s.len
end
func twice(s: text) -> int or error
    let a = try await parse(s)
    return a * 2
end
async func relay(s: text) -> int or error
    let a = try await parse(s)
    return a + 1
end
func main() -> error
    print(try await parse("abcd"))
    match await parse("bad")
        ok(x) -> print("unexpected")
        err(e) -> print("failed: {e.message}")
    end
    print("fallback {await parse("bad") ?? 9}")
    let r = await relay("bad")
    return
end
"#;
    // `twice` is a plain function using await: it compiles and runs
    let Some((out, code)) = run(src) else { return };
    assert_eq!(out, "4\nfailed: cannot parse 'bad'\nfallback 9\n");
    assert_eq!(code, 0);
}

#[test]
fn await_works_in_plain_functions_and_map() {
    let src = r#"package main
use task
use list
async func work(n: int) -> int
    await sleep(10)
    return n * 10
end
func load(n: int) -> int
    await sleep(10)
    return n + 1
end
func outer() -> int
    return load(1) + load(2)
end
func pause()
    await sleep(5)
end
func with_defer()
    defer pause()
    print("body")
end
func main()
    print("load {load(4)}")
    let r = list.map([1, 2, 3], func(i) await work(i) end)
    print("map {r[0]} {r[1]} {r[2]}")
    with_defer()
    spawn ticker()
    print("outer {outer()}")
end
func ticker()
    for i in 0..3
        print("tick")
        await sleep(3)
    end
end
"#;
    let Some((out, code)) = run(src) else { return };
    assert!(out.starts_with("load 5
map 10 20 30
body
"), "{}", out);
    assert!(out.contains("tick") && out.ends_with("outer 5
"), "{}", out);
    assert_eq!(code, 0);
}

#[test]
fn await_only_in_a_plain_function_pulls_in_the_runtime() {
    let src = "package main
func pause()
    await sleep(5)
end
func main()
    pause()
    print(\"done\")
end
";
    let Some((out, code)) = run(src) else { return };
    assert_eq!(out, "done
");
    assert_eq!(code, 0);
}

#[test]
fn await_is_rejected_in_drop_and_c_callbacks() {
    let drop_src = "package main
type R = record
    x: int
end
func (ref r: R) drop()
    await sleep(1)
end
func main()
end
";
    let file = parse_source(drop_src).unwrap();
    let e = zila::codegen_x64::generate_asm(&file).unwrap_err();
    assert!(e.contains("`await` cannot be used in a `drop` method"), "{}", e);
    let cb_src = "package main
func cb(x: int) -> int
    await sleep(1)
    return x
end
func main()
    let f: extern \"system\" func(int) -> int = cb
end
";
    let file = parse_source(cb_src).unwrap();
    let e = zila::codegen_x64::generate_asm(&file).unwrap_err();
    assert!(e.contains("`await` cannot be used in a function called from C"), "{}", e);
}

#[test]
fn a_plain_function_awaiting_a_stuck_future_is_a_deadlock() {
    let src = "package main
async func never() -> int
    let c = channel(int)
    return c.receive()
end
func wait_it() -> int
    return await never()
end
func main()
    print(wait_it())
end
";
    let Some((out, code)) = run(src) else { return };
    assert!(out.contains("deadlock"), "{}", out);
    assert_ne!(code, 0);
}

#[test]
fn awaiting_twice_stops_with_a_clear_error() {
    let src = "package main\nasync func f() -> int\n    return 1\nend\nfunc main()\n    let x = f()\n    print(await x)\n    print(await x)\nend\n";
    let Some((out, code)) = run(src) else { return };
    assert!(out.contains("fatal: future already awaited"), "{}", out);
    assert_ne!(code, 0);
}

#[test]
fn futures_and_tasks_do_not_leak() {
    let src = r#"package main
use task
async func double(n: int) -> int
    return n * 2
end
async func greet(name: text) -> text
    return "hello {name}"
end
func main()
    let before = live_objects()
    for i in 0..3000
        let r = await double(i)
        let g = await greet("x")
        if r < 0 or g.len == 0
            print("never")
        end
    end
    print("awaited {live_objects() - before}")
    for i in 0..1000
        let w = greet("nobody awaits this one")
        task.yield()
    end
    task.yield()
    print("dropped {live_objects() - before}")
end
"#;
    let Some((out, _)) = run(src) else { return };
    // The scheduler allocates its tables on first use, so a few objects appear
    // once. What matters: the count does not grow with the number of futures.
    let nums: Vec<i64> = out.lines().map(|l| l.split_whitespace().last().unwrap().parse().unwrap()).collect();
    assert_eq!(nums.len(), 2, "{}", out);
    assert!(nums[0] <= 10, "startup overhead is {}", nums[0]);
    assert_eq!(nums[0], nums[1], "futures leaked between the phases: {}", out);
}

#[test]
fn sleeps_overlap_and_fire_in_deadline_order() {
    let src = r#"package main
use task
async func nap(ms: int) -> int
    await sleep(ms)
    return ms
end
async func tell(c: Channel<int>, ms: int)
    await sleep(ms)
    c.send(ms)
end
func main()
    let t0 = task.now()
    await sleep(80)
    print("slept {task.now() - t0 >= 80}")
    let s = task.now()
    let a = nap(100)
    let b = nap(100)
    let c = nap(100)
    print(await a + await b + await c)
    print("parallel {task.now() - s < 250}")
    let ch = channel(int)
    tell(ch, 30)
    tell(ch, 10)
    tell(ch, 20)
    print("{ch.receive()} {ch.receive()} {ch.receive()}")
end
"#;
    let Some((out, code)) = run(src) else { return };
    assert_eq!(out, "slept true\n300\nparallel true\n10 20 30\n");
    assert_eq!(code, 0);
}

#[test]
fn waiting_on_a_timer_is_not_deadlock_but_a_stuck_program_is() {
    let timer = "package main\nfunc main()\n    await sleep(150)\n    print(\"woke\")\nend\n";
    let Some((out, code)) = run(timer) else { return };
    assert_eq!(out, "woke\n");
    assert_eq!(code, 0);
    let stuck = "package main\nfunc main()\n    let c = channel(int)\n    c.receive()\nend\n";
    let Some((out, code)) = run(stuck) else { return };
    assert!(out.contains("deadlock"), "{}", out);
    assert_ne!(code, 0);
}

#[test]
fn a_future_is_not_an_int() {
    let src = "package main\nasync func f() -> int\n    return 1\nend\nfunc main()\n    let x: int = f()\nend\n";
    let file = parse_source(src).unwrap();
    let e = zila::codegen_x64::generate_asm(&file).unwrap_err();
    assert!(e.contains("expected int, found Future<int>"), "{}", e);
}

const COMBINATORS: &str = r#"package main
use task
async func after(ms: int, v: int) -> int
    await sleep(ms)
    return v
end
async func label(ms: int, s: text) -> text
    await sleep(ms)
    return s
end
async func quiet(ms: int)
    await sleep(ms)
end
func main() -> error
    let t0 = task.now()
    print("in time {try await timeout(500, after(10, 7))}")
    match await timeout(60, after(400, 1))
        ok(v) -> print("unexpected")
        err(e) -> print("failed: {e.message}")
    end
    print("fast {task.now() - t0 < 300}")
    try await timeout(500, quiet(10))
    let t1 = task.now()
    let xs = await all([after(90, 1), after(30, 2), after(60, 3)])
    print("all {xs[0]}{xs[1]}{xs[2]} {task.now() - t1 < 200}")
    let names = await all([label(20, "a"), label(5, "b")])
    print("texts {names[0]}{names[1]}")
    let t2 = task.now()
    let first = await race([after(300, 1), after(40, 2), after(200, 3)])
    print("race {first} {task.now() - t2 < 150}")
    print("race text {await race([label(100, "slow"), label(10, "fast")])}")
    return
end
"#;

#[test]
fn timeout_all_and_race() {
    let Some((out, code)) = run(COMBINATORS) else { return };
    assert_eq!(
        out,
        "in time 7\nfailed: timed out\nfast true\nall 123 true\ntexts ab\nrace 2 true\nrace text fast\n"
    );
    assert_eq!(code, 0);
}

#[test]
fn combinators_do_not_leak_per_iteration() {
    let src = r#"package main
use task
async func after(ms: int, v: int) -> int
    await sleep(ms)
    return v
end
async func label(ms: int, s: text) -> text
    await sleep(ms)
    return s
end
async func round(n: int)
    for i in 0..n
        let a = await all([after(0, i), after(0, i + 1)])
        let r = await race([after(0, 1), after(1, 2)])
        let m = await timeout(100, label(0, "t"))
        if a[0] < 0 or r < 0
            print("never")
        end
    end
end
func main()
    await round(400)
    await sleep(300)
    let before = live_objects()
    await round(800)
    await sleep(300)
    print("{live_objects() - before}")
end
"#;
    // warm-up round first, so tables have reached their peak size
    let Some((out, _)) = run(src) else { return };
    let growth: i64 = out.trim().parse().unwrap_or_else(|_| panic!("{}", out));
    // peak concurrency varies with timing; a leak of even one object every two rounds would be 400+
    assert!(growth <= 150, "grew by {} over 800 more rounds", growth);
}

#[test]
fn timeout_needs_a_future_and_all_a_list_of_futures() {
    let file = parse_source("package main\nfunc main()\n    let x = timeout(5, 3)\nend\n").unwrap();
    let e = zila::codegen_x64::generate_asm(&file).unwrap_err();
    assert!(e.contains("timeout() needs a Future"), "{}", e);
    let file = parse_source("package main\nfunc main()\n    let x = await all([1, 2])\nend\n").unwrap();
    let e = zila::codegen_x64::generate_asm(&file).unwrap_err();
    assert!(e.contains("all() needs a list of futures"), "{}", e);
}

// The `drop` hook closes files and sockets (std/fs.br, std/net.br).
#[test]
fn drop_hook_runs_when_the_last_reference_goes_away() {
    let src = r#"
package main

type Handle = record
    id: int
    name: text
end

func (ref h: Handle) drop()
    print("closing {h.name} ({h.id})")
end

type Pair = record
    first: Handle
    second: Handle
end

func early(flag: bool) -> int
    let h = Handle{ id: 1, name: "early" }
    if flag
        return 7
    end
    return 8
end

func risky(fail: bool) -> int or error
    let h = Handle{ id: 2, name: "risky" }
    if fail
        return error("boom")
    end
    return 1
end

func chain() -> int or error
    let a = Handle{ id: 3, name: "chain" }
    let b = try risky(true)
    return b
end

func main() -> error
    print("-- scope end")
    let h = Handle{ id: 10, name: "main-local" }
    print("-- early return")
    print(early(true))
    print("-- try failure")
    match chain()
        ok(v) -> print("ok")
        err(e) -> print("failed: {e.message}")
    end
    print("-- reassignment")
    var x = Handle{ id: 11, name: "first" }
    x = Handle{ id: 12, name: "second" }
    print("-- list")
    var hs: []Handle = []
    hs.add(Handle{ id: 13, name: "in-list" })
    hs.clear()
    print("-- nested")
    let p = Pair{ first: Handle{ id: 14, name: "left" }, second: Handle{ id: 15, name: "right" } }
    print("-- end of main")
    return
end
"#;
    let Some((out, code)) = run(src) else { return };
    assert_eq!(code, 0);
    let expected = "-- scope end\n-- early return\nclosing early (1)\n7\n-- try failure\nclosing risky (2)\nclosing chain (3)\nfailed: boom\n-- reassignment\nclosing first (11)\n-- list\nclosing in-list (13)\n-- nested\n-- end of main\nclosing left (14)\nclosing right (15)\nclosing second (12)\nclosing main-local (10)\n";
    assert_eq!(out, expected);
}

#[test]
fn drop_must_take_nothing_and_return_nothing() {
    let file = parse_source("package main\ntype H = record\n    x: int\nend\nfunc (ref h: H) drop() -> int\n    return 1\nend\nfunc main()\nend\n").unwrap();
    let e = zila::codegen_x64::generate_asm(&file).unwrap_err();
    assert!(e.contains("`drop` method takes no parameters and returns nothing"), "{}", e);
}

fn temp_path(name: &str) -> String {
    let p = std::env::temp_dir().join(format!("zila-fs-test-{}-{}", std::process::id(), name));
    p.to_str().unwrap().replace('\\', "/")
}

#[test]
fn files_round_trip_missing_files_and_large_files() {
    let path = temp_path("a.txt");
    let src = r#"package main
use fs
use task
var ticks = 0
async func ticker()
    while true
        await sleep(2)
        ticks += 1
    end
end
func main() -> error
    try await fs.write("PATH", "hello, files: acentuação ok")
    print(try await fs.read("PATH"))
    match await fs.read("PATH.missing")
        ok(t) -> print("unexpected")
        err(e) -> print("error: {e.message}")
    end
    var big = "0123456789abcdef"
    for i in 0..18
        big = big + big
    end
    let t = ticker()
    try await fs.write("PATH", big)
    let again = try await fs.read("PATH")
    print("identical {again == big} {again.len}")
    print("ticker ran {ticks > 0}")
    return
end
"#
    .replace("PATH", &path);
    let result = run(&src);
    let _ = std::fs::remove_file(&path);
    let Some((out, code)) = result else { return };
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "hello, files: acentuação ok", "{}", out);
    assert!(lines[1].starts_with("error: ") && lines[1].contains("a.txt.missing"), "{}", out);
    assert!(lines[1].contains("cannot find the file"), "{}", out);
    assert_eq!(lines[2], "identical true 4194304");
    assert_eq!(lines[3], "ticker ran true");
    assert_eq!(code, 0);
}

#[test]
fn files_do_not_leak_objects_or_handles() {
    let path = temp_path("loop.txt");
    let src = r#"package main
use fs
async func round(n: int) -> error
    for i in 0..n
        try await fs.write("PATH", "round {i}")
        let r = try await fs.read("PATH")
        if r.len == 0
            print("never")
        end
    end
    return
end
func main() -> error
    try await round(100)
    let before = live_objects()
    try await round(1000)
    print("growth {live_objects() - before}")
    return
end
"#
    .replace("PATH", &path);
    let result = run(&src);
    let _ = std::fs::remove_file(&path);
    let Some((out, code)) = result else { return };
    assert_eq!(code, 0, "{}", out);
    let growth: i64 = out.trim().trim_start_matches("growth ").parse().unwrap_or_else(|_| panic!("{}", out));
    assert!(growth <= 20, "grew by {}", growth);
}

#[test]
fn tcp_echo_refused_and_peer_closed() {
    let src = r#"
package main

use net
use task

async func echo_once(l: net.Listener) -> error
    let conn = try await l.accept()
    let msg = try await conn.read(64)
    try await conn.write("echo: {msg}")
    await conn.close()
    return
end

func main() -> error
    let listener = try await net.listen(0)
    print("listening on a free port: {listener.port > 0}")
    let server = echo_once(listener)
    let client = try await net.connect("127.0.0.1", listener.port)
    try await client.write("ping")
    let reply = try await client.read(64)
    print("client got: {reply}")
    try await server
    let closed = try await client.read(64)
    print("after the server closed, read returned {closed.len} bytes")

    // nobody listens on this port any more
    let dead = listener.port
    await listener.close()
    match await net.connect("127.0.0.1", dead)
        ok(c) -> print("unexpected connect")
        err(e) -> print("refused: {e.message}")
    end
    return
end
"#;
    let Some((out, code)) = run(src) else { return };
    assert_eq!(code, 0, "{}", out);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "listening on a free port: true");
    assert_eq!(lines[1], "client got: echo: ping");
    assert_eq!(lines[2], "after the server closed, read returned 0 bytes");
    assert!(lines[3].starts_with("refused: connect to 127.0.0.1:") && lines[3].contains("refused"), "{}", out);
}

#[test]
fn writing_to_a_closed_peer_eventually_fails() {
    let src = r#"
package main

use net

async func accept_and_close(l: net.Listener) -> error
    let conn = try await l.accept()
    await conn.close()
    return
end

func main() -> error
    let l = try await net.listen(0)
    let server = accept_and_close(l)
    let c = try await net.connect("127.0.0.1", l.port)
    try await server
    let r = try await c.read(16)
    print("read after peer closed: {r.len} bytes")
    var failed_at = -1
    for i in 0..20
        match await c.write("data {i}")
            ok -> print("")
            err(e) ->
                if failed_at < 0
                    failed_at = i
                    print("write {i} failed: {e.message}")
                end
        end
        if failed_at >= 0
            break
        end
    end
    print("a write failed eventually: {failed_at >= 0}")
    return
end
"#;
    let Some((out, code)) = run(src) else { return };
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains("read after peer closed: 0 bytes"), "{}", out);
    assert!(out.contains("a write failed eventually: true"), "{}", out);
}

#[test]
fn many_connections_at_once_and_in_sequence_without_leaks() {
    let src = r#"
package main

use net
use task

async func handle(conn: net.Conn) -> error
    let msg = try await conn.read(64)
    try await conn.write("re: {msg}")
    await conn.close()
    return
end

async func serve(l: net.Listener, n: int) -> error
    for i in 0..n
        let conn = try await l.accept()
        handle(conn)
    end
    return
end

async func ask(port: long, id: int) -> text or error
    let c = try await net.connect("127.0.0.1", port)
    try await c.write("hello {id}")
    let r = try await c.read(64)
    await c.close()
    return r
end

async func sequential(port: long, n: int) -> error
    for i in 0..n
        let r = try await ask(port, i)
        if r.len == 0
            return error("empty reply")
        end
    end
    return
end

func main() -> error
    let l = try await net.listen(0)
    let t0 = task.now()
    let server = serve(l, 100)
    var clients = [ask(l.port, 0)]
    for i in 1..100
        clients.add(ask(l.port, i))
    end
    let replies = await all(clients)
    var good = 0
    for i in 0..replies.len
        match replies[i]
            ok(r) -> if r == "re: hello {i}" good += 1 end
            err(e) -> print("client {i} failed: {e.message}")
        end
    end
    try await server
    print("100 simultaneous clients, correct replies: {good}")

    let s2 = serve(l, 1100)
    try await sequential(l.port, 100)
    let before = live_objects()
    try await sequential(l.port, 1000)
    print("object growth after 1000 connect/close cycles: {live_objects() - before}")
    try await s2
    return
end
"#;
    let Some((out, code)) = run(src) else { return };
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains("correct replies: 100"), "{}", out);
    let growth: i64 = out.lines().last().unwrap().rsplit(' ').next().unwrap().parse().unwrap_or_else(|_| panic!("{}", out));
    assert!(growth <= 20, "grew by {}", growth);
}

#[test]
fn numeric_addresses_parse_and_bad_ones_do_not() {
    // `net.connect` takes the numeric path for dotted quads: no name lookup is involved
    let src = r#"package main
use net
func main()
    print(net.parse_ip("127.0.0.1"))
    print(net.parse_ip("10.0.0.255"))
    print(net.parse_ip("256.1.1.1"))
    print(net.parse_ip("1.2.3"))
    print(net.parse_ip("a.b.c.d"))
    print(net.parse_ip("1..2.3"))
end
"#;
    let Some((out, _)) = run(src) else { return };
    assert_eq!(out, "2130706433\n167772415\n-1\n-1\n-1\n-1\n");
}

#[test]
fn ten_thousand_async_calls_do_not_leak() {
    let src = r#"package main
async func twice(n: int) -> int
    return n * 2
end
func main()
    let warm = await twice(1)
    let before = live_objects()
    var sum = 0
    for i in 0..10000
        sum += await twice(i)
    end
    print("{sum} {live_objects() - before}")
end
"#;
    let Some((out, code)) = run(src) else { return };
    assert_eq!(code, 0);
    let growth: i64 = out.trim().rsplit(' ').next().unwrap().parse().unwrap();
    assert!(out.starts_with("99990000 ") && growth <= 4, "{}", out);
}

#[test]
fn sleeps_files_tcp_channels_and_timeouts_work_together() {
    let path = temp_path("e2e.txt");
    let src = r#"package main
use fs
use net
use task

async func serve(l: net.Listener) -> error
    let conn = try await l.accept()
    let msg = try await conn.read(64)
    try await conn.write("echo: {msg}")
    await conn.close()
    return
end

async func fetch(port: long) -> text or error
    let c = try await net.connect("127.0.0.1", port)
    try await c.write("ping")
    return try await c.read(64)
end

async func slow() -> int
    await sleep(400)
    return 1
end

async func announce(ch: Channel<text>, ms: int, what: text)
    await sleep(ms)
    ch.send(what)
end

func main() -> error
    try await fs.write("PATH", "stored text")
    print("file: {try await fs.read("PATH")}")

    let l = try await net.listen(0)
    let server = serve(l)
    let reply = try await timeout(2000, fetch(l.port))
    print("tcp: {reply}")
    try await server

    match await timeout(50, slow())
        ok(v) -> print("unexpected")
        err(e) -> print("timeout: {e.message}")
    end

    let ch = channel(text)
    announce(ch, 30, "second")
    announce(ch, 10, "first")
    print("channel: {ch.receive()} then {ch.receive()}")
    return
end
"#
    .replace("PATH", &path);
    let result = run(&src);
    let _ = std::fs::remove_file(&path);
    let Some((out, code)) = result else { return };
    assert_eq!(out, "file: stored text\ntcp: echo: ping\ntimeout: timed out\nchannel: first then second\n");
    assert_eq!(code, 0);
}

#[test]
fn the_async_example_builds_and_runs() {
    let src = std::fs::read_to_string("examples/async.br").unwrap();
    let path = temp_path("example.txt");
    let src = src.replace("zila-example.txt", &path);
    let result = run(&src);
    let _ = std::fs::remove_file(&path);
    let Some((out, code)) = result else { return };
    assert_eq!(out, "title 1, title 2, title 3\ntitles 7 7 7\nwritten by an async function\necho: hello\n");
    assert_eq!(code, 0);
}

#[test]
fn reading_through_a_pointer_field_does_not_release_the_buffer() {
    // `b.ptr[i]` evaluates `b.ptr` (a raw pointer, not a counted object). It used to be
    // released as if it were one, freeing the buffer behind `ptr` after a few reads.
    let src = r#"package main
use c

#[shared]
type Buf = record
    ptr: *u8
end

func (ref b: Buf) drop()
    c.free(b.ptr)
end

func main()
    let before = live_objects()
    let b = Buf{ ptr: c.alloc(16) }
    var total = 0
    for i in 0..5000
        unsafe
            b.ptr[3] = 7
            total += int(b.ptr[3])
        end
    end
    let alive = live_objects() - before
    print("{total} {alive}")
end
"#;
    let Some((out, code)) = run(src) else { return };
    assert_eq!(code, 0, "{}", out);
    // 35000 from the reads; the record and its buffer are both still alive
    assert_eq!(out, "35000 2\n");
}
