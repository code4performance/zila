//! Native backend: assembly generation (does not need ml64/link).

use brasa::codegen_x64::generate_asm;
use brasa::parse_source;

fn asm(src: &str) -> Result<String, String> {
    let f = parse_source(src).map_err(|d| d.msg)?;
    generate_asm(&f)
}

#[test]
fn examples_compile() {
    for p in ["examples/hello.br", "examples/ui.br"] {
        let src = std::fs::read_to_string(p).unwrap();
        let a = asm(&src).unwrap_or_else(|e| panic!("{}: {}", p, e));
        assert!(a.contains("brasa_start proc"), "{}", p);
    }
}

#[test]
fn ui_is_compiled_from_brasa_source() {
    let a = asm("package main\nuse ui\nfunc main()\n    ui.run()\nend\n").unwrap();
    // The std library's functions are emitted as Brasa procedures...
    assert!(a.contains("B_ui_wndproc proc"));
    assert!(a.contains("B_ui_Control_layout proc"));
    // ...and call Win32 directly.
    assert!(a.contains("extern CreateWindowExA:proc"));
}

#[test]
fn extern_requires_unsafe() {
    let e = asm("package main\nextern func Beep(f: u32, d: u32) -> i32\nfunc main()\n    Beep(440, 100)\nend\n").unwrap_err();
    assert!(e.contains("requires an 'unsafe' block"), "{}", e);
}

#[test]
fn captured_variables_are_read_only() {
    let src = "package main\nfunc main()\n    var n = 0\n    let f = func()\n        n += 1\n    end\nend\n";
    let e = asm(src).unwrap_err();
    assert!(e.contains("capture variables by value"), "{}", e);
}

#[test]
fn let_is_immutable() {
    let e = asm("package main\nfunc main()\n    let x = 1\n    x = 2\nend\n").unwrap_err();
    assert!(e.contains("declared with 'let'"), "{}", e);
}

#[test]
fn type_errors_are_reported_with_line() {
    let e = asm("package main\nfunc f(s: text)\nend\nfunc main()\n    f(42)\nend\n").unwrap_err();
    assert!(e.starts_with("main.br:5:"), "{}", e);
    assert!(e.contains("expected text, found i64"), "{}", e);
}

#[test]
fn match_must_be_exhaustive() {
    let src = "package main\ntype C = enum\n    A, B, D\nend\nfunc f(c: C) -> int\n    match c\n        A -> return 1\n        B -> return 2\n    end\n    return 0\nend\nfunc main()\nend\n";
    let e = asm(src).unwrap_err();
    assert!(e.contains("not exhaustive") && e.contains("D"), "{}", e);
    let ok = src.replace("B -> return 2\n", "B -> return 2\n        _ -> return 3\n");
    assert!(asm(&ok).is_ok());
}

#[test]
fn result_must_be_matched_with_ok_and_err() {
    let src = "package main\nfunc g() -> int or error\n    return 1\nend\nfunc main()\n    match g()\n        ok(v) -> print(v)\n    end\nend\n";
    let e = asm(src).unwrap_err();
    assert!(e.contains("missing err"), "{}", e);
}

#[test]
fn try_needs_a_fallible_function() {
    let src = "package main\nfunc g() -> int or error\n    return 1\nend\nfunc main()\n    let x = try g()\nend\n";
    let e = asm(src).unwrap_err();
    assert!(e.contains("'try' can only be used"), "{}", e);
}

#[test]
fn ignoring_a_failable_call_is_an_error() {
    let src = "package main\nfunc g() -> error\n    return\nend\nfunc main()\n    g()\nend\n";
    let e = asm(src).unwrap_err();
    assert!(e.contains("result is ignored"), "{}", e);
}

#[test]
fn spawn_pulls_in_the_task_library() {
    let a = asm("package main\nfunc w()\nend\nfunc main()\n    spawn w()\n    let c = channel(int)\n    c.send(1)\nend\n").unwrap();
    assert!(a.contains("B_task_start proc"));
    assert!(a.contains("B_task_chan_send proc"));
    assert!(a.contains("extern CreateFiber:proc"));
}

#[test]
fn await_needs_an_async_function_or_main() {
    let e = asm("package main
async func g() -> int
    return 1
end
func helper() -> int
    return await g()
end
func main()
end
").unwrap_err();
    assert!(e.contains("`await` can only be used inside an `async func` or inside `main`"), "{}", e);
}

#[test]
fn async_functions_cannot_take_ref_parameters() {
    let src = "package main
type R = record
    x: int
end
async func g(ref r: R)
end
func main()
end
";
    let e = asm(src).unwrap_err();
    assert!(e.contains("cannot take `ref` parameters"), "{}", e);
}
