//! Threads: `spawn_thread`, `shared`, transferable types, `sync.Mutex`/`sync.Atomic`, per-thread
//! schedulers and thread-safe channels. Programs are compiled to native Windows executables and
//! run with a time limit, so a hang fails the test instead of stalling the run. Linux is not
//! covered here: its thread layer is not written yet (see the change's tasks).

use brasa::{compile_to_native, parse_source};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

static N: AtomicUsize = AtomicUsize::new(0);

struct Ran {
    out: String,
    err: String,
    code: i32,
    secs: f64,
}

fn build(src: &str) -> Result<std::path::PathBuf, String> {
    let file = parse_source(src).map_err(|d| d.to_string())?;
    let exe = std::env::temp_dir().join(format!("brasa-threads-test-{}-{}.exe", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    compile_to_native(&file, exe.to_str().unwrap())?;
    Ok(exe)
}

/// Runs a program for at most `limit` seconds (it is killed after that and the test fails).
fn run_limit(src: &str, limit: u64) -> Ran {
    let exe = build(src).unwrap_or_else(|e| panic!("compile error: {}\nprogram:\n{}", e, src));
    let start = Instant::now();
    let mut child = Command::new(&exe).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let deadline = start + Duration::from_secs(limit);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(&exe);
            panic!("the program ran for more than {} seconds (hung?):\n{}", limit, src);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let secs = start.elapsed().as_secs_f64();
    let o = child.wait_with_output().unwrap();
    let _ = std::fs::remove_file(&exe);
    Ran {
        out: String::from_utf8_lossy(&o.stdout).replace('\r', ""),
        err: String::from_utf8_lossy(&o.stderr).replace('\r', ""),
        code: o.status.code().unwrap_or(-1),
        secs,
    }
}

fn run(src: &str) -> Ran {
    run_limit(src, 30)
}

fn expect(src: &str, expected: &str) {
    let r = run(src);
    assert_eq!(r.out, expected, "program:\n{}\nstderr: {}", src, r.err);
    assert_eq!(r.code, 0, "program:\n{}\nstderr: {}", src, r.err);
}

/// The error a program is rejected with (at parse or compile time).
fn rejected(src: &str) -> String {
    match build(src) {
        Ok(exe) => {
            let _ = std::fs::remove_file(&exe);
            panic!("expected a compile error for:\n{}", src)
        }
        Err(e) => e,
    }
}

fn main_with(body: &str) -> String {
    format!("package main\n\nuse thread\nuse sync\n\nfunc main() -> error\n{}\n    return\nend\n", body)
}

// ---------------------------------------------------------------- spawn_thread

#[test]
fn run_and_join() {
    expect(
        r#"package main

use thread
use sync

func add(total: shared sync.Atomic, n: int)
    total.add(n)
end

func main() -> error
    let total = shared sync.Atomic{ value: 0 }
    var ts: []thread.Thread = []
    for i in 0..4
        ts.add(spawn_thread add(total, i))
    end
    for t in ts
        t.join()
    end
    print("{total.load()}")
    return
end
"#,
        "6\n",
    );
}

#[test]
fn threads_really_run_in_parallel() {
    if std::thread::available_parallelism().map_or(1, |n| n.get()) < 2 {
        eprintln!("one core: skipping");
        return;
    }
    let src = r#"package main

use thread
use task

func spin()
    let deadline = task.now() + 500
    var n: long = 0
    while task.now() < deadline
        n += 1
    end
end

func main() -> error
    let t0 = task.now()
    let a = spawn_thread spin()
    let b = spawn_thread spin()
    a.join()
    b.join()
    print("{task.now() - t0}")
    return
end
"#;
    let r = run(src);
    assert_eq!(r.code, 0);
    let ms: i64 = r.out.trim().parse().unwrap();
    assert!(ms >= 500 && ms < 950, "two 500 ms threads took {} ms", ms);
}

#[test]
fn abort_in_a_thread_stops_the_program() {
    let r = run(r#"package main

use thread

func boom()
    abort("boom")
end

func main() -> error
    let t = spawn_thread boom()
    t.join()
    print("not reached")
    return
end
"#);
    assert_eq!(r.code, 101, "stderr: {}", r.err);
    assert!(r.err.contains("boom"), "stderr: {}", r.err);
    assert_eq!(r.out, "");
}

#[test]
fn main_returning_ends_the_process_with_threads_running() {
    let r = run_limit(
        r#"package main

use thread

func forever()
    while true
        thread.sleep(10)
    end
end

func main() -> error
    let t = spawn_thread forever()
    print("done")
    return
end
"#,
        10,
    );
    assert_eq!((r.out.as_str(), r.code), ("done\n", 0));
}

#[test]
fn spawn_thread_needs_a_call() {
    let e = rejected(&main_with("    let t = spawn_thread 5"));
    assert!(e.contains("requires a function call"), "{}", e);
}

#[test]
fn spawn_thread_as_statement_and_expression_parse() {
    parse_source(&main_with("    spawn_thread f(1)\n    let t = spawn_thread f(2)\n    t.join()")).unwrap();
}

// ---------------------------------------------------------------- transferable types

const NODE_TYPES: &str = "package main

use thread

type Node = record
    value: int
end

type Owner = record
    next: Rc<Node>
end

type Job = record
    owner: Owner
    id: int
end
";

#[test]
fn plain_data_is_transferable() {
    expect(
        r#"package main

use thread

type Item = record
    id: int
    name: text
end

type Job = record
    items: []Item
    scores: Map<text, int>
end

func run(job: Job)
    print("{job.items.len} {job.items[1].name} {job.scores["a"]}")
end

func main() -> error
    let job = Job{ items: [Item{ id: 1, name: "x" }, Item{ id: 2, name: "y" }], scores: { "a": 7 } }
    let t = spawn_thread run(job)
    t.join()
    return
end
"#,
        "2 y 7\n",
    );
}

#[test]
fn non_transferable_field_names_the_path() {
    let src = format!(
        r#"{}
func run(job: Job)
    print("{{job.id}}")
end

func main() -> error
    let job = Job{{ owner: Owner{{ next: new Node{{ value: 1 }} }}, id: 3 }}
    let t = spawn_thread run(job)
    t.join()
    return
end
"#,
        NODE_TYPES
    );
    let e = rejected(&src);
    assert!(e.contains("argument 'job'"), "{}", e);
    assert!(e.contains("job.owner.next"), "{}", e);
    assert!(e.contains("new Node"), "{}", e);
    assert!(e.contains("not transferable"), "{}", e);
    assert!(e.contains("shared") && e.contains("Mutex") && e.contains("Atomic"), "{}", e);
}

#[test]
fn a_new_object_is_not_transferable() {
    let src = format!(
        r#"{}
func show(n: Rc<Node>)
    print("{{n.value}}")
end

func main() -> error
    let n = new Node{{ value: 1 }}
    let t = spawn_thread show(n)
    t.join()
    return
end
"#,
        NODE_TYPES
    );
    let e = rejected(&src);
    assert!(e.contains("argument 'n'") && e.contains("new Node"), "{}", e);
}

#[test]
fn captured_variable_is_checked() {
    let src = format!(
        r#"{}
func run(f: func())
    f()
end

func main() -> error
    let n = new Node{{ value: 1 }}
    let t = spawn_thread run(func() print("{{n.value}}") end)
    t.join()
    return
end
"#,
        NODE_TYPES
    );
    let e = rejected(&src);
    assert!(e.contains("captured variable 'n'"), "{}", e);
}

#[test]
fn mutable_global_is_rejected() {
    let e = rejected(
        r#"package main

use thread

var counter = 0

func work()
    counter += 1
end

func main() -> error
    let t = spawn_thread work()
    t.join()
    return
end
"#,
    );
    assert!(e.contains("counter") && e.contains("global"), "{}", e);
    assert!(e.contains("shared Atomic") && e.contains("Mutex"), "{}", e);
}

#[test]
fn channel_of_new_objects_is_rejected() {
    let src = format!(
        r#"{}
type NodeRef = Rc<Node>

func feed(ch: Channel<NodeRef>)
    ch.send(new Node{{ value: 1 }})
end

func main() -> error
    let ch = channel(NodeRef)
    let t = spawn_thread feed(ch)
    t.join()
    return
end
"#,
        NODE_TYPES
    );
    let e = rejected(&src);
    assert!(e.contains("argument 'ch'") && e.contains("new Node"), "{}", e);
}

#[test]
fn shared_value_must_be_transferable() {
    let src = format!(
        r#"{}
func main() -> error
    let n = new Node{{ value: 1 }}
    let o = Owner{{ next: n }}
    let s = shared o
    return
end
"#,
        NODE_TYPES
    );
    let e = rejected(&src);
    assert!(e.contains("not transferable") && e.contains("new Node"), "{}", e);
}

// ---------------------------------------------------------------- shared

#[test]
fn assigning_through_shared_is_an_error() {
    let e = rejected(
        r#"package main

type Counter = record
    count: int
end

func main() -> error
    let s = shared Counter{ count: 0 }
    s.count = s.count + 1
    return
end
"#,
    );
    assert!(e.contains("immutable") && e.contains("Mutex<Counter>"), "{}", e);
}

#[test]
fn mutating_a_shared_list_is_an_error() {
    let e = rejected(
        r#"package main

func main() -> error
    let xs = shared [1, 2, 3]
    xs.add(4)
    return
end
"#,
    );
    assert!(e.contains("immutable") && e.contains("Mutex"), "{}", e);
}

#[test]
fn reading_shared_from_many_threads() {
    expect(
        r#"package main

use thread
use sync

type Config = record
    name: text
    items: []int
end

func read(c: shared Config, hits: shared sync.Atomic)
    if c.name == "cfg" and c.items.len == 3
        hits.add(1)
    end
end

func main() -> error
    let c = shared Config{ name: "cfg", items: [1, 2, 3] }
    let hits = shared sync.Atomic{ value: 0 }
    var ts: []thread.Thread = []
    for i in 0..4
        ts.add(spawn_thread read(c, hits))
    end
    for t in ts
        t.join()
    end
    print("{hits.load()}")
    return
end
"#,
        "4\n",
    );
}

#[test]
fn shared_list_is_freed_after_the_last_thread() {
    expect(
        r#"package main

use thread

func drop_it(xs: shared []int)
    let n = xs.len
end

func round()
    let xs = shared [1, 2, 3, 4]
    var ts: []thread.Thread = []
    for i in 0..4
        ts.add(spawn_thread drop_it(xs))
    end
    for t in ts
        t.join()
    end
end

func main() -> error
    round()
    let before = live_objects()
    round()
    print("{live_objects() - before}")
    return
end
"#,
        "0\n",
    );
}

#[test]
fn single_thread_program_has_no_lock_prefixes() {
    let plain = parse_source("package main\n\nfunc main() -> error\n    let xs = [1, 2, 3]\n    print(\"{xs.len}\")\n    return\nend\n").unwrap();
    let asm = brasa::codegen_x64::generate_asm(&plain).unwrap();
    assert!(!asm.contains("lock "), "a program without threads must not use lock prefixes");
    let threaded = parse_source("package main\n\nfunc main() -> error\n    let xs = shared [1, 2, 3]\n    print(\"{xs.len}\")\n    return\nend\n").unwrap();
    let asm = brasa::codegen_x64::generate_asm(&threaded).unwrap();
    assert!(asm.contains("lock inc"), "a program with `shared` counts atomically");
}

// ---------------------------------------------------------------- Mutex and Atomic

#[test]
fn mutex_counter_under_contention() {
    expect(
        r#"package main

use thread
use sync

func count(m: shared sync.Mutex<int>)
    for i in 0..10000
        m.update(func(n: int) n + 1 end)
    end
end

func main() -> error
    let m = shared sync.Mutex(0)
    var ts: []thread.Thread = []
    for i in 0..8
        ts.add(spawn_thread count(m))
    end
    for t in ts
        t.join()
    end
    print("{m.get()}")
    return
end
"#,
        "80000\n",
    );
}

#[test]
fn atomic_counter_without_a_lock() {
    expect(
        r#"package main

use thread
use sync

func count(a: shared sync.Atomic)
    for i in 0..100000
        a.add(1)
    end
end

func main() -> error
    let a = shared sync.Atomic{ value: 0 }
    var ts: []thread.Thread = []
    for i in 0..4
        ts.add(spawn_thread count(a))
    end
    for t in ts
        t.join()
    end
    print("{a.load()}")
    print("{a.swap(5)} {a.compare_swap(5, 9)} {a.load()}")
    return
end
"#,
        "400000\n400000 5 9\n",
    );
}

#[test]
fn locking_twice_is_an_error() {
    let r = run(&main_with("    let m = sync.Mutex(0)\n    m.lock()\n    m.lock()\n    print(\"not reached\")"));
    assert_ne!(r.code, 0);
    assert!(r.err.contains("already locked by this thread"), "stderr: {}", r.err);
}

#[test]
fn unlock_with_defer_after_early_return() {
    expect(
        r#"package main

use thread
use sync

func take(m: shared sync.Mutex<int>, early: bool) -> int
    m.lock()
    defer m.unlock()
    if early
        return 1
    end
    m.locked_set(m.locked_get() + 10)
    return 2
end

func other(m: shared sync.Mutex<int>, done: shared sync.Atomic)
    m.lock()
    m.locked_set(m.locked_get() + 100)
    m.unlock()
    done.add(1)
end

func main() -> error
    let m = shared sync.Mutex(0)
    let done = shared sync.Atomic{ value: 0 }
    take(m, true)
    let t = spawn_thread other(m, done)
    t.join()
    take(m, false)
    print("{m.get()} {done.load()}")
    return
end
"#,
        "110 1\n",
    );
}

// ---------------------------------------------------------------- copy on write

#[test]
fn both_threads_append_to_a_list_received_by_value() {
    expect(
        r#"package main

use thread
use sync

func joined(a: []int, b: []int) -> []int
    var r: []int = []
    for x in a
        r.add(x)
    end
    for x in b
        r.add(x)
    end
    return r
end

func append_one(xs: []int, v: int, out: shared sync.Mutex<[]int>)
    var mine = xs
    mine.add(v)
    out.update(func(all: []int) joined(all, mine) end)
end

func main() -> error
    var base: []int = [1, 2]
    let out = shared sync.Mutex(base)
    let a = spawn_thread append_one(base, 10, out)
    let b = spawn_thread append_one(base, 20, out)
    a.join()
    b.join()
    let parts = out.get()
    print("{base.len} {parts.len}")
    return
end
"#,
        "2 8\n",
    );
}

// ---------------------------------------------------------------- channels and schedulers

#[test]
fn producers_and_one_consumer() {
    expect(
        r#"package main

use thread

func producer(ch: Channel<int>, base: int)
    for i in 0..1000
        ch.send(base + i)
    end
end

func main() -> error
    let ch = channel(int)
    var ts: []thread.Thread = []
    for k in 0..4
        ts.add(spawn_thread producer(ch, k * 1000))
    end
    var seen: Map<int, int> = {}
    var sum = 0
    for i in 0..4000
        let v = ch.receive()
        seen[v] = 1
        sum += v
    end
    for t in ts
        t.join()
    end
    print("{seen.len} {sum}")
    return
end
"#,
        "4000 7998000\n",
    );
}

#[test]
fn a_send_wakes_a_sleeping_thread() {
    let r = run(
        r#"package main

use thread

func late(ch: Channel<int>)
    thread.sleep(150)
    ch.send(42)
end

func main() -> error
    let ch = channel(int)
    let t = spawn_thread late(ch)
    print("{ch.receive()}")
    t.join()
    return
end
"#,
    );
    assert_eq!((r.out.as_str(), r.code), ("42\n", 0));
    assert!(r.secs >= 0.14, "returned after {} s", r.secs);
}

#[test]
fn closing_wakes_every_receiver() {
    expect(
        r#"package main

use thread
use sync

func wait_for(ch: Channel<int>, woke: shared sync.Atomic)
    let v = ch.receive()
    woke.add(1)
end

func main() -> error
    let ch = channel(int)
    let woke = shared sync.Atomic{ value: 0 }
    var ts: []thread.Thread = []
    for i in 0..3
        ts.add(spawn_thread wait_for(ch, woke))
    end
    thread.sleep(100)
    ch.close()
    for t in ts
        t.join()
    end
    print("{woke.load()}")
    return
end
"#,
        "3\n",
    );
}

#[test]
fn tasks_stay_on_their_thread() {
    expect(
        r#"package main

use thread
use sync

func note(me: long, bad: shared sync.Atomic)
    if thread.id() != me
        bad.add(1)
    end
end

func worker(bad: shared sync.Atomic)
    let me = thread.id()
    for i in 0..5
        spawn note(me, bad)
    end
    await sleep(30)
end

func main() -> error
    let bad = shared sync.Atomic{ value: 0 }
    let a = spawn_thread worker(bad)
    let b = spawn_thread worker(bad)
    a.join()
    b.join()
    print("{bad.load()}")
    return
end
"#,
        "0\n",
    );
}

#[test]
fn async_on_a_thread() {
    expect(
        r#"package main

use thread
use sync

async func nap(ms: int) -> int
    await sleep(ms)
    return ms
end

func run(out: shared sync.Atomic)
    let v = await nap(40)
    out.add(v)
end

func main() -> error
    let out = shared sync.Atomic{ value: 0 }
    let t = spawn_thread run(out)
    t.join()
    print("{out.load()}")
    return
end
"#,
        "40\n",
    );
}

#[test]
fn deadlock_in_a_thread_is_reported() {
    let r = run(
        r#"package main

use thread

func stuck()
    let ch = channel(int)
    let v = ch.receive()
end

func main() -> error
    let t = spawn_thread stuck()
    t.join()
    print("not reached")
    return
end
"#,
    );
    assert_ne!(r.code, 0);
    assert!(r.err.contains("deadlock") || r.out.contains("deadlock"), "out: {} err: {}", r.out, r.err);
    assert!(!r.out.contains("not reached"));
}

#[test]
fn join_waits_without_burning_cpu() {
    let r = run(
        r#"package main

use thread

func nap()
    thread.sleep(200)
end

func main() -> error
    let t = spawn_thread nap()
    t.join()
    return
end
"#,
    );
    assert_eq!(r.code, 0);
    assert!(r.secs >= 0.19, "join returned after {} s", r.secs);
}

// ---------------------------------------------------------------- the allocator and live_objects

#[test]
fn allocator_stress_does_not_leak() {
    expect(
        r#"package main

use thread

func churn()
    for i in 0..10000
        let s = "item {i}"
        var xs: []text = []
        xs.add(s)
        xs.add(s + "!")
    end
end

func main() -> error
    var ts: []thread.Thread = []
    ts.add(spawn_thread churn())
    ts[0].join()
    ts.clear()
    let before = live_objects()
    for i in 0..8
        ts.add(spawn_thread churn())
    end
    for t in ts
        t.join()
    end
    ts.clear()
    print("{live_objects() - before}")
    return
end
"#,
        "0\n",
    );
}

#[test]
fn many_short_threads() {
    expect(
        r#"package main

use thread
use sync

func bump(a: shared sync.Atomic)
    a.add(1)
end

func main() -> error
    let a = shared sync.Atomic{ value: 0 }
    for round in 0..4
        var ts: []thread.Thread = []
        for i in 0..50
            ts.add(spawn_thread bump(a))
        end
        for t in ts
            t.join()
        end
    end
    print("{a.load()}")
    return
end
"#,
        "200\n",
    );
}

// ---------------------------------------------------------------- examples

fn example(name: &str, expected: &str) {
    let path = format!("{}/examples/{}", env!("CARGO_MANIFEST_DIR"), name);
    let src = std::fs::read_to_string(&path).unwrap();
    let r = run(&src);
    assert_eq!(r.out, expected, "{}: {}", name, r.err);
    assert_eq!(r.code, 0);
}

#[test]
fn example_parallel_sum() {
    example("threads_sum.br", "sum = 50005000\n");
}

#[test]
fn example_mutex_counter() {
    example("threads_mutex.br", "mutex  = 80000\natomic = 80000\n");
}

// ---------------------------------------------------------------- other targets

#[test]
fn kernel_rejects_spawn_thread() {
    let boot = parse_source("package boot\n\nfunc main()\nend\n").unwrap();
    let kernel = parse_source("package kernel\n\nfunc work()\nend\n\nfunc main()\n    let t = spawn_thread work()\nend\n").unwrap();
    let e = brasa::codegen_x64::build_os(&boot, &kernel).err().expect("the kernel must reject spawn_thread");
    assert!(e.contains("spawn_thread") && e.contains("bare-metal"), "{}", e);
}

#[test]
fn linux_target_says_threads_are_not_available_yet() {
    let file = parse_source(&main_with("    let t = spawn_thread f(1)")).unwrap();
    let e = brasa::codegen_x64::generate_asm_for(&file, &brasa::codegen_x64::CompileOptions::default(), brasa::codegen_x64::Target::Linux)
        .err()
        .expect("expected an error");
    assert!(e.contains("not available on Linux"), "{}", e);
}
