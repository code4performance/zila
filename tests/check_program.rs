//! The check-only entry point (`zila::codegen_x64::check_program`): the typed tree and the errors of
//! a program, without the assembly, over the texts of open documents.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use zila::codegen_x64::{check_program, generate_asm};
use zila::parse_source;

fn check(src: &str) -> zila::codegen_x64::Checked {
    let file = parse_source(src).unwrap_or_else(|d| panic!("{}", d));
    check_program(&file, None, HashMap::new())
}

const GOOD: &str = "package main\n\nfunc twice(n: int) -> int\n    return n * 2\nend\n\nfunc main()\n    let x = twice(4)\n    print(\"{x}\")\nend\n";

#[test]
fn a_correct_program_has_a_tree_and_no_errors() {
    let c = check(GOOD);
    assert!(c.errors.is_empty(), "{:?}", c.errors);
    for name in ["twice", "main"] {
        let sym = c.program.function_symbol("main", name).unwrap_or_else(|| panic!("{} is not declared", name));
        let body = c.program.body(sym).unwrap_or_else(|| panic!("{} has no body", name));
        assert!(body.statements() > 0 && body.is_complete(), "{}", name);
    }
}

#[test]
fn a_type_error_is_reported_and_the_tree_is_still_there() {
    // line 4: a text where an int is wanted
    let src = "package main\n\nfunc bad() -> int\n    return \"no\"\nend\n\nfunc main()\n    print(\"{bad()}\")\nend\n";
    let c = check(src);
    assert_eq!(c.errors.len(), 1, "{:?}", c.errors);
    assert_eq!((c.errors[0].module.as_str(), c.errors[0].line), ("main", 4), "{:?}", c.errors);
    assert!(!c.errors[0].message.is_empty() && !c.errors[0].message.contains(".zila:"), "{:?}", c.errors);
    // both functions have a checked body, the faulty one included
    for name in ["bad", "main"] {
        let sym = c.program.function_symbol("main", name).unwrap();
        assert!(c.program.body(sym).is_some(), "{} has no body", name);
    }
    // what the build says is the same error
    let build = generate_asm(&parse_source(src).unwrap()).unwrap_err();
    assert!(build.ends_with(&c.errors[0].message), "{} / {:?}", build, c.errors);
}

#[test]
fn an_error_in_one_function_does_not_hide_the_next() {
    let src = "package main\n\nfunc a() -> int\n    return \"no\"\nend\n\nfunc b() -> text\n    return 1\nend\n\nfunc main()\nend\n";
    let c = check(src);
    let lines: Vec<u32> = c.errors.iter().map(|e| e.line).collect();
    assert_eq!(lines.len(), 2, "{:?}", c.errors);
    assert!(lines.contains(&4) && lines.contains(&8), "{:?}", lines);
}

#[test]
fn an_error_in_the_declarations_is_reported() {
    let c = check("package main\n\nfunc f(x: Nope)\nend\n\nfunc main()\nend\n");
    assert_eq!(c.errors.len(), 1, "{:?}", c.errors);
    assert_eq!(c.errors[0].line, 3, "{:?}", c.errors);
}

#[test]
fn checking_leaves_the_build_as_it_was() {
    let file = parse_source(GOOD).unwrap();
    let before = generate_asm(&file).unwrap();
    let _ = check_program(&file, None, HashMap::new());
    assert_eq!(generate_asm(&file).unwrap(), before);
}

#[test]
fn the_examples_that_build_have_no_check_errors() {
    let mut n = 0;
    for entry in std::fs::read_dir("examples").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "zila") {
            continue;
        }
        let Ok(file) = parse_source(&std::fs::read_to_string(&path).unwrap()) else { continue };
        if file.package != "main" || zila::comptime::uses_comptime(&file) {
            continue;
        }
        if generate_asm(&file).is_ok() {
            let c = check_program(&file, Some(&path), HashMap::new());
            assert!(c.errors.is_empty(), "{}: {:?}", path.display(), c.errors);
            n += 1;
        }
    }
    assert!(n >= 5, "only {} examples were checked", n);
}

// ---- packages over an overlay ----

static N: AtomicUsize = AtomicUsize::new(0);

fn project(files: &[(&str, &str)]) -> PathBuf {
    let d = std::env::temp_dir().join(format!("zila_check_{}_{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("zila.toml"), "[package]\nname = \"app\"\n").unwrap();
    for (rel, text) in files {
        let p = d.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    d
}

const MAIN_USING_UTIL: &str = "package main\nuse util\n\nfunc main()\n    print(\"{util.twice(4)}\")\nend\n";
const UTIL_OK: &str = "package util\n\npub func twice(n: int) -> int\n    return n * 2\nend\n";

fn check_in(dir: &Path, overlay: HashMap<PathBuf, String>) -> zila::codegen_x64::Checked {
    let file = parse_source(MAIN_USING_UTIL).unwrap();
    check_program(&file, Some(&dir.join("main.zila")), overlay)
}

#[test]
fn a_package_of_the_project_is_checked_from_disk() {
    let dir = project(&[("util/util.zila", UTIL_OK)]);
    let c = check_in(&dir, HashMap::new());
    assert!(c.errors.is_empty(), "{:?}", c.errors);
    assert!(c.program.function_symbol("util", "twice").is_some());
}

#[test]
fn an_open_document_replaces_the_file_of_a_package() {
    let dir = project(&[("util/util.zila", UTIL_OK)]);
    // the editor has the file open with an error that is not saved
    let broken = "package util\n\npub func twice(n: int) -> int\n    return \"two\"\nend\n";
    let overlay = HashMap::from([(dir.join("util").join("util.zila"), broken.to_string())]);
    let c = check_in(&dir, overlay);
    assert_eq!(c.errors.len(), 1, "{:?}", c.errors);
    // the line counts the merged text of the package (its header lines come first), not the file
    assert_eq!(c.errors[0].module, "util", "{:?}", c.errors);
    assert!(c.errors[0].message.contains("expected int, found text"), "{:?}", c.errors);
    // the next check, without the overlay, sees the disk again
    assert!(check_in(&dir, HashMap::new()).errors.is_empty());
}

#[test]
fn a_document_that_is_not_on_disk_is_part_of_its_package() {
    let dir = project(&[("util/util.zila", "package util\n\nfunc helper() -> int\n    return 1\nend\n")]);
    // `twice` exists only in a file that is open and unsaved
    let overlay = HashMap::from([(dir.join("util").join("more.zila"), UTIL_OK.to_string())]);
    let c = check_in(&dir, overlay);
    assert!(c.errors.is_empty(), "{:?}", c.errors);
    assert!(c.program.function_symbol("util", "twice").is_some());
}

#[test]
fn an_error_has_the_range_of_what_it_is_about() {
    // line 4: `let x: int = "text"`; the error is about the initializer
    let c = check("package main\n\nfunc main()\n    let x: int = \"text\"\nend\n");
    assert_eq!(c.errors.len(), 1, "{:?}", c.errors);
    let s = c.errors[0].span;
    assert!(!s.is_none(), "{:?}", c.errors);
    assert_eq!(c.errors[0].line, 4);
    assert!(s.contains(4, 18), "{:?}", s); // inside the text literal
    assert_eq!((s.line, s.end_line), (4, 4));

    // an unknown name: the range is that name
    let c = check("package main\n\nfunc main()\n    let y = 1\n    print(\"{missing}\")\nend\n");
    assert_eq!(c.errors.len(), 1, "{:?}", c.errors);
    assert!(!c.errors[0].span.is_none() && c.errors[0].span.line == 5, "{:?}", c.errors);
}
