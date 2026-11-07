//! `brasa-parse --test`, `#[test]`, `assert` and `assert_eq`. The runner builds native
//! executables (a toolchain-less machine is detected by the exit status 2 and the tests skip).

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

fn temp_file(text: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("brasa-testrun-{}-{}.br", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    std::fs::write(&p, text).unwrap();
    p
}

/// Runs `--test` on the files; `None` when there is no native toolchain.
fn run_tests(files: &[&PathBuf], extra: &[&str]) -> Option<(String, String, i32)> {
    let out = Command::new(env!("CARGO_BIN_EXE_brasa-parse")).arg("--test").args(files).args(extra).output().unwrap();
    let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout).replace('\r', ""), String::from_utf8_lossy(&out.stderr).replace('\r', ""));
    let code = out.status.code().unwrap();
    if code == 2 && stderr.contains("could not find Visual Studio") {
        eprintln!("no native toolchain: skipping");
        return None;
    }
    Some((stdout, stderr, code))
}

/// The lines the runner printed, without the header line.
fn lines(out: &str) -> Vec<String> {
    out.lines().filter(|l| !l.starts_with("running ")).map(|l| l.to_string()).collect()
}

const MESSAGES: &str = r#"package main

func loud(n: int) -> int
    print("loud {n}")
    return n
end

func check(n: int) -> error
    assert(n > 0, "n must be positive, got {n}")
    return
end

#[test]
func t_int() -> error
    assert_eq(1 + 2, 4)
    return
end

#[test]
func t_text() -> error
    assert_eq("ab" + "c", "abd")
    return
end

#[test]
func t_float() -> error
    assert_eq(1.5, 2.5)
    return
end

#[test]
func t_bool() -> error
    assert_eq(true, false)
    return
end

#[test]
func t_assert() -> error
    assert(1 > 2)
    return
end

#[test]
func t_message() -> error
    let x = 0
    assert(x > 0, "x must be positive")
    return
end

#[test]
func t_helper() -> error
    try check(5)
    try check(0 - 2)
    return
end

#[test]
func t_once() -> error
    assert_eq(loud(7), 7)
    return
end

#[test]
func t_ok() -> error
    assert(true)
    assert_eq(2, 2)
    assert_eq("x", "x")
    return
end
"#;

#[test]
fn failed_assertions_name_the_line_and_the_values() {
    let f = temp_file(MESSAGES);
    let Some((out, _, code)) = run_tests(&[&f], &[]) else { return };
    let _ = std::fs::remove_file(&f);
    assert_eq!(code, 1, "{}", out);
    assert_eq!(
        lines(&out),
        [
            "FAIL t_int: line 15: assertion failed: left 3, right 4",
            "FAIL t_text: line 21: assertion failed: left abc, right abd",
            "FAIL t_float: line 27: assertion failed: left 1.5, right 2.5",
            "FAIL t_bool: line 33: assertion failed: left true, right false",
            "FAIL t_assert: line 39: assertion failed",
            "FAIL t_message: line 46: x must be positive",
            "FAIL t_helper: line 9: n must be positive, got -2",
            "loud 7",
            "ok t_once",
            "ok t_ok",
            "test result: 2 passed, 7 failed",
            "error: 7 test(s) failed",
        ],
        "{}",
        out
    );
}

#[test]
fn assert_compiles_in_a_function_that_returns_no_error() {
    // outside a test run it aborts, so it is allowed anywhere
    let src = "package main

func f()
    assert(true)
end

func main()
    f()
end
";
    assert!(brasa::codegen_x64::generate_asm(&brasa::parse_source(src).unwrap()).is_ok());
    let src = "package main

func g() -> int
    assert_eq(1, 1)
    return 1
end

func main()
    print(g())
end
";
    assert!(brasa::codegen_x64::generate_asm(&brasa::parse_source(src).unwrap()).is_ok());
}

#[test]
fn assert_in_a_function_that_returns_a_value_or_an_error_works() {
    let src = "package main\n\nfunc g(n: int) -> int or error\n    assert(n > 0)\n    return n\nend\n\nfunc main()\n    print(g(1) ?? 0)\nend\n";
    assert!(brasa::codegen_x64::generate_asm(&brasa::parse_source(src).unwrap()).is_ok());
}

#[test]
fn a_function_named_assert_takes_precedence() {
    let src = "package main\n\nfunc assert(c: bool)\n    print(c)\nend\n\nfunc f()\n    assert(true)\nend\n\nfunc main()\n    f()\nend\n";
    assert!(brasa::codegen_x64::generate_asm(&brasa::parse_source(src).unwrap()).is_ok());
}

#[test]
fn passing_tests_exit_with_zero() {
    let f = temp_file("package main\n\n#[test]\nfunc a() -> error\n    assert_eq(1, 1)\n    return\nend\n\n#[test]\nfunc plain()\n    let x = 1 + 1\nend\n");
    let Some((out, _, code)) = run_tests(&[&f], &[]) else { return };
    let _ = std::fs::remove_file(&f);
    assert_eq!(code, 0, "{}", out);
    assert!(out.starts_with("running 2 test(s) in "), "{}", out);
    assert_eq!(lines(&out), ["ok a", "ok plain", "test result: 2 passed, 0 failed"]);
}

#[test]
fn one_failing_test_among_passing_ones() {
    let f = temp_file("package main\n\n#[test]\nfunc first() -> error\n    return\nend\n\n#[test]\nfunc second() -> error\n    assert_eq(1, 2)\n    return\nend\n");
    let Some((out, _, code)) = run_tests(&[&f], &[]) else { return };
    let _ = std::fs::remove_file(&f);
    assert_eq!(code, 1);
    assert_eq!(lines(&out), ["ok first", "FAIL second: line 10: assertion failed: left 1, right 2", "test result: 1 passed, 1 failed", "error: 1 test(s) failed"]);
}

#[test]
fn the_files_own_main_is_not_run() {
    let f = temp_file("package main\n\n#[test]\nfunc a() -> error\n    return\nend\n\nfunc main()\n    print(\"main ran\")\nend\n");
    let Some((out, _, code)) = run_tests(&[&f], &[]) else { return };
    let _ = std::fs::remove_file(&f);
    assert_eq!(code, 0);
    assert!(!out.contains("main ran"), "{}", out);
}

#[test]
fn a_crash_stops_the_run_and_is_reported() {
    let f = temp_file("package main\n\n#[test]\nfunc fine() -> error\n    return\nend\n\n#[test]\nfunc boom() -> error\n    var z = 0\n    print(10 / z)\n    return\nend\n\n#[test]\nfunc never() -> error\n    return\nend\n");
    let Some((out, _, code)) = run_tests(&[&f], &[]) else { return };
    let _ = std::fs::remove_file(&f);
    assert_eq!(code, 1);
    assert!(out.contains("ok fine") && !out.contains("never"), "{}", out);
    assert!(out.contains("run stopped early ("), "{}", out);
}

#[test]
fn invalid_tests_are_reported_by_name() {
    let with_param = temp_file("package main\n\n#[test]\nfunc needs(x: int) -> error\n    return\nend\n");
    let is_async = temp_file("package main\n\n#[test]\nasync func later() -> error\n    return\nend\n");
    let wrong_result = temp_file("package main\n\n#[test]\nfunc value() -> int\n    return 1\nend\n");
    for (f, name) in [(&with_param, "needs"), (&is_async, "later"), (&wrong_result, "value")] {
        let Some((out, err, code)) = run_tests(&[f], &[]) else { return };
        assert_eq!(code, 1, "{} {}", out, err);
        assert!(err.contains(name) && !out.contains("test result"), "{} {}", out, err);
        let _ = std::fs::remove_file(f);
    }
}

#[test]
fn files_with_tests_must_be_in_package_main() {
    let f = temp_file("package lib\n\n#[test]\nfunc a() -> error\n    return\nend\n");
    let Some((_, err, code)) = run_tests(&[&f], &[]) else { return };
    let _ = std::fs::remove_file(&f);
    assert_eq!(code, 1);
    assert!(err.contains("package main"), "{}", err);
}

#[test]
fn files_without_tests_are_not_a_failure() {
    let f = temp_file("package main\n\nfunc main()\n    print(1)\nend\n");
    let Some((out, _, code)) = run_tests(&[&f], &[]) else { return };
    let _ = std::fs::remove_file(&f);
    assert_eq!(code, 0);
    assert!(out.contains("no tests in "), "{}", out);
}

#[test]
fn several_files_run_in_turn_and_any_failure_counts() {
    let good = temp_file("package main\n\n#[test]\nfunc a() -> error\n    return\nend\n");
    let bad = temp_file("package main\n\n#[test]\nfunc b() -> error\n    assert(false)\n    return\nend\n");
    let Some((out, _, code)) = run_tests(&[&good, &bad], &[]) else { return };
    let _ = (std::fs::remove_file(&good), std::fs::remove_file(&bad));
    assert_eq!(code, 1);
    assert_eq!(out.matches("running 1 test(s) in ").count(), 2, "{}", out);
    assert!(out.contains("ok a") && out.contains("FAIL b: line 5: assertion failed"), "{}", out);
}

#[test]
fn a_file_that_does_not_compile_fails_the_run() {
    let f = temp_file("package main\n\n#[test]\nfunc a() -> error\n    let x: int = \"text\"\n    return\nend\n");
    let Some((out, err, code)) = run_tests(&[&f], &[]) else { return };
    let _ = std::fs::remove_file(&f);
    assert_eq!(code, 1, "{} {}", out, err);
    assert!(!err.is_empty() && !out.contains("test result"), "{} {}", out, err);
}

#[test]
fn no_opt_is_honoured() {
    let f = temp_file("package main\n\n#[test]\nfunc a() -> error\n    assert_eq(2 * 3, 6)\n    return\nend\n");
    let Some((out, _, code)) = run_tests(&[&f], &["--no-opt"]) else { return };
    let _ = std::fs::remove_file(&f);
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains("ok a"), "{}", out);
}

#[test]
fn the_example_passes() {
    let f = PathBuf::from("examples/testing.br");
    let Some((out, _, code)) = run_tests(&[&f], &[]) else { return };
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains("test result: 3 passed, 0 failed"), "{}", out);
}
