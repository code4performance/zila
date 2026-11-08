//! `new`, `Rc<T>`, `Weak<T>`, `weak(x)` and `upgrade()`. Programs are compiled to native
//! executables and run (skipped when the Visual Studio build tools are missing).

use zila::{compile_to_native, parse_source};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

fn run(src: &str) -> Option<(String, i32)> {
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d));
    let exe = std::env::temp_dir().join(format!("zila-weak-test-{}-{}.exe", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    match compile_to_native(&file, exe.to_str().unwrap()) {
        Ok(()) => {}
        Err(e) if e.contains("could not find Visual Studio") || e.contains("vcvars64") => {
            eprintln!("no native toolchain: skipping");
            return None;
        }
        Err(e) => panic!("compile error: {}", e),
    }
    let out = Command::new(&exe).output().unwrap();
    let _ = std::fs::remove_file(&exe);
    Some((String::from_utf8_lossy(&out.stdout).replace('\r', ""), out.status.code().unwrap_or(-1)))
}

/// The error a program is rejected with (at parse or compile time).
fn rejected(src: &str) -> String {
    let file = match parse_source(src) {
        Ok(f) => f,
        Err(d) => return d.to_string(),
    };
    let exe = std::env::temp_dir().join(format!("zila-weak-test-{}-{}.exe", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    match compile_to_native(&file, exe.to_str().unwrap()) {
        Ok(()) => {
            let _ = std::fs::remove_file(&exe);
            panic!("expected a compile error")
        }
        Err(e) => e,
    }
}

fn check(body: &str, expected: &str) {
    let src = format!("package main\n\ntype Node = record\n    value: int\nend\n\nfunc main() -> error\n{}\n    return\nend\n", body);
    let Some((out, code)) = run(&src) else { return };
    assert_eq!(out, expected, "program:\n{}", src);
    assert_eq!(code, 0);
}

fn expect(src: &str, expected: &str) {
    let Some((out, code)) = run(src) else { return };
    assert_eq!(out, expected, "program:\n{}", src);
    assert_eq!(code, 0);
}

#[test]
fn create_and_read() {
    check("    let n = new Node{ value: 7 }\n    print(\"{n.value}\")", "7\n");
}

#[test]
fn missing_literal_and_non_record() {
    let e = rejected("package main\ntype Node = record\n    value: int\nend\nfunc main()\n    let n = new Node\nend\n");
    assert!(e.contains("record literal"), "{}", e);
    let e = rejected("package main\nfunc main()\n    let n = new int\nend\n");
    assert!(e.contains("record literal"), "{}", e);
}

#[test]
fn two_names_one_object() {
    check(
        "    let a = new Node{ value: 1 }\n    let b = a\n    b.value = 5\n    print(\"{a.value}\")\n    a.value = 8\n    print(\"{b.value}\")",
        "5\n8\n",
    );
}

#[test]
fn passed_to_a_function_and_ref_method() {
    expect(
        r#"package main

type Node = record
    value: int
end

func (ref n: Node) bump()
    n.value = n.value + 1
end

func set(n: Rc<Node>)
    n.value = 9
end

func main() -> error
    let n = new Node{ value: 1 }
    set(n)
    print("{n.value}")
    n.bump()
    let m = n
    m.bump()
    print("{n.value}")
    return
end
"#,
        "9\n11\n",
    );
}

#[test]
fn reference_counted_in_a_list() {
    expect(
        r#"package main

type Node = record
    value: int
end

func fill()
    let before = live_objects()
    let n = new Node{ value: 1 }
    var xs: []Rc<Node> = []
    xs.push(n)
    xs.push(n)
    print("{live_objects() - before}")
end

func main() -> error
    let before = live_objects()
    fill()
    print("{live_objects() - before}")
    return
end
"#,
        "3\n0\n",
    );
}

#[test]
fn weak_upgrade_states() {
    expect(
        r#"package main

type Node = record
    value: int
end

func gone() -> Weak<Node>
    let n = new Node{ value: 3 }
    return weak(n)
end

func main() -> error
    let n = new Node{ value: 3 }
    let w = weak(n)
    let up = w.upgrade()
    if up != nil
        print("alive {up.value}")
    end
    let g = gone()
    if g.upgrade() == nil
        print("gone")
    end
    var none: Weak<Node> = nil
    if none.upgrade() == nil
        print("nil upgrade")
    end
    if weak(n) == weak(n)
        print("same")
    end
    return
end
"#,
        "alive 3\ngone\nnil upgrade\nsame\n",
    );
}

#[test]
fn weak_handle_lifetime() {
    expect(
        r#"package main

type Node = record
    value: int
end

func inner() -> Weak<Node>
    let n = new Node{ value: 1 }
    return weak(n)
end

func main() -> error
    let before = live_objects()
    let w = inner()
    print("handle {live_objects() - before}")
    print("{w.upgrade() == nil}")
    return
end
"#,
        "handle 1\ntrue\n",
    );
}

#[test]
fn drop_runs_once_and_upgrade_is_nil_inside() {
    expect(
        r#"package main

type Res = record
    id: int
    me: Weak<Res>
end

func (ref r: Res) drop()
    let alive = r.me.upgrade() != nil
    print("drop {r.id} upgrade alive: {alive}")
end

func scope()
    let a = new Res{ id: 1 }
    a.me = weak(a)
    let b = a
    let c = b
    print("copies made")
end

func use_weak(a: Rc<Res>)
    let w = weak(a)
    print("in use_weak")
end

func weak_only()
    let a = new Res{ id: 2 }
    use_weak(a)
    print("weak released")
end

func main() -> error
    scope()
    print("after scope")
    weak_only()
    return
end
"#,
        "copies made\ndrop 1 upgrade alive: false\nafter scope\nin use_weak\nweak released\ndrop 2 upgrade alive: false\n",
    );
}

const TREE: &str = r#"package main

type Node = record
    value: int
    parent: Weak<Node>
    children: []Rc<Node>
end

func build() -> int
    let parent = new Node{ value: 0 }
    let w = weak(parent)
    for i in 0..10
        let c = new Node{ value: i, parent: w }
        parent.children.push(c)
    end
    var found = 0
    for c in parent.children
        let p = c.parent.upgrade()
        if p != nil
            found = found + 1
        end
    end
    return found
end

type Link = record
    other: Rc<Link>
end

func strong_cycle()
    let a = new Link{}
    let b = new Link{ other: a }
    a.other = b
end

func main() -> error
    let before = live_objects()
    print("found {build()}")
    print("tree growth {live_objects() - before}")
    let b2 = live_objects()
    strong_cycle()
    print("cycle growth {live_objects() - b2}")
    return
end
"#;

#[test]
fn tree_with_weak_parents_is_freed_and_strong_cycle_leaks() {
    expect(TREE, "found 10\ntree growth 0\ncycle growth 2\n");
}

#[test]
fn children_outlive_the_parent() {
    expect(
        r#"package main

type Node = record
    value: int
    parent: Weak<Node>
end

func make(ref out: []Rc<Node>)
    let parent = new Node{ value: 0 }
    for i in 0..3
        out.push(new Node{ value: i, parent: weak(parent) })
    end
end

func main() -> error
    var kids: []Rc<Node> = []
    make(kids)
    var nils = 0
    for k in kids
        if k.parent.upgrade() == nil
            nils = nils + 1
        end
    end
    print("{nils}")
    return
end
"#,
        "3\n",
    );
}

#[test]
fn no_growth_over_many_iterations() {
    let src = TREE.replace(
        "    let before = live_objects()\n    print(\"found {build()}\")",
        "    let before = live_objects()\n    for i in 0..5000\n        build()\n    end\n    print(\"found {build()}\")",
    );
    let src = src.replace(
        "    strong_cycle()\n    print(\"cycle growth",
        "    strong_cycle()\n    print(\"cycle growth {live_objects() - b2}\")\n    let b3 = live_objects()\n    for i in 0..5000\n        build()\n    end\n    print(\"loop growth {live_objects() - b3}\")\n    print(\"cycle growth",
    );
    let Some((out, code)) = run(&src) else { return };
    assert_eq!(out, "found 10\ntree growth 0\ncycle growth 2\nloop growth 0\ncycle growth 2\n");
    assert_eq!(code, 0);
}

#[test]
fn identity_not_contents() {
    check(
        "    let a = new Node{ value: 1 }\n    let b = new Node{ value: 1 }\n    print(\"{a == b} {a == a} {a != b}\")",
        "false true true\n",
    );
}

#[test]
fn rejected_programs() {
    let e = rejected("package main\ntype Node = record\n    value: int\nend\nfunc main()\n    let n = new Node{ value: 1 }\n    let w = weak(n)\n    print(\"{n == w}\")\nend\n");
    assert!(e.contains("Rc<Node>") && e.contains("Weak<Node>"), "{}", e);
    let e = rejected("package main\nfunc main()\n    let w = weak(5)\nend\n");
    assert!(e.contains("weak() takes an Rc<T>"), "{}", e);
    let e = rejected("package main\ntype Node = record\n    value: int\nend\nfunc take(n: Node)\nend\nfunc main()\n    take(new Node{ value: 1 })\nend\n");
    assert!(e.contains("Rc<Node>") && e.contains("Node"), "{}", e);
    let e = rejected("package main\ntype Rc = record\n    value: int\nend\nfunc main()\nend\n");
    assert!(e.contains("defined twice"), "{}", e);
}

#[test]
fn closures_capture_weak_weakly_and_rc_strongly() {
    expect(
        r#"package main

type Node = record
    value: int
    cb: func() -> int
end

func weak_cycle()
    let n = new Node{ value: 4 }
    let w = weak(n)
    n.cb = func()
        let p = w.upgrade()
        if p == nil
            return -1
        end
        return p.value
    end
    print("cb {n.cb()}")
end

func strong_capture() -> func() -> int
    let n = new Node{ value: 6 }
    return func()
        return n.value
    end
end

func main() -> error
    let before = live_objects()
    weak_cycle()
    print("weak growth {live_objects() - before}")
    let f = strong_capture()
    print("strong {f()}")
    return
end
"#,
        "cb 4\nweak growth 0\nstrong 6\n",
    );
}

#[test]
fn the_tree_example_builds_and_prints_what_the_docs_say() {
    let src = std::fs::read_to_string("examples/tree.br").unwrap();
    let Some((out, code)) = run(&src) else { return };
    assert_eq!(out, "root/src/util.br\ntop-level entries: 2\nobjects left after the tree is gone: 0\n");
    assert_eq!(code, 0);
}
