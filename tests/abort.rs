//! `abort`, the run-time failures that end a program, wrapping integer arithmetic, `checked_*`
//! and list `get`. Programs are built for Windows and (through WSL) for Linux, with and without
//! the optimizer; a machine without the toolchain skips the tests that need it.

use brasa::codegen_x64::{compile_to_linux, compile_to_native_with, generate_asm_for, CompileOptions, Target};
use brasa::parse_source;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

const ON: CompileOptions = CompileOptions { optimize: true, test_mode: false };
const OFF: CompileOptions = CompileOptions { optimize: false, test_mode: false };

struct Ran {
    out: String,
    err: String,
    code: i32,
}

fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("brasa-abort-test-{}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst), name))
}

/// Builds and runs on Windows with the two streams apart; `None` without the toolchain.
fn windows(src: &str, options: &CompileOptions) -> Option<Ran> {
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = temp("prog.exe");
    match compile_to_native_with(&file, exe.to_str().unwrap(), options) {
        Ok(()) => {}
        Err(e) if e.contains("could not find Visual Studio") || e.contains("vcvars64") => {
            eprintln!("no native toolchain: skipping");
            return None;
        }
        Err(e) => panic!("compile error: {}", e),
    }
    let o = Command::new(&exe).output().unwrap();
    let _ = std::fs::remove_file(&exe);
    Some(Ran {
        out: String::from_utf8_lossy(&o.stdout).replace('\r', ""),
        err: String::from_utf8_lossy(&o.stderr).replace('\r', ""),
        code: o.status.code().unwrap_or(-1),
    })
}

fn wsl_path(p: &std::path::Path) -> String {
    let s = p.to_str().unwrap().replace('\\', "/");
    let mut chars = s.chars();
    match (chars.next(), chars.next()) {
        (Some(d), Some(':')) => format!("/mnt/{}{}", d.to_ascii_lowercase(), &s[2..]),
        _ => s,
    }
}

fn wsl_works() -> bool {
    static CHECK: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *CHECK.get_or_init(|| match Command::new("wsl.exe").args(["-e", "sh", "-c", "echo ready"]).output() {
        Ok(o) => String::from_utf8_lossy(&o.stdout).contains("ready"),
        Err(_) => false,
    })
}

/// Builds for Linux and runs under WSL with the two streams apart; `None` without WSL.
fn linux(src: &str, options: &CompileOptions) -> Option<Ran> {
    if !wsl_works() {
        eprintln!("WSL is not available: skipping");
        return None;
    }
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = temp("prog");
    compile_to_linux(&file, exe.to_str().unwrap(), options).unwrap_or_else(|e| panic!("compile error: {}", e));
    let n = exe.file_name().unwrap().to_str().unwrap().to_string();
    let script = format!("cp '{}' /tmp/{n} && chmod +x /tmp/{n} && /tmp/{n} 2>/tmp/{n}.err; echo \"__EXIT:$?\"; cat /tmp/{n}.err; rm -f /tmp/{n} /tmp/{n}.err", wsl_path(&exe), n = n);
    let o = Command::new("wsl.exe").args(["-e", "sh", "-c", &script]).output().unwrap();
    let _ = std::fs::remove_file(&exe);
    let text = String::from_utf8_lossy(&o.stdout).replace('\r', "");
    let (body, rest) = text.rsplit_once("__EXIT:").unwrap_or_else(|| panic!("no exit marker in: {}", text));
    let (code, err) = rest.split_once('\n').unwrap_or((rest, ""));
    Some(Ran { out: body.to_string(), err: err.to_string(), code: code.trim().parse().unwrap() })
}

/// Every way a program is built and run here: (label, result).
fn everywhere(src: &str) -> Vec<(&'static str, Ran)> {
    let mut all = Vec::new();
    for (label, options) in [("windows", &ON), ("windows --no-opt", &OFF)] {
        if let Some(r) = windows(src, options) {
            all.push((label, r));
        }
    }
    for (label, options) in [("linux", &ON), ("linux --no-opt", &OFF)] {
        if let Some(r) = linux(src, options) {
            all.push((label, r));
        }
    }
    all
}

/// The program must print `out` on standard output, `err` on standard error and end with `code`
/// everywhere.
fn expect(src: &str, out: &str, err: &str, code: i32) {
    for (label, r) in everywhere(src) {
        assert_eq!((r.out.as_str(), r.err.as_str(), r.code), (out, err, code), "{}", label);
    }
}

fn main_of(body: &str) -> String {
    let indented: String = body.lines().map(|l| format!("    {}\n", l)).collect();
    format!("package main\n\nfunc main()\n{}end\n", indented)
}

// ---- the abort itself ----

#[test]
fn an_abort_writes_one_line_to_standard_error_and_exits_with_101() {
    // `abort` is on line 5
    expect(&main_of("print(\"a\")\nabort(\"stop\")"), "a\n", "abort: main.br:5: stop\n", 101);
}

#[test]
fn nothing_runs_on_an_abort() {
    let src = "package main\n\ntype R = record\n    n: int\nend\n\nfunc (ref r: R) drop()\n    print(\"drop\")\nend\n\nfunc main()\n    let r = R { n: 1 }\n    defer print(\"cleanup\")\n    print(\"before\")\n    abort(\"stop\")\nend\n";
    expect(src, "before\n", "abort: main.br:15: stop\n", 101);
}

#[test]
fn panic_is_not_a_function() {
    let file = parse_source(&main_of("panic(\"x\")")).unwrap();
    let e = generate_asm_for(&file, &CompileOptions::default(), Target::Windows).unwrap_err();
    assert!(e.contains("panic"), "{}", e);
}

#[test]
fn abort_can_end_a_branch_that_needs_a_value() {
    let src = "package main\n\nfunc pick(n: int) -> int\n    if n > 0\n        return n\n    end\n    abort(\"unreachable\")\n    return 0\nend\n\nfunc main()\n    print(pick(3))\nend\n";
    expect(src, "3\n", "", 0);
}

#[test]
fn a_function_named_abort_takes_precedence() {
    let src = "package main\n\nfunc abort(m: text)\n    print(\"mine {m}\")\nend\n\nfunc main()\n    abort(\"x\")\nend\n";
    expect(src, "mine x\n", "", 0);
}

#[test]
fn out_of_memory_has_no_location() {
    let src = main_of("let xs: []text = []\nvar i = 0\nwhile true\n    xs.add(\"some text number {i}\")\n    i += 1\nend");
    if let Some(r) = linux(&src, &ON) {
        assert_eq!((r.code, r.err.as_str()), (101, "abort: out of memory\n"));
    }
}

// ---- each site reports its own line ----

const SITES: &str = "package main

func first(xs: []int) -> int
    return xs[0]
end

func main()
    print(\"go\")
    let empty: []int = []
    print(first(empty))
end
";

#[test]
fn every_checking_site_reports_the_message_and_the_line_of_the_failing_statement() {
    // the statement under test is the last of each body: line 5 for two lines, 6 for three
    let cases: [(&str, &str, &str); 9] = [
        ("let xs = [1, 2, 3]\nvar i = 5\nprint(xs[i])", "index out of bounds", "6"),
        ("let t = \"abc\"\nvar i = 9\nprint(t.byte(i))", "byte position 9 is out of range", "6"),
        ("let xs = [1, 2, 3]\nvar i = 5\nxs[i] = 1", "index out of bounds", "6"),
        ("var z = 0\nprint(10 / z)", "division by zero", "5"),
        ("var z = 0\nprint(10 % z)", "division by zero", "5"),
        ("let m = { \"a\": 1 }\nprint(m[\"b\"])", "map key not found", "5"),
        ("let xs: []int = []\nlet x = xs.take()", "take from an empty list", "5"),
        ("let z = 0.0\nlet x = int(1.0 / z)", "float out of range", "5"),
        ("let s = \"x\"\nabort(\"custom {s}\")", "custom x", "5"),
    ];
    for (body, message, line) in cases {
        for (label, r) in everywhere(&main_of(body)) {
            assert_eq!(r.out, "", "{} {}", label, body);
            assert_eq!(r.err, format!("abort: main.br:{}: {}\n", line, message), "{} {}", label, body);
            assert_eq!(r.code, 101, "{} {}", label, body);
        }
    }
}

#[test]
fn an_inlined_helper_reports_the_line_of_its_own_source() {
    // `first` is small enough to be inlined; the line is the one in its body
    for (label, r) in everywhere(SITES) {
        assert_eq!((r.out.as_str(), r.code), ("go\n", 101), "{}", label);
        assert_eq!(r.err, "abort: main.br:4: index out of bounds\n", "{}", label);
    }
}

// ---- assertions ----

#[test]
fn assert_outside_a_test_run_aborts_in_any_function() {
    let src = "package main\n\nfunc check(n: int)\n    assert(n > 0)\nend\n\nfunc main()\n    check(1)\n    assert_eq(1 + 1, 2)\n    check(0)\nend\n";
    expect(src, "", "abort: main.br:4: assertion failed\n", 101);
    let src = "package main\n\nfunc main()\n    assert(1 > 2, \"one is not two\")\nend\n";
    expect(src, "", "abort: main.br:4: one is not two\n", 101);
    let src = "package main\n\nfunc main()\n    assert_eq(3, 4)\nend\n";
    expect(src, "", "abort: main.br:4: assertion failed: left 3, right 4\n", 101);
}

#[test]
fn assert_in_a_function_returning_an_error_aborts_in_a_normal_build() {
    let src = "package main\n\nfunc g(n: int) -> int or error\n    assert(n > 0)\n    return n\nend\n\nfunc main()\n    print(g(1) ?? 0)\n    print(g(0) ?? 0)\nend\n";
    expect(src, "1\n", "abort: main.br:4: assertion failed\n", 101);
}

#[test]
fn test_mode_is_off_by_default() {
    assert!(!CompileOptions::default().test_mode);
}

fn run_tests(src: &str) -> Option<(String, String, i32)> {
    let p = temp("tests.br");
    std::fs::write(&p, src).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_brasa-parse")).arg("--test").arg(&p).output().unwrap();
    let _ = std::fs::remove_file(&p);
    let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout).replace('\r', ""), String::from_utf8_lossy(&out.stderr).replace('\r', ""));
    let code = out.status.code().unwrap();
    if code == 2 && stderr.contains("could not find Visual Studio") {
        return None;
    }
    Some((stdout, stderr, code))
}

#[test]
fn under_test_a_failed_assert_is_a_failed_test_and_an_assert_in_a_plain_helper_is_an_abort() {
    let src = "package main\n\nfunc helper(n: int)\n    assert(n > 0)\nend\n\n#[test]\nfunc a() -> error\n    assert_eq(1, 2)\n    return\nend\n\n#[test]\nfunc b()\n    helper(0)\nend\n\n#[test]\nfunc never()\n    print(\"never\")\nend\n";
    let Some((out, err, code)) = run_tests(src) else { return };
    assert_eq!(code, 1, "{}{}", out, err);
    assert!(out.contains("FAIL a: line 9: assertion failed: left 1, right 2"), "{}", out);
    assert!(out.contains("run stopped early (by an abort, exit code 101)"), "{}", out);
    assert!(err.contains("abort: main.br:4: assertion failed"), "{}", err);
    assert!(!out.contains("never"), "{}", out);
}

#[test]
fn a_crashing_test_run_is_described_as_an_abort() {
    let src = "package main\n\n#[test]\nfunc boom()\n    var z = 0\n    print(10 / z)\nend\n";
    let Some((out, err, code)) = run_tests(src) else { return };
    assert_eq!(code, 1);
    assert!(out.contains("run stopped early (by an abort, exit code 101)"), "{}", out);
    assert!(err.contains("abort: main.br:6: division by zero"), "{}", err);
}

// ---- wrapping ----

#[test]
fn min_divided_by_minus_one_wraps() {
    let src = "package main

func div(a: long, b: long) -> long
    return a / b
end

func rem(a: long, b: long) -> long
    return a % b
end

func main()
    let zero: long = 0
    let max: long = 9223372036854775807
    let m = zero - max - 1
    var d: long = -1
    println(div(m, d), rem(m, d), m / d, m % d)
    println(div(7, d), rem(7, d), div(-7, 2), rem(-7, 2))
end
";
    expect(src, "-9223372036854775808 0 -9223372036854775808 0\n-7 0 -3 -1\n", "", 0);
}

#[test]
fn arithmetic_wraps_the_same_way_everywhere() {
    let src = "package main

func add(a: long, b: long) -> long
    return a + b
end

func main()
    let zero: long = 0
    let one: long = 1
    let big: long = 9223372036854775807
    println(add(big, 1))
    println(big + one)
    println(big * 2, zero - big - 2)
    var small: i32 = 2147483647
    small += 1
    println(small)
    var b: u8 = 255
    b += 1
    println(b)
end
";
    expect(src, "-9223372036854775808\n-9223372036854775808\n-2 9223372036854775807\n-2147483648\n0\n", "", 0);
}

#[test]
fn a_constant_zero_divisor_is_not_folded() {
    let asm = generate_asm_for(&parse_source(&main_of("print(1 / 0)")).unwrap(), &ON, Target::Windows).unwrap();
    assert!(asm.contains("jz rt_div_zero"), "{}", asm);
}

// ---- checked arithmetic ----

#[test]
fn checked_arithmetic_returns_values_and_errors() {
    let calls = [
        "add(2, 3)", "add(max, 1)", "add(min, -1)", "add(max, min)", "sub(5, 7)", "sub(min, 1)", "sub(max, -1)", "sub(0, min)",
        "mul(6, 7)", "mul(max, 2)", "mul(min, -1)", "mul(-1, min)", "mul(min, 1)", "mul(0, min)", "mul(-4611686018427387904, 2)",
        "mul(4611686018427387904, 2)", "div(7, 2)", "div(1, 0)", "div(min, -1)", "div(min, 1)",
    ];
    let mut src = String::from("package main

use math

func main()
    let zero: long = 0
    let max: long = 9223372036854775807
    let min = zero - max - 1
");
    for c in calls {
        // `c` is like "add(2, 3)"; split into name and args to call `checked_<name>_long(<args>)`
        let open = c.find('(').unwrap();
        let name = &c[..open];
        let args = &c[open..];
        src.push_str(&format!("    match math.checked_{}_long{}
        ok(v) -> println(\"ok {{v}}\")
        err(e) -> println(\"{{e.kind.name}}\")
    end
", name, args));
    }
    src.push_str("end
");
    let src = src.as_str();
    let want = "ok 5
std.Overflow
std.Overflow
ok -1
ok -2
std.Overflow
std.Overflow
std.Overflow
ok 42
std.Overflow
std.Overflow
std.Overflow
ok -9223372036854775808
ok 0
ok -9223372036854775808
std.Overflow
ok 3
std.InvalidInput
std.Overflow
ok -9223372036854775808
";
    expect(src, want, "", 0);
}

// ---- list get ----

#[test]
fn get_returns_the_element_or_nothing() {
    let src = "package main

type P = record
    n: int
end

func main()
    let xs = [10, 20, 30]
    println(xs.get(1) ?? -1, xs.get(3) ?? -1, xs.get(-1) ?? -1, xs.get(0) ?? -1)
    let ts = [\"a\", \"b\"]
    let t = ts.get(1)
    if t != nil
        println(t)
    end
    println(ts.get(2) == nil, ts.get(-1) == nil)
    let ps = [P { n: 4 }]
    let p = ps.get(0)
    if p != nil
        println(p.n)
    end
    println(ps.get(1) == nil)
    let bs = [true, false]
    println(bs.get(1) ?? true, bs.get(2) ?? true)
end
";
    expect(src, "20 -1 -1 10\nb\ntrue true\n4\ntrue\nfalse true\n", "", 0);
}

#[test]
fn get_does_not_leak() {
    let src = "package main

type P = record
    name: text
end

func xsum(i: int) -> int
    let xs = [1, 2, 3]
    return xs.get(i % 4) ?? 0
end

func main()
    let ts = [\"one {1}\", \"two {2}\"]
    let ps = [P { name: \"p {1}\" }]
    // the first failure interns its error kind once, for the life of the program
    var total = xsum(3)
    let before = live_objects()
    for i in 0..1000
        let t = ts.get(i % 3)
        if t != nil
            total += t.len
        end
        let p = ps.get(i % 2)
        if p != nil
            total += p.name.len
        end
        total += xsum(i)
    end
    println(total > 0, live_objects() - before)
end
";
    expect(src, "true 0\n", "", 0);
}

#[test]
fn get_works_on_slices() {
    let src = "package main

use list

func main()
    let xs = [1, 2, 3, 4, 5]
    let s = list.slice(xs, 1, 4)
    println(s.get(0) ?? -1, s.get(2) ?? -1, s.get(3) ?? -1, s.get(-1) ?? -1)
end
";
    expect(src, "2 4 -1 -1\n", "", 0);
}
