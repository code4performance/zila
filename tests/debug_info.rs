//! `--debug`: the debug-information file, and executables that are the same with and without it.
//! Programs are built, never run.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use zila::codegen_x64::{compile_to_linux, compile_to_native_with, debug_info_path, generate_asm_for, CompileOptions, Target};
use zila::lsp::{parse_json, Json};
use zila::parse_source;

static N: AtomicUsize = AtomicUsize::new(0);

fn scratch() -> PathBuf {
    let d = std::env::temp_dir().join(format!("zila_dbg_{}_{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn opts(debug: bool) -> CompileOptions {
    CompileOptions { optimize: true, test_mode: false, debug }
}

fn examples() -> Vec<(String, zila::ast::File)> {
    let mut v = Vec::new();
    for e in std::fs::read_dir("examples").unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_none_or(|x| x != "zila") {
            continue;
        }
        let file = parse_source(&std::fs::read_to_string(&p).unwrap()).unwrap();
        if file.package == "main" && !zila::comptime::uses_comptime(&file) {
            v.push((p.file_stem().unwrap().to_string_lossy().to_string(), file));
        }
    }
    v
}

#[test]
fn the_executable_is_the_same_with_and_without_debug_and_only_one_has_the_file() {
    let dir = scratch();
    let mut built = 0;
    for (name, file) in examples() {
        let (plain, dbg) = (dir.join(format!("{}.plain.exe", name)), dir.join(format!("{}.dbg.exe", name)));
        if compile_to_native_with(&file, plain.to_str().unwrap(), &opts(false)).is_err() {
            continue; // an example the native backend does not take: nothing to compare
        }
        compile_to_native_with(&file, dbg.to_str().unwrap(), &opts(true)).unwrap();
        assert_eq!(std::fs::read(&plain).unwrap(), std::fs::read(&dbg).unwrap(), "{} differs with --debug", name);
        assert!(debug_info_path(dbg.to_str().unwrap()).is_file(), "{}: no debug file", name);
        assert!(!debug_info_path(plain.to_str().unwrap()).exists(), "{}: a debug file without --debug", name);
        built += 1;
    }
    assert!(built >= 10, "{} examples built", built);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_linux_executable_is_the_same_with_and_without_debug() {
    let dir = scratch();
    let mut built = 0;
    for (name, file) in examples() {
        let (plain, dbg) = (dir.join(format!("{}.plain", name)), dir.join(format!("{}.dbg", name)));
        if compile_to_linux(&file, plain.to_str().unwrap(), &opts(false)).is_err() {
            continue;
        }
        compile_to_linux(&file, dbg.to_str().unwrap(), &opts(true)).unwrap();
        assert_eq!(std::fs::read(&plain).unwrap(), std::fs::read(&dbg).unwrap(), "{} differs with --debug (linux)", name);
        assert!(debug_info_path(dbg.to_str().unwrap()).is_file());
        built += 1;
    }
    assert!(built >= 10, "{} examples built", built);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_assembly_without_debug_has_no_notes_and_with_debug_has_the_same_code() {
    let file = parse_source(&std::fs::read_to_string("examples/tree.zila").unwrap()).unwrap();
    let plain = generate_asm_for(&file, &opts(false), Target::Windows).unwrap();
    let dbg = generate_asm_for(&file, &opts(true), Target::Windows).unwrap();
    assert!(!plain.contains(";@"));
    assert!(dbg.contains(";@L") && dbg.contains(";@E"));
    // taking the notes out gives the assembly of a build without them
    let stripped: String = dbg.lines().filter(|l| !l.starts_with(";@")).map(|l| format!("{}\n", l)).collect();
    let plain_text: String = plain.lines().map(|l| format!("{}\n", l)).collect();
    assert_eq!(stripped, plain_text);
}

fn num(j: &Json) -> f64 {
    match j {
        Json::Num(n) => *n,
        other => panic!("{:?}", other),
    }
}

#[test]
fn the_file_has_functions_a_line_table_locals_and_type_descriptors() {
    let dir = scratch();
    let src = "package main\n\ntype Point = record\n    x: int\n    y: int\nend\n\nfunc sum(p: Point, k: int) -> int\n    let t = p.x + p.y\n    let label = \"total\"\n    return t * k\nend\n\nfunc main()\n    let p = Point{x: 1, y: 2}\n    let names = [\"a\", \"b\"]\n    let n = sum(p, 3)\n    print(\"{n} {names.len}\")\nend\n";
    let file = parse_source(src).unwrap();
    let exe = dir.join("p.exe");
    compile_to_native_with(&file, exe.to_str().unwrap(), &opts(false).with_debug()).unwrap();
    let text = std::fs::read_to_string(debug_info_path(exe.to_str().unwrap())).unwrap();
    let d = parse_json(&text).unwrap();
    assert_eq!(d.get("target").str(), Some("windows"));
    assert_eq!(num(d.get("base")), 0x140000000u64 as f64);

    // functions: both, with an address range each and their declaration line
    let Json::Arr(fs) = d.get("functions") else { panic!() };
    let find = |name: &str| fs.iter().find(|f| f.get("name").str() == Some(name)).unwrap_or_else(|| panic!("no function {}", name));
    let (sum, main) = (find("sum"), find("main"));
    assert!(num(sum.get("start")) < num(sum.get("end")) && num(main.get("start")) < num(main.get("end")));
    assert_eq!((num(sum.get("line")), num(main.get("line"))), (8.0, 14.0));

    // the locals of `sum`: two parameters and two locals, with where they live and where they show
    let Json::Arr(locals) = sum.get("locals") else { panic!() };
    let names: Vec<&str> = locals.iter().map(|l| l.get("name").str().unwrap()).collect();
    assert_eq!(names, ["p", "k", "t", "label"]);
    assert_eq!(locals[0].get("parameter"), &Json::Bool(true));
    assert_eq!(locals[2].get("parameter"), &Json::Bool(false));
    assert_eq!(num(locals[2].get("firstLine")), 9.0);
    assert_eq!(locals[2].get("location").get("kind").str(), Some("frame"));
    assert_eq!(locals[1].get("type").get("kind").str(), Some("int"));
    assert_eq!(locals[3].get("type").get("kind").str(), Some("text"));
    // the record is named by key and described once
    assert_eq!(locals[0].get("type").get("ref").str(), Some("main.Point"));
    let point = d.get("types").get("main.Point");
    assert_eq!(point.get("kind").str(), Some("record"));
    let Json::Arr(fields) = point.get("fields") else { panic!() };
    assert_eq!(fields.iter().map(|f| f.get("name").str().unwrap()).collect::<Vec<_>>(), ["x", "y"]);
    assert_ne!(num(fields[0].get("offset")), num(fields[1].get("offset")));
    // a list local says what it holds
    let Json::Arr(mlocals) = main.get("locals") else { panic!() };
    let names_local = mlocals.iter().find(|l| l.get("name").str() == Some("names")).unwrap();
    assert_eq!(names_local.get("type").get("kind").str(), Some("list"));
    assert_eq!(names_local.get("type").get("element").get("kind").str(), Some("text"));

    // the line table: addresses grow, every statement line of both functions has one
    let Json::Arr(lines) = d.get("lines") else { panic!() };
    let rows: Vec<(f64, String, f64)> = lines
        .iter()
        .map(|r| {
            let Json::Arr(v) = r else { panic!() };
            (num(&v[0]), v[1].str().unwrap().to_string(), num(&v[2]))
        })
        .collect();
    assert!(rows.windows(2).all(|w| w[0].0 <= w[1].0), "addresses are in order");
    let have: Vec<f64> = rows.iter().filter(|r| r.1 == "main").map(|r| r.2).collect();
    for line in [9.0, 10.0, 11.0, 15.0, 16.0, 17.0, 18.0] {
        assert!(have.contains(&line), "line {} has no address: {:?}", line, have);
    }
    // each row of a function lies inside it
    for (addr, _, line) in &rows {
        let in_function = fs.iter().any(|f| *addr >= num(f.get("start")) && *addr < num(f.get("end")));
        assert!(in_function, "line {} at {:x} is in no function", line, *addr as u64);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

trait WithDebug {
    fn with_debug(self) -> Self;
}

impl WithDebug for CompileOptions {
    fn with_debug(mut self) -> Self {
        self.debug = true;
        self
    }
}

use zila::ast::{DeclKind, ExprKind, Stmt, StmtKind};

/// The lines of the statements of a block, nested blocks included (closures have their own code).
fn statement_lines(b: &[Stmt], out: &mut Vec<u32>) {
    fn in_expr(e: &zila::ast::Expr, out: &mut Vec<u32>) {
        match &e.kind {
            ExprKind::If { branches, else_body } => {
                for (_, body) in branches {
                    statement_lines(body, out);
                }
                if let Some(b) = else_body {
                    statement_lines(b, out);
                }
            }
            ExprKind::Match { arms, .. } => arms.iter().for_each(|a| statement_lines(&a.body, out)),
            _ => {}
        }
    }
    for s in b {
        out.push(s.line);
        match &s.kind {
            StmtKind::Unsafe(body) | StmtKind::While { body, .. } | StmtKind::For { body, .. } => statement_lines(body, out),
            StmtKind::Expr(e) | StmtKind::Return(Some(e)) => in_expr(e, out),
            StmtKind::Var(v) => {
                if let Some(e) = &v.value {
                    in_expr(e, out)
                }
            }
            _ => {}
        }
    }
}

/// Whether the statement at `index` can never run: it follows a `return`, `break` or `continue`
/// of its own block.
fn after_a_jump(b: &[Stmt], line: u32) -> bool {
    let mut jumped = false;
    for s in b {
        if jumped && s.line == line {
            return true;
        }
        jumped |= matches!(s.kind, StmtKind::Return(_) | StmtKind::Break | StmtKind::Continue);
        let nested: Vec<&[Stmt]> = match &s.kind {
            StmtKind::Unsafe(body) | StmtKind::While { body, .. } | StmtKind::For { body, .. } => vec![body],
            StmtKind::Expr(e) => match &e.kind {
                ExprKind::If { branches, else_body } => branches.iter().map(|b| b.1.as_slice()).chain(else_body.as_deref()).collect(),
                ExprKind::Match { arms, .. } => arms.iter().map(|a| a.body.as_slice()).collect(),
                _ => vec![],
            },
            _ => vec![],
        };
        if nested.iter().any(|n| after_a_jump(n, line)) {
            return true;
        }
    }
    false
}

#[test]
fn every_statement_line_of_the_examples_has_an_address() {
    let dir = scratch();
    let (mut checked, mut lines_seen) = (0, 0);
    let mut missing: Vec<String> = Vec::new();
    for (name, file) in examples() {
        let exe = dir.join(format!("{}.exe", name));
        if compile_to_native_with(&file, exe.to_str().unwrap(), &opts(true)).is_err() {
            continue;
        }
        let d = parse_json(&std::fs::read_to_string(debug_info_path(exe.to_str().unwrap())).unwrap()).unwrap();
        let Json::Arr(rows) = d.get("lines") else { panic!() };
        let have: std::collections::HashSet<u32> = rows
            .iter()
            .filter_map(|r| {
                let Json::Arr(v) = r else { return None };
                (v[1].str() == Some("main")).then(|| num(&v[2]) as u32)
            })
            .collect();
        let Json::Arr(functions) = d.get("functions") else { panic!() };
        let generated: std::collections::HashSet<&str> = functions.iter().filter(|f| f.get("module").str() == Some("main")).map(|f| f.get("name").str().unwrap()).collect();
        for decl in &file.decls {
            let DeclKind::Func(f) = &decl.kind else { continue };
            // a function nothing reaches is not in the program; a generic one has no code of its own
            if f.receiver.is_some() || !f.generics.is_empty() || !generated.contains(f.name.as_str()) {
                continue;
            }
            let mut lines = Vec::new();
            statement_lines(&f.body, &mut lines);
            for l in lines {
                lines_seen += 1;
                if !have.contains(&l) && !after_a_jump(&f.body, l) {
                    missing.push(format!("{}: line {} of `{}`", name, l, f.name));
                }
            }
            checked += 1;
        }
    }
    assert!(missing.is_empty(), "statement lines with no address:
{}", missing.join("
"));
    assert!(checked >= 30, "{} functions checked", checked);
    eprintln!("{} functions, {} statement lines", checked, lines_seen);
    let _ = std::fs::remove_dir_all(&dir);
}
