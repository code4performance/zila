//! Generics and interfaces: methods on any type, monomorphization, inference, bounds. Programs are
//! compiled to native executables and run (skipped when the Visual Studio build tools are
//! missing).

use zila::{compile_to_native, parse_source};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

fn run(src: &str) -> Option<(String, i32)> {
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = std::env::temp_dir().join(format!("zila-generics-test-{}-{}.exe", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
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
    let exe = std::env::temp_dir().join(format!("zila-generics-test-{}-{}.exe", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    match compile_to_native(&file, exe.to_str().unwrap()) {
        Ok(()) => panic!("expected a compile error"),
        Err(e) => e,
    }
}


fn expect(src: &str, want: &str) {
    let Some((out, code)) = run(src) else { return };
    assert_eq!(out, want, "program output");
    assert_eq!(code, 0);
}

#[test]
fn methods_on_builtin_types() {
    expect(
        r#"package main

func (n: int) double() -> int
    return n * 2
end

func (n: int) describe() -> text
    return "int {n}"
end

func (s: text) shout() -> text
    return s + "!"
end

func (xs: []int) total() -> int
    var t = 0
    for x in xs
        t += x
    end
    return t
end

func main()
    println(21.double(), 5.describe())
    let n = 4
    println(n.double().double())
    println("hey".shout())
    println([1, 2, 3].total())
end
"#,
        "42 int 5
16
hey!
6
",
    );
}

fn fails(src: &str, needles: &[&str]) {
    let e = compile_error(src);
    for n in needles {
        assert!(e.contains(n), "expected {:?} in: {}", n, e);
    }
}

const PRELUDE: &str = r#"package main

type Printable = interface
    func to_text(self) -> text
end

type Point = record
    x: int
    y: int
end

type Dog = record
    name: text
end

type Cat = record
    lives: int
end

func (p: Point) to_text() -> text
    return "({p.x}, {p.y})"
end

func (c: Cat) to_text() -> int
    return c.lives
end

"#;

fn with_prelude(body: &str) -> String {
    format!("{}{}", PRELUDE, body)
}

#[test]
fn generic_functions_and_inference() {
    expect(
        r#"package main

func first<T>(items: []T) -> T
    return items[0]
end

func pair<A, B>(a: A, b: B) -> text
    return "{a} {b}"
end

func empty<T>() -> []T
    let xs: []T = []
    return xs
end

func twice<T>(x: T, f: func(T) -> T) -> T
    return f(f(x))
end

func main()
    println(first([1, 2, 3]), first(["a", "b"]), first([1.5, 2.5]))
    println(pair(1, "x"), pair("y", true))
    let xs: []int = empty()
    xs.add(7)
    println(xs.len, xs[0])
    println(twice(3, func(n) n * 2 end))
end
"#,
        "1 a 1.5\n1 x y true\n1 7\n12\n",
    );
}

#[test]
fn inference_errors() {
    fails(
        "package main\nfunc empty<T>() -> []T\n    let xs: []T = []\n    return xs\nend\nfunc main()\n    let a = empty()\nend\n",
        &["cannot infer the type of T"],
    );
    fails(
        "package main\nfunc same<T>(a: T, b: T) -> T\n    return a\nend\nfunc main()\n    let n = 1
    let a = same(n, \"x\")\nend\n",
        &["T is both", "int", "text"],
    );
}

#[test]
fn generic_records_and_methods() {
    expect(
        r#"package main

type Stack<T> = record
    items: []T
end

func (s: Stack<T>) push(x: T)
    s.items.add(x)
end

func (s: Stack<T>) pop() -> T
    return s.items.take()
end

func (s: Stack<T>) size() -> int
    return s.items.len
end

func main()
    let a: Stack<int> = Stack { items: [] }
    a.push(1)
    a.push(2)
    let b: Stack<text> = Stack { items: ["x"] }
    b.push("y")
    println(a.size(), a.pop(), a.pop())
    println(b.size(), b.pop(), b.pop())
end
"#,
        "2 2 1\n2 y x\n",
    );
}

#[test]
fn generic_record_errors() {
    let decl = "package main\ntype Stack<T> = record\n    items: []T\nend\n";
    fails(&format!("{}func take(s: Stack<int>)\nend\nfunc main()\n    let a: Stack<text> = Stack {{ items: [] }}\n    take(a)\nend\n", decl), &["Stack<int>", "Stack<text>"]);
    fails(&format!("{}func main()\n    let a: Stack<int, text> = Stack {{ items: [] }}\nend\n", decl), &["Stack takes 1 type argument"]);
    fails(&format!("{}func main()\n    let a = Stack {{ items: [] }}\nend\n", decl), &["cannot infer the type arguments of Stack"]);
}

#[test]
fn interfaces_and_bounds() {
    expect(
        &with_prelude(
            r#"func (n: int) to_text() -> text
    return "int {n}"
end

func show<T: Printable>(x: T) -> text
    return "<" + x.to_text() + ">"
end

type Box<T: Printable> = record
    item: T
end

func (b: Box<T>) describe() -> text
    return "box of " + b.item.to_text()
end

func main()
    println(show(Point { x: 1, y: 2 }), show(42))
    let b: Box<Point> = Box { item: Point { x: 3, y: 4 } }
    println(b.describe())
end
"#,
        ),
        "<(1, 2)> <int 42>\nbox of (3, 4)\n",
    );
}

#[test]
fn bound_errors() {
    fails(
        &with_prelude("func show<T: Printable>(x: T) -> text\n    return x.to_text()\nend\nfunc main()\n    show(Dog { name: \"rex\" })\nend\n"),
        &["Dog does not satisfy Printable", "no method 'to_text'"],
    );
    fails(
        &with_prelude("func show<T: Printable>(x: T) -> text\n    return x.to_text()\nend\nfunc main()\n    show(Cat { lives: 9 })\nend\n"),
        &["Cat does not satisfy Printable", "returns int where Printable needs text"],
    );
    fails(
        &with_prelude("type Box<T: Printable> = record\n    item: T\nend\nfunc main()\n    let b: Box<Dog> = Box { item: Dog { name: \"a\" } }\nend\n"),
        &["Dog does not satisfy Printable"],
    );
    fails(
        &with_prelude("func show<T: Dog>(x: T)\nend\nfunc main()\nend\n"),
        &["must be an interface"],
    );
}

#[test]
fn errors_at_the_generic_definition() {
    fails(
        &with_prelude("func show<T>(x: T) -> text\n    return x.to_text()\nend\nfunc main()\nend\n"),
        &["T has no method 'to_text'", "bound"],
    );
    fails(
        "package main\nfunc add<T>(a: T, b: T) -> T\n    return a + b\nend\nfunc main()\nend\n",
        &["operator '+' is not available for T"],
    );
}

#[test]
fn interface_declaration_errors() {
    fails(
        "package main\ntype P = interface\n    func a(self)\n    func a(self)\nend\nfunc main()\nend\n",
        &["declares method 'a' twice"],
    );
    fails(&with_prelude("func show(x: Printable)\nend\nfunc main()\nend\n"), &["interfaces can only bound type parameters", "<T: Printable>"]);
}

#[test]
fn unsupported_forms() {
    fails("package main\ntype Opt<T> = enum\n    Some(T)\n    None\nend\nfunc main()\nend\n", &["generic enums are not supported"]);
    fails("package main\ntype L<T> = []T\nfunc main()\nend\n", &["generic type aliases are not supported"]);
    fails("package main\nasync func f<T>(x: T) -> T\n    return x\nend\nfunc main()\nend\n", &["async generic functions are not supported"]);
}

#[test]
fn generics_do_not_leak() {
    expect(
        r#"package main

type Pair<A, B> = record
    a: A
    b: B
end

func make<T>(x: T, n: int) -> []T
    let xs: []T = []
    for i in 0..n
        xs.add(x)
    end
    return xs
end

func swap<A, B>(p: Pair<A, B>) -> Pair<B, A>
    return Pair { b: p.a, a: p.b }
end

func main()
    var before = 0
    for round in 0..400
        let words = make("w{round}", 3)
        let p: Pair<text, []text> = Pair { a: "k", b: words }
        let q: Pair<[]text, text> = swap(p)
        if q.a.len != 3
            println("bad")
        end
        if round == 1
            before = live_objects()
        end
        if round == 399
            println("grew {live_objects() - before}")
        end
    end
end
"#,
        "grew 0\n",
    );
}

#[test]
fn arguments_are_evaluated_once_and_instances_nest() {
    expect(
        r#"package main

var calls = 0

func next_list() -> []int
    calls += 1
    return [calls, calls + 10]
end

func first<T>(items: []T) -> T
    return items[0]
end

type Box<T> = record
    value: T
end

func (b: Box<T>) get() -> T
    return b.value
end

type IntBox = Box<int>

func wrap<T>(x: T) -> Box<T>
    return Box { value: x }
end

func main()
    println(first(next_list()), first(next_list()), calls)
    let nested: Box<Box<int>> = wrap(wrap(5))
    println(nested.get().get())
    let boxes: []Box<text> = [wrap("a"), wrap("b")]
    println(boxes[1].get(), first(boxes).get())
    let m: Map<text, Box<int>> = { "k": wrap(9) }
    println(m["k"].get())
    let keep = wrap(3.5)
    let f = func(n: int) float(n) + keep.get() end
    println(f(2))
    let ch = channel(IntBox)
    ch.send(wrap(4))
    println(ch.receive().get())
end
"#,
        "1 2 2\n5\nb a\n9\n5.5\n4\n",
    );
}

#[test]
fn the_generics_example_prints_what_the_docs_say() {
    let src = std::fs::read_to_string("examples/generics.zila").unwrap();
    if let Some((out, code)) = run(&src) {
        assert_eq!(code, 0);
        assert_eq!(out, "popped 4
popped alan
first of two lists: 10 and x
showing (1, 2)
showing dog rex
showing the number 42
");
    }
}

#[test]
fn explicit_type_arguments() {
    expect(
        r#"package main

func empty<T>() -> []T
    let xs: []T = []
    return xs
end

func pair<A, B>(a: A, b: B) -> text
    return "{a}/{b}"
end

func main()
    let n = empty<int>()
    println(n.len)
    println(pair<int, text>(1, "x"))
    let a = 1
    let b = 2
    let c = 3
    println(a < b, b > (c))
end
"#,
        "0\n1/x\ntrue false\n",
    );
}

#[test]
fn too_many_type_arguments() {
    let e = compile_error(
        r#"package main

func id<T>(x: T) -> T
    return x
end

func main()
    println(id<int, int>(1))
end
"#,
    );
    assert!(e.contains("type argument"), "{}", e);
}

#[test]
fn self_in_a_user_interface() {
    expect(
        r#"package main

type Shape = interface
    func bigger(self, other: Self) -> bool
end

type Square = record
    side: int
end

func (a: Square) bigger(b: Square) -> bool
    return a.side > b.side
end

func pick<T: Shape>(a: T, b: T) -> bool
    return a.bigger(b)
end

func main()
    println(pick(Square { side: 3 }, Square { side: 2 }))
end
"#,
        "true\n",
    );
    let e = compile_error(
        r#"package main

type Shape = interface
    func bigger(self, other: Self) -> bool
end

type Odd = record
    side: int
end

func (a: Odd) bigger(b: int) -> bool
    return a.side > b
end

func pick<T: Shape>(a: T, b: T) -> bool
    return a.bigger(b)
end

func main()
    println(pick(Odd { side: 3 }, Odd { side: 2 }))
end
"#,
    );
    assert!(e.contains("does not satisfy Shape"), "{}", e);
}

#[test]
fn self_outside_an_interface() {
    let e = compile_error(
        r#"package main

func f(a: Self) -> int
    return 1
end

func main()
    println(1)
end
"#,
    );
    assert!(e.contains("`Self` is only valid inside an interface"), "{}", e);
}
