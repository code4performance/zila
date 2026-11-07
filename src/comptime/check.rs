//! The comptime subset (spec `comptime-reflection`): a reachability check over the call graph
//! from each generator. Standard modules are not allow-listed by name; the check follows real
//! calls, so `text` passes and `fs.read` fails because it reaches an `extern` function.

use super::reflect::Universe;
use super::walk::*;
use crate::ast::*;
use std::collections::{HashMap, HashSet};

type R<T> = Result<T, String>;

/// Calls the compiler gives meaning to that reach the console, the clock or tasks.
const IMPURE_BUILTINS: &[&str] = &["print", "println", "sleep", "timeout", "channel", "heap_bytes", "live_objects"];

/// The module whose functions are trusted: it is the generators' own API.
const TRUSTED: &str = "reflect";

struct Found {
    /// (priority, construct): the highest priority is reported.
    violations: Vec<(u8, String)>,
    /// (module, name, is a method name)
    calls: Vec<(String, String, bool)>,
}

struct Analyzer<'a> {
    uni: &'a Universe,
    module: &'a str,
    uses: HashSet<String>,
    bound: HashSet<String>,
    externs: HashSet<String>,
    gvars: HashSet<String>,
    free: HashSet<String>,
    found: Found,
}

impl<'a> Analyzer<'a> {
    fn viol(&mut self, prio: u8, what: &str) {
        self.found.violations.push((prio, what.to_string()));
    }
}

impl<'a> Visit for Analyzer<'a> {
    fn expr(&mut self, e: &mut Expr) -> R<bool> {
        match &e.kind {
            ExprKind::Ident(n) if !self.bound.contains(n) => {
                if self.gvars.contains(n) {
                    self.viol(3, "global `var`");
                }
                if self.externs.contains(n) {
                    self.viol(10, "`extern`");
                }
                if self.free.contains(n) {
                    self.found.calls.push((self.module.to_string(), n.clone(), false));
                } else if IMPURE_BUILTINS.contains(&n.as_str()) {
                    { let w = format!("`{}`", n); self.viol(6, &w); }
                }
            }
            ExprKind::Field { expr, name, .. } => {
                let module_ref = matches!(&expr.kind, ExprKind::Ident(p) if self.uses.contains(p) && !self.bound.contains(p));
                if module_ref {
                    if let ExprKind::Ident(p) = &expr.kind {
                        if p != TRUSTED {
                            self.found.calls.push((p.clone(), name.clone(), false));
                            if let Some(f) = self.uni.file(p) {
                                if f.decls.iter().any(|d| matches!(&d.kind, DeclKind::Var(v) if v.mutable && v.name == *name)) {
                                    self.found.violations.push((3, "global `var`".to_string()));
                                }
                            }
                        }
                    }
                } else {
                    self.found.calls.push((String::new(), name.clone(), true));
                }
            }
            ExprKind::Await(_) => self.viol(7, "`await`"),
            ExprKind::Deref(_) => self.viol(5, "`raw pointer`"),
            _ => {}
        }
        Ok(true)
    }

    fn stmt(&mut self, s: &mut Stmt) -> R<Option<Vec<Stmt>>> {
        match &s.kind {
            StmtKind::Spawn(_) => self.viol(8, "`spawn`"),
            StmtKind::Asm(_) => self.viol(9, "`asm`"),
            StmtKind::Unsafe(_) => self.viol(4, "`unsafe`"),
            _ => {}
        }
        Ok(None)
    }

    fn ty(&mut self, t: &mut Type) {
        if matches!(t, Type::Pointer(_)) {
            self.viol(5, "`raw pointer`");
        }
    }
}

fn analyze(uni: &Universe, module: &str, fd: &FuncDecl) -> Found {
    let file = uni.file(module);
    let mut externs = HashSet::new();
    let mut gvars = HashSet::new();
    let mut free = HashSet::new();
    let mut uses = HashSet::new();
    if let Some(f) = file {
        for u in &f.uses {
            uses.insert(u[0].clone());
        }
        for d in &f.decls {
            match &d.kind {
                DeclKind::Extern(e) => {
                    externs.insert(e.name.clone());
                }
                DeclKind::Var(v) if v.mutable => {
                    gvars.insert(v.name.clone());
                }
                DeclKind::Func(f) if f.receiver.is_none() => {
                    free.insert(f.name.clone());
                }
                _ => {}
            }
        }
    }
    let mut a = Analyzer {
        uni,
        module,
        uses,
        bound: bound_names(fd),
        externs,
        gvars,
        free,
        found: Found { violations: Vec::new(), calls: Vec::new() },
    };
    let _ = a.uni;
    if fd.is_async {
        a.viol(7, "`async`");
    }
    let mut copy = fd.clone();
    let _ = walk_func(&mut copy, &mut a);
    a.found
}

/// The functions named `name` in `module` (free functions, or methods when `method`).
fn funcs_named<'a>(uni: &'a Universe, module: &str, name: &str, method: bool) -> Vec<&'a FuncDecl> {
    let Some(f) = uni.file(module) else { return Vec::new() };
    f.decls
        .iter()
        .filter_map(|d| match &d.kind {
            DeclKind::Func(fd) if fd.name == name && fd.receiver.is_some() == method => Some(fd),
            _ => None,
        })
        .collect()
}

struct Walk<'a> {
    uni: &'a Universe,
    root_module: &'a str,
    visited: HashSet<(String, String, bool)>,
    chain: Vec<String>,
}

impl<'a> Walk<'a> {
    fn label(&self, module: &str, name: &str) -> String {
        if module == self.root_module {
            name.to_string()
        } else {
            format!("{}.{}", module, name)
        }
    }

    fn visit(&mut self, module: &str, name: &str, method: bool) -> R<()> {
        if module == TRUSTED {
            return Ok(());
        }
        if !self.visited.insert((module.to_string(), name.to_string(), method)) {
            return Ok(());
        }
        let decls = funcs_named(self.uni, module, name, method);
        if decls.is_empty() {
            return Ok(());
        }
        self.chain.push(self.label(module, name));
        for fd in decls {
            let found = analyze(self.uni, module, fd);
            if let Some((_, what)) = found.violations.iter().max_by_key(|(p, _)| *p) {
                return Err(format!("{} is not allowed in comptime code: {}", what, self.chain.join(" -> ")));
            }
            for (m, n, is_method) in found.calls {
                if is_method {
                    // a method call is followed by name into this module and the ones it uses
                    let mut mods: Vec<String> = vec![module.to_string()];
                    if let Some(f) = self.uni.file(module) {
                        for u in &f.uses {
                            mods.push(u[0].clone());
                        }
                    }
                    for mm in mods {
                        self.visit(&mm, &n, true)?;
                    }
                } else {
                    self.visit(&m, &n, false)?;
                }
            }
        }
        self.chain.pop();
        Ok(())
    }
}

/// Checks the generator `name` of `module` and everything it reaches. Returns the functions
/// reached (module, name), sorted, which the cache key hashes.
pub fn check_generator(uni: &Universe, module: &str, name: &str) -> R<Vec<(String, String)>> {
    let mut w = Walk { uni, root_module: module, visited: HashSet::new(), chain: Vec::new() };
    w.visit(module, name, false)?;
    let mut v: Vec<(String, String)> = w.visited.into_iter().map(|(m, n, _)| (m, n)).collect();
    v.sort();
    v.dedup();
    Ok(v)
}

/// Removes `line: N` from a debug print so that moving code does not change it.
fn strip_lines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find("line: ") {
        out.push_str(&rest[..p + 6]);
        rest = &rest[p + 6..];
        let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
        out.push('0');
        rest = &rest[digits..];
    }
    out.push_str(rest);
    out
}

/// The canonical text of the functions reached and of the constants of their modules.
pub fn closure_text(uni: &Universe, closure: &[(String, String)]) -> String {
    let mut out = String::new();
    let mut mods: HashMap<&str, ()> = HashMap::new();
    for (m, n) in closure {
        mods.insert(m.as_str(), ());
        for method in [false, true] {
            for fd in funcs_named(uni, m, n, method) {
                out.push_str(&strip_lines(&format!("{}::{} {:?}\n", m, n, fd)));
            }
        }
    }
    let mut names: Vec<&str> = mods.keys().copied().collect();
    names.sort();
    for m in names {
        if let Some(f) = uni.file(m) {
            for d in &f.decls {
                if let DeclKind::Var(v) = &d.kind {
                    if !v.mutable {
                        out.push_str(&strip_lines(&format!("{}::{:?}\n", m, v)));
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comptime::reflect::{ModFile, Universe};

    /// The libraries the spec names stay inside the subset: every function of `text`, `list`,
    /// `format` and `math` that does not need a platform call is allowed, and the ones that do
    /// (none today) would have to be listed here.
    #[test]
    fn the_pure_libraries_are_in_the_subset() {
        let mut modules = Vec::new();
        for name in ["text", "list", "format", "math", "float", "errors", "set", "sorted", "derive", "reflect"] {
            let src = crate::codegen_x64::std_source(name).unwrap();
            modules.push(ModFile { name: name.to_string(), file: crate::parser::parse_source(src).unwrap() });
        }
        let uni = Universe { modules };
        let mut bad = Vec::new();
        for m in ["text", "list", "format", "math"] {
            let file = uni.file(m).unwrap().clone();
            for d in &file.decls {
                if let DeclKind::Func(fd) = &d.kind {
                    if fd.receiver.is_none() {
                        if let Err(e) = check_generator(&uni, m, &fd.name) {
                            bad.push(format!("{}.{}: {}", m, fd.name, e));
                        }
                    }
                }
            }
        }
        assert!(bad.is_empty(), "{:#?}", bad);
    }
}
