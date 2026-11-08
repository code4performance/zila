//! Floating point: types, literals, arithmetic, comparison, conversion, printing. Programs are
//! compiled to native executables and run (skipped when the Visual Studio build tools are
//! missing).

use zila::{compile_to_native, parse_source};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

fn run(src: &str) -> Option<(String, i32)> {
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = std::env::temp_dir().join(format!("zila-floats-test-{}-{}.exe", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
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

/// The compile error of a program that must not compile.
fn compile_error(src: &str) -> String {
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = std::env::temp_dir().join(format!("zila-floats-test-{}-{}.exe", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    match compile_to_native(&file, exe.to_str().unwrap()) {
        Ok(()) => panic!("expected a compile error"),
        Err(e) => e,
    }
}

fn main_of(body: &str) -> String {
    let indented: String = body.lines().map(|l| format!("    {}
", l)).collect();
    format!("package main

type P = record
    x: float
    n: int
end

func main()
{}end
", indented)
}

fn expect_ok(body: &str) {
    let Some((out, code)) = run(&main_of(body)) else { return };
    assert_eq!(out, "ok
", "program output");
    assert_eq!(code, 0);
}

#[test]
fn declarations_and_literals() {
    expect_ok(
        r#"var x: float = 1.5
let y: f32 = 2.5
let z: f64 = 0.25
let p = P { x: 2.0, n: 1 }
let zero: float = 0.0
if x == 1.5 and y == 2.5 and z == 0.25 and p.x == 2.0 and zero == 0.0 and 2e3 == 2000.0
    println("ok")
end"#,
    );
}

#[test]
fn arithmetic() {
    expect_ok(
        r#"let a = 1.5 + 2.25
let b = 5.0 - 0.5
let c = 2.0 * 4.0
let d = 1.0 / 4.0
let e = -(1.5)
let f: f32 = 1.5
let g = f * 2.0 + 0.5
var h = 1.0
h += 0.5
h *= 3.0
if a == 3.75 and b == 4.5 and c == 8.0 and d == 0.25 and e == -1.5 and g == 3.5 and h == 4.5
    println("ok")
end"#,
    );
}

#[test]
fn comparisons_and_nan() {
    expect_ok(
        r#"let zero = 0.0
let nan = zero / zero
let inf = 1.0 / zero
let ok1 = 1.5 < 2.5 and 2.5 <= 2.5 and not (3.0 > 3.5) and 3.5 >= 3.5 and 1.0 != 2.0
let ok2 = not (nan == nan) and not (nan < 1.0) and not (nan > 1.0) and not (nan <= 1.0) and not (nan >= 1.0) and nan != nan
let ok3 = inf > 1.0e300 and -inf < -1.0e300
let g: f32 = 1.0
let h: f32 = 2.0
let ok4 = g < h and h >= g and g != h
if ok1 and ok2 and ok3 and ok4
    println("ok")
end"#,
    );
}

#[test]
fn functions_records_and_lists() {
    let src = r#"package main

type P = record
    x: float
    y: f32
end

func scale(v: float, k: float) -> float
    return v * k
end

func main()
    let p = P { x: 1.5, y: 2.5 }
    let xs = [1.5, 2.5, 3.0]
    let add = func(a: float) a + p.x end
    if scale(p.x, 2.0) == 3.0 and p.y == 2.5 and xs[1] == 2.5 and xs.len == 3 and add(1.0) == 2.5
        println("ok")
    end
end
"#;
    let Some((out, code)) = run(src) else { return };
    assert_eq!(out, "ok
");
    assert_eq!(code, 0);
}

#[test]
fn type_errors() {
    let e = compile_error(&main_of("let x: float = 3"));
    assert!(e.contains("expected float, found int") && e.contains("3.0"), "{}", e);
    let e = compile_error(&main_of("let a = 1
let b = 1.5
let c = a + b"));
    assert!(e.contains("'+'") && e.contains("float"), "{}", e);
    let e = compile_error(&main_of("let a: f32 = 1.0
let b = 1.5
let c = a * b"));
    assert!(e.contains("f32") && e.contains("float"), "{}", e);
    let e = compile_error(&main_of("let a = 1.5 % 2.0"));
    assert!(e.contains("'%'"), "{}", e);
    let e = compile_error(&main_of("let a = 1
if a == 1.5
    println(\"x\")
end"));
    assert!(e.contains("cannot compare"), "{}", e);
    let e = compile_error(&main_of("let m: Map<float, int> = {}"));
    assert!(e.contains("float cannot be a map key"), "{}", e);
}

#[test]
fn conversions() {
    expect_ok(
        r#"let a = float(7) / 2.0
let b = int(3.99)
let c = int(-3.99)
let big: u64 = 18446744073709551615
let d = float(big)
let e: f32 = f32(0.1)
let f = f64(e)
let g = int(f32(2.75))
let h = i32(-7.9)
let m: long = -9223372036854775807
let ok = a == 3.5 and b == 3 and c == -3 and d == 18446744073709551616.0 and f != 0.1 and f > 0.0999 and f < 0.1001 and g == 2 and h == -7 and long(-9223372036854775808.0) == m - 1 and int(-2147483648.0) == -2147483647 - 1 and long(7) == 7
if ok
    println("ok")
end"#,
    );
}

#[test]
fn out_of_range_conversion_aborts() {
    for expr in ["1.0 / zero", "zero / zero", "-1.0 / zero", "1.0e19"] {
        let src = main_of(&format!("let zero = 0.0\nlet x = int({})\nprintln(\"unreachable {{x}}\")", expr));
        let Some((out, code)) = run(&src) else { return };
        // main_of puts the statement with the conversion on line 10
        assert_eq!(out, "abort: main.br:10: float out of range\n", "{}", expr);
        assert_eq!(code, 101);
    }
}

fn print_all(body: &str) -> String {
    let Some((out, code)) = run(&main_of(body)) else { return String::new() };
    assert_eq!(code, 0, "{}", out);
    out
}

#[test]
fn printing() {
    let out = print_all(
        r#"println(1.5 + 2.25)
println(5.0 - 0.5)
println(2.0 * 4.0)
println(1.0 / 4.0)
println(-(1.5))
println(0.1 + 0.2)
println(0.1)
println(3.0)
println(2e3)
println(1e21)
println(0.0000001)
println(1.0e15, 1.0e16, 123456789012345680.0)
println(0.0001, 0.00001)
println(100.0, 123.456, 0.5)
let zero = 0.0
println(1.0 / zero, -1.0 / zero, zero / zero, -zero)
println(f64(f32(0.1)))
let g: f32 = 0.1
println(g, g * 3.0, 16777216.0)
println("v={1.25} w={-0.5}")"#,
    );
    assert_eq!(
        out,
        "3.75\n4.5\n8.0\n0.25\n-1.5\n0.30000000000000004\n0.1\n3.0\n2000.0\n1e21\n1e-7\n\
         1000000000000000.0 1e16 1.2345678901234568e17\n0.0001 1e-5\n100.0 123.456 0.5\n\
         inf -inf NaN -0.0\n0.10000000149011612\n0.1 0.3 16777216.0\nv=1.25 w=-0.5\n"
    );
}

#[test]
fn printing_extremes() {
    let out = print_all(
        r#"println(1.7976931348623157e308)
println(5e-324)
println(2.2250738585072014e-308)
println(9007199254740993.0)
println(4.35)
println(0.3)
println(123456789.123456789)"#,
    );
    assert_eq!(
        out,
        "1.7976931348623157e308\n5e-324\n2.2250738585072014e-308\n9007199254740992.0\n4.35\n0.3\n123456789.12345679\n"
    );
}

#[test]
fn extern_calls() {
    let src = r#"package main

#[link("ucrt")]
extern "C" func sqrt(x: f64) -> f64
extern "C" func pow(x: f64, y: f64) -> f64
extern "C" func fma(x: f64, y: f64, z: f64) -> f64
extern "C" func sqrtf(x: f32) -> f32

func main()
    unsafe
        println(sqrt(2.25), pow(2.0, 10.0), fma(2.0, 3.0, 4.0))
        println(sqrtf(2.25))
    end
end
"#;
    let Some((out, code)) = run(src) else { return };
    assert_eq!(code, 0, "{}", out);
    assert!(out.starts_with("1.5 1024.0 10.0\n1.5\n"), "{}", out);
}

#[test]
fn floats_in_collections_closures_and_tasks() {
    let src = r#"package main

type Shape = record
    w: float
    h: float
    name: text
end

func area(s: Shape) -> float
    return s.w * s.h
end

func worker(ch: Channel<float>, k: float)
    ch.send(k * 2.5)
end

func main()
    var before = 0
    for round in 0..400
        let m: Map<text, float> = { "a": 1.5, "b": 2.5 }
        m["c"] = m["a"] + m["b"]
        let shapes = [Shape { w: 2.0, h: 3.5, name: "r" }, Shape { w: 1.5, h: 1.5, name: "s" }]
        var total = 0.0
        for s in shapes
            total += area(s)
        end
        let k = 0.5
        let scale = func(x: float) x * k end
        let ch = channel(float)
        spawn worker(ch, total)
        let got = ch.receive()
        if m["c"] != 4.0 or total != 9.25 or scale(3.0) != 1.5 or got != 23.125
            println("bad {round}")
        end
        if round == 1
            before = live_objects()
        end
        if round == 399
            println("leaked {live_objects() - before}")
        end
    end
    println("done")
end
"#;
    let Some((out, code)) = run(src) else { return };
    assert_eq!(out, "leaked 0\ndone\n");
    assert_eq!(code, 0);
}

#[test]
fn math_helpers() {
    let src = r#"package main

use math

func main()
    println(math.sqrt(2.25), math.sqrt(2.0))
    println(math.floor(-1.5), math.ceil(1.2), math.trunc(-1.7), math.floor(2.0))
    println(math.round(2.5), math.round(-2.5), math.round(2.4), math.round(0.49999999999999994))
    println(math.abs(-3.5), math.abs(2.0), math.abs(-0.0))
    println(math.min(1.5, 2.5), math.max(1.5, 2.5))
    println(math.pow(2.0, 10), math.pow(2.0, -2), math.pow(1.5, 0))
    println(math.fixed(3.14159, 2), math.fixed(2.0, 3), math.fixed(-0.001, 2), math.fixed(0.5, 0), math.fixed(1234.5678, 1))
    println(math.sqrt(-1.0), math.fixed(1.0 / 0.0, 2))
end
"#;
    let Some((out, code)) = run(src) else { return };
    assert_eq!(code, 0, "{}", out);
    assert_eq!(
        out,
        "1.5 1.4142135623730951\n-2.0 2.0 -1.0 2.0\n3.0 -3.0 2.0 0.0\n3.5 2.0 0.0\n1.5 2.5\n\
         1024.0 0.25 1.0\n3.14 2.000 0.00 1 1234.6\nNaN inf\n"
    );
}

fn bits_to_literal(f: f64) -> String {
    format!("{:e}", f)
}

/// Native printing against the Rust-side layout over a spread of bit patterns.
#[test]
fn native_printing_matches_the_reference_formatter() {
    let mut values: Vec<f64> = vec![
        0.0, 1.0, 0.1, 0.5, 1e15, 1e16, 9999999999999998.0, 123456.789, 1e-4, 9.999e-5, 1e22, 1e23, 5e-324, 1.7976931348623157e308,
        2.2250738585072014e-308, 4.35, 0.3, 2.5e-5, 8.41e21, 5.0e-324 * 3.0, 9007199254740993.0, 1.0 / 3.0, 2.0 / 3.0, 100.0, 1e2, 12345678.9,
    ];
    let mut x: u64 = 0x9E3779B97F4A7C15;
    while values.len() < 600 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let f = f64::from_bits(x);
        if f.is_finite() {
            values.push(f);
        }
        // and ordinary-sized values
        let y = (x >> 11) as f64 / (1u64 << 53) as f64;
        values.push(y * 10f64.powi((x % 40) as i32 - 20));
    }
    let mut body = String::new();
    for v in &values {
        body.push_str(&format!("println({}{})\n", if v.is_sign_negative() { "-" } else { "" }, bits_to_literal(v.abs())));
    }
    let expected: String = values.iter().map(|v| format!("{}\n", zila::float_text(*v))).collect();
    let Some((out, code)) = run(&main_of(&body)) else { return };
    assert_eq!(code, 0);
    for (i, (a, b)) in out.lines().zip(expected.lines()).enumerate() {
        assert_eq!(a, b, "value #{} ({:e})", i, values[i]);
    }
    assert_eq!(out.lines().count(), values.len());
}

#[test]
fn native_f32_printing_matches_the_reference_formatter() {
    let mut vals: Vec<f32> = vec![0.1, 1.0, 16777216.0, 3.4028235e38, 1e-45, 0.3, 1.0 / 3.0, 123456.79, 1e10, 7.0e-5];
    let mut x: u32 = 0x9E3779B9;
    while vals.len() < 300 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let f = f32::from_bits(x);
        if f.is_finite() {
            vals.push(f);
        }
    }
    let mut body = String::new();
    for v in &vals {
        body.push_str(&format!("let v: f32 = {}{:e}\nprintln(v)\n", if v.is_sign_negative() { "-" } else { "" }, v.abs()));
    }
    let expected: String = vals.iter().map(|v| format!("{}\n", zila::float_text_f32(*v))).collect();
    let Some((out, code)) = run(&main_of(&body)) else { return };
    assert_eq!(code, 0);
    for (i, (a, b)) in out.lines().zip(expected.lines()).enumerate() {
        assert_eq!(a, b, "value #{} ({:e})", i, vals[i]);
    }
    assert_eq!(out.lines().count(), vals.len());
}

#[test]
fn a_bare_metal_kernel_rejects_floats() {
    let boot = parse_source(&std::fs::read_to_string("os/boot.br").unwrap()).unwrap();
    for body in ["let x = 1.5", "let x: float = 0.0", "let x: f32 = 0.0"] {
        let kernel = parse_source(&format!("package main\nfunc console_write(s: text)\nend\nfunc main()\n    {}\nend\n", body)).unwrap();
        let e = zila::codegen_x64::build_os(&boot, &kernel).unwrap_err();
        assert!(e.contains("floating point") && e.contains("bare-metal"), "{}: {}", body, e);
    }
}

#[test]
fn the_floats_example_prints_what_the_docs_say() {
    let src = std::fs::read_to_string("examples/floats.br").unwrap();
    if let Some((out, code)) = run(&src) {
        assert_eq!(code, 0);
        assert_eq!(out, "average 20.625
0.1 + 0.2 = 0.30000000000000004
1 / 3 = 0.3333333333333333
savings 1628.8946267774422
half of 7 is 3.5
cut off: 3 and -3
inf -inf NaN
false true true
");
    }
}
