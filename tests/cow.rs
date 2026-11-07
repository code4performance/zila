//! Copy on write (spec `copy-on-write`): lists and maps have value semantics, a copy costs nothing
//! until a write, and a write to a value someone else holds copies it first. Every program is
//! compiled and run both optimized and unoptimized, and both must print the same (skipped when the
//! Visual Studio build tools are missing).

use brasa::codegen_x64::{compile_to_native_with, CompileOptions};
use brasa::parse_source;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

const ON: CompileOptions = CompileOptions { optimize: true, test_mode: false };
const OFF: CompileOptions = CompileOptions { optimize: false, test_mode: false };

fn run_with(src: &str, options: &CompileOptions) -> Option<(String, i32)> {
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = std::env::temp_dir().join(format!("brasa-cow-test-{}-{}.exe", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    match compile_to_native_with(&file, exe.to_str().unwrap(), options) {
        Ok(()) => {}
        Err(e) if e.contains("could not find Visual Studio") || e.contains("vcvars64") => {
            eprintln!("no native toolchain: skipping");
            return None;
        }
        Err(e) => panic!("compile error (optimize = {}): {}", options.optimize, e),
    }
    let out = Command::new(&exe).output().unwrap();
    let _ = std::fs::remove_file(&exe);
    let text = String::from_utf8_lossy(&out.stdout).replace('\r', "") + &String::from_utf8_lossy(&out.stderr).replace('\r', "");
    Some((text, out.status.code().unwrap_or(-1)))
}

/// Runs the program both ways; the results must agree.
fn run(src: &str) -> Option<(String, i32)> {
    let a = run_with(src, &OFF)?;
    let b = run_with(src, &ON)?;
    assert_eq!(a, b, "optimized and unoptimized runs differ");
    Some(a)
}

fn expect(src: &str, want: &str) {
    let Some((out, code)) = run(src) else { return };
    assert_eq!(out, want);
    assert_eq!(code, 0);
}

const HEAD: &str = "package main

type Bag = record
    name: text
    items: []int
end

type Inner = record
    tags: []text
    n: int
end

type Outer = record
    parts: []Inner
    m: Map<text, Inner>
end

#[shared]
type Box = record
    items: []int
end

";

fn prog(body: &str) -> String {
    let indented: String = body.lines().map(|l| format!("    {}\n", l)).collect();
    format!("{}func main()\n{}end\n", HEAD, indented)
}

fn prog_with(funcs: &str, body: &str) -> String {
    let indented: String = body.lines().map(|l| format!("    {}\n", l)).collect();
    format!("{}{}\nfunc main()\n{}end\n", HEAD, funcs, indented)
}

// ---- value semantics ----

#[test]
fn a_copied_record_does_not_share_its_list() {
    expect(
        &prog("let a = Bag { name: \"a\", items: [1, 2] }\nvar b = a\nb.items.push(3)\nprint(\"{a.items.len} {b.items.len}\")"),
        "2 3\n",
    );
}

#[test]
fn a_copied_list_does_not_alias() {
    expect(&prog("var xs = [1, 2, 3]\nvar ys = xs\nys[0] = 99\nprint(\"{xs[0]} {ys[0]}\")"), "1 99\n");
}

#[test]
fn a_list_passed_to_a_function_is_not_changed_by_it() {
    let f = "func add_one(xs: []int) -> int\n    xs.push(1)\n    return xs.len\nend\n";
    expect(&prog_with(f, "let xs = [5]\nlet n = add_one(xs)\nprint(\"{n} {xs.len}\")"), "2 1\n");
}

#[test]
fn a_map_inside_a_record_inside_a_list_is_independent() {
    let src = prog(
        "var o = Outer { parts: [Inner { tags: [\"a\"], n: 1 }], m: {} }\n\
         o.m[\"k\"] = Inner { tags: [], n: 2 }\n\
         var copy = o\n\
         copy.m[\"j\"] = Inner { tags: [], n: 3 }\n\
         copy.parts[0].n = 7\n\
         print(\"{o.m.has(\"j\")} {copy.m.has(\"j\")} {o.parts[0].n} {copy.parts[0].n}\")",
    );
    expect(&src, "false true 1 7\n");
}

#[test]
fn a_returned_list_field_can_be_appended_without_changing_the_record() {
    let f = "func items_of(b: Bag) -> []int\n    return b.items\nend\n";
    expect(
        &prog_with(f, "let b = Bag { name: \"b\", items: [1] }\nvar r = items_of(b)\nr.push(2)\nprint(\"{b.items.len} {r.len}\")"),
        "1 2\n",
    );
}

#[test]
fn shared_records_are_exempt() {
    expect(
        &prog("let a = Box { items: [1] }\nlet b = a\nb.items.push(2)\nprint(\"{a.items.len} {b.items.len}\")"),
        "2 2\n",
    );
}

#[test]
fn a_closure_keeps_the_list_it_captured() {
    expect(
        &prog("var xs = [1, 2]\nlet f = func() xs.len end\nxs.push(3)\nprint(\"{f()} {xs.len}\")"),
        "2 3\n",
    );
}

// ---- what counts as a write ----

#[test]
fn every_write_kind_leaves_the_original_alone() {
    let kinds = [
        ("ys[0] = 9", "1 3"),
        ("ys[1] += 5", "1 3"),
        ("ys.push(4)", "1 3"),
        ("ys.add(4)", "1 3"),
        ("let t = ys.take()", "1 3"),
        ("ys.clear()", "1 3"),
    ];
    for (op, _) in kinds {
        let body = format!("let xs = [1, 2, 3]\nvar ys = xs\n{}\nprint(\"{{xs[0]}} {{xs.len}}\")", op);
        expect(&prog(&body), "1 3\n");
    }
    let mkinds = ["ms[\"a\"] = 9", "ms[\"a\"] += 1", "ms.remove(\"a\")", "ms.clear()", "ms[\"new\"] = 1"];
    for op in mkinds {
        let body = format!("let m: Map<text, int> = {{ \"a\": 1, \"b\": 2 }}\nvar ms = m\n{}\nprint(\"{{m[\"a\"]}} {{m.len}}\")", op);
        expect(&prog(&body), "1 2\n");
    }
}

#[test]
fn a_path_write_copies_only_the_path() {
    let src = prog(
        "let a = Outer { parts: [Inner { tags: [\"p\"], n: 1 }, Inner { tags: [\"q\"], n: 2 }], m: {} }\n\
         var b = a\n\
         b.parts[1].tags.push(\"x\")\n\
         print(\"{a.parts[1].tags.len} {b.parts[1].tags.len} {a.parts[0].tags.len}\")",
    );
    expect(&src, "1 2 1\n");
}

#[test]
fn a_map_value_path_write() {
    let src = prog(
        "var a = Outer { parts: [], m: {} }\n\
         a.m[\"k\"] = Inner { tags: [\"t\"], n: 0 }\n\
         var b = a\n\
         b.m[\"k\"].tags.push(\"u\")\n\
         b.m[\"k\"].n = 5\n\
         print(\"{a.m[\"k\"].tags.len} {b.m[\"k\"].tags.len} {a.m[\"k\"].n} {b.m[\"k\"].n}\")",
    );
    expect(&src, "1 2 0 5\n");
}

#[test]
fn reads_do_not_copy() {
    let src = prog(
        "let xs = [1, 2, 3, 4]\nlet before = live_objects()\nlet ys = xs\nvar t = 0\nfor x in ys\n    t += x\nend\nt += ys[1] + ys.len\nprint(\"{live_objects() - before} {t}\")",
    );
    expect(&src, "0 16\n");
}

#[test]
fn a_copy_costs_nothing_until_a_write() {
    let src = prog(
        "var xs: []int = []\nxs.push(0)\nfor i in 0..1000\n    xs.push(i)\nend\nlet before = live_objects()\nvar ys = xs\nlet same = live_objects() - before\nys.push(1)\nlet first = live_objects() - before\nys.push(2)\nys[0] = 5\nlet later = live_objects() - before\nprint(\"{same} {first > 0} {later == first} {xs.len} {ys.len}\")",
    );
    expect(&src, "0 true true 1001 1003\n");
}

#[test]
fn unique_writes_do_not_copy() {
    let src = prog(
        "var xs: []int = []\nxs.push(0)\nlet before = live_objects()\nfor i in 0..10000\n    xs.push(i)\nend\nxs[3] = 1\nprint(\"{live_objects() - before} {xs.len}\")",
    );
    expect(&src, "0 10001\n");
}

#[test]
fn a_field_list_is_written_in_place_when_unique() {
    let src = prog(
        "var b = Bag { name: \"n\", items: [0] }\nlet before = live_objects()\nfor i in 0..1000\n    b.items.push(i)\nend\nb.items[0] = 7\nprint(\"{live_objects() - before} {b.items.len}\")",
    );
    expect(&src, "0 1001\n");
}

#[test]
fn an_out_of_range_write_on_a_shared_list_does_not_copy_and_reports_the_same_error() {
    let src = prog("let xs = [1, 2]\nvar ys = xs\nys[5] = 1\nprint(\"unreachable\")");
    let Some((out, code)) = run(&src) else { return };
    assert!(out.contains("index out of bounds"), "{}", out);
    assert!(!out.contains("unreachable"), "{}", out);
    assert_ne!(code, 0);
    // the same program with an unshared list fails the same way
    let src = prog("var ys = [1, 2]\nys[5] = 1\nprint(\"unreachable\")");
    let Some((out2, code2)) = run(&src) else { return };
    assert!(out2.contains("index out of bounds"), "{}", out2);
    assert_eq!(code, code2);
}

#[test]
fn statements_that_read_and_write_the_same_list() {
    let f = "func total(xs: []int) -> int\n    var t = 0\n    for x in xs\n        t += x\n    end\n    return t\nend\n";
    expect(
        &prog_with(f, "var xs = [1, 2, 3]\nxs[0] = xs[2] + 1\nxs[1] = total(xs)\nxs[2] = xs[0] * xs[1]\nxs.push(xs.len)\nprint(\"{xs[0]} {xs[1]} {xs[2]} {xs[3]}\")"),
        "4 9 36 3\n",
    );
}

// ---- ref ----

#[test]
fn ref_writes_the_callers_list() {
    let f = "func fill(ref xs: []int, n: int)\n    for i in 0..n\n        xs.push(i)\n    end\nend\n";
    let src = prog_with(f, "var xs: []int = []\nxs.push(100)\nlet before = live_objects()\nfill(ref xs, 50)\nprint(\"{live_objects() - before} {xs.len} {xs[50]}\")");
    expect(&src, "0 51 49\n");
}

#[test]
fn ref_on_a_copy_changes_only_the_copy() {
    let f = "func fill(ref xs: []int)\n    xs.push(9)\n    xs[0] = 7\nend\n";
    expect(&prog_with(f, "let xs = [1]\nvar ys = xs\nfill(ref ys)\nprint(\"{xs.len} {xs[0]} {ys.len} {ys[0]}\")"), "1 1 2 7\n");
}

#[test]
fn ref_through_a_record_and_a_map() {
    let f = "func fill(ref b: Bag)\n    b.items.push(1)\nend\nfunc put(ref m: Map<text, int>)\n    m[\"k\"] = 1\nend\nfunc swap(ref xs: []int)\n    xs = [8, 9]\nend\n";
    expect(
        &prog_with(
            f,
            "var a = Bag { name: \"a\", items: [] }\nvar b = a\nfill(ref b)\nvar m: Map<text, int> = {}\nlet n = m\nput(ref m)\nvar ys = [1]\nswap(ref ys)\nprint(\"{a.items.len} {b.items.len} {n.len} {m.len} {ys.len}\")",
        ),
        "0 1 0 1 2\n",
    );
}

#[test]
fn ref_passed_on_and_to_a_field() {
    let f = "func inner(ref xs: []int)\n    xs.push(1)\nend\nfunc outer(ref xs: []int)\n    inner(ref xs)\n    inner(ref xs)\nend\n";
    expect(&prog_with(f, "var b = Bag { name: \"a\", items: [] }\nouter(ref b.items)\nprint(\"{b.items.len}\")"), "2\n");
}

// ---- iteration while mutating ----

#[test]
fn appending_inside_a_loop_over_the_list() {
    expect(
        &prog("var xs = [1, 2, 3]\nvar seen = 0\nfor x in xs\n    xs.push(x)\n    seen += 1\nend\nprint(\"{seen} {xs.len}\")"),
        "3 6\n",
    );
}

#[test]
fn removing_inside_a_loop_over_the_map() {
    expect(
        &prog("var m: Map<text, int> = { \"a\": 1, \"b\": 2, \"c\": 3 }\nvar seen = 0\nfor k, v in m\n    m.remove(k)\n    seen += 1\nend\nprint(\"{seen} {m.len}\")"),
        "3 0\n",
    );
}

#[test]
fn a_loop_that_ends_leaves_nothing_behind() {
    expect(
        &prog("var xs = [1, 2, 3]\nfor x in xs\n    xs.push(x)\nend\nxs.clear()\nlet before = live_objects()\nfor x in 0..3\n    xs.push(x)\nend\nfor x in xs\n    xs.push(x)\nend\nxs.clear()\nxs.clear()\nprint(\"{xs.len}\")"),
        "0\n",
    );
}

// ---- leaks ----

#[test]
fn copy_and_write_loops_do_not_leak() {
    let src = r#"package main

type Item = record
    label: text
    n: int
end

type Rec = record
    names: []text
    items: []Item
    m: Map<text, Item>
end

func work(i: int) -> int
    var a = Rec { names: ["x", "y"], items: [Item { label: "l", n: i }], m: {} }
    a.m["k"] = Item { label: "v", n: i }
    var b = a
    var c = b
    b.names.push("z")
    b.items[0].n = 5
    b.items.push(Item { label: "n", n: 2 })
    b.m["j"] = Item { label: "w", n: 3 }
    c.m["k"].n = 9
    c.names[0] = "q"
    var d = c.items
    d.push(Item { label: "d", n: 1 })
    var e = c.m
    e.remove("k")
    e.clear()
    return a.names.len + b.names.len + b.items.len + c.items.len + d.len + b.m.len + a.m["k"].n
end

func main()
    let warm = work(1)
    let before = live_objects()
    var t = 0
    for i in 0..5000
        t += work(i)
    end
    print("{live_objects() - before} {t > 0}")
end
"#;
    expect(src, "0 true\n");
}

#[test]
fn error_paths_do_not_leave_a_double_release() {
    let src = prog("let xs = [1, 2]\nvar ys = xs\nys.push(3)\nys[7] = 1\nprint(\"no\")");
    let Some((out, code)) = run(&src) else { return };
    assert!(out.contains("index out of bounds"), "{}", out);
    assert_ne!(code, 0);
}

// ---- optimizer interplay ----

#[test]
fn hot_loops_that_write_lists_held_in_locals_agree_with_and_without_optimization() {
    let f = "func sum(xs: []int) -> int\n    var t = 0\n    for x in xs\n        t += x\n    end\n    return t\nend\nfunc twice(xs: []int) -> int\n    return xs.len * 2\nend\n";
    let body = "var xs: []int = []\nvar ys = xs\nvar acc = 0\nfor i in 0..2000\n    xs.push(i)\n    if i % 3 == 0\n        ys = xs\n    end\n    xs[i] = xs[i] + acc % 7\n    acc += twice(xs) + sum(ys) % 11\nend\nprint(\"{acc} {xs.len} {ys.len} {sum(xs)}\")";
    let Some((out, code)) = run(&prog_with(f, body)) else { return };
    assert_eq!(code, 0, "{}", out);
    assert!(out.ends_with('\n') && out.lines().count() == 1, "{}", out);
}

#[test]
fn a_helper_that_receives_a_list_is_still_a_call() {
    let src = prog_with("func first(xs: []int) -> int\n    return xs[0]\nend\n", "let xs = [4]\nprint(first(xs))");
    let file = parse_source(&src).unwrap();
    let asm = brasa::codegen_x64::generate_asm_with(&file, &ON).unwrap();
    let main_start = asm.find("B_main_main proc").unwrap();
    let main_body = &asm[main_start..asm[main_start..].find("B_main_main endp").map(|i| i + main_start).unwrap()];
    assert!(main_body.contains("call B_main_first"), "{}", main_body);
}

/// Random programs that copy lists and maps and write to them; both builds must print the same.
#[test]
fn random_copy_and_write_programs_agree() {
    let mut seed: u64 = 0x9E3779B97F4A7C15;
    let mut next = move |n: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % n
    };
    for _ in 0..6 {
        let mut body = String::from("var a: []int = [1, 2, 3]\nvar b: []int = [4]\nvar m: Map<int, int> = { 1: 1 }\nvar n: Map<int, int> = { 2: 2 }\nvar bag = Bag { name: \"x\", items: [7] }\nvar bag2 = bag\n");
        for _ in 0..60 {
            let k = next(9);
            let v = next(50);
            let line = match next(14) {
                0 => "b = a".to_string(),
                1 => "a = b".to_string(),
                2 => format!("a.push({})", v),
                3 => format!("b.push({})", v),
                4 => format!("if a.len > 0\n    a[{} % a.len] = {}\nend", k, v),
                5 => "n = m".to_string(),
                6 => format!("m[{}] = {}", k, v),
                7 => format!("n[{}] = {}", k, v),
                8 => format!("m.remove({})", k),
                9 => "bag2 = bag".to_string(),
                10 => format!("bag.items.push({})", v),
                11 => format!("bag2.items.push({})", v),
                12 => "if b.len > 1\n    let t = b.take()\nend".to_string(),
                _ => format!("bag2.items = a"),
            };
            body.push_str(&line);
            body.push('\n');
        }
        body.push_str("var s = 0\nfor x in a\n    s += x\nend\nfor x in b\n    s += 2 * x\nend\nfor k, v in m\n    s += k + v\nend\nfor k, v in n\n    s += 3 * k + v\nend\nfor x in bag.items\n    s += x\nend\nfor x in bag2.items\n    s += 5 * x\nend\nprint(\"{a.len} {b.len} {m.len} {n.len} {bag.items.len} {bag2.items.len} {s}\")\n");
        let before = "let base = live_objects()\n";
        let _ = before;
        let Some((out, code)) = run(&prog(&body)) else { return };
        assert_eq!(code, 0, "{}\n{}", out, body);
    }
}
