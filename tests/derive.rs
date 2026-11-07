//! The standard interfaces `Eq`, `Ord` and `Hash`: derivation for records and enums, overriding,
//! the rules of the built-in types, identity types, and the operators. Programs are compiled to
//! native executables and run (skipped when the Visual Studio build tools are missing).
//!
//! What `==` did before this change (task 2.1), kept here so the break is explicit:
//! - records: compared the two pointers (identity). Two records with equal fields were never `==`,
//!   and because plain records are copied when stored, two names for "the same" record never were.
//!   Now plain records compare field by field, and `#[shared]` records keep identity.
//! - enums without data: compared the variant number (unchanged). Enums with data: a compile error
//!   ("enums with data cannot be compared with '=='"); now they derive `eq` and `compare`.
//! - lists and maps: compared the two pointers; now element by element (maps: same keys and equal
//!   values) and lists are ordered lexicographically; maps are never ordered.
//! - optionals (`T?` is the reference type itself): `x == nil` is still a pointer test.
//! - pointers, functions and channels: raw address comparison, unchanged.
//! - text, integers, floats: unchanged (floats keep IEEE `==` and `<`).

use brasa::{compile_to_native, parse_source};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

fn run(src: &str) -> Option<(String, i32)> {
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = std::env::temp_dir().join(format!("brasa-derive-test-{}-{}.exe", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
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
    let exe = std::env::temp_dir().join(format!("brasa-derive-test-{}-{}.exe", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    match compile_to_native(&file, exe.to_str().unwrap()) {
        Ok(()) => panic!("expected a compile error"),
        Err(e) => e,
    }
}

const TYPES: &str = r#"package main

type P = record
    x: int
    y: int
end

type Line = record
    from: P
    to: P
end

type Shape = enum
    Dot
    Circle(int)
    Rect(int, int)
end

"#;

fn prog(extra: &str, body: &str) -> String {
    let indented: String = body.lines().map(|l| format!("    {}\n", l)).collect();
    format!("{}{}\nfunc main()\n{}end\n", TYPES, extra, indented)
}

#[test]
fn bounds_available_without_use() {
    let src = prog(
        "func largest<T: Ord>(xs: []T) -> T\n    var best = xs[0]\n    for x in xs\n        if x.compare(best) > 0\n            best = x\n        end\n    end\n    return best\nend\n",
        "print(\"{largest([3, 9, 4])} {largest([\"b\", \"a\"])}\")",
    );
    let Some((out, code)) = run(&src) else { return };
    assert_eq!(out, "9 b\n");
    assert_eq!(code, 0);
}

#[test]
fn ord_implies_eq() {
    let src = prog(
        "func same<T: Ord>(a: T, b: T) -> bool\n    return a == b\nend\n",
        "print(\"{same(3, 3)} {same(3, 4)} {same(\"a\", \"a\")} {same(P { x: 1, y: 2 }, P { x: 1, y: 2 })}\")",
    );
    let Some((out, code)) = run(&src) else { return };
    assert_eq!(out, "true false true true\n");
    assert_eq!(code, 0);
}

#[test]
fn record_equality_and_order() {
    let src = prog(
        "",
        r#"let a = P { x: 1, y: 2 }
let b = P { x: 1, y: 2 }
print("{a == b} {a != b}")
print("{P { x: 1, y: 9 } < P { x: 2, y: 0 }}")
print("{a.compare(P { x: 1, y: 3 }) < 0} {a.compare(b)} {P { x: 2, y: 0 }.compare(a) > 0}")
print("{a <= b} {a >= b} {a > b}")"#,
    );
    let Some((out, code)) = run(&src) else { return };
    assert_eq!(out, "true false\ntrue\ntrue 0 true\ntrue true false\n");
    assert_eq!(code, 0);
}

#[test]
fn equal_values_hash_alike() {
    let src = prog(
        "",
        r#"let a = P { x: 5, y: 6 }
let b = P { x: 5, y: 6 }
print("{a.hash() == b.hash()}")
let seen: Map<long, bool> = {}
var i = 0
while i < 1000
    seen[P { x: i, y: i * 7 }.hash()] = true
    i += 1
end
print("{seen.len >= 990}")"#,
    );
    let Some((out, code)) = run(&src) else { return };
    assert_eq!(out, "true\ntrue\n");
    assert_eq!(code, 0);
}

#[test]
fn nested_types() {
    let src = prog(
        "",
        r#"let a = Line { from: P { x: 1, y: 2 }, to: P { x: 3, y: 4 } }
let b = Line { from: P { x: 1, y: 2 }, to: P { x: 3, y: 4 } }
let c = Line { from: P { x: 1, y: 2 }, to: P { x: 3, y: 5 } }
print("{a == b} {a == c} {a < c} {a.hash() == b.hash()} {a.hash() == c.hash()}")"#,
    );
    let Some((out, code)) = run(&src) else { return };
    assert_eq!(out, "true false true true false\n");
    assert_eq!(code, 0);
}

#[test]
fn enums() {
    let src = prog(
        "",
        r#"print("{Dot < Circle(1)} {Circle(5) < Rect(0, 0)} {Rect(1, 2) < Rect(1, 3)}")
print("{Circle(2) == Circle(2)} {Circle(2) != Rect(2, 2)} {Dot == Dot} {Circle(3) > Circle(2)}")
print("{Circle(2).hash() == Circle(2).hash()} {Circle(2).hash() == Rect(2, 2).hash()}")"#,
    );
    let Some((out, code)) = run(&src) else { return };
    assert_eq!(out, "true true true\ntrue true true true\ntrue false\n");
    assert_eq!(code, 0);
}

#[test]
fn recursive_record() {
    let src = r#"package main

type Tree = record
    value: int
    left: Tree?
    right: Tree?
end

func build(n: int) -> Tree?
    if n == 0
        return nil
    end
    return Tree { value: n, left: build(n - 1), right: build(n - 1) }
end

func main()
    let a = build(4)
    let b = build(4)
    let c = build(3)
    print("{a == b} {a == c} {a.hash() == b.hash()}")
    let m: Map<Tree, int> = {}
    m[a] = 1
    print("{m.has(b)} {m.has(c)}")
end
"#;
    let Some((out, code)) = run(src) else { return };
    assert_eq!(out, "true false true\ntrue false\n");
    assert_eq!(code, 0);
}

#[test]
fn custom_equality() {
    let src = r#"package main

type User = record
    id: int
    name: text
end

func (a: User) eq(b: User) -> bool
    return a.id == b.id
end

func (a: User) hash() -> long
    return long(a.id)
end

func main()
    let a = User { id: 1, name: "ana" }
    let b = User { id: 1, name: "bob" }
    print("{a == b}")
    let m: Map<User, int> = {}
    m[a] = 1
    m[b] = 2
    print("{m.len} {m[a]}")
end
"#;
    let Some((out, code)) = run(src) else { return };
    assert_eq!(out, "true\n1 2\n");
    assert_eq!(code, 0);
}

#[test]
fn custom_order() {
    let src = r#"package main

type Task = record
    priority: int
    name: text
end

func (a: Task) compare(b: Task) -> int
    return b.priority - a.priority
end

func main()
    let hi = Task { priority: 9, name: "hi" }
    let lo = Task { priority: 1, name: "lo" }
    print("{hi < lo} {hi.compare(lo) < 0} {lo > hi}")
end
"#;
    let Some((out, code)) = run(src) else { return };
    assert_eq!(out, "true true true\n");
    assert_eq!(code, 0);
}

#[test]
fn eq_without_hash() {
    let src = r#"package main

type User = record
    id: int
end

func (a: User) eq(b: User) -> bool
    return a.id == b.id
end

func main()
    let m: Map<User, int> = {}
    print("{m.len}")
end
"#;
    let e = compile_error(src);
    assert!(e.contains("User cannot be a map key because it defines eq but not hash"), "{}", e);
}

#[test]
fn wrong_signature() {
    let src = r#"package main

type Q = record
    x: int
end

func (a: Q) eq(b: int) -> bool
    return a.x == b
end

func main()
    let m: Map<Q, int> = {}
    print("{m.len}")
end
"#;
    let e = compile_error(src);
    assert!(e.contains("`eq` has the wrong signature for Eq"), "{}", e);
}

#[test]
fn text_by_content() {
    let src = prog(
        "",
        r#"let a = "ab" + "c"
let b = "a" + "bc"
print("{a == b} {a.compare(b)} {a.hash() == b.hash()} {"a".compare("b")} {"b".compare("a")} {"ab".compare("a")}")"#,
    );
    let Some((out, code)) = run(&src) else { return };
    assert_eq!(out, "true 0 true -1 1 1\n");
    assert_eq!(code, 0);
}

#[test]
fn floats_are_not_hashable() {
    for decl in ["let m: Map<float, int> = {}", "let m: Map<F, int> = {}"] {
        let src = prog("type F = record\n    x: float\nend\n", &format!("{}\nprint(\"{{m.len}}\")", decl));
        let e = compile_error(&src);
        assert!(e.contains("cannot be a map key"), "{}", e);
        assert!(e.contains("not Hash"), "{}", e);
        assert!(e.contains("NaN is not equal to itself"), "{}", e);
    }
    let src = prog("type F = record\n    x: float\nend\n", "let m: Map<F, int> = {}\nprint(\"{m.len}\")");
    let e = compile_error(&src);
    assert!(e.contains("field `x: float` is not Hash"), "{}", e);
}

#[test]
fn floats_are_comparable() {
    let src = prog(
        "func largest<T: Ord>(xs: []T) -> T\n    var best = xs[0]\n    for x in xs\n        if x.compare(best) > 0\n            best = x\n        end\n    end\n    return best\nend\n",
        r#"let nan = 0.0 / 0.0
print("{largest([1.5, 9.25, 4.0])}")
let r = largest([1.0, nan, 3.0])
print("{r == r}")
print("{nan.compare(nan)} {nan.compare(1.0)} {1.0.compare(nan)} {(0.0).compare(-0.0)}")"#,
    );
    let Some((out, code)) = run(&src) else { return };
    assert_eq!(out, "9.25\nfalse\n0 1 -1 0\n");
    assert_eq!(code, 0);
}

#[test]
fn containers() {
    let src = prog(
        "",
        r#"print("{[1, 2] == [1, 2]} {[1, 2] < [1, 3]} {[1] < [1, 0]} {[1, 2] != [1, 2]}")
let m1: Map<text, int> = { "a": 1, "b": 2 }
let m2: Map<text, int> = { "b": 2, "a": 1 }
let m3: Map<text, int> = { "b": 3, "a": 1 }
print("{m1 == m2} {m1 == m3}")
let xs: []P = [P { x: 1, y: 1 }]
let ys: []P = [P { x: 1, y: 1 }]
print("{xs == ys}")"#,
    );
    let Some((out, code)) = run(&src) else { return };
    assert_eq!(out, "true true true false\ntrue false\ntrue\n");
    assert_eq!(code, 0);
}

#[test]
fn lists_and_optionals_as_keys() {
    let src = prog(
        "",
        r#"let m: Map<[]int, text> = {}
m[[1, 2, 3]] = "a"
print("{m[[1, 2, 3]]} {m.has([1, 2])}")
let o: Map<P?, int> = {}
o[P { x: 1, y: 2 }] = 7
print("{o[P { x: 1, y: 2 }]}")"#,
    );
    let Some((out, code)) = run(&src) else { return };
    assert_eq!(out, "a false\n7\n");
    assert_eq!(code, 0);
}

#[test]
fn maps_are_not_ordered() {
    let src = prog("", "let a: Map<text, int> = {}\nlet b: Map<text, int> = {}\nprint(\"{a < b}\")");
    let e = compile_error(&src);
    assert!(e.contains("a map has no order"), "{}", e);
}

#[test]
fn identity() {
    let src = r#"package main

#[shared]
type Node = record
    id: int
end

func main()
    let a = Node { id: 1 }
    let b = Node { id: 1 }
    let c = a
    print("{a == b} {a == a} {a == c} {a != b}")
    let m: Map<Node, int> = {}
    m[a] = 1
    m[b] = 2
    m[c] = 3
    print("{m.len} {m[a]} {m[b]}")
end
"#;
    let Some((out, code)) = run(src) else { return };
    assert_eq!(out, "false true true true\n2 3 2\n");
    assert_eq!(code, 0);
}

#[test]
fn eq_on_a_shared_type() {
    let src = r#"package main

#[shared]
type Node = record
    id: int
    note: text
end

func (a: Node) eq(b: Node) -> bool
    return a.id == b.id
end

func (a: Node) hash() -> long
    return long(a.id)
end

func main()
    let a = Node { id: 1, note: "x" }
    let b = Node { id: 1, note: "y" }
    print("{a == b}")
    let m: Map<Node, int> = {}
    m[a] = 1
    m[b] = 2
    print("{m.len}")
end
"#;
    let Some((out, code)) = run(src) else { return };
    assert_eq!(out, "true\n1\n");
    assert_eq!(code, 0);
}

#[test]
fn no_identity_order() {
    let src = r#"package main

#[shared]
type Node = record
    id: int
end

func main()
    let a = Node { id: 1 }
    let b = Node { id: 2 }
    print("{a < b}")
end
"#;
    let e = compile_error(src);
    assert!(e.contains("Node is not Ord"), "{}", e);
}

#[test]
fn generic_maximum() {
    let src = prog(
        "func max<T: Ord>(a: T, b: T) -> T\n    if a > b\n        return a\n    end\n    return b\nend\n",
        r#"print("{max(3, 9)} {max("pear", "apple")}")
let m = max(P { x: 1, y: 5 }, P { x: 2, y: 0 })
print("{m.x} {m.y}")"#,
    );
    let Some((out, code)) = run(&src) else { return };
    assert_eq!(out, "9 pear\n2 0\n");
    assert_eq!(code, 0);
}

#[test]
fn missing_capability() {
    let src = r#"package main

type Bag = record
    items: Map<text, int>
end

func main()
    let a = Bag { items: {} }
    let b = Bag { items: {} }
    print("{a < b}")
end
"#;
    let e = compile_error(src);
    assert!(e.contains("operator '<' is not defined for Bag"), "{}", e);
    assert!(e.contains("a map has no order"), "{}", e);
}

#[test]
fn bound_too_weak() {
    let src = prog("func lt<T: Eq>(a: T, b: T) -> bool\n    return a < b\nend\n", "print(\"{lt(1, 2)}\")");
    let e = compile_error(&src);
    assert!(e.contains("operator '<'") && e.contains("T: Ord"), "{}", e);
}

#[test]
fn unbounded_comparison_suggests_eq() {
    let src = prog("func same<T>(a: T, b: T) -> bool\n    return a == b\nend\n", "print(\"{same(1, 2)}\")");
    let e = compile_error(&src);
    assert!(e.contains("operator '=='") && e.contains("T: Eq"), "{}", e);
}

#[test]
fn bounded_comparison() {
    let src = prog(
        "func same<T: Eq>(a: T, b: T) -> bool\n    return a == b\nend\n",
        "print(\"{same(1, 1)} {same(\"a\", \"b\")} {same(P { x: 1, y: 1 }, P { x: 1, y: 1 })}\")",
    );
    let Some((out, code)) = run(&src) else { return };
    assert_eq!(out, "true false true\n");
    assert_eq!(code, 0);
}

#[test]
fn not_equal_calls_eq_once() {
    let src = r#"package main

type R = record
    x: int
end

var calls = 0

func (a: R) eq(b: R) -> bool
    calls += 1
    return a.x == b.x
end

func main()
    let a = R { x: 1 }
    let b = R { x: 2 }
    print("{a != b} {calls}")
end
"#;
    let Some((out, code)) = run(src) else { return };
    assert_eq!(out, "true 1\n");
    assert_eq!(code, 0);
}

#[test]
fn derived_method_satisfies() {
    let src = prog(
        "func largest<T: Ord>(a: T, b: T) -> T\n    if a.compare(b) >= 0\n        return a\n    end\n    return b\nend\n",
        "let p = largest(P { x: 1, y: 1 }, P { x: 1, y: 2 })\nprint(\"{p.y}\")",
    );
    let Some((out, code)) = run(&src) else { return };
    assert_eq!(out, "2\n");
    assert_eq!(code, 0);
}

#[test]
fn record_keys() {
    let src = prog(
        "",
        r#"let m: Map<P, text> = {}
m[P { x: 1, y: 2 }] = "a"
print("{m[P { x: 1, y: 2 }]}")
m[P { x: 1, y: 2 }] = "b"
print("{m.len} {m[P { x: 1, y: 2 }]} {m.has(P { x: 2, y: 1 })}")
let s: Map<Shape, int> = {}
s[Circle(1)] = 1
s[Rect(1, 2)] = 2
s[Dot] = 3
print("{s[Circle(1)]} {s[Rect(1, 2)]} {s[Dot]} {s.has(Circle(2))}")
for k, v in s
    print("{v}")
end"#,
    );
    let Some((out, code)) = run(&src) else { return };
    assert_eq!(out, "a\n1 b false\n1 2 3 false\n1\n2\n3\n");
    assert_eq!(code, 0);
}

#[test]
fn unsupported_key_names_the_field() {
    let src = r#"package main

type Pt = record
    x: float
    y: float
end

func main()
    let m: Map<Pt, int> = {}
    print("{m.len}")
end
"#;
    let e = compile_error(src);
    assert!(e.contains("Pt cannot be a map key because field `x: float` is not Hash"), "{}", e);
}

// ---- compile-only checks (no program is run: Linux, the operating system and the OS libraries
// need WSL, QEMU, a window or the network, so they are audited by generating code) ----

use brasa::codegen_x64::{build_os, generate_asm_for, CompileOptions, Target};

#[test]
fn linux_output_for_general_keys_generates() {
    let src = prog(
        "",
        "let m: Map<P, text> = {}\nm[P { x: 1, y: 2 }] = \"a\"\nlet s: Map<Shape, int> = {}\ns[Circle(1)] = 1\nprint(m.len + s.len)\nprint(P { x: 1, y: 2 } < P { x: 1, y: 3 })",
    );
    let file = parse_source(&src).unwrap();
    let asm = generate_asm_for(&file, &CompileOptions::default(), Target::Linux).unwrap();
    assert!(asm.contains("B_maphash_") && asm.contains("B_mapeq_") && asm.contains("rt_map_new"));
    let win = generate_asm_for(&file, &CompileOptions::default(), Target::Windows).unwrap();
    assert!(win.contains("B_maphash_") && win.contains("rt_dv_text_cmp") == false);
}

#[test]
fn the_os_libraries_and_kernel_still_compile() {
    for lib in ["net", "fs", "sys", "task", "tls", "ui"] {
        let src = format!("package main\nuse {}\nuse task\nfunc main()\n    print(1)\nend\n", lib);
        let file = parse_source(&src).unwrap();
        for target in [Target::Windows, Target::Linux] {
            match generate_asm_for(&file, &CompileOptions::default(), target) {
                Ok(_) => {}
                Err(e) if target == Target::Linux && e.contains("not available on Linux") => {}
                Err(e) => panic!("{} {:?}: {}", lib, target, e),
            }
        }
    }
    let boot = parse_source(&std::fs::read_to_string("os/boot.br").unwrap()).unwrap();
    let kernel = parse_source(&std::fs::read_to_string("os/kernel.br").unwrap()).unwrap();
    build_os(&boot, &kernel).unwrap();
}

#[test]
fn the_derive_example_runs() {
    let src = std::fs::read_to_string("examples/derive.br").unwrap();
    let Some((out, code)) = run(&src) else { return };
    assert_eq!(code, 0, "{}", out);
    for line in ["apples: 3", "pears: 5", "pair seen: true", "latest: 2.1.0", "bigger: 9", "same user: true", "users: 1"] {
        assert!(out.contains(line), "missing `{}` in:\n{}", line, out);
    }
}
