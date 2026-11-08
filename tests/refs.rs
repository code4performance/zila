//! The `ref` escape rule (spec `ref-parameters`): compile-error style, no linker needed.

use zila::codegen_x64::generate_asm;
use zila::parse_source;

fn asm(src: &str) -> Result<String, String> {
    let f = parse_source(src).map_err(|d| d.msg)?;
    generate_asm(&f)
}

const HEAD: &str = "package main
type R = record
    x: int
    name: text
    items: []int
end
type Holder = record
    last: R
    r: R
end
var global: R
func g(ref r: R)
    r.x = 1
end
func take(r: R)
end
";

/// A program with `func f(ref r: R, ref v: int)` whose body is `body`.
fn prog(ret: &str, body: &str) -> String {
    format!("{HEAD}func f(ref r: R, ref v: int){ret}\n{body}\nend\nfunc main()\nend\n")
}

fn rejected(body: &str) {
    rejected_with("", body)
}

fn rejected_with(ret: &str, body: &str) {
    let e = asm(&prog(ret, body)).unwrap_err();
    assert!(e.contains("cannot escape") && e.contains("use `new T`"), "{}: {}", body, e);
}

fn accepted(ret: &str, body: &str) {
    if let Err(e) = asm(&prog(ret, body)) {
        panic!("{}: {}", body, e);
    }
}

#[test]
fn return_is_rejected() {
    rejected_with(" -> R", "    return r");
}

#[test]
fn stores_are_rejected() {
    rejected("    var h = Holder{}\n    h.last = r");
    rejected("    var xs: []R = []\n    xs.add(r)");
    rejected("    var m: Map<int, R> = {}\n    m[1] = r");
    rejected("    let a = [r]");
    rejected("    let h = Holder{r: r}");
    rejected("    global = r");
}

#[test]
fn assignment_to_a_variable_is_rejected() {
    rejected("    let c = r");
    rejected("    var c = r");
    rejected("    var c = Holder{}.r\n    c = r");
}

#[test]
fn lambdas_cannot_mention_a_ref() {
    rejected("    let k = func()\n        take(r)\n    end");
    rejected("    let k = func()\n        return r.x\n    end");
    rejected("    (func()\n        let n = r.x\n    end)()");
}

#[test]
fn spawn_is_rejected() {
    let head = format!(
        "{}func work(r: R)\nend\nfunc work2(ref r: R)\nend\n",
        HEAD.replacen("package main\n", "package main\nuse task\n", 1)
    );
    for body in ["spawn work(r)", "spawn work2(ref r)"] {
        let src = format!("{head}func f(ref r: R)\n    {body}\nend\nfunc main()\nend\n");
        let e = asm(&src).unwrap_err();
        assert!(e.contains("cannot escape"), "{}: {}", body, e);
    }
}

#[test]
fn async_call_is_rejected() {
    let src = format!("{HEAD}async func a(r: R)\nend\nfunc f(ref r: R)\n    a(r)\nend\nfunc main()\nend\n");
    let e = asm(&src).unwrap_err();
    assert!(e.contains("cannot escape"), "{}", e);
}

#[test]
fn address_of_is_rejected() {
    rejected("    let p = &r");
    rejected("    let p = &v");
}

#[test]
fn by_value_argument_is_rejected() {
    rejected("    take(r)");
}

#[test]
fn ref_receivers_are_restricted() {
    let bad = |body: &str, ret: &str| {
        let src = format!("{HEAD}func (ref s: R) m(){ret}\n    {body}\nend\nfunc main()\nend\n");
        let e = asm(&src).unwrap_err();
        assert!(e.contains("cannot escape") && e.contains("use `new T`"), "{}: {}", body, e);
    };
    bad("return s", " -> R");
    bad("var xs: []R = []\n    xs.add(s)", "");
    bad("let t = s", "");
}

#[test]
fn a_lambdas_own_ref_parameter_cannot_be_returned() {
    rejected("    let k = func(ref p: R)\n        return p\n    end");
}

#[test]
fn borrowing_uses_are_accepted() {
    accepted("", "    r.x = 4\n    r.name = \"a\"\n    let n = r.name\n    r.items.add(1)\n    for a in r.items\n        let b = a\n    end\n    r.items[0] = 2\n    let l = r.items.len");
}

#[test]
fn passing_down_is_accepted() {
    accepted("", "    g(ref r)");
    let src = format!("{HEAD}func h(ref r: R)\n    g(ref r)\nend\nfunc f(ref r: R)\n    h(ref r)\nend\nfunc main()\nend\n");
    asm(&src).unwrap();
}

#[test]
fn methods_on_a_ref_are_accepted() {
    let src = format!("{HEAD}func (ref s: R) bump()\n    s.x += 1\n    g(ref s)\n    s.other()\nend\nfunc (s: R) other()\nend\nfunc f(ref r: R)\n    r.bump()\nend\nfunc main()\nend\n");
    asm(&src).unwrap();
}

#[test]
fn scalar_refs_can_be_read() {
    accepted(" -> int", "    v += 10\n    return v");
    accepted("", "    let c = v\n    var d = c + v");
}

#[test]
fn shadowing_is_respected() {
    accepted(" -> R", "    let r = Holder{}.r\n    return r");
    accepted("", "    let k = func(r: R)\n        take(r)\n    end");
}

#[test]
fn unsafe_is_exempt() {
    accepted("", "    unsafe\n        let p = &r\n        global = r\n    end");
}

#[test]
fn async_method_cannot_take_a_ref_receiver() {
    let src = format!("{HEAD}async func (ref s: R) m()\nend\nfunc main()\nend\n");
    let e = asm(&src).unwrap_err();
    assert!(e.contains("cannot take `ref` parameters"), "{}", e);
}
