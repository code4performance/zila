//! The entry point that checks a program and stops before the assembly is put together.
//!
//! `check_program` loads the packages (over the texts of the documents an editor has open),
//! declares everything, checks every function body and returns the typed program together with the
//! errors found. The language server uses it: a program with a type error still has a tree, and
//! one error does not hide the others.
//!
//! The body check (`check_function_body`) reports nothing by itself: it marks what it does not
//! understand as `Opaque`. The errors therefore come from the same code that finds them in a build,
//! run function by function: its output is thrown away and an error ends only that function, not
//! the run. Errors are given the module and line the compiler had reached and, when it knows it, the range of the statement or
//! expression it was compiling (`CheckError::span`).
//!
//! Like a build, it runs comptime generators (`load` expands them). A caller that must not run
//! code from the files it checks (the server) asks `comptime::uses_comptime` first.

use super::*;
use std::path::{Path, PathBuf};

/// One error: the module (package) and line the compiler had reached, and what it said.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckError {
    pub module: String,
    pub line: u32,
    /// The range of the statement or expression the error is about; `Span::default()` (see
    /// `Span::is_none`) when only the line is known.
    pub span: Span,
    pub message: String,
}

/// What `check_program` returns.
pub struct Checked {
    /// The declaration tables and the checked bodies. After an error in loading or declaring, only
    /// what had been built before it.
    pub program: TypedProgram,
    /// In the order found; empty when the program is correct.
    pub errors: Vec<CheckError>,
    /// Every name of the project's packages with its range and what it refers to; empty after an
    /// error in loading or declaring.
    pub index: PositionIndex,
}

/// Checks `file` (the main package, already parsed) without emitting. `source` is the path of the
/// file, to find the project (`zila.toml`) it belongs to; `overlay` holds the text of open
/// documents, by path, which are read instead of the files on disk.
pub fn check_program(file: &File, source: Option<&Path>, overlay: HashMap<PathBuf, String>) -> Checked {
    let (mut checked, problem) = crate::package::scoped(source, overlay, || check(file));
    if let Some(p) = problem {
        checked.errors.insert(0, CheckError { module: file.package.clone(), line: 1, span: Span::default(), message: p });
    }
    checked
}

/// Splits `module.zila:LINE: message` as the compiler writes its errors; `at` is used for an error
/// without that prefix.
fn split_error(text: &str, at: (&str, u32), span: Span) -> CheckError {
    let parsed = text.split_once(".zila:").and_then(|(module, rest)| {
        let (line, message) = rest.split_once(": ")?;
        Some(CheckError { module: module.to_string(), line: line.trim().parse().ok()?, span: Span::default(), message: message.to_string() })
    });
    let mut e = parsed.unwrap_or_else(|| CheckError { module: at.0.to_string(), line: at.1, span: Span::default(), message: text.to_string() });
    if !span.is_none() && span.line <= e.line && e.line <= span.end_line {
        e.span = span;
    }
    e
}

fn check(file: &File) -> Checked {
    let mut c = Compiler::new(Target::Windows);
    let mut errors = Vec::new();
    let main = file.package.clone();
    if let Err(e) = c.load(file).map_err(crate::comptime::map_error) {
        errors.push(split_error(&e, (&main, 1), c.err_span.take()));
        return Checked { program: c.tp, errors, index: PositionIndex::default() };
    }
    if let Err(e) = c.declare().map_err(crate::comptime::map_error) {
        errors.push(split_error(&e, (&main, c.line), c.err_span.take()));
        return Checked { program: c.tp, errors, index: PositionIndex::default() };
    }
    if let Err(e) = c.check_templates() {
        errors.push(split_error(&e, (&c.cur_mod, c.line), c.err_span.take()));
    }
    let mods: Vec<(String, Vec<Decl>)> = c.modules.iter().map(|m| (m.name.clone(), m.file.decls.clone())).collect();
    for (m, decls) in mods.iter().rev() {
        c.cur_mod = m.clone();
        if let Err(e) = c.gen_init(m, decls) {
            errors.push(split_error(&e, (m, c.line), c.err_span.take()));
        }
        for d in decls {
            c.line = d.line;
            c.span = d.span;
            c.err_span.take();
            let DeclKind::Func(fd) = &d.kind else { continue };
            if !fd.generics.is_empty() || c.generic_receiver(m, fd).is_some() {
                continue;
            }
            if let Err(e) = c.gen_func(m, fd, d.attrs.iter().any(|a| a.name == "naked")) {
                errors.push(split_error(&e, (m, c.line), c.err_span.take()));
            }
        }
    }
    if let Err(e) = c.drain_pending() {
        errors.push(split_error(&e, (&c.cur_mod, c.line), c.err_span.take()));
    }
    let mut index = c.index_program();
    let modules: Vec<String> = index.modules().filter(|m| *m != main).map(|m| m.to_string()).collect();
    for m in modules {
        if let Some(files) = crate::package::module_files(&m) {
            index.files.insert(m, files);
        }
    }
    Checked { program: c.tp, errors, index }
}
