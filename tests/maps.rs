//! Maps: `Map<K, V>`, literals, lookup, methods, iteration order, memory behavior. Programs are
//! compiled to native executables and run (skipped when the Visual Studio build tools are
//! missing).

use brasa::{compile_to_native, parse_source};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

fn run(src: &str) -> Option<(String, i32)> {
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = std::env::temp_dir().join(format!("brasa-maps-test-{}-{}.exe", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
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
    let text = String::from_utf8_lossy(&out.stdout).replace('\r', "") + &String::from_utf8_lossy(&out.stderr).replace('\r', "");
    Some((text, out.status.code().unwrap_or(-1)))
}

/// The compile error of a program that must not compile.
fn compile_error(src: &str) -> String {
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = std::env::temp_dir().join(format!("brasa-maps-test-{}-{}.exe", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    match compile_to_native(&file, exe.to_str().unwrap()) {
        Ok(()) => panic!("expected a compile error"),
        Err(e) => e,
    }
}

fn main_of(body: &str) -> String {
    let indented: String = body.lines().map(|l| format!("    {}\n", l)).collect();
    format!("package main\n\ntype Point = record\n    x: int\n    y: int\nend\n\nfunc main()\n{}end\n", indented)
}

#[test]
fn literals_lookup_and_update() {
    let src = main_of(
        r#"let m = { "b": 2, "a": 1 }
print("len {m.len}")
m["a"] = 10
m["c"] = 3
print("{m["a"]} {m["b"]} {m["c"]} len {m.len}")
m["a"] += 1
print("a {m["a"]}")
let e: Map<text, int> = {}
print("empty {e.len}")"#,
    );
    let Some((out, code)) = run(&src) else { return };
    assert_eq!(out, "len 2\n10 2 3 len 3\na 11\nempty 0\n");
    assert_eq!(code, 0);
}

#[test]
fn missing_key_aborts() {
    let src = main_of(
        r#"let m = { "a": 1 }
print("before")
print("{m["nope"]}")
print("after")"#,
    );
    let Some((out, code)) = run(&src) else { return };
    assert_eq!(out, "before\nabort: main.br:11: map key not found\n");
    assert_eq!(code, 101);
}

#[test]
fn methods_has_get_remove_clear() {
    let src = main_of(
        r#"let m = { "a": 1, "b": 2 }
print("{m.has("a")} {m.has("z")} {m.get("z", 0)} {m.get("a", 0)}")
m.remove("a")
m.remove("nothing")
print("{m.has("a")} len {m.len}")
m.clear()
print("len {m.len}")
m["x"] = 5
print("x {m["x"]} len {m.len}")"#,
    );
    let Some((out, code)) = run(&src) else { return };
    assert_eq!(out, "true false 0 1\nfalse len 1\nlen 0\nx 5 len 1\n");
    assert_eq!(code, 0);
}

#[test]
fn iteration_is_in_insertion_order() {
    let src = main_of(
        r#"let m = { "b": 1, "a": 2 }
m["c"] = 3
m["a"] = 20
for k, v in m
    print("{k}={v}")
end
m.remove("b")
m["b"] = 9
for k in m
    print(k)
end"#,
    );
    let Some((out, code)) = run(&src) else { return };
    assert_eq!(out, "b=1\na=20\nc=3\na\nc\nb\n");
    assert_eq!(code, 0);
}

#[test]
fn ten_thousand_integer_keys() {
    let src = main_of(
        r#"var m: Map<int, int> = {}
for i in 0..10000
    m[i] = i * 2
end
for i in 0..10000
    if i % 2 == 0
        m.remove(i)
    end
end
var ok = 0
for i in 0..10000
    if i % 2 == 1
        if m[i] == i * 2
            ok += 1
        end
    else if m.has(i)
        ok += 100000
    end
end
print("len {m.len} ok {ok}")"#,
    );
    let Some((out, code)) = run(&src) else { return };
    assert_eq!(out, "len 5000 ok 5000\n");
    assert_eq!(code, 0);
}

#[test]
fn text_keys_survive_growth_and_churn() {
    let src = main_of(
        r#"var m: Map<text, int> = {}
for i in 0..3000
    m["key {i}"] = i
end
for i in 0..3000
    if i % 3 == 0
        m.remove("key {i}")
    end
end
for i in 0..3000
    if i % 3 == 0
        m["key {i}"] = i + 1
    end
end
var bad = 0
for i in 0..3000
    var want = i
    if i % 3 == 0
        want = i + 1
    end
    if m["key {i}"] != want
        bad += 1
    end
end
print("len {m.len} bad {bad}")"#,
    );
    let Some((out, code)) = run(&src) else { return };
    assert_eq!(out, "len 3000 bad 0\n");
    assert_eq!(code, 0);
}

#[test]
fn maps_are_values_and_can_be_stored_and_returned() {
    let src = r#"package main

type Registry = record
    names: Map<text, int>
end

func fill(ref m: Map<text, int>)
    m["from"] = 1
end

func make() -> Map<text, int>
    return { "made": 7 }
end

func main()
    let m: Map<text, int> = {}
    fill(ref m)
    var alias = m
    alias["alias"] = 2
    print("{m["from"]} {alias["alias"]} {m.len} {alias.len}")
    let r = Registry { names: make() }
    print("{r.names["made"]}")
    let groups: List<Map<text, int>> = [make(), make()]
    print("{groups.len} {groups[1]["made"]}")
end
"#;
    let Some((out, code)) = run(src) else { return };
    assert_eq!(out, "1 2 1 2
7
2 7
");
    assert_eq!(code, 0);
}

#[test]
fn values_can_be_records_and_lists() {
    let src = main_of(
        r#"let pts: Map<text, Point> = { "o": Point { x: 1, y: 2 } }
print("{pts["o"].y}")
var lists: Map<int, List<text>> = {}
lists[1] = ["a", "b"]
print("{lists[1].len}")"#,
    );
    let Some((out, code)) = run(&src) else { return };
    assert_eq!(out, "2\n2\n");
    assert_eq!(code, 0);
}

#[test]
fn maps_do_not_leak() {
    let src = r#"package main

type Item = record
    n: int
    label: text
end

func work(n: int) -> int
    var m: Map<text, Item> = {}
    for i in 0..20
        m["k {i}"] = Item { n: i, label: "item {i}" }
    end
    m["k 3"] = Item { n: n, label: "replaced {n}" }
    m.remove("k 4")
    m.remove("k 5")
    var total = 0
    for k, v in m
        total += v.n + k.len
    end
    let again: Map<int, text> = { 1: "one", 2: "two" }
    for k, v in again
        total += k + v.len
    end
    m.clear()
    return total + m.get("gone", Item { n: 0, label: "d" }).n
end

func main()
    let warm = work(1)
    let before = live_objects()
    var total = 0
    for i in 0..5000
        total += work(i)
    end
    let growth = live_objects() - before
    print("{growth}")
end
"#;
    let Some((out, code)) = run(src) else { return };
    assert_eq!(out, "0\n");
    assert_eq!(code, 0);
}

#[test]
fn compile_errors() {
    let e = compile_error("package main\n\ntype Spot = record\n    x: float\n    y: int\nend\n\nfunc main()\n    let m: Map<Spot, int> = {}\n    print(\"{m.len}\")\nend\n");
    assert!(e.contains("Spot cannot be a map key because field `x: float` is not Hash"), "{}", e);
    let e = compile_error(&main_of("let m: Map<float, int> = {}\nprint(\"{m.len}\")"));
    assert!(e.contains("float cannot be a map key because it is not Hash"), "{}", e);
    let e = compile_error(&main_of("let m = { \"a\": 1, \"b\": \"two\" }\nprint(\"{m.len}\")"));
    assert!(e.contains("map value"), "{}", e);
    let e = compile_error(&main_of("let m = {}\nprint(\"{m.len}\")"));
    assert!(e.contains("empty map") && e.contains("type annotation"), "{}", e);
    let e = compile_error(&main_of("let m = { \"a\": 1 }\nprint(\"{m[1]}\")"));
    assert!(e.contains("map key"), "{}", e);
}

#[test]
fn a_map_program_prints_in_insertion_order_and_aborts_on_a_missing_key() {
    let src = main_of(
        r#"let m = { "b": 1, "a": 2 }
m["c"] = 3
m["a"] += 18
for k, v in m
    print("{k}={v}")
end
m.remove("b")
m["b"] = 9
for k in m
    print(k)
end
print("{m.has("a")} {m.has("zz")} {m.get("zz", 7)} len {m.len}")
var ids: Map<int, text> = {}
ids[7] = "seven"
ids[3] = "three"
for k, v in ids
    print("{k} {v}")
end
m.clear()
print("cleared {m.len}")
print("{m["nope"]}")"#,
    );
    let expected = "b=1
a=20
c=3
a
c
b
true false 7 len 3
7 seven
3 three
cleared 0
";
    // the program prints these lines and stops with an abort
    if let Some((native, code)) = run(&src) {
        assert_eq!(native, format!("{}abort: main.br:29: map key not found\n", expected));
        assert_eq!(code, 101);
    }
}

#[test]
fn a_bare_metal_kernel_rejects_maps() {
    let boot = parse_source(&std::fs::read_to_string("os/boot.br").unwrap()).unwrap();
    let kernel = parse_source("package main\nfunc console_write(s: text)\nend\nfunc main()\n    let m: Map<text, int> = {}\nend\n").unwrap();
    let e = brasa::codegen_x64::build_os(&boot, &kernel).unwrap_err();
    assert!(e.contains("maps") && e.contains("bare-metal"), "{}", e);
}

#[test]
fn the_maps_example_builds_and_prints_what_the_docs_say() {
    let src = std::fs::read_to_string("examples/maps.br").unwrap();
    let Some((out, code)) = run(&src) else { return };
    assert_eq!(out, "red: 3\ngreen: 2\nblue: 1\n2 prices, apple costs 3\nno pears today\nkiwi costs 0\n");
    assert_eq!(code, 0);
}

#[test]
fn record_enum_and_list_keys() {
    let src = main_of(
        r#"let m: Map<Point, text> = {}
m[Point { x: 1, y: 2 }] = "a"
print(m[Point { x: 1, y: 2 }])
m[Point { x: 1, y: 2 }] = "b"
print("{m.len} {m[Point { x: 1, y: 2 }]}")
let l: Map<[]int, int> = {}
l[[1, 2]] = 5
l[[1, 2]] += 1
print("{l[[1, 2]]} {l.has([2, 1])} {l.len}")
let lit: Map<Point, int> = { Point { x: 0, y: 0 }: 10 }
print(lit[Point { x: 0, y: 0 }])"#,
    );
    let Some((out, code)) = run(&src) else { return };
    assert_eq!(out, "a\n1 b\n6 false 1\n10\n");
    assert_eq!(code, 0);
}

#[test]
fn many_record_keys() {
    let src = main_of(
        r#"let m: Map<Point, int> = {}
var i = 0
while i < 100000
    m[Point { x: i, y: i % 7 }] = i
    i += 1
end
i = 0
while i < 100000
    if i % 2 == 0
        m.remove(Point { x: i, y: i % 7 })
    end
    i += 1
end
var ok = true
i = 0
while i < 100000
    if m.has(Point { x: i, y: i % 7 }) != (i % 2 == 1)
        ok = false
    end
    i += 1
end
print("{m.len} {ok}")"#,
    );
    let Some((out, code)) = run(&src) else { return };
    assert_eq!(out, "50000 true\n");
    assert_eq!(code, 0);
}

#[test]
fn record_keys_are_released() {
    let src = r#"package main

type Name = record
    first: text
    last: text
end

func work(n: int) -> int
    let m: Map<Name, int> = {}
    for i in 0..30
        m[Name { first: "f {i}", last: "l" }] = i
    end
    m[Name { first: "f 1", last: "l" }] = 99
    m.remove(Name { first: "f 2", last: "l" })
    if n % 2 == 0
        m.clear()
    end
    return m.len
end

func main()
    let warm = work(1)
    let before = live_objects()
    var total = 0
    for i in 0..5000
        total += work(i)
    end
    print("grew {live_objects() - before} {total > 0}")
end
"#;
    let Some((out, code)) = run(src) else { return };
    assert_eq!(out, "grew 0 true\n");
    assert_eq!(code, 0);
}
