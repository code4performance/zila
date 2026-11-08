//! `pub`: private-by-default declarations, packages, fields, construction, generics, interfaces.
//! Programs are compiled to assembly only (no native toolchain needed). Each test registers its
//! own library package names, so tests do not interfere.

use zila::codegen_x64::{generate_asm_for, CompileOptions, Target};
use zila::parse_source;

fn compile(src: &str) -> Result<String, String> {
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    generate_asm_for(&file, &CompileOptions::default(), Target::Windows)
}

fn ok(src: &str) {
    if let Err(e) = compile(src) {
        panic!("expected the program to compile: {}", e);
    }
}

fn err(src: &str) -> String {
    match compile(src) {
        Ok(_) => panic!("expected a compile error"),
        Err(e) => e,
    }
}

fn lib(name: &str, body: &str) {
    zila::comptime::register_package(name, &format!("package {}\n{}", name, body));
}

fn main_with(uses: &str, body: &str) -> String {
    format!("package main\n{}\nfunc main()\n{}\nend\n", uses, body)
}

#[test]
fn a_private_helper_is_usable_inside_its_package_only() {
    lib("vis_a", "func helper() -> int\n    return 1\nend\npub func api() -> int\n    return helper() + 1\nend\n");
    ok(&main_with("use vis_a", "    print(vis_a.api())"));
    let e = err(&main_with("use vis_a", "    print(vis_a.helper())"));
    assert!(e.contains("'helper' is private to package 'vis_a'; mark it `pub` to use it from another package"), "{}", e);
    assert!(e.contains("main.br:"), "{}", e);
}

#[test]
fn an_undefined_name_is_not_reported_as_private() {
    lib("vis_b", "pub func api() -> int\n    return 1\nend\n");
    let e = err(&main_with("use vis_b", "    print(vis_b.no_such())"));
    assert!(e.contains("has no function 'no_such'") && !e.contains("private"), "{}", e);
}

#[test]
fn globals_variants_and_function_values_respect_pub() {
    lib(
        "vis_c",
        "let secret = 7\npub let open = 8\ntype Hidden = enum
    A
    B
end\npub type Color = enum
    Red
    Green
end\nfunc quiet() -> int\n    return 1\nend\n",
    );
    ok(&main_with("use vis_c", "    print(vis_c.open)\n    let c = vis_c.Red\n    print(1)"));
    for (code, name) in [("print(vis_c.secret)", "secret"), ("let h = vis_c.A", "A"), ("let f = vis_c.quiet", "quiet")] {
        let e = err(&main_with("use vis_c", &format!("    {}", code)));
        assert!(e.contains(&format!("'{}' is private to package 'vis_c'", name)), "{}: {}", code, e);
    }
}

#[test]
fn private_types_cannot_be_named_from_another_package() {
    lib("vis_d", "type Hidden = record\n    pub a: int\nend\npub type Shown = record\n    pub a: int\nend\ntype Alias = int\n");
    ok(&main_with("use vis_d", "    let s = vis_d.Shown { a: 1 }\n    print(s.a)"));
    let e = err(&main_with("use vis_d", "    let h: vis_d.Hidden = vis_d.Shown { a: 1 }"));
    assert!(e.contains("'Hidden' is private to package 'vis_d'"), "{}", e);
    let e = err(&main_with("use vis_d", "    let x: vis_d.Alias = 1"));
    assert!(e.contains("'Alias' is private to package 'vis_d'"), "{}", e);
}

#[test]
fn private_fields_block_reads_writes_and_literals() {
    lib(
        "vis_e",
        "pub type Buffer = record\n    len: int\n    pub name: text\nend\npub func new_buffer() -> Buffer\n    return Buffer { len: 3, name: \"b\" }\nend\npub type Point = record\n    pub x: int\n    pub y: int\nend\n",
    );
    ok(&main_with("use vis_e", "    let b = vis_e.new_buffer()\n    print(b.name)\n    let p = vis_e.Point { x: 1, y: 2 }\n    print(p.x)"));
    let e = err(&main_with("use vis_e", "    let b = vis_e.new_buffer()\n    print(b.len)"));
    assert!(e.contains("field 'len' of 'vis_e.Buffer' is private to package 'vis_e'"), "{}", e);
    let e = err(&main_with("use vis_e", "    var b = vis_e.new_buffer()\n    b.len = 1"));
    assert!(e.contains("field 'len' of 'vis_e.Buffer' is private to package 'vis_e'"), "{}", e);
    let e = err(&main_with("use vis_e", "    let b = vis_e.Buffer { len: 1, name: \"x\" }"));
    assert!(e.contains("cannot create 'vis_e.Buffer' here: it has private fields; use a function from package 'vis_e'"), "{}", e);
}

#[test]
fn whole_values_pass_freely() {
    lib(
        "vis_f",
        "pub type Buffer = record\n    len: int\nend\npub func new_buffer() -> Buffer\n    return Buffer { len: 3 }\nend\npub func size(b: Buffer) -> int\n    return b.len\nend\n",
    );
    ok(&main_with("use vis_f", "    var all: List<vis_f.Buffer> = []\n    all.add(vis_f.new_buffer())\n    print(vis_f.size(all[0]))"));
}

#[test]
fn methods_need_pub_from_another_package() {
    lib(
        "vis_g",
        "pub type Stack = record\n    pub n: int\nend\npub func (s: Stack) push(x: int)\n    s.n = s.n + x\nend\nfunc (s: Stack) hidden() -> int\n    return s.n\nend\npub func new_stack() -> Stack\n    return Stack { n: 0 }\nend\n",
    );
    ok(&main_with("use vis_g", "    var s = vis_g.new_stack()\n    s.push(2)"));
    let e = err(&main_with("use vis_g", "    var s = vis_g.new_stack()\n    print(s.hidden())"));
    assert!(e.contains("'hidden' is private to package 'vis_g'"), "{}", e);
}

#[test]
fn a_public_signature_may_not_name_a_private_type() {
    let e = err("package main\ntype Handle = record\n    pub a: int\nend\npub func open() -> Handle\n    return Handle { a: 1 }\nend\nfunc main()\nend\n");
    assert!(e.contains("pub func 'open' exposes private type 'Handle'; mark 'Handle' pub or make 'open' private"), "{}", e);
    let e = err("package main\ntype Handle = record\n    a: int\nend\npub type Pair = record\n    pub h: Handle\nend\nfunc main()\nend\n");
    assert!(e.contains("exposes private type 'Handle'"), "{}", e);
    let e = err("package main\ntype Handle = record\n    a: int\nend\npub func take(h: Handle)\nend\nfunc main()\nend\n");
    assert!(e.contains("exposes private type 'Handle'"), "{}", e);
    // a private field of a private type and a private function are fine
    ok("package main\ntype Handle = record\n    a: int\nend\npub type Owner = record\n    h: Handle\nend\nfunc inner(h: Handle)\nend\nfunc main()\nend\n");
}

#[test]
fn generics_keep_the_definers_rights_and_resolve_arguments_in_the_callers_package() {
    lib(
        "vis_h",
        "func twice(x: int) -> int\n    return x * 2\nend\npub func apply<T>(x: T, f: func(T) -> int) -> int\n    return twice(f(x))\nend\npub type Box<T> = record\n    v: T\nend\npub func box<T>(x: T) -> Box<T>\n    return Box { v: x }\nend\n",
    );
    ok(
        "package main\nuse vis_h\ntype Mine = record\n    n: int\nend\nfunc main()\n    let m = Mine { n: 4 }\n    print(vis_h.apply(m, func(x) x.n end))\n    let b = vis_h.box(m)\nend\n",
    );
    let e = err(&main_with("use vis_h", "    print(vis_h.twice(1))"));
    assert!(e.contains("'twice' is private to package 'vis_h'"), "{}", e);
}

#[test]
fn only_public_methods_satisfy_an_interface_of_another_package() {
    lib("vis_i", "pub type Printable = interface\n    func show(self) -> text\nend\npub func render<T: Printable>(x: T) -> text\n    return x.show()\nend\n");
    let good = "package main\nuse vis_i\ntype P = record\n    n: int\nend\npub func (p: P) show() -> text\n    return \"p\"\nend\nfunc main()\n    print(vis_i.render(P { n: 1 }))\nend\n";
    ok(good);
    let bad = good.replace("pub func (p: P) show", "func (p: P) show");
    let e = err(&bad);
    assert!(e.contains("'show' is private to package 'main'"), "{}", e);
}

#[test]
fn the_compiler_needs_no_pub_for_main_tests_and_inserted_calls() {
    ok("package main\nfunc test_one()\nend\nfunc main()\n    print(1)\nend\n");
    ok("package main\nuse format\nfunc main()\n    print(format.fmt(\"%\", 1) ?? \"\")\nend\n");
}

#[test]
fn the_standard_library_exports_what_programs_use() {
    ok(&main_with("use text\nuse list\nuse math", "    print(text.trim(\" a \"))\n    print(list.map([1], func(x) x end).len)\n    print(math.abs(-1.0))"));
}
