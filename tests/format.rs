//! The `format` library: templates, layout, padding and number helpers. Programs are compiled
//! to native executables and run (skipped when the Visual Studio build tools are missing).

use zila::{compile_to_native, parse_source};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

fn native(src: &str) -> Option<String> {
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = std::env::temp_dir().join(format!("zila-format-test-{}-{}.exe", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
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

fn main_of(body: &str) -> String {
    let indented: String = body.lines().map(|l| format!("    {}\n", l)).collect();
    format!("package main\n\nuse format\n\nfunc main()\n{}end\n", indented)
}

/// Runs the body natively and checks the output.
fn expect(body: &str, want: &str) {
    let src = main_of(body);
    if let Some(out) = native(&src) {
        assert_eq!(out, want, "native output");
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
    expect(&(show("% is %", "\"sky\", \"blue\"") + &show("%1 %0 %1", "\"a\", \"b\"")), "[sky is blue]\n[b a b]\n");
}

#[test]
fn a_percent_sign_is_written_twice() {
    expect(&show("100%% %", "\"x\""), "[100% x]\n");
}

#[test]
fn missing_arguments_and_unclosed_layouts_are_errors() {
    expect(
        &(show("% %", "\"one\"") + &show("a %(<5", "\"x\"") + &show("%2", "\"x\"") + &show("%(<q)", "\"x\"")),
        "error: missing argument 1\nerror: unclosed '(' in placeholder\nerror: missing argument 2\nerror: bad layout '<q'\n",
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
    );
}

#[test]
fn templates_keep_multibyte_text_whole() {
    expect(&show("ação: %(>5)", "\"é\""), "[ação:     é]\n");
}

#[test]
fn padding_helpers() {
    let body = "print(\"[\" + format.pad_left(\"7\", 3) + \"]\")\nprint(\"[\" + format.pad_right(\"7\", 3) + \"]\")\nprint(\"[\" + format.center(\"7\", 4) + \"]\")\nprint(\"[\" + format.pad_left(\"7\", 3, \"0\") + \"]\")\nprint(\"[\" + format.center(\"ab\", 7, \"*\") + \"]\")\nprint(\"[\" + format.pad_left(\"abcdef\", 3) + \"]\")\n";
    expect(body, "[  7]\n[7  ]\n[ 7  ]\n[007]\n[**ab***]\n[abcdef]\n");
}

#[test]
fn numbers_as_text() {
    let body = "print(format.int(-42))\nprint(format.hex(255))\nprint(format.bin(5))\nprint(format.hex(-255))\nprint(format.hex(0))\nprint(format.thousands(1234567))\nprint(format.thousands(-1234567))\nprint(format.thousands(999))\nprint(format.thousands(1000))\n";
    expect(body, "-42\nff\n101\n-ff\n0\n1,234,567\n-1,234,567\n999\n1,000\n");
}

#[test]
fn fixed_decimals_round_halves_away_from_zero() {
    let body = "print(format.float(3.14159, 2))\nprint(format.float(2.5, 0))\nprint(format.float(-0.005, 2))\nprint(format.float(0.5, 0))\nprint(format.float(1.0, 3))\n";
    expect(body, "3.14\n3\n-0.01\n1\n1.000\n");
}

#[test]
fn the_extreme_integers_format_correctly() {
    let body = "let big: long = 9223372036854775807\nlet least = 0 - big - 1\nprint(format.hex(big))\nprint(format.bin(least).len)\nprint(format.hex(least))\nprint(format.thousands(least))\nprint(format.thousands(big))\n";
    expect(body, "7fffffffffffffff\n65\n-8000000000000000\n-9,223,372,036,854,775,808\n9,223,372,036,854,775,807\n");
}

#[test]
fn templates_and_helpers_build_a_table() {
    let body = "let names = [\"apples\", \"fig\", \"kiwi\"]\nlet prices = [1.5, 12.25, 100.0]\nvar i = 0\nwhile i < names.len\n    let row = format.text(\"%(<8)|%(>8)\", [names[i], format.float(prices[i], 2)]) ?? \"?\"\n    print(row)\n    i += 1\nend\n";
    expect(body, "apples  |    1.50\nfig     |   12.25\nkiwi    |  100.00\n");
}

#[test]
fn layouts_pad_and_align() {
    let body = "print(\"[\" + (format.text(\"%(<5)|%(>5)|%(^5)|%(*^7)|%(05)|%(>4)\", [\"ab\", \"ab\", \"ab\", \"ab\", \"42\", \"é\"]) ?? \"?\") + \"]\")\nprint(format.text(\"%1 %0 %1 100%%\", [\"a\", \"b\"]) ?? \"?\")\n";
    expect(body, "[ab   |   ab| ab  |**ab***|00042|   é]\nb a b 100%\n");
}

#[test]
fn the_report_example_runs() {
    let src = std::fs::read_to_string("examples/report.zila").unwrap();
    if let Some(out) = native(&src) {
        assert!(out.contains("item    |     qty|     price") && out.contains("Ação    |       7|    100.00"), "{}", out);
        assert!(out.contains("98,000"), "{}", out);
    }
}

// ---- values of any type (native only) ----

/// `format.fmt(template, args)` printed (or the error message).
fn fmt_show(template: &str, args: &str) -> String {
    format!(
        "match format.fmt(\"{}\"{})\n    ok(v) -> print(\"[\" + v + \"]\")\n    err(e) -> print(\"error: \" + e.message)\nend\n",
        template,
        if args.is_empty() { String::new() } else { format!(", {}", args) }
    )
}

#[test]
fn values_of_any_type_fill_the_placeholders() {
    expect(
        "print(format.fmt(\"% has % items and costs % (paid: %)\", \"ana\", 3, 2.5, true) ?? \"?\")\nprint(format.fmt(\"%1 %0 %1\", 7, \"x\") ?? \"?\")\nprint(format.fmt(\"[%(>5)]\", 42) ?? \"?\")\nprint(format.fmt(\"100%% sure\") ?? \"?\")\n",
        "ana has 3 items and costs 2.5 (paid: true)\nx 7 x\n[   42]\n100% sure\n",
    );
}

#[test]
fn other_integer_widths_and_f32_are_values_too() {
    expect(
        "let small: i32 = 7\nlet tiny: u8 = 200\nlet big: u64 = 5\nlet half: f32 = 1.5\nprint(format.fmt(\"% % % %(.1f)\", small, tiny, big, half) ?? \"?\")\n",
        "7 200 5 1.5\n",
    );
}

#[test]
fn a_type_of_the_program_is_formatted_through_to_value() {
    let src = "package main\n\nuse format\n\ntype Point = record\n    x: int\n    y: int\nend\n\npub func (p: Point) to_value() -> format.Value\n    return format.Text(\"({p.x}, {p.y})\")\nend\n\nfunc main()\n    print(format.fmt(\"at %, %(>8)|\", Point { x: 1, y: 2 }, Point { x: 3, y: 4 }) ?? \"?\")\nend\n";
    if let Some(out) = native(src) {
        assert_eq!(out, "at (1, 2),   (3, 4)|\n");
    }
}

#[test]
fn type_letters_format_integers() {
    expect(
        &fmt_show("%(x) %(X) %(b) %(,) %(d)", "255, 255, 5, 1234567, 0 - 9"),
        "[ff FF 101 1,234,567 -9]\n",
    );
}

#[test]
fn width_fill_and_zeros_combine_with_types() {
    expect(&fmt_show("%(08x)|%(>10,)|%(*^9d)", "255, 1234567, 0 - 42"), "[000000ff| 1,234,567|***-42***]\n");
}

#[test]
fn floats_take_a_precision_and_round_halves_away_from_zero() {
    expect(
        &fmt_show("%(.2f)|%(f)|%(>8.1f)|%(.0f)|%(.1)", "3.14159, 2.5, 0.0 - 0.05, 2.5, 0.25"),
        "[3.14|2.500000|    -0.1|3|0.3]\n",
    );
}

#[test]
fn the_extreme_integers_work_with_types() {
    expect(
        &format!("let big: long = 9223372036854775807
{}", fmt_show("%(x) %(b)", "big, 0 - 1")),
        "[7fffffffffffffff -1]\n",
    );
}

#[test]
fn letters_that_do_not_fit_are_errors_that_name_them() {
    let body = fmt_show("%(x)", "\"text\"") + &fmt_show("%(f)", "3") + &fmt_show("%(.2)", "3") + &fmt_show("%(,)", "1.5") + &fmt_show("%(.2d)", "3") + &fmt_show("%(q)", "1") + &fmt_show("%(.f)", "1.5");
    expect(
        &body,
        "error: 'x' needs an integer, got text\nerror: 'f' needs a float, got integer\nerror: a precision needs a float, got integer\nerror: ',' needs an integer, got float\nerror: a precision goes with 'f', not 'd'\nerror: bad layout 'q'\nerror: bad layout '.f'\n",
    );
}

#[test]
fn template_errors_are_the_same_with_values() {
    expect(
        &(fmt_show("% %", "1") + &fmt_show("a %(<5", "1") + &fmt_show("%2", "1")),
        "error: missing argument 1\nerror: unclosed '(' in placeholder\nerror: missing argument 2\n",
    );
}

#[test]
fn eight_values_are_allowed_and_nine_are_not() {
    expect("print(format.fmt(\"%%%%%%%%%%%%%%%%\", 1, 2, 3, 4, 5, 6, 7, 8) ?? \"?\")\n", "%%%%%%%%\n");
    let nine = main_of("print(format.fmt(\"%\", 1, 2, 3, 4, 5, 6, 7, 8, 9) ?? \"?\")\n");
    let file = parse_source(&nine).unwrap();
    let e = zila::codegen_x64::generate_asm(&file).unwrap_err();
    assert!(e.contains("format.fmt takes at most eight values"), "{}", e);
}

#[test]
fn text_and_the_helpers_run() {
    let body = "print(format.text(\"%(>5)|%(05)\", [\"ab\", \"42\"]) ?? \"?\")\nprint(format.hex(255))\nprint(format.thousands(1234567))\n";
    expect(body, "   ab|00042\nff\n1,234,567\n");
}

#[test]
fn the_values_example_runs() {
    let src = std::fs::read_to_string("examples/values.zila").unwrap();
    if let Some(out) = native(&src) {
        assert!(out.contains("Apples  |   1,200|      1.50") && out.contains("hex 000000ff, binary 1010, upper BEEF"), "{}", out);
        assert!(out.contains("ready: true, ratio: 0.3, at (3, 4)"), "{}", out);
    }
}
