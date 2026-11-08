//! `int` and `long`: aliases of `i32` and `i64`, literal defaults, lossless widening, conversions,
//! type names in errors and the element ceiling. Programs are compiled to native executables and
//! run where that is needed (skipped when the Visual Studio build tools are missing).

use zila::codegen_x64::generate_asm;
use zila::{compile_to_native, parse_source};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

fn main_of(body: &str) -> String {
    let indented: String = body.lines().map(|l| format!("    {}\n", l)).collect();
    format!("package main\n\nfunc main()\n{}end\n", indented)
}

/// Compiles `src` to assembly only.
fn asm(src: &str) -> Result<String, String> {
    let f = parse_source(src).map_err(|d| d.msg)?;
    generate_asm(&f)
}

fn ok(src: &str) {
    if let Err(e) = asm(src) {
        panic!("should compile: {}\n{}", e, src);
    }
}

fn fails(src: &str, wanted: &[&str]) {
    let e = asm(src).expect_err(&format!("should not compile:\n{}", src));
    for w in wanted {
        assert!(e.contains(w), "expected {:?} in: {}", w, e);
    }
}

fn run(src: &str) -> Option<(String, i32)> {
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = std::env::temp_dir().join(format!("zila-int-test-{}-{}.exe", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
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
    let text = String::from_utf8_lossy(&out.stdout).replace('\r', "") + &String::from_utf8_lossy(&out.stderr).replace('\r', "");
    Some((text, out.status.code().unwrap_or(-1)))
}

#[test]
fn int_and_long_are_the_names_of_i32_and_i64() {
    ok(&main_of("let a: int = 1\nlet b: i32 = a\nlet c: long = 1\nlet d: i64 = c\nlet e: float = 1.5\nlet f: f64 = e\nlet g: int = b"));
    // they are the same types in parameters, returns, fields and generics
    ok("package main\ntype R = record\n    n: int\n    m: long\nend\nfunc f(a: i32, b: i64) -> R\n    return R { n: a, m: b }\nend\nfunc g(r: R) -> i32\n    return r.n\nend\nfunc main()\n    let l: []int = []\n    let k: []i32 = l\n    let r = f(1, 2)\n    let x: int = g(r)\nend\n");
}

#[test]
fn narrowing_is_explicit() {
    fails(&main_of("let c: long = 1\nlet a: int = c"), &["expected int, found long", "int(x)"]);
    ok(&main_of("let c: long = 1\nlet a: int = int(c)"));
}

#[test]
fn literals_default_to_int_and_adapt() {
    ok(&main_of("let x = 40\nlet y: long = 3000000000\nlet z: u8 = 255\nlet w: i64 = 40"));
    fails(&main_of("let z = 3000000000"), &["out of range", "long"]);
    fails(&main_of("let x: int = 2147483648"), &["out of range"]);
    // `let x = 40` is an int
    fails(&main_of("let x = 40\nlet t: text = x"), &["expected text, found int"]);
}

#[test]
fn errors_show_int_and_long() {
    fails("package main\nfunc f() -> int\n    return \"a\"\nend\nfunc main()\nend\n", &["expected int, found text"]);
    fails(&main_of("let c: long = 1\nlet t: text = c"), &["expected text, found long"]);
    fails(&main_of("let a: u16 = 1\nlet t: text = a"), &["found u16"]);
    // aliases print the everyday name whichever spelling the program used
    fails(&main_of("let a: i32 = 1\nlet t: text = a"), &["found int"]);
    fails(
        "package main\ntype Stack<T> = record\n    items: []T\nend\nfunc take(s: Stack<long>)\nend\nfunc main()\n    let a: Stack<int> = Stack { items: [] }\n    take(a)\nend\n",
        &["expected Stack<long>, found Stack<int>"],
    );
}

#[test]
fn lossless_widening_is_implicit() {
    // a call, a return, an initializer, a field and an operand
    ok("package main\ntype R = record\n    n: long\nend\nfunc take(x: long) -> long\n    return x\nend\nfunc give(a: int) -> long\n    return a\nend\nfunc main()\n    let a: int = 1\n    let x: long = a\n    let r = R { n: a }\n    let s = take(a)\n    let t = give(a)\n    let u: i64 = a + x\nend\n");
    // the sum of an int and a long is a long
    ok(&main_of("let a: int = 1\nlet b: long = 2\nlet c: long = a + b\nlet d: long = b * a"));
    fails(&main_of("let a: int = 1\nlet b: long = 2\nlet c: int = a + b"), &["expected int, found long"]);
    // unsigned into a wider signed type, small signed into wider signed
    ok(&main_of("let a: u32 = 1\nlet b: i64 = a\nlet c: u8 = 1\nlet d: i16 = c\nlet e: i8 = 1\nlet f: int = e\nlet g: u16 = 1\nlet h: u64 = g"));
    // a comparison between an int and a long
    ok(&main_of("let a: int = 1\nlet b: long = 2\nlet c = a < b"));
}

#[test]
fn nothing_lossy_is_implicit() {
    fails(&main_of("let c: long = 1\nlet a: int = c"), &["expected int, found long"]);
    fails(&main_of("let c: u64 = 1\nlet a: int = c"), &["expected int, found u64"]);
    fails(&main_of("let c: i32 = 1\nlet a: u64 = c"), &["expected u64, found int"]);
    fails(&main_of("let c: u32 = 1\nlet a: int = c"), &["expected int, found u32"]);
    fails(&main_of("let c: u32 = 1\nlet a: u8 = c"), &["found u32"]);
    fails(&main_of("let c = 1.5\nlet a: int = c"), &["expected int, found float"]);
    fails(&main_of("let c: int = 1\nlet a: float = c"), &["expected float, found int"]);
    fails(&main_of("let a: u32 = 1\nlet b: int = 2\nlet c = a + b"), &["convert"]);
}

#[test]
fn generics_and_signatures_need_exact_types() {
    fails(
        "package main\nfunc max<T>(a: T, b: T) -> T\n    return a\nend\nfunc main()\n    let a: int = 1\n    let b: long = 2\n    let m = max(a, b)\nend\n",
        &["T is both int and long"],
    );
    // a function value needs the exact signature
    fails("package main\nfunc g(x: int)\nend\nfunc main()\n    let f: func(long) = g\nend\n", &["func(long)", "func(int)"]);
    ok("package main\nfunc g(x: int)\nend\nfunc main()\n    let f: func(i32) = g\nend\n");
    // an interface method needs the exact return type
    fails(
        "package main\ntype Sized = interface\n    func size(self) -> long\nend\ntype B = record\n    n: int\nend\nfunc (b: B) size() -> int\n    return b.n\nend\nfunc show<T: Sized>(x: T)\nend\nfunc main()\n    show(B { n: 1 })\nend\n",
        &["does not satisfy Sized"],
    );
}

#[test]
fn lengths_and_indices_are_int() {
    ok("package main\nfunc main()\n    let xs = [1, 2, 3]\n    var sum = 0\n    for i in 0..xs.len\n        sum += xs[i]\n    end\n    let n: int = xs.len\n    let t: int = \"abc\".len\n    let m: Map<text, int> = {}\n    let k: int = m.len\nend\n");
}

#[test]
fn int_arithmetic_wraps_at_32_bits_before_widening() {
    let src = main_of("let a: int = 65536\nlet b: int = 65536\nlet wrapped: long = a * b\nlet exact = long(a) * long(b)\nprintln(wrapped, exact)\nlet big: int = 2147483647\nprintln(big + 1)");
    let Some((out, code)) = run(&src) else { return };
    assert_eq!(out, "0 4294967296\n-2147483648\n");
    assert_eq!(code, 0);
}

#[test]
fn int_long_and_float_conversions() {
    let src = main_of("let f = 3.99\nprintln(int(f), int(-f), long(7), long(3000000000.0), float(7) / 2.0)\nlet u: u32 = 4000000000\nprintln(long(u), int(u))");
    let Some((out, code)) = run(&src) else { return };
    assert_eq!(out, "3 -3 7 3000000000 3.5\n4000000000 -294967296\n");
    assert_eq!(code, 0);
}

#[test]
fn a_float_outside_the_int_range_stops_the_program() {
    // 2^31 does not fit an int but fits a long
    let src = main_of("let big = 2147483648.0\nprintln(long(big))\nlet x = int(big)\nprintln(\"unreachable\")");
    let Some((out, code)) = run(&src) else { return };
    assert_eq!(out, "2147483648\nabort: main.zila:6: float out of range\n");
    assert_eq!(code, 101);
}

#[test]
fn the_c_names_keep_the_c_sizes() {
    // c.int and c.long are 32 bits (Windows C ABI); c.longlong is 64
    ok("package main\nuse c\nextern \"C\" func f(a: c.int, b: c.long, c: c.longlong) -> c.int\nfunc main()\nend\n");
}

#[test]
fn file_sizes_and_offsets_are_long() {
    let fs = std::fs::read_to_string("std/fs.zila").unwrap();
    assert!(fs.contains("func file_size(f: File, path: text) -> long or error"));
    assert!(fs.contains("count: long, offset: long"));
}

fn ceiling_program(body: &str) -> String {
    main_of(&format!("__set_max_len(3)\n{}", body))
}

#[test]
fn a_list_past_the_ceiling_stops_the_program() {
    let src = ceiling_program("let xs: []int = []\nfor i in 0..10\n    xs.add(i)\n    println(xs.len)\nend");
    let Some((out, code)) = run(&src) else { return };
    assert_eq!(out, "1\n2\n3\nabort: list, map or text is too long\n");
    assert_eq!(code, 101);
}

#[test]
fn a_text_past_the_ceiling_stops_the_program() {
    let src = ceiling_program("var s = \"ab\"\nprintln(s.len)\ns = s + \"cd\"\nprintln(\"unreachable\")");
    let Some((out, code)) = run(&src) else { return };
    assert_eq!(out, "2\nabort: list, map or text is too long\n");
    assert_eq!(code, 101);
}

#[test]
fn a_map_past_the_ceiling_stops_the_program() {
    let src = ceiling_program("let m: Map<int, int> = {}\nfor i in 0..10\n    m[i] = i\n    println(m.len)\nend");
    let Some((out, code)) = run(&src) else { return };
    assert_eq!(out, "1\n2\n3\nabort: list, map or text is too long\n");
    assert_eq!(code, 101);
}

#[test]
fn the_ceiling_does_not_stop_a_collection_that_fits() {
    let src = ceiling_program("let xs: []int = []\nxs.add(1)\nxs.add(2)\nxs.add(3)\nlet m: Map<int, int> = {}\nm[1] = 1\nm[2] = 2\nm[2] = 3\nprintln(xs.len, m.len, \"abc\".len)");
    let Some((out, code)) = run(&src) else { return };
    assert_eq!(out, "3 2 3\n");
    assert_eq!(code, 0);
}

#[test]
fn a_literal_on_the_left_takes_the_type_on_the_right() {
    ok(&main_of("let a: u32 = 7\nlet b = 0 - a\nlet c = (1 + 2) * a\nlet d = 5 < a\nlet e: u8 = 3\nlet f = 250 - e"));
    ok(&main_of("let a: long = 7\nlet b = 3000000000 - a"));
}
