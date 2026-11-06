//! `defer`: a call that runs when the enclosing function ends. Programs are compiled to native
//! executables and run (skipped when the Visual Studio build tools are missing), and the
//! interpreter is exercised through the command-line tool.

use brasa::{compile_to_native, parse_source};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

fn run(src: &str) -> Option<(String, i32)> {
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = std::env::temp_dir().join(format!("brasa-defer-test-{}-{}.exe", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
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

const TOUR: &str = r#"package main

type Counter = record
    n: int
end

func (ref c: Counter) report()
    print("counter is {c.n}")
end

func order()
    defer print("1")
    defer print("2")
    defer print("3")
    print("body")
end

func captured() -> int
    var n = 1
    defer print("n = {n}")
    n = 2
    var c = Counter{ n: 0 }
    defer c.report()
    c.n = 5
    return n * 10
end

func in_a_loop()
    for i in 0..3
        defer print("loop {i}")
    end
    print("loop done")
end

func early(flag: bool) -> int
    defer print("cleanup")
    if flag
        return 42
    end
    print("not early")
    return 7
end

func fails(n: int) -> int or error
    defer print("fails cleanup {n}")
    if n < 0
        return error("negative")
    end
    return n
end

func chain() -> int or error
    defer print("chain cleanup")
    let v = try fails(-1)
    print("never printed")
    return v
end

func not_reached(go: bool)
    if go
        defer print("deferred in if")
    end
    print("end of not_reached")
end

func with_break()
    for i in 0..3
        defer print("after break {i}")
        if i == 1
            break
        end
    end
    print("with_break done")
end

func main() -> error
    print("-- order")
    order()
    print("-- captured")
    print("returned {captured()}")
    print("-- loop")
    in_a_loop()
    print("-- early")
    print("got {early(true)}")
    print("got {early(false)}")
    print("-- try failure")
    match chain()
        ok(v) -> print("unexpected")
        err(e) -> print("caller got: {e.message}")
    end
    print("-- not reached")
    not_reached(false)
    not_reached(true)
    print("-- break")
    with_break()
    return
end
"#;

const TOUR_OUTPUT: &str = "-- order\nbody\n3\n2\n1\n-- captured\ncounter is 5\nn = 1\nreturned 20\n-- loop\nloop done\nloop 2\nloop 1\nloop 0\n-- early\ncleanup\ngot 42\nnot early\ncleanup\ngot 7\n-- try failure\nfails cleanup -1\nchain cleanup\ncaller got: negative\n-- not reached\nend of not_reached\nend of not_reached\ndeferred in if\n-- break\nwith_break done\nafter break 1\nafter break 0\n";

#[test]
fn order_captures_exits_loops_and_break() {
    let Some((out, code)) = run(TOUR) else { return };
    assert_eq!(out, TOUR_OUTPUT);
    assert_eq!(code, 0);
}

#[test]
fn lambdas_and_async_functions_have_their_own_defers() {
    let src = r#"package main
async func work() -> int
    defer print("async cleanup")
    print("async body")
    return 7
end
func main() -> error
    let f = func(n: int)
        defer print("lambda cleanup {n}")
        print("lambda body {n}")
    end
    f(1)
    print("value {await work()}")
    return
end
"#;
    let Some((out, code)) = run(src) else { return };
    assert_eq!(out, "lambda body 1\nlambda cleanup 1\nasync body\nasync cleanup\nvalue 7\n");
    assert_eq!(code, 0);
}

#[test]
fn a_deferred_call_that_can_fail_is_accepted_and_ignored() {
    let src = r#"package main
type Res = record
    id: int
end
func (ref r: Res) drop()
    print("drop {r.id}")
end
func close_it(id: int) -> error
    print("closing {id}")
    return error("close failed")
end
func acquire() -> int or error
    let r = Res{ id: 1 }
    defer close_it(1)
    return 5
end
func main() -> error
    let v = try acquire()
    print("value {v}")
    return
end
"#;
    let Some((out, code)) = run(src) else { return };
    // the deferred call runs first, then the local is released; the failure changes nothing
    assert_eq!(out, "closing 1\ndrop 1\nvalue 5\n");
    assert_eq!(code, 0);
}

#[test]
fn defers_do_not_leak() {
    let src = r#"package main
type Counter = record
    n: int
    label: text
end
func note(c: Counter)
    let s = "note {c.label} {c.n}"
    if s.len == 0
        print("never")
    end
end
func work(n: int) -> int
    var c = Counter{ n: n, label: "item {n}" }
    defer note(c)
    defer note(c)
    if n % 2 == 0
        return n
    end
    return n + 1
end
func main()
    let warm = work(1)
    let before = live_objects()
    var total = 0
    for i in 0..5000
        total += work(i)
    end
    let growth = live_objects() - before
    print("{total} {growth}")
end
"#;
    let Some((out, code)) = run(src) else { return };
    assert_eq!(code, 0);
    let parts: Vec<&str> = out.trim().split(' ').collect();
    assert_eq!(parts[1], "0", "live objects grew: {}", out);
}

#[test]
fn only_a_call_can_be_deferred() {
    let file = parse_source("package main\nfunc main()\n    var x = 1\n    defer x + 1\nend\n").unwrap();
    let e = brasa::codegen_x64::generate_asm(&file).unwrap_err();
    assert!(e.contains("`defer` needs a call"), "{}", e);
}

#[test]
fn the_interpreter_runs_deferred_calls_at_function_exit() {
    let dir = std::env::temp_dir().join(format!("brasa-defer-interp-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("d.br");
    std::fs::write(
        &file,
        "package main\nfunc work(n: int) -> int\n    defer print(\"first deferred\")\n    defer print(\"second deferred\")\n    print(\"working {n}\")\n    if n > 1\n        return n * 2\n    end\n    return 0\nend\nfunc main()\n    print(\"result {work(5)}\")\n    print(\"result {work(1)}\")\nend\n",
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_brasa-parse")).arg(&file).arg("--run").output().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let text = String::from_utf8_lossy(&out.stdout).replace('\r', "");
    assert_eq!(
        text,
        "working 5\nsecond deferred\nfirst deferred\nresult 10\nworking 1\nsecond deferred\nfirst deferred\nresult 0\n"
    );
    assert!(out.status.success());
}

#[test]
fn return_break_and_continue_inside_if_work_in_the_interpreter() {
    let dir = std::env::temp_dir().join(format!("brasa-flow-interp-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("f.br");
    std::fs::write(
        &file,
        "package main\nfunc first_even(limit: int) -> int\n    for i in 1..limit\n        if i % 2 == 1\n            continue\n        end\n        if i > 3\n            break\n        end\n        return i\n    end\n    return -1\nend\nfunc main()\n    print(first_even(10))\nend\n",
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_brasa-parse")).arg(&file).arg("--run").output().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(String::from_utf8_lossy(&out.stdout).replace('\r', ""), "2\n");
}

#[test]
fn the_defer_example_builds_and_prints_what_the_docs_say() {
    let src = std::fs::read_to_string("examples/defer.br").unwrap();
    let Some((out, code)) = run(&src) else { return };
    assert_eq!(
        out,
        "[job 21] start\n[job 21] cleaning up\n[job 21] finished\nresult: 42\n[job -1] start\n[job -1] finished\nfailed: negative id\n"
    );
    assert_eq!(code, 0);
}
