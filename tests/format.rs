//! The `format` library: templates, layout, padding and number helpers. Programs are compiled
//! to native executables and run (skipped when the Visual Studio build tools are missing);
//! programs without error cases also run in the interpreter and must print the same.

use brasa::{compile_to_native, parse_source};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

fn native(src: &str) -> Option<String> {
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = std::env::temp_dir().join(format!("brasa-format-test-{}-{}.exe", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
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
    assert_eq!(out.status.code(), Some(0), "exit code; output: {}", String::from_utf8_lossy(&out.stdout));
    Some(String::from_utf8_lossy(&out.stdout).replace('\r', ""))
}

fn interpreted(src: &str) -> String {
    let file = std::env::temp_dir().join(format!("brasa-format-interp-{}-{}.br", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    std::fs::write(&file, src).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_brasa-parse")).arg(&file).arg("--run").output().unwrap();
    let _ = std::fs::remove_file(&file);
    assert!(out.status.success(), "interpreter failed: {}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).replace('\r', "")
}

fn main_of(body: &str) -> String {
    let indented: String = body.lines().map(|l| format!("    {}\n", l)).collect();
    format!("package main\n\nuse format\n\nfunc main()\n{}end\n", indented)
}

/// Runs the body natively, and in the interpreter when `both`, and checks the output.
fn expect(body: &str, want: &str, both: bool) {
    let src = main_of(body);
    if let Some(out) = native(&src) {
        assert_eq!(out, want, "native output");
    }
    if both {
        assert_eq!(interpreted(&src), want, "interpreter output");
    }
}

/// `format.text` of a template and arguments, printed (or the error message).
fn show(template: &str, args: &str) -> String {
    format!(
        "match format.text(\"{}\", [{}])\n    ok(v) -> print(\"[\" + v + \"]\")\n    err(e) -> print(\"error: \" + e.message)\nend\n",
        template, args
    )
}

#[test]
fn templates_replace_positional_and_indexed_placeholders() {
    expect(&(show("% is %", "\"sky\", \"blue\"") + &show("%1 %0 %1", "\"a\", \"b\"")), "[sky is blue]\n[b a b]\n", false);
}

#[test]
fn a_percent_sign_is_written_twice() {
    expect(&show("100%% %", "\"x\""), "[100% x]\n", false);
}

#[test]
fn missing_arguments_and_unclosed_layouts_are_errors() {
    expect(
        &(show("% %", "\"one\"") + &show("a %(<5", "\"x\"") + &show("%2", "\"x\"") + &show("%(<q)", "\"x\"")),
        "error: missing argument 1\nerror: unclosed '(' in placeholder\nerror: missing argument 2\nerror: bad layout '<q'\n",
        false,
    );
}

#[test]
fn layouts_align_fill_and_pad_with_zeros() {
    expect(
        &(show("[%(<5)] [%(>5)] [%(^5)]", "\"ab\", \"ab\", \"ab\"")
            + &show("%(*^7)", "\"ab\"")
            + &show("%(05)", "\"42\"")
            + &show("%0(>4) %(5)|", "\"7\", \"x\"")
            + &show("%(>3)", "\"abcdef\"")
            + &show("%(>4)", "\"é\"")
            + &show("%(-<4)", "\"é\"")),
        "[[ab   ] [   ab] [ ab  ]]\n[**ab***]\n[00042]\n[   7 7    |]\n[abcdef]\n[   é]\n[é---]\n",
        false,
    );
}

#[test]
fn templates_keep_multibyte_text_whole() {
    expect(&show("ação: %(>5)", "\"é\""), "[ação:     é]\n", false);
}

#[test]
fn padding_helpers() {
    let body = "print(\"[\" + format.pad_left(\"7\", 3) + \"]\")\nprint(\"[\" + format.pad_right(\"7\", 3) + \"]\")\nprint(\"[\" + format.center(\"7\", 4) + \"]\")\nprint(\"[\" + format.pad_left(\"7\", 3, \"0\") + \"]\")\nprint(\"[\" + format.center(\"ab\", 7, \"*\") + \"]\")\nprint(\"[\" + format.pad_left(\"abcdef\", 3) + \"]\")\n";
    expect(body, "[  7]\n[7  ]\n[ 7  ]\n[007]\n[**ab***]\n[abcdef]\n", true);
}

#[test]
fn numbers_as_text() {
    let body = "print(format.int(-42))\nprint(format.hex(255))\nprint(format.bin(5))\nprint(format.hex(-255))\nprint(format.hex(0))\nprint(format.thousands(1234567))\nprint(format.thousands(-1234567))\nprint(format.thousands(999))\nprint(format.thousands(1000))\n";
    expect(body, "-42\nff\n101\n-ff\n0\n1,234,567\n-1,234,567\n999\n1,000\n", true);
}

#[test]
fn fixed_decimals_round_halves_away_from_zero() {
    let body = "print(format.float(3.14159, 2))\nprint(format.float(2.5, 0))\nprint(format.float(-0.005, 2))\nprint(format.float(0.5, 0))\nprint(format.float(1.0, 3))\n";
    expect(body, "3.14\n3\n-0.01\n1\n1.000\n", true);
}

#[test]
fn the_extreme_integers_format_correctly() {
    let body = "let least = 0 - 9223372036854775807 - 1\nprint(format.hex(9223372036854775807))\nprint(format.bin(least).len)\nprint(format.hex(least))\nprint(format.thousands(least))\nprint(format.thousands(9223372036854775807))\n";
    expect(body, "7fffffffffffffff\n65\n-8000000000000000\n-9,223,372,036,854,775,808\n9,223,372,036,854,775,807\n", true);
}

#[test]
fn templates_and_helpers_build_a_table() {
    let body = "let names = [\"apples\", \"fig\", \"kiwi\"]\nlet prices = [1.5, 12.25, 100.0]\nvar i = 0\nwhile i < names.len\n    let row = format.text(\"%(<8)|%(>8)\", [names[i], format.float(prices[i], 2)]) ?? \"?\"\n    print(row)\n    i += 1\nend\n";
    expect(body, "apples  |    1.50\nfig     |   12.25\nkiwi    |  100.00\n", true);
}

#[test]
fn layouts_agree_in_the_interpreter() {
    let body = "print(\"[\" + (format.text(\"%(<5)|%(>5)|%(^5)|%(*^7)|%(05)|%(>4)\", [\"ab\", \"ab\", \"ab\", \"ab\", \"42\", \"é\"]) ?? \"?\") + \"]\")\nprint(format.text(\"%1 %0 %1 100%%\", [\"a\", \"b\"]) ?? \"?\")\n";
    expect(body, "[ab   |   ab| ab  |**ab***|00042|   é]\nb a b 100%\n", true);
}

#[test]
fn the_report_example_agrees_with_the_interpreter() {
    let src = std::fs::read_to_string("examples/report.br").unwrap();
    let interp = interpreted(&src);
    assert!(interp.contains("item    |     qty|     price") && interp.contains("Ação    |       7|    100.00"), "{}", interp);
    assert!(interp.contains("98,000"), "{}", interp);
    if let Some(out) = native(&src) {
        assert_eq!(out, interp);
    }
}
