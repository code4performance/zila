//! The position index: every name of a sample program with its range, and what it refers to.

use std::collections::HashMap;
use zila::codegen_x64::{check_program, Checked, IndexEntry, Referent};
use zila::parse_source;

const SAMPLE: &str = r#"package main

type Point = record
    x: int
    y: int
end

type Shape = enum
    Circle(r: int)
    Empty
end

func (p: Point) sum() -> int
    return p.x + p.y
end

func area(s: Shape) -> int
    match s
        Circle(r) -> return r * r
        Empty -> return 0
    end
end

func main()
    let p = Point{x: 1, y: 2}
    let total = p.sum()
    var n = total
    for i in 0..3
        n += i
    end
    let s = Circle(2)
    print("{area(s)} {n}")
    let f = func(a: int) a + 1 end
    print("{f(1)}")
end
"#;

fn checked() -> Checked {
    let file = parse_source(SAMPLE).unwrap();
    let c = check_program(&file, None, HashMap::new());
    assert!(c.errors.is_empty(), "{:?}", c.errors);
    c
}

/// The text a range covers (a range on one line).
fn text_of(e: &IndexEntry) -> String {
    let line = SAMPLE.lines().nth(e.span.line as usize - 1).unwrap();
    assert_eq!(e.span.line, e.span.end_line, "{:?}", e);
    line.chars().skip(e.span.col as usize - 1).take((e.span.end_col - e.span.col) as usize).collect()
}

/// The entries on `line` (from 1) named `name`.
fn on<'a>(c: &'a Checked, line: u32, name: &str) -> Vec<&'a IndexEntry> {
    c.index.entries("main").iter().filter(|e| e.span.line == line && e.name == name).collect()
}

fn one<'a>(c: &'a Checked, line: u32, name: &str) -> &'a IndexEntry {
    let v = on(c, line, name);
    assert_eq!(v.len(), 1, "line {} `{}`: {:?}", line, name, v);
    v[0]
}

#[test]
fn every_name_of_the_sample_is_in_the_index_where_it_is_written_and_resolves() {
    let c = checked();
    let entries = c.index.entries("main");
    assert!(entries.len() > 50, "{} entries", entries.len());
    for e in entries {
        assert_eq!(text_of(e), e.name, "the range of {:?} does not cover its name", e);
        assert_ne!(e.target, Referent::Unknown, "{:?} does not resolve", e);
        // a use of something the program declares has its declaration in the index
        if !e.is_declaration && !matches!(e.target, Referent::Builtin { .. } | Referent::Package { .. } | Referent::TypeParameter { .. }) {
            assert!(c.index.declaration_of(&e.target).is_some(), "{:?} has no declaration", e);
        }
    }
}

#[test]
fn names_resolve_to_what_they_mean() {
    let c = checked();
    let main = |name: &str| Referent::Function { module: "main".into(), name: name.into() };
    let point = "main.Point".to_string();
    let shape = "main.Shape".to_string();

    // declarations
    assert!(one(&c, 3, "Point").is_declaration);
    assert_eq!(one(&c, 4, "x").target, Referent::Field { type_key: point.clone(), name: "x".into() });
    assert_eq!(one(&c, 9, "Circle").target, Referent::Variant { enum_key: shape.clone(), name: "Circle".into() });
    assert_eq!(one(&c, 13, "sum").target, Referent::Method { type_key: point.clone(), name: "sum".into() });
    assert_eq!(one(&c, 17, "area").target, main("area"));
    assert_eq!(one(&c, 17, "area").ty.as_deref(), Some("func area(s: Shape) -> int"));

    // the receiver and its members
    let recv = one(&c, 13, "p");
    assert!(recv.is_declaration);
    let uses = on(&c, 14, "p");
    assert_eq!(uses.len(), 2);
    assert!(uses.iter().all(|u| u.target == recv.target && !u.is_declaration));
    assert_eq!(uses[0].ty.as_deref(), Some("Point"));
    assert_eq!(on(&c, 14, "x")[0].target, Referent::Field { type_key: point.clone(), name: "x".into() });
    assert_eq!(on(&c, 14, "x")[0].ty.as_deref(), Some("int"));

    // a type in a parameter and in a literal, and its fields in the literal
    assert_eq!(on(&c, 13, "Point")[0].target, Referent::Type { module: "main".into(), name: "Point".into() });
    let lit = on(&c, 25, "Point");
    assert_eq!(lit[0].target, Referent::Type { module: "main".into(), name: "Point".into() });
    assert_eq!(on(&c, 25, "y")[0].target, Referent::Field { type_key: point.clone(), name: "y".into() });

    // a method call, a local and its uses
    assert_eq!(on(&c, 26, "sum")[0].target, Referent::Method { type_key: point.clone(), name: "sum".into() });
    let total = one(&c, 26, "total");
    assert!(total.is_declaration);
    assert_eq!(total.ty.as_deref(), Some("int"));
    assert_eq!(on(&c, 27, "total")[0].target, total.target);

    // loop variable, match bindings and variants
    let i = one(&c, 28, "i");
    assert!(i.is_declaration);
    assert_eq!(on(&c, 29, "i")[0].target, i.target);
    let r = on(&c, 19, "r").into_iter().find(|e| e.is_declaration).unwrap();
    assert!(r.is_declaration);
    assert!(on(&c, 19, "Circle")[0].target == Referent::Variant { enum_key: shape.clone(), name: "Circle".into() });
    assert!(on(&c, 19, "r").iter().filter(|e| !e.is_declaration).all(|e| e.target == r.target));
    assert_eq!(on(&c, 20, "Empty")[0].target, Referent::Variant { enum_key: shape.clone(), name: "Empty".into() });

    // a variant used as a constructor, a call of a function, a built-in, a closure
    assert_eq!(on(&c, 31, "Circle")[0].target, Referent::Variant { enum_key: shape.clone(), name: "Circle".into() });
    assert_eq!(on(&c, 32, "area")[0].target, main("area"));
    assert_eq!(on(&c, 32, "print")[0].target, Referent::Builtin { name: "print".into() });
    let f = one(&c, 33, "f");
    assert_eq!(on(&c, 34, "f")[0].target, f.target);
    let a = on(&c, 33, "a").into_iter().find(|e| e.is_declaration).unwrap();
    assert!(a.is_declaration);
    assert_eq!(on(&c, 33, "a").len(), 2);

    // references of a thing: its declaration and every use
    let refs = c.index.references(&main("area"));
    assert_eq!(refs.len(), 2);
    let (module, decl) = c.index.declaration_of(&main("area")).unwrap();
    assert_eq!((module, decl.span.line), ("main", 17));

    // lookup by position: line 14 is `    return p.x + p.y`, the `x` is at column 14
    let at = c.index.at("main", 14, 14).unwrap();
    assert_eq!(at.name, "x");
    assert!(c.index.at("main", 14, 3).is_none(), "indentation names nothing");
}

#[test]
fn a_package_of_the_project_is_indexed_and_the_standard_library_is_not() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("zila_index_{}_{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("util")).unwrap();
    std::fs::write(dir.join("zila.toml"), "[package]\nname = \"app\"\n").unwrap();
    std::fs::write(dir.join("util/util.zila"), "package util\n\npub func twice(n: int) -> int\n    return n * 2\nend\n").unwrap();
    let main = "package main\n\nuse util\n\nfunc main()\n    let x = util.twice(4)\n    print(\"{x}\")\nend\n";
    let file = parse_source(main).unwrap();
    let c = check_program(&file, Some(&dir.join("main.zila")), HashMap::new());
    assert!(c.errors.is_empty(), "{:?}", c.errors);
    let names: Vec<&str> = c.index.modules().collect();
    assert_eq!(names, ["main", "util"]);
    let uses: Vec<&IndexEntry> = c.index.entries("main").iter().filter(|e| e.line_is(6)).collect();
    let twice = uses.iter().find(|e| e.name == "twice").unwrap();
    assert_eq!(twice.target, Referent::Function { module: "util".into(), name: "twice".into() });
    let pkg = uses.iter().find(|e| e.name == "util").unwrap();
    assert_eq!(pkg.target, Referent::Package { name: "util".into() });
    let (module, decl) = c.index.declaration_of(&twice.target).unwrap();
    assert_eq!((module, decl.name.as_str()), ("util", "twice"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Over the examples that build: no panic, and every range covers exactly the name it carries.
#[test]
fn the_ranges_of_the_examples_cover_their_names() {
    let (mut files, mut names, mut unknown) = (0, 0, 0);
    for entry in std::fs::read_dir("examples").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "zila") {
            continue;
        }
        let src = std::fs::read_to_string(&path).unwrap();
        let Ok(file) = parse_source(&src) else { continue };
        if file.package != "main" || zila::comptime::uses_comptime(&file) {
            continue;
        }
        let c = check_program(&file, Some(&path), HashMap::new());
        if !c.errors.is_empty() {
            continue;
        }
        files += 1;
        let lines: Vec<&str> = src.lines().collect();
        for e in c.index.entries("main") {
            names += 1;
            if e.target == Referent::Unknown {
                unknown += 1;
            }
            if e.span.line != e.span.end_line {
                continue;
            }
            let line = lines[e.span.line as usize - 1];
            let text: String = line.chars().skip(e.span.col as usize - 1).take((e.span.end_col - e.span.col) as usize).collect();
            assert_eq!(text, e.name, "{}:{}: {:?}", path.display(), e.span.line, e);
        }
    }
    assert!(files >= 5, "{} examples", files);
    eprintln!("{} files, {} names, {} unresolved", files, names, unknown);
}
