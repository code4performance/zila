//! The Linux layer of a standard library (`std/<name>.linux.br`) must offer the same public
//! surface as the Windows one (`std/<name>.br`): a program that compiles on one target must
//! compile on the other (spec `static-linking`).

use brasa::ast::*;
use brasa::parse_source;
use std::collections::{BTreeMap, BTreeSet};

/// The public declarations of a library: `"func name"` or `"type name"` to the full signature
/// (never a body).
fn surface(src: &str) -> BTreeMap<String, String> {
    let file = parse_source(src).unwrap_or_else(|d| panic!("parse error: {}", d.msg));
    let mut out = BTreeMap::new();
    for d in &file.decls {
        if !d.public {
            continue;
        }
        match &d.kind {
            DeclKind::Func(f) => {
                let params: Vec<String> = f.params.iter().map(|p| format!("{}{}{}:{:?}", if p.by_ref { "ref " } else { "" }, p.name, if p.variadic { "..." } else { "" }, p.ty)).collect();
                let recv = f.receiver.as_ref().map(|r| format!("{}{}:{:?}", if r.by_ref { "ref " } else { "" }, r.name, r.ty)).unwrap_or_default();
                let recv_name = f.receiver.as_ref().map(|r| format!("{:?}.", r.ty)).unwrap_or_default();
                out.insert(format!("func {}{}", recv_name, f.name), format!("{}<{:?}>({}) -> {:?}{}{}", recv, f.generics, params.join(", "), f.ret, if f.is_async { " async" } else { "" }, if f.comptime { " comptime" } else { "" }));
            }
            DeclKind::Type(t) => {
                // a record is its field names: the type of a platform handle (`usize` or `long`)
                // is an implementation detail, and a line number is not part of the surface
                let body = match &t.body {
                    TypeBody::Record(fields) => format!("record {}", fields.iter().filter(|f| f.public).map(|f| f.name.clone()).collect::<Vec<_>>().join(", ")),
                    TypeBody::Enum(vs) => format!("enum {}", vs.iter().map(|v| v.name.clone()).collect::<Vec<_>>().join(", ")),
                    TypeBody::Interface(sigs) => format!("interface {}", sigs.iter().map(|s| s.name.clone()).collect::<Vec<_>>().join(", ")),
                    TypeBody::Alias(a) => format!("alias {:?}", a),
                };
                out.insert(format!("type {}", t.name), format!("<{:?}> = {}", t.generics, body));
            }
            DeclKind::Var(v) => {
                out.insert(format!("var {}", v.name), format!("{}: {:?}", if v.mutable { "var" } else { "let" }, v.ty));
            }
            DeclKind::Extern(_) | DeclKind::Embed(_) => {}
        }
    }
    out
}

/// The file of one layer of a library: the core ones are in `std/`, the others are packages whose
/// Linux layer is `packages/<lib>/linux/<lib>.br`.
fn layer(lib: &str, linux: bool) -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let pkg = root.join("packages").join(lib);
    let path = if pkg.is_dir() {
        if linux { pkg.join("linux").join(format!("{}.br", lib)) } else { pkg.join(format!("{}.br", lib)) }
    } else {
        root.join("std").join(if linux { format!("{}.linux.br", lib) } else { format!("{}.br", lib) })
    };
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {}", path.display(), e))
}

/// How one library's two layers differ: `"<lib>: only in <file>: <decl>"` or `"<lib>: signature: <decl>"`.
fn differences(lib: &str) -> BTreeSet<String> {
    let win = surface(&layer(lib, false));
    let lin = surface(&layer(lib, true));
    let mut out = BTreeSet::new();
    for (k, v) in &win {
        match lin.get(k) {
            None => {
                out.insert(format!("{}: only in {}.br: {}", lib, lib, k));
            }
            Some(o) if o != v => {
                out.insert(format!("{}: signature: {}", lib, k));
            }
            _ => {}
        }
    }
    for k in lin.keys() {
        if !win.contains_key(k) {
            out.insert(format!("{}: only in {}.linux.br: {}", lib, lib, k));
        }
    }
    out
}

fn libraries() -> Vec<String> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut libs: Vec<String> = std::fs::read_dir(root.join("std"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".linux.br")).map(|n| n.to_string()))
        .collect();
    for e in std::fs::read_dir(root.join("packages")).unwrap().flatten() {
        if e.path().join("linux").is_dir() {
            libs.push(e.file_name().to_string_lossy().to_string());
        }
    }
    libs.sort();
    libs
}

/// Differences that exist today. Most are helpers that one layer declares `pub` only so that
/// another standard library can call them (the scheduler of `task`, the system calls of `sys`).
/// The list may only shrink: a new difference fails the test, and one that is fixed must be
/// removed from the file.
fn known() -> BTreeSet<String> {
    let text = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("data").join("std_parity_known.txt")).unwrap();
    text.lines().map(|l| l.trim()).filter(|l| !l.is_empty() && !l.starts_with('#')).map(|l| l.to_string()).collect()
}

#[test]
fn the_linux_layers_do_not_drift_further_from_the_windows_ones() {
    let libs = libraries();
    assert!(!libs.is_empty());
    let now: BTreeSet<String> = libs.iter().flat_map(|l| differences(l)).collect();
    let known = known();
    let new: Vec<&String> = now.difference(&known).collect();
    assert!(new.is_empty(), "the Linux layer differs from the Windows one in a way that is not in tests/data/std_parity_known.txt:
{}", new.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("
"));
    let fixed: Vec<&String> = known.difference(&now).collect();
    assert!(fixed.is_empty(), "these differences are gone; remove them from tests/data/std_parity_known.txt:
{}", fixed.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("
"));
}

/// Prints the current differences, to refresh the file: `cargo test --test std_parity print -- --ignored --nocapture`.
#[test]
#[ignore]
fn print_the_differences() {
    for l in libraries() {
        for d in differences(&l) {
            println!("{}", d);
        }
    }
}
