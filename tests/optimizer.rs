//! The optimizer: optimized and unoptimized programs must behave the same, folding and
//! branch fusion must show in the generated assembly, and the benchmarks must improve.
//! Programs are compiled to native executables and run (skipped when the Visual Studio build
//! tools are missing).

use brasa::codegen_x64::{compile_to_native_with, generate_asm_with, CompileOptions};
use brasa::parse_source;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

static N: AtomicUsize = AtomicUsize::new(0);

const ON: CompileOptions = CompileOptions { optimize: true, test_mode: false };
const OFF: CompileOptions = CompileOptions { optimize: false, test_mode: false };

/// Compiles and runs `src`; `None` when there is no native toolchain.
fn run_with(src: &str, options: &CompileOptions) -> Option<(String, i32, Duration)> {
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = std::env::temp_dir().join(format!("brasa-opt-test-{}-{}.exe", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    match compile_to_native_with(&file, exe.to_str().unwrap(), options) {
        Ok(()) => {}
        Err(e) if e.contains("could not find Visual Studio") => {
            eprintln!("no native toolchain: skipping: {}", e);
            return None;
        }
        Err(e) => panic!("compile error (optimize = {}): {}", options.optimize, e),
    }
    let started = Instant::now();
    let out = Command::new(&exe).output().unwrap();
    let took = started.elapsed();
    let _ = std::fs::remove_file(&exe);
    let text = String::from_utf8_lossy(&out.stdout).replace('\r', "") + &String::from_utf8_lossy(&out.stderr).replace('\r', "");
    Some((text, out.status.code().unwrap_or(-1), took))
}

/// Runs the program both ways and checks that they agree; returns the (shared) output.
fn same(src: &str) -> Option<String> {
    let (a, ca, _) = run_with(src, &OFF)?;
    let (b, cb, _) = run_with(src, &ON)?;
    if a != b {
        let dir = std::env::temp_dir();
        let _ = std::fs::write(dir.join("brasa-opt-diff.br"), src);
        let first = a.lines().zip(b.lines()).position(|(x, y)| x != y).unwrap_or(0);
        panic!(
            "optimized output differs from unoptimized output at line {}: {:?} (unoptimized) vs {:?} (optimized); program saved to {}",
            first + 1,
            a.lines().nth(first),
            b.lines().nth(first),
            dir.join("brasa-opt-diff.br").display()
        );
    }
    assert_eq!(ca, cb, "exit codes differ");
    Some(a)
}

fn asm(src: &str, options: &CompileOptions) -> String {
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    generate_asm_with(&file, options).unwrap()
}

/// The instruction lines of one procedure (labels, comments and blank lines left out).
fn proc_body(asm: &str, name: &str) -> Vec<String> {
    let start = format!("{} proc", name);
    let end = format!("{} endp", name);
    let mut inside = false;
    let mut out = Vec::new();
    for l in asm.lines() {
        if l.trim() == start {
            inside = true;
            continue;
        }
        if l.trim() == end {
            break;
        }
        let t = l.trim();
        if inside && !t.is_empty() && !t.ends_with(':') && !t.starts_with(';') {
            out.push(t.to_string());
        }
    }
    out
}

fn main_of(body: &str) -> String {
    let indented: String = body.lines().map(|l| format!("    {}\n", l)).collect();
    format!("package main\n\nfunc main()\n{}end\n", indented)
}

pub const FIB: &str = r#"package main

func fib(n: int) -> int
    if n < 2
        return n
    end
    return fib(n - 1) + fib(n - 2)
end

func main()
    print(fib(38))
end
"#;

pub const LOOP: &str = r#"package main

func main()
    var sum = 0
    for i in 0..100000000
        sum += i * 2 + 1
    end
    print(sum)
end
"#;

pub const SORT: &str = r#"package main

use list

func main()
    var seed: long = 12345
    let xs: []long = []
    for i in 0..100000
        seed = (seed * 1103515245 + 12345) % 2147483648
        xs.add(seed)
    end
    let s = list.sort_by(xs, func(a, b) a < b end)
    print("{s[0]} {s[99999]}")
end
"#;

pub const JOIN: &str = r#"package main

use text
use list

func main()
    let pieces = list.map(list.range(0, 100000), func(n) "{n}" end)
    let joined = text.join(pieces, ",")
    print(text.split(joined, ",").len)
end
"#;

/// Compiles `src` once and times it: one run to get past the first-run scan Windows does on a
/// new executable, then the best of three.
fn bench(src: &str, options: &CompileOptions) -> Option<Duration> {
    let file = parse_source(src).unwrap();
    let exe = std::env::temp_dir().join(format!("brasa-bench-{}-{}.exe", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    match compile_to_native_with(&file, exe.to_str().unwrap(), options) {
        Ok(()) => {}
        Err(e) if e.contains("could not find Visual Studio") => return None,
        Err(e) => panic!("compile error: {}", e),
    }
    let _ = Command::new(&exe).output().unwrap();
    let mut best = Duration::MAX;
    for _ in 0..3 {
        let started = Instant::now();
        let out = Command::new(&exe).output().unwrap();
        assert_eq!(out.status.code(), Some(0));
        best = best.min(started.elapsed());
    }
    let _ = std::fs::remove_file(&exe);
    Some(best)
}

fn benchmarks() -> [(&'static str, &'static str, &'static str); 5] {
    [
        ("fib", FIB, "B_main_fib"),
        ("loop", LOOP, "B_main_main"),
        ("sort", SORT, "B_list_sort_by__i64"),
        ("join", JOIN, "B_text_join"),
        ("helpers", HELPERS, "B_main_main"),
    ]
}

/// Prints the instruction counts and run times of the benchmarks.
#[test]
fn benchmark_report() {
    for (name, src, func) in benchmarks() {
        let before = proc_body(&asm(src, &OFF), func).len();
        let after = proc_body(&asm(src, &ON), func).len();
        let (Some(t0), Some(t1)) = (bench(src, &OFF), bench(src, &ON)) else { return };
        eprintln!("BENCH {:<5} {:>5} -> {:>5} instructions in {}; {:>8.3?} -> {:>8.3?}", name, before, after, func, t0, t1);
        if name == "fib" {
            assert!(after * 100 <= before * 60, "fib code only {} -> {}", before, after);
            assert!(t1.as_secs_f64() <= t0.as_secs_f64() * 0.75, "fib {:?} -> {:?}", t0, t1);
        }
        if name == "loop" {
            assert!(t1.as_secs_f64() <= t0.as_secs_f64() * 0.75, "loop {:?} -> {:?}", t0, t1);
        }
        if name == "helpers" {
            assert!(t1.as_secs_f64() <= t0.as_secs_f64() * 0.75, "helpers {:?} -> {:?}", t0, t1);
        }
    }
}

#[test]
fn constants_are_folded() {
    let src = main_of("print(2 * 3 + 1)\nprint(1.5 + 2.25)\nprint(not true)\nprint(10 - 3 > 5 and 2 < 3)");
    let on = asm(&src, &ON);
    let off = asm(&src, &OFF);
    let body_on = proc_body(&on, "B_main_main");
    let body_off = proc_body(&off, "B_main_main");
    assert!(body_on.iter().any(|l| l == "mov rax, 7"), "{:?}", body_on);
    assert!(body_on.iter().any(|l| l.starts_with("mov rax, ") && l.contains("0400E000000000000h")), "{:?}", body_on);
    for op in ["imul", "addsd", "setl", "setg"] {
        assert!(!body_on.iter().any(|l| l.starts_with(op)), "{} left in {:?}", op, body_on);
    }
    assert!(body_off.iter().any(|l| l.starts_with("imul")));
    if let Some(out) = same(&src) {
        assert_eq!(out, "7
3.75
false
true
");
    }
}

#[test]
fn integer_folding_wraps_like_the_program() {
    let Some(out) = same(&main_of(
        r#"let big: long = 9223372036854775807
print(big + 1)
let a: u8 = 200 + 100
let b: i8 = 100 + 100
let c: u32 = 0 - 1
let d: i32 = 2147483647 + 1
let e: u64 = 0 - 1
println(a, b, c, d, e)
println(1 << 65, 5 >> 1, 0 - 8 >> 1, 7 bitand 3, 5 bitor 2, 6 bitxor 3, bitnot 0)
let f: u64 = 18446744073709551615 / 2
let g: u8 = 255 % 7
println(f, g, 0 - 7 / 2, 0 - 7 % 3, 7 / 7)"#,
    )) else {
        return;
    };
    assert!(out.starts_with("-9223372036854775808
44 -56 4294967295 -2147483648 -1
"), "{}", out);
}

#[test]
fn division_by_a_constant_zero_still_fails_at_run_time() {
    for body in ["print(1 / 0)", "print(5 % 0)", "let x = 1
print(x / (2 - 2))"] {
        let Some((a, ca, _)) = run_with(&main_of(body), &OFF) else { return };
        let (b, cb, _) = run_with(&main_of(body), &ON).unwrap();
        assert_eq!((a.clone(), ca), (b, cb), "{}", body);
        assert_eq!(ca, 101, "{} should abort: {}", body, a);
        assert!(a.contains("abort: main.br:") && a.contains(": division by zero"), "{}: {}", body, a);
    }
}

#[test]
fn float_folding_follows_ieee() {
    let Some(out) = same(&main_of(
        r#"print(0.0 / 0.0)
print(1.0 / 0.0)
print(-1.0 / 0.0)
print(-0.0 + 0.0)
print(0.1 + 0.2)
print(1.5 * 4.0 - 0.25)
let f: f32 = 0.1 + 0.2
print(f)
print(1.5 < 2.5, 2.5 <= 2.5, 3.0 > 3.5, 0.0 / 0.0 == 0.0 / 0.0, 0.0 / 0.0 != 0.0 / 0.0, 0.0 / 0.0 < 1.0)
print(-(1.5) * -2.0)"#,
    )) else {
        return;
    };
    assert_eq!(out, "NaN
inf
-inf
0.0
0.30000000000000004
5.75
0.3
true true false false true false
3.0
");
}

/// Conditions of every shape and comparison over each numeric type, with values at the edges
/// (zero, the largest and smallest, NaN, infinities, signed zero): the optimized jumps must pick
/// the same branches as the unoptimized comparisons.
fn condition_program() -> String {
    let mut src = String::from("package main\n\n");
    let ops = ["==", "!=", "<", "<=", ">", ">="];
    let types: [(&str, &str, &str); 8] = [
        ("int", "[0, 1, 3, 4, 0 - 1, 0 - 5, 2147483647, 0 - 2147483647]", "3"),
        ("long", "[0, 1, 3, 4, 0 - 1, 0 - 5, 9223372036854775807, 0 - 9223372036854775807]", "3"),
        ("u8", "[0, 1, 3, 4, 200, 255]", "3"),
        ("u32", "[0, 1, 3, 4, 4000000000, 4294967295]", "3"),
        ("i32", "[0, 1, 3, 4, 0 - 1, 2147483647, 0 - 2147483647]", "3"),
        ("u64", "[0, 1, 3, 4, 9223372036854775807]", "3"),
        ("float", "[0.0, 1.0, 3.0, 4.0, 0.0 - 1.0, 0.0 / 0.0, 1.0 / 0.0, 0.0 - 1.0 / 0.0, 0.0 - 0.0, 1.0e308]", "3.0"),
        ("f32", "[0.0, 1.0, 3.0, 4.0, 0.0 - 1.0, 0.0 / 0.0, 3.4e38]", "3.0"),
    ];
    for (t, _, lit) in types {
        src.push_str(&format!("func check_{}(a: {}, b: {}) -> text\n    var s = \"\"\n", t, t, t));
        for op in ops {
            let mut conds: Vec<String> = vec![
                format!("a {} b", op),
                format!("a {} {}", op, lit),
                format!("not (a {} b)", op),
                format!("a {} b and a {} {}", op, op, lit),
                format!("a {} b or b {} {}", op, op, lit),
                format!("not (a {} b or a {} {})", op, op, lit),
                format!("(a {} b and not (b {} {})) or a {} {}", op, op, lit, op, lit),
            ];
            for c in conds.drain(..) {
                src.push_str(&format!("    if {}\n        s = s + \"1\"\n    else\n        s = s + \"0\"\n    end\n", c));
            }
            src.push_str(&format!("    var n = 0\n    while a {} b and n < 2\n        n += 1\n    end\n    s = s + \"{{n}}\"\n", op));
        }
        src.push_str("    return s\nend\n\n");
    }
    src.push_str("func main()\n");
    for (t, vals, _) in types {
        src.push_str(&format!("    let v_{}: []{} = {}\n    for a in v_{}\n        for b in v_{}\n            println(check_{}(a, b))\n        end\n    end\n", t, t, vals, t, t, t));
    }
    src.push_str("end\n");
    src
}

#[test]
fn fused_conditions_pick_the_same_branches() {
    let Some(out) = same(&condition_program()) else { return };
    assert!(out.lines().count() > 200, "{}", out.lines().count());
}

#[test]
fn comparisons_in_conditions_jump_on_the_flags() {
    let on = asm(FIB, &ON);
    let body = proc_body(&on, "B_main_fib");
    assert!(!body.iter().any(|l| l.starts_with("setl") || l.starts_with("movzx")), "{:?}", body);
    assert!(body.iter().any(|l| l == "jge ifnext_4" || l.starts_with("jge ")), "{:?}", body);
    let f = asm(&condition_program(), &ON);
    let check = proc_body(&f, "B_main_check_float");
    assert!(check.iter().any(|l| l.starts_with("jbe ")) && check.iter().any(|l| l.starts_with("jp ")), "float jumps missing");
}

#[test]
fn inline_assembly_is_left_alone() {
    let src = r#"package main

func raw() -> int
    var r = 0
    unsafe
        asm
            mov rax, 5
            jmp over
            mov rax, 99
            over:
            mov rax, rax
        end
    end
    return r
end

func main()
    print(raw())
end
"#;
    let on = proc_body(&asm(src, &ON), "B_main_raw");
    let off = proc_body(&asm(src, &OFF), "B_main_raw");
    assert_eq!(on, off, "a function with an asm block must not change");
}

#[test]
fn peephole_removes_dead_jumps_and_reloads() {
    let src = r#"package main

func pick(n: int) -> int
    if n > 5
        return 1
    else
        return 2
    end
end

func main()
    print(pick(3))
end
"#;
    let on = proc_body(&asm(src, &ON), "B_main_pick");
    let off = proc_body(&asm(src, &OFF), "B_main_pick");
    assert!(on.len() < off.len(), "{} vs {}", on.len(), off.len());
    // no jump to the next instruction and nothing between a jump and a label
    let text = asm(src, &ON);
    let lines: Vec<&str> = text.lines().collect();
    for w in lines.windows(2) {
        if let Some(target) = w[0].trim().strip_prefix("jmp ") {
            assert_ne!(w[1].trim(), format!("{}:", target), "jump to the next line: {}", target);
        }
    }
    same(src);
}

#[test]
fn only_functions_that_can_own_heap_values_zero_their_slots() {
    let src = r#"package main

func numeric(n: int) -> int
    var t = 0
    for i in 0..n
        t += i
    end
    return t
end

func with_text(n: int) -> int
    let s = "x{n}"
    return s.len
end

func main()
    print(numeric(10) + with_text(5))
end
"#;
    let on = asm(src, &ON);
    let zeroing = |asm: &str, f: &str| proc_body(asm, f).iter().any(|l| l.starts_with("jb Z_"));
    assert!(!zeroing(&on, "B_main_numeric"), "numeric function still zeroes its slots");
    assert!(zeroing(&on, "B_main_with_text"), "a function with a text must zero its slots");
    assert!(zeroing(&asm(src, &OFF), "B_main_numeric"));
    assert_eq!(same(src).unwrap_or_else(|| "47\n".into()), "47\n");
}

#[test]
fn heap_values_in_small_functions_do_not_leak() {
    let Some(out) = same(
        r#"package main

type P = record
    name: text
end

func a(n: int) -> int
    let l = [n, n + 1]
    return l.len
end

func b(n: int) -> int
    let m: Map<text, int> = { "k": n }
    return m["k"]
end

func c(n: int) -> int
    let p = P { name: "x{n}" }
    return p.name.len
end

func d(n: int) -> int
    let f = func(x: int) x + n end
    return f(1)
end

func e(n: int) -> int or error
    if n > 100
        return error("big")
    end
    return n
end

func g(n: int) -> int
    defer print("")
    return n
end

func main()
    var before = 0
    for i in 0..300
        let x = a(i) + b(i) + c(i) + d(i) + (e(i) ?? 0) + g(i) + (e(1000) ?? 7)
        if x < 0
            print("bad")
        end
        if i == 1
            before = live_objects()
        end
        if i == 299
            print("grew {live_objects() - before}")
        end
    end
end
"#,
    ) else {
        return;
    };
    assert!(out.ends_with("grew 0\n"), "{}", out);
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// A random integer expression of type `t` over the variables `a` and `b` and literals.
fn int_expr(r: &mut Rng, depth: u32, t: &str) -> String {
    if depth == 0 || r.below(5) == 0 {
        return match r.below(4) {
            0 => "a".into(),
            1 => "b".into(),
            2 => format!("{}", r.below(7)),
            _ => format!("{}", [0u64, 1, 2, 3, 100, 127, 200, 255][r.below(8) as usize]),
        };
    }
    let a = int_expr(r, depth - 1, t);
    let b = int_expr(r, depth - 1, t);
    match r.below(12) {
        0 => format!("({} + {})", a, b),
        1 => format!("({} - {})", a, b),
        2 => format!("({} * {})", a, b),
        3 => format!("({} bitand {})", a, b),
        4 => format!("({} bitor {})", a, b),
        5 => format!("({} bitxor {})", a, b),
        6 => format!("({} << {})", a, r.below(70)),
        7 => format!("({} >> {})", a, r.below(70)),
        8 => format!("({} / {})", a, 2 + r.below(8)),
        9 => format!("({} % {})", a, 2 + r.below(8)),
        10 => format!("(0 - {})", a),
        _ => format!("({} bitxor {})", a, b),
    }
}

fn bool_expr(r: &mut Rng, depth: u32, operand: &dyn Fn(&mut Rng) -> String) -> String {
    if depth == 0 || r.below(3) == 0 {
        let op = ["<", "<=", ">", ">=", "==", "!="][r.below(6) as usize];
        return format!("({} {} {})", operand(r), op, operand(r));
    }
    match r.below(3) {
        0 => format!("({} and {})", bool_expr(r, depth - 1, operand), bool_expr(r, depth - 1, operand)),
        1 => format!("({} or {})", bool_expr(r, depth - 1, operand), bool_expr(r, depth - 1, operand)),
        _ => format!("(not {})", bool_expr(r, depth - 1, operand)),
    }
}

fn float_expr(r: &mut Rng, depth: u32) -> String {
    if depth == 0 || r.below(5) == 0 {
        return match r.below(4) {
            0 => "a".into(),
            1 => "b".into(),
            2 => format!("k{}", r.below(3)),
            _ => format!("k{}", 3 + r.below(3)),
        };
    }
    let a = float_expr(r, depth - 1);
    let b = float_expr(r, depth - 1);
    match r.below(5) {
        0 => format!("({} + {})", a, b),
        1 => format!("({} - {})", a, b),
        2 => format!("({} * {})", a, b),
        3 => format!("({} / {})", a, b),
        _ => format!("(-{})", a),
    }
}

/// A few thousand random expressions, written both with variables (computed by the program)
/// and with the variables replaced by literals (computed by the compiler): all must agree.
fn random_case(seed: u64) {
    let mut r = Rng(seed);
    let mut src = String::from("package main\n\n");
    let mut mains = String::from("func main()\n");
    let int_types = ["int", "u8", "i32", "u32", "u64"];
    let int_vals = ["0", "1", "5", "127", "200", "255"];
    for (ti, t) in int_types.iter().enumerate() {
        src.push_str(&format!("func run_{}(a: {}, b: {})\n", ti, t, t));
        for _ in 0..120 {
            let e = int_expr(&mut r, 3, t);
            src.push_str(&format!("    println({})\n", e));
            let c = bool_expr(&mut r, 2, &|r: &mut Rng| int_expr(r, 2, t));
            src.push_str(&format!("    if {}\n        println(1)\n    else\n        println(0)\n    end\n", c));
        }
        src.push_str("end\n\n");
        for a in int_vals {
            for b in int_vals {
                mains.push_str(&format!("    run_{}({}, {})\n", ti, a, b));
            }
        }
    }
    for (ti, t) in ["float", "f32"].iter().enumerate() {
        src.push_str(&format!("func runf_{}(a: {}, b: {})\n", ti, t, t));
        for (k, v) in ["0.5", "1.5", "2.5", "0.1", "3.0", "1.0e30"].iter().enumerate() {
            src.push_str(&format!("    let k{}: {} = {}
", k, t, v));
        }
        for _ in 0..120 {
            let e = float_expr(&mut r, 3);
            src.push_str(&format!("    println({})\n", e));
            let c = bool_expr(&mut r, 2, &|r: &mut Rng| float_expr(r, 2));
            src.push_str(&format!("    if {}\n        println(1)\n    else\n        println(0)\n    end\n", c));
        }
        src.push_str("end\n\n");
        let nan = "(0.0 / 0.0)";
        for a in ["0.0", "1.5", "3.0", nan, "(1.0 / 0.0)"] {
            for b in ["0.0", "2.5", nan, "(0.0 - 1.0)"] {
                mains.push_str(&format!("    runf_{}({}, {})\n", ti, a, b));
            }
        }
    }
    mains.push_str("end\n");
    src.push_str(&mains);
    let Some(out) = same(&src) else { return };
    assert!(out.lines().count() > 20000, "only {} lines", out.lines().count());
}

#[test]
fn random_expressions_agree() {
    random_case(0x9E3779B97F4A7C15);
}

#[test]
fn random_expressions_agree_again() {
    random_case(0x2545F4914F6CDD1D);
    random_case(0xD1B54A32D192ED03);
}

// ---- variables in registers ----

const REGS: [&str; 5] = ["rbx", "r12", "r13", "r14", "r15"];

fn uses_reg(body: &[String]) -> bool {
    body.iter().any(|l| REGS.iter().any(|r| l.split(|c: char| !c.is_alphanumeric()).any(|w| w == *r)))
}

const REG_PROGRAM: &str = r#"package main

func work(n: int) -> int
    var a = 1
    var b = 2
    var c = 3
    var d = 4
    var e = 5
    var f = 6
    var total = 0
    for i in 0..n
        a = a + i
        b = b + a
        c = c + b
        d = d + c
        e = e + d
        f = f + e
        if i % 7 == 3
            continue
        end
        total += a + f
        if total > 1000000000
            break
        end
    end
    return total + a + b + c + d + e + f
end

func rec(n: int) -> int
    var s = 0
    var k = 0
    while k < n
        s += k * n
        k += 1
    end
    if n == 0
        return s
    end
    return s + rec(n - 1) + k
end

func early(n: int) -> int
    var k = 0
    while k < 1000
        k += 1
        if k * k > n
            return k
        end
    end
    return -1
end

func bump(n: int) -> int
    var k = 0
    while k < 100
        n += 3
        k += 1
    end
    return n
end

func main()
    print(work(50))
    print(rec(40))
    var x = 0
    for j in 0..10
        x += work(j) + rec(j) + early(j * 100) + bump(j)
    end
    print(x)
end
"#;

#[test]
fn register_variables_agree_with_memory_variables() {
    if let Some(out) = same(REG_PROGRAM) {
        assert_eq!(out.lines().count(), 3, "{}", out);
    }
}

#[test]
fn loop_variables_live_in_registers() {
    let on = proc_body(&asm(LOOP, &ON), "B_main_main");
    let off = proc_body(&asm(LOOP, &OFF), "B_main_main");
    assert!(uses_reg(&on), "{:?}", on);
    assert!(on.iter().any(|l| l.starts_with("inc r")), "{:?}", on);
    assert!(!uses_reg(&off), "--no-opt must keep variables in memory: {:?}", off);
    // the registers are saved on entry and restored on exit
    let saves = on.iter().filter(|l| l.starts_with("mov qword ptr [rbp - ") && REGS.iter().any(|r| l.ends_with(&format!(", {}", r)))).count();
    let restores = on.iter().filter(|l| l.starts_with("mov r") && l.contains(", qword ptr [rbp - ") && REGS.iter().any(|r| l.starts_with(&format!("mov {},", r)))).count();
    assert!(saves >= 1 && saves == restores, "{:?}", on);
}

#[test]
fn floats_text_and_asm_functions_keep_their_variables_in_memory() {
    let src = r#"package main

func f() -> float
    var t = 0.0
    while t < 10.0
        t += 1.0
        t += 0.5
        t -= 0.25
    end
    return t
end

func g() -> int
    var s = "a"
    var n = 0
    while n < 10
        s = s + "b"
        n += 1
        n += 0
    end
    return n
end

func main()
    print(f())
    print(g())
end
"#;
    let a = asm(src, &ON);
    assert!(!uses_reg(&proc_body(&a, "B_main_f")), "{:?}", proc_body(&a, "B_main_f"));
    // `g` has an integer loop counter, so it may use a register, but never for `s`
    if let Some(out) = same(src) {
        assert_eq!(out.lines().count(), 2, "{}", out);
        assert_eq!(out.lines().nth(1), Some("10"));
    }
}

#[test]
fn registers_survive_calls_between_functions_that_use_them() {
    let src = r#"package main

func inner(n: int) -> int
    var a = 0
    var b = 0
    for i in 0..n
        a += i
        b += a
    end
    return a + b
end

func outer(n: int) -> int
    var x = 0
    var y = 0
    for i in 0..n
        x += inner(i)
        y += i * 2
        x += y
    end
    return x * 1000 + y
end

func main()
    print(outer(30))
    print(outer(5) + outer(6))
end
"#;
    if let Some(out) = same(src) {
        assert_eq!(out.lines().count(), 2, "{}", out);
    }
}

#[test]
fn runtime_helpers_never_touch_the_registers_given_to_variables() {
    let a = asm(LOOP, &ON);
    let mut current = String::new();
    for l in a.lines() {
        let t = l.trim();
        if let Some(n) = t.strip_suffix(" proc") {
            current = n.to_string();
            continue;
        }
        if t.ends_with(" endp") {
            current.clear();
            continue;
        }
        if !current.is_empty() && !current.starts_with("B_") && !current.starts_with('L') {
            let words: Vec<&str> = t.split(|c: char| !c.is_alphanumeric()).collect();
            assert!(!REGS.iter().any(|r| words.contains(r)), "{} uses a variable register: {}", current, t);
        }
    }
}

#[test]
fn registers_survive_tasks_errors_and_shadowing() {
    let src = r#"package main
use task

func worker(id: int, n: int)
    var acc = 0
    for i in 0..n
        acc += i * id
        task.yield()
        acc += 1
    end
    print("{id}: {acc}")
end

func check(i: int) -> int or error
    if i == 7
        return error("seven")
    end
    return i
end

func total(n: int) -> int or error
    var t = 0
    for i in 0..n
        t += try check(i)
    end
    return t
end

func shadow(n: int) -> int
    var x = 1
    var s = 0
    for i in 0..n
        s += x
        if i > 2
            var x = 100
            x += i
            s += x
        end
    end
    return s + x
end

func main() -> error
    spawn worker(1, 50)
    spawn worker(2, 50)
    worker(3, 50)
    task.yield()
    task.yield()
    match total(5)
        ok(v) -> print("total {v}")
        err(e) -> print("failed {e.message}")
    end
    match total(10)
        ok(v) -> print("total {v}")
        err(e) -> print("failed {e.message}")
    end
    print(shadow(10))
    return
end
"#;
    if let Some(out) = same(src) {
        assert!(out.contains("total 10"), "{}", out);
        assert!(out.contains("failed seven"), "{}", out);
    }
}

// ---- inlining ----

/// Whether the procedure `name` of `asm` contains a `call` of a symbol containing `callee`.
fn calls(asm: &str, name: &str, callee: &str) -> bool {
    proc_body(asm, name).iter().any(|l| l.starts_with("call ") && l.contains(callee))
}

const INLINE_PROGRAM: &str = r#"package main

use text
use math

let factor = 3
var calls = 0

func square(x: int) -> int
    return x * x
end

func scale(n: int) -> int
    return n * factor
end

func twice(x: int) -> int
    return x + x
end

func second(a: int, b: int) -> int
    return b
end

func unused(a: int, b: int, c: int) -> int
    return a
end

func small(a: i8, b: u32) -> long
    return long(a) + long(b)
end

func widen(a: i8) -> i8
    return a + 100
end

func fhalf(x: f32) -> f32
    return x * 0.5
end

func quad(x: int) -> int
    return square(square(x))
end

func chain(x: int) -> int
    return quad(x) + twice(square(x))
end

func is_big(x: int) -> bool
    return x > 100 or x < -100
end

func add3(a: int, b: int, c: int) -> int
    return a * 100 + b * 10 + c
end

func bump() -> int
    calls += 1
    return calls
end

func tick(label: text) -> int
    print(label)
    return 1
end

func change(ref v: int) -> int
    v += 10
    return v
end

func main()
    let factor = 100
    let n = 7
    let b = 5
    let x = 2
    print(square(n))
    print(scale(n))
    print(scale(5) + factor)
    print(twice(tick("a") + tick("b")))
    print(second(tick("x"), tick("y")))
    print(unused(tick("p"), tick("q"), tick("r")))
    print(twice(bump()))
    print(twice(bump()) + calls)
    print(add3(n, bump(), n))
    var m = 1
    print(add3(m, change(ref m), m))
    print(small(-5, 4000000000))
    print(small(127, 4294967295))
    print(widen(100))
    print(fhalf(3.0))
    print(quad(3))
    print(chain(4))
    print(is_big(n))
    print(is_big(0 - 500))
    print(text.is_space(b))
    print(text.is_space(32))
    print(math.sqrt(16.0))
    print(square(x) + x)
    let f = square
    print(f(9))
    var i = 0
    var sum = 0
    while i < 10
        sum += square(i) + twice(i)
        if is_big(sum)
            sum -= 50
        end
        i += 1
    end
    print(sum)
end
"#;

#[test]
fn inlined_calls_behave_like_calls() {
    let Some(out) = same(INLINE_PROGRAM) else { return };
    let lines: Vec<&str> = out.lines().collect();
    // arguments are evaluated once, left to right, even when the parameter is unused or repeated
    assert_eq!(&lines[..8], ["49", "21", "115", "a", "b", "4", "x", "y"], "{}", out);
    assert_eq!(&lines[8..14], ["1", "p", "q", "r", "1", "2"], "{}", out);
    assert!(out.contains("3999999995\n"), "{}", out);
}

#[test]
fn run_time_errors_inside_inlined_bodies_are_unchanged() {
    let src = r#"package main

func half(a: int, b: int) -> int
    return a / b
end

func main()
    print(half(10, 2))
    var zero = 0
    print(half(10, zero))
    print("not reached")
end
"#;
    let Some((out, code, _)) = run_with(src, &ON) else { return };
    let (off, off_code, _) = run_with(src, &OFF).unwrap();
    assert_eq!(out, off);
    assert_eq!(code, off_code);
    assert_ne!(code, 0, "{}", out);
    assert!(out.starts_with("5\n") && !out.contains("not reached"), "{}", out);
}

const KINDS_PROGRAM: &str = r#"package main

type Counter = record
    n: int
end

func (c: Counter) get() -> int
    return c.n
end

func square(x: int) -> int
    return x * x
end

func down(n: int) -> int
    return down(n - 1)
end

func label(n: int) -> text
    return "x"
end

func ident<T>(x: T) -> T
    return x
end

async func slow(x: int) -> int
    return x + 1
end

func raw() -> int
    var r = 0
    unsafe
        asm
            mov rax, 5
        end
    end
    return r
end

func main()
    let c = Counter { n: 4 }
    print(square(3))
    print(c.get())
    print(label(1))
    print(ident(5))
    let fut = slow(2)
    print(raw())
    print(down(1))
end
"#;

#[test]
fn only_small_ordinary_functions_lose_their_call() {
    let on = asm(KINDS_PROGRAM, &ON);
    let off = asm(KINDS_PROGRAM, &OFF);
    assert!(!calls(&on, "B_main_main", "B_main_square"), "square should be inlined");
    assert!(calls(&off, "B_main_main", "B_main_square"), "--no-opt must keep the call");
    for callee in ["_get", "B_main_down", "B_main_label", "B_main_ident", "B_main_slow", "B_main_raw"] {
        assert!(calls(&on, "B_main_main", callee), "{} must still be called", callee);
        assert!(calls(&off, "B_main_main", callee), "{} must be called with --no-opt", callee);
    }
    // nothing else refers to it, so after inlining the function itself is pruned
    assert!(proc_body(&on, "B_main_square").is_empty());
}

#[test]
fn an_inlined_function_still_works_as_a_value() {
    let src = main_of("let f = square\nprint(f(6))\nprint(square(7))\n").replace("func main", "func square(x: int) -> int\n    return x * x\nend\n\nfunc main");
    let Some(out) = same(&src) else { return };
    assert_eq!(out, "36\n49\n");
}

#[test]
fn library_functions_are_inlined_without_seeing_the_callers_names() {
    let src = r#"package main

use math

func main()
    let x = 16.0
    let n = 2.5
    var total = 0.0
    for i in 0..10
        total += math.sqrt(x) + math.floor(n)
    end
    print(total)
    print(math.sqrt(x))
end
"#;
    let on = asm(src, &ON);
    assert!(!calls(&on, "B_main_main", "B_math_sqrt"));
    assert!(!calls(&on, "B_main_main", "B_math_floor"));
    assert!(calls(&asm(src, &OFF), "B_main_main", "B_math_sqrt"));
    let Some(out) = same(src) else { return };
    assert!(out.starts_with("60"), "{}", out);
}

/// Helper functions with random bodies, called with variables whose names collide with the
/// parameters' and with literals, nested and in both orders: all must agree with `--no-opt`.
fn inline_random_case(seed: u64) {
    let mut r = Rng(seed);
    let mut src = String::from("package main\n\n");
    let types = ["int", "i32", "u8", "u32", "u64"];
    let per_type = 12;
    for (ti, t) in types.iter().enumerate() {
        for k in 0..per_type {
            let depth = 2 + r.below(2) as u32;
            let e = int_expr(&mut r, depth, t);
            src.push_str(&format!("func h{}_{}(a: {}, b: {}) -> {}\n    return {}\nend\n\n", ti, k, t, t, t, e));
        }
    }
    let mut mains = String::from("func main()\n");
    for (ti, t) in types.iter().enumerate() {
        src.push_str(&format!("func go{}()\n    let a: {} = 5\n    let b: {} = 200\n", ti, t, t));
        for k in 0..per_type {
            let j = (k + 1 + r.below(5) as usize) % per_type;
            src.push_str(&format!("    println(h{ti}_{k}(b, a))\n    println(h{ti}_{k}(a, 3))\n    println(h{ti}_{k}(7, b))\n"));
            src.push_str(&format!("    println(h{ti}_{k}(h{ti}_{j}(a, b), h{ti}_{j}(b, 1)))\n"));
        }
        src.push_str("end\n\n");
        mains.push_str(&format!("    go{}()\n", ti));
    }
    mains.push_str("end\n");
    src.push_str(&mains);
    let on = asm(&src, &ON);
    let off = asm(&src, &OFF);
    let count = |a: &str| (0..types.len()).map(|ti| proc_body(a, &format!("B_main_go{}", ti)).iter().filter(|l| l.starts_with("call B_main_h")).count()).sum::<usize>();
    assert!(count(&on) < count(&off) / 2, "calls with optimization {}, without {}", count(&on), count(&off));
    let Some(out) = same(&src) else { return };
    assert!(out.lines().count() >= 5 * 12 * 4, "only {} lines", out.lines().count());
}

#[test]
fn random_helper_functions_agree() {
    inline_random_case(0x9E3779B97F4A7C15);
    inline_random_case(0x1234567887654321);
}

pub const HELPERS: &str = r#"package main

func square(x: int) -> int
    return x * x
end

func is_even(n: int) -> bool
    return n % 2 == 0
end

func mix(a: int, b: int) -> int
    return (a * 31 + b) bitand 1048575
end

func main()
    var sum = 0
    for i in 0..100000000
        if is_even(i)
            sum += mix(i, 7)
        else
            sum += square(i bitand 255)
        end
    end
    print(sum)
end
"#;
