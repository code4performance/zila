//! Expansion: runs the generators a program asks for and splices their output into the program
//! before it is compiled (`#[derive]`, `#[generated]` functions and block forms).
//!
//! The program is expanded in rounds: each round finds every request in the current syntax
//! tree, runs all the generators of the round as one batch, and applies the results. Output
//! that itself asks for generation takes another round; after 8 rounds the expansion stops.
//!
//! Generated declarations are spliced into the module of the type they were generated for.
//! Their top-level functions, types and constants are renamed `__g<n>_<name>`, which keeps a
//! generated helper from capturing or colliding with a user name (hygiene); methods attach to
//! their receiver type as written.

use super::check::check_generator;
use super::reflect::*;
use super::run::{run_calls, CallSpec, Outcome};
use super::state::*;
use super::walk::*;
use crate::ast::*;
use crate::codegen_x64::Target;
use crate::lexer::Lexer;
use crate::parser::Parser;
use std::collections::{HashMap, HashSet};

type R<T> = Result<T, String>;

/// Rounds of expansion before giving up.
const MAX_ROUNDS: usize = 8;

/// The program after expansion.
pub struct Expansion {
    /// The program's own file: generators removed, requests replaced by generated code.
    pub main: File,
    /// Declarations to add to other modules (specializations of their generated functions).
    pub extra: HashMap<String, Vec<Decl>>,
}

/// Names that start with `__` are for generated code.
fn check_reserved(main: &File) -> R<()> {
    for d in &main.decls {
        let name = match &d.kind {
            DeclKind::Func(f) if f.receiver.is_none() => Some(&f.name),
            DeclKind::Type(t) => Some(&t.name),
            DeclKind::Var(v) => Some(&v.name),
            _ => None,
        };
        if let Some(n) = name {
            if n.starts_with("__") {
                return Err(format!(
                    "{}.br:{}: '{}' is not allowed: names that start with `__` are reserved for generated code",
                    main.package, d.line, n
                ));
            }
        }
    }
    Ok(())
}

fn has_attr(d: &Decl, name: &str) -> bool {
    d.attrs.iter().any(|a| a.name == name)
}

/// Does the program use anything this module expands?
fn needs_expansion(main: &File) -> bool {
    struct Find(bool);
    impl Visit for Find {
        fn expr(&mut self, e: &mut Expr) -> R<bool> {
            if matches!(e.kind, ExprKind::Block { .. }) {
                self.0 = true;
            }
            Ok(true)
        }
        fn stmt(&mut self, s: &mut Stmt) -> R<Option<Vec<Stmt>>> {
            if matches!(s.kind, StmtKind::Block { .. }) {
                self.0 = true;
            }
            Ok(None)
        }
    }
    for d in &main.decls {
        match &d.kind {
            DeclKind::Func(fd) => {
                if fd.comptime || has_attr(d, "generated") {
                    return true;
                }
            }
            DeclKind::Type(_) => {
                if d.attrs.iter().any(|a| a.name == "derive" && !a.args.is_empty()) {
                    return true;
                }
            }
            _ => {}
        }
    }
    // calls to generated functions of imported packages, and blocks
    let mut f = Find(false);
    let mut copy = main.clone();
    for d in copy.decls.iter_mut() {
        let _ = walk_decl(d, &mut f);
    }
    if f.0 {
        return true;
    }
    imported_generated(main)
}

/// Does an imported package declare a `#[generated]` function (which the program may call)?
fn imported_generated(main: &File) -> bool {
    for u in &main.uses {
        if let Some(src) = super::package_source(&u[0], Target::Windows) {
            if src.contains("#[generated") {
                return true;
            }
        }
    }
    false
}

/// Does the program ask for code generation? Tools that must never run generators (the language
/// server) do not compile such a program.
pub fn uses_comptime(main: &File) -> bool {
    needs_expansion(main)
}

pub fn expand(main: &File, target: Target) -> R<Expansion> {
    check_reserved(main)?;
    if !needs_expansion(main) {
        return Ok(Expansion { main: main.clone(), extra: HashMap::new() });
    }
    clear_units();
    let mut x = Expander::new(main, target)?;
    x.run()?;
    Ok(x.finish())
}

fn load_universe(main: &File, target: Target) -> Universe {
    let mut modules = vec![ModFile { name: main.package.clone(), file: main.clone() }];
    let mut todo: Vec<String> = main.uses.iter().map(|u| u[0].clone()).collect();
    let mut seen: HashSet<String> = HashSet::new();
    seen.insert(main.package.clone());
    while let Some(n) = todo.pop() {
        if !seen.insert(n.clone()) {
            continue;
        }
        if let Some(src) = super::package_source(&n, target) {
            if let Ok(f) = crate::parser::parse_source(src) {
                todo.extend(f.uses.iter().map(|u| u[0].clone()));
                modules.push(ModFile { name: n, file: f });
            }
        }
    }
    Universe { modules }
}

fn block_set(uni: &Universe) -> HashSet<String> {
    let mut set = HashSet::new();
    for m in &uni.modules {
        for d in &m.file.decls {
            if let DeclKind::Func(fd) = &d.kind {
                if fd.comptime && has_attr(d, "block") {
                    set.insert(fd.name.clone());
                }
            }
        }
    }
    set
}

enum Kind {
    Derive,
    Generated,
    Block,
}

impl Kind {
    fn word(&self) -> &'static str {
        match self {
            Kind::Derive => "derive",
            Kind::Generated => "generated",
            Kind::Block => "block",
        }
    }
}

/// A request found in the program.
struct Pending {
    kind: Kind,
    /// The generator: module and function name.
    gen: (String, String),
    /// The display name of the generator (`json.json` when it is in another package).
    gen_label: String,
    table: TypeTable,
    root: usize,
    raw: Option<String>,
    site_line: u32,
    site_desc: String,
    ty_name: String,
    /// derive: the index of the type declaration
    decl_idx: usize,
    /// generated: the specialization it creates
    spec: Option<SpecInfo>,
}

#[derive(Clone)]
struct SpecInfo {
    key: (String, String),
    module: String,
    template: FuncDecl,
    template_attrs: Vec<Attr>,
    template_public: bool,
    comptime_idx: usize,
    arg_ty: Type,
    name: String,
}

/// What a call to a generated function looks like.
struct CallInfo {
    key: (String, String),
    module: String,
    fname: String,
    template: FuncDecl,
    template_attrs: Vec<Attr>,
    template_public: bool,
    gen: (String, String),
    comptime_idx: usize,
    /// the type, as written at the call site (a name of the program's own module is single)
    arg_ty: Type,
    /// the type was given as an explicit type argument (`f<T>(...)`), not as an argument
    targ_form: bool,
}

struct Expander {
    target: Target,
    uni: Universe,
    main_name: String,
    blocks: HashSet<String>,
    extra: HashMap<String, Vec<Decl>>,
    specs: HashMap<(String, String), String>,
    counter: usize,
    chains: HashMap<String, Vec<String>>,
    gen_methods: HashMap<(String, String), String>,
    closures: HashMap<(String, String), Vec<(String, String)>>,
}

impl Expander {
    fn new(main: &File, target: Target) -> R<Expander> {
        let uni = load_universe(main, target);
        let blocks = block_set(&uni);
        let mut x = Expander {
            target,
            main_name: main.package.clone(),
            uni,
            blocks,
            extra: HashMap::new(),
            specs: HashMap::new(),
            counter: 0,
            chains: HashMap::new(),
            gen_methods: HashMap::new(),
            closures: HashMap::new(),
        };
        // the subset applies to every comptime function of the program, used or not
        let main_module = x.main_name.clone();
        let comptime: Vec<(String, u32)> = x.main().decls.iter().filter_map(|d| match &d.kind {
            DeclKind::Func(f) if f.comptime => Some((f.name.clone(), d.line)),
            _ => None,
        }).collect();
        for (name, line) in comptime {
            x.closure_for(&main_module, &name, line)?;
        }
        x.check_runtime_calls()?;
        Ok(x)
    }

    fn main(&self) -> &File {
        &self.uni.modules[0].file
    }

    fn main_mut(&mut self) -> &mut File {
        &mut self.uni.modules[0].file
    }

    fn finish(mut self) -> Expansion {
        // comptime functions and generated templates are not compiled with the program
        let main = self.main_mut();
        main.decls.retain(|d| match &d.kind {
            DeclKind::Func(f) => !(f.comptime || d.attrs.iter().any(|a| a.name == "generated")),
            _ => true,
        });
        let main = self.uni.modules.remove(0).file;
        Expansion { main, extra: self.extra }
    }

    /// The functions a generator reaches, after checking the subset.
    fn closure_for(&mut self, module: &str, name: &str, line: u32) -> R<Vec<(String, String)>> {
        let key = (module.to_string(), name.to_string());
        if let Some(c) = self.closures.get(&key) {
            return Ok(c.clone());
        }
        let c = check_generator(&self.uni, module, name).map_err(|e| format!("{}.br:{}: {}", self.main_name, user_line(line), e))?;
        self.closures.insert(key, c.clone());
        Ok(c)
    }

    /// Runtime code may not call comptime functions.
    fn check_runtime_calls(&self) -> R<()> {
        let mut names: HashSet<(String, String)> = HashSet::new();
        for m in &self.uni.modules {
            for d in &m.file.decls {
                if let DeclKind::Func(f) = &d.kind {
                    if f.comptime {
                        names.insert((m.name.clone(), f.name.clone()));
                    }
                }
            }
        }
        if names.is_empty() {
            return Ok(());
        }
        struct Scan<'a> {
            names: &'a HashSet<(String, String)>,
            module: &'a str,
            uses: HashSet<String>,
            bound: HashSet<String>,
            hit: Option<(String, u32)>,
        }
        impl<'a> Visit for Scan<'a> {
            fn expr(&mut self, e: &mut Expr) -> R<bool> {
                match &e.kind {
                    ExprKind::Ident(n) if !self.bound.contains(n) && self.names.contains(&(self.module.to_string(), n.clone())) => {
                        self.hit.get_or_insert((n.clone(), e.line));
                    }
                    ExprKind::Field { expr, name, .. } => {
                        if let ExprKind::Ident(p) = &expr.kind {
                            if self.uses.contains(p) && !self.bound.contains(p) && self.names.contains(&(p.clone(), name.clone())) {
                                self.hit.get_or_insert((format!("{}.{}", p, name), e.line));
                            }
                        }
                    }
                    _ => {}
                }
                Ok(true)
            }
        }
        for d in &self.main().decls {
            let DeclKind::Func(f) = &d.kind else { continue };
            if f.comptime || has_attr(d, "generated") {
                continue;
            }
            let mut copy = f.clone();
            let mut s = Scan {
                names: &names,
                module: &self.main_name,
                uses: self.main().uses.iter().map(|u| u[0].clone()).collect(),
                bound: bound_names(f),
                hit: None,
            };
            let _ = walk_func(&mut copy, &mut s);
            if let Some((n, line)) = s.hit {
                return Err(format!(
                    "{}.br:{}: comptime function `{}` runs only at build time and cannot be called from runtime code",
                    self.main_name, line, n
                ));
            }
        }
        Ok(())
    }

    // ---------------------------------------------------------------------
    // Finding generators
    // ---------------------------------------------------------------------

    fn func_in(&self, module: &str, name: &str) -> Option<(&Decl, &FuncDecl)> {
        let f = self.uni.file(module)?;
        f.decls.iter().find_map(|d| match &d.kind {
            DeclKind::Func(fd) if fd.name == name && fd.receiver.is_none() => Some((d, fd)),
            _ => None,
        })
    }

    /// The `#[derive] comptime func` an attribute argument names.
    fn find_derive(&self, arg: &Expr, ty: &str, line: u32) -> R<((String, String), String)> {
        let main = self.main();
        let uses: Vec<String> = main.uses.iter().map(|u| u[0].clone()).collect();
        let at = format!("{}.br:{}:", self.main_name, line);
        let is_derive = |m: &str, n: &str| self.func_in(m, n).map_or(false, |(d, fd)| fd.comptime && has_attr(d, "derive"));
        match &arg.kind {
            ExprKind::Ident(n) => {
                let mut found: Vec<String> = Vec::new();
                let mut mods = vec![self.main_name.clone()];
                mods.extend(uses.iter().cloned());
                mods.dedup();
                for m in &mods {
                    if is_derive(m, n) && !found.contains(m) {
                        found.push(m.clone());
                    }
                }
                match found.len() {
                    0 => Err(format!("{} no derive named `{}` was found in the imported packages (on type `{}`)", at, n, ty)),
                    1 => {
                        let label = if found[0] == self.main_name { n.clone() } else { format!("{}.{}", found[0], n) };
                        Ok(((found[0].clone(), n.clone()), label))
                    }
                    _ => Err(format!(
                        "{} derive `{}` on type `{}` is ambiguous: it is defined in {}; write {}",
                        at,
                        n,
                        ty,
                        found.iter().map(|m| format!("`{}`", m)).collect::<Vec<_>>().join(" and "),
                        found.iter().map(|m| format!("`{}.{}`", m, n)).collect::<Vec<_>>().join(" or ")
                    )),
                }
            }
            ExprKind::Field { expr, name, .. } => {
                if let ExprKind::Ident(p) = &expr.kind {
                    if (uses.contains(p) || *p == self.main_name) && is_derive(p, name) {
                        return Ok(((p.clone(), name.clone()), format!("{}.{}", p, name)));
                    }
                    return Err(format!("{} no derive named `{}.{}` was found in the imported packages (on type `{}`)", at, p, name, ty));
                }
                Err(format!("{} a derive is named like `describe` or `json.json` (on type `{}`)", at, ty))
            }
            _ => Err(format!("{} a derive is named like `describe` or `json.json` (on type `{}`)", at, ty)),
        }
    }

    /// The generator named by `#[generated(...)]` on the function `fname` of `module`.
    fn generator_of(&self, module: &str, attrs: &[Attr], line: u32) -> R<(String, String)> {
        let a = attrs.iter().find(|a| a.name == "generated").ok_or("internal error: not generated")?;
        let at = format!("{}.br:{}:", self.main_name, user_line(line));
        let uses: Vec<String> = self.uni.file(module).map(|f| f.uses.iter().map(|u| u[0].clone()).collect()).unwrap_or_default();
        let ok = |m: &str, n: &str| self.func_in(m, n).map_or(false, |(_, fd)| fd.comptime);
        match &a.args[0].kind {
            ExprKind::Ident(n) if ok(module, n) => Ok((module.to_string(), n.clone())),
            ExprKind::Ident(n) => Err(format!("{} the generator `{}` is not a comptime function of this package", at, n)),
            ExprKind::Field { expr, name, .. } => match &expr.kind {
                ExprKind::Ident(p) if (uses.contains(p) || p == module) && ok(p, name) => Ok((p.clone(), name.clone())),
                _ => Err(format!("{} the generator `{}` was not found", at, name)),
            },
            _ => Err(format!("{} the generator must be a name", at)),
        }
    }

    fn user_type_in_main(&self, n: &str) -> bool {
        self.uni.type_decl(&self.main_name, n).is_some()
    }

    /// Qualifies the names of the program's own types, for use in another module.
    fn qualify_for(&self, t: &Type, module: &str) -> Type {
        if module == self.main_name {
            return t.clone();
        }
        struct Q<'a>(&'a Expander, &'a str);
        impl<'a> Visit for Q<'a> {
            fn ty(&mut self, t: &mut Type) {
                if let Type::Named { path, .. } = t {
                    if path.len() == 1 && self.0.user_type_in_main(&path[0]) && !is_builtin_type(&path[0]) {
                        path.insert(0, self.1.to_string());
                    }
                }
            }
        }
        let mut t = t.clone();
        walk_ty(&mut t, &mut Q(self, &self.main_name));
        t
    }

    /// Is `e` a call to a generated function? Returns what is needed to specialize it.
    fn call_info(&self, e: &Expr) -> R<Option<CallInfo>> {
        let ExprKind::Call { callee, args, targs } = &e.kind else { return Ok(None) };
        let (module, fname) = match &callee.kind {
            ExprKind::Ident(n) => (self.main_name.clone(), n.clone()),
            ExprKind::Field { expr, name, .. } => match &expr.kind {
                ExprKind::Ident(p) if self.main().uses.iter().any(|u| u[0] == *p) => (p.clone(), name.clone()),
                _ => return Ok(None),
            },
            _ => return Ok(None),
        };
        let Some((d, fd)) = self.func_in(&module, &fname) else { return Ok(None) };
        if !has_attr(d, "generated") {
            return Ok(None);
        }
        let at = format!("{}.br:{}:", self.main_name, e.line);
        let Some(ci) = fd.params.iter().position(|p| p.comptime) else {
            return Err(format!("{} `{}` is #[generated] but has no `comptime` parameter", at, fname));
        };
        let gen = self.generator_of(&module, &d.attrs, d.line)?;
        let (arg_ty, targ_form) = if targs.len() == 1 {
            (targs[0].clone(), true)
        } else {
            let Some(a) = args.get(ci) else {
                return Err(format!("{} `{}` needs a type for its `comptime` parameter `{}`", at, fname, fd.params[ci].name));
            };
            let not_type = || {
                format!(
                    "{} the argument for the `comptime` parameter `{}` of `{}` must be a type known at compile time",
                    at, fd.params[ci].name, fname
                )
            };
            let t = match &a.value.kind {
                ExprKind::Ident(n) if is_builtin_type(n) || self.user_type_in_main(n) => Type::Named { path: vec![n.clone()], args: vec![] },
                ExprKind::Field { expr, name, .. } => match &expr.kind {
                    ExprKind::Ident(p) if self.uni.type_decl(p, name).is_some() => Type::Named { path: vec![p.clone(), name.clone()], args: vec![] },
                    _ => return Err(not_type()),
                },
                _ => return Err(not_type()),
            };
            (t, false)
        };
        let canon = canon_type(&arg_ty, &self.main_name);
        Ok(Some(CallInfo {
            key: (format!("{}.{}", module, fname), canon),
            module,
            fname,
            template: fd.clone(),
            template_attrs: d.attrs.clone(),
            template_public: d.public,
            gen,
            comptime_idx: ci,
            arg_ty,
            targ_form,
        }))
    }

    // ---------------------------------------------------------------------
    // One round
    // ---------------------------------------------------------------------

    fn run(&mut self) -> R<()> {
        for round in 1..=MAX_ROUNDS + 1 {
            let pend = self.collect()?;
            if pend.is_empty() {
                let n = self.main().decls.len();
                return self.rewrite(Vec::new(), n);
            }
            if round == MAX_ROUNDS + 1 {
                let p = &pend[0];
                let chain = self.chains.get(&p.ty_name).cloned().unwrap_or_default();
                let chain = if chain.is_empty() { p.gen_label.clone() } else { chain.join(" -> ") };
                return Err(format!(
                    "{}.br:{}: code generation did not settle after {} rounds: {} (the output of a generator keeps asking for more generation; used by `{}`)",
                    self.main_name,
                    p.site_line,
                    MAX_ROUNDS,
                    chain,
                    p.site_desc
                ));
            }
            let outcomes = self.execute(&pend)?;
            self.apply(pend, outcomes)?;
        }
        Ok(())
    }

    fn table_for_type(&self, name: &str) -> (TypeTable, usize) {
        let mut r = Reflector::new(&self.uni);
        let root = r.declared(&self.main_name, name).unwrap_or(0);
        (r.table, root)
    }

    fn collect(&mut self) -> R<Vec<Pending>> {
        let mut out: Vec<Pending> = Vec::new();
        // derive attributes
        let decls: Vec<(usize, Decl)> = self.main().decls.iter().cloned().enumerate().collect();
        for (idx, d) in &decls {
            let DeclKind::Type(td) = &d.kind else { continue };
            for a in d.attrs.iter().filter(|a| a.name == "derive") {
                for arg in &a.args {
                    let line = user_line(d.line);
                    let (gen, label) = self.find_derive(arg, &td.name, line)?;
                    self.closure_for(&gen.0, &gen.1, d.line)?;
                    let (table, root) = self.table_for_type(&td.name);
                    out.push(Pending {
                        kind: Kind::Derive,
                        gen,
                        gen_label: label,
                        table,
                        root,
                        raw: None,
                        site_line: line,
                        site_desc: "derive attribute".into(),
                        ty_name: td.name.clone(),
                        decl_idx: *idx,
                        spec: None,
                    });
                }
            }
        }
        // generated calls and blocks, in the bodies of the program's functions
        struct Collect<'a> {
            x: &'a Expander,
            found: Vec<(CallInfoOwned, u32)>,
            blocks: Vec<(String, String, u32, bool)>,
            seen: HashSet<(String, String)>,
            err: Option<String>,
        }
        struct CallInfoOwned(CallInfo);
        impl<'a> Visit for Collect<'a> {
            fn expr(&mut self, e: &mut Expr) -> R<bool> {
                match &e.kind {
                    ExprKind::Block { name, raw } => self.blocks.push((name.clone(), raw.clone(), e.line, false)),
                    ExprKind::Call { .. } => match self.x.call_info(e) {
                        Ok(Some(ci)) => {
                            if !self.x.specs.contains_key(&ci.key) && self.seen.insert(ci.key.clone()) {
                                self.found.push((CallInfoOwned(ci), e.line));
                            }
                        }
                        Ok(None) => {}
                        Err(m) => {
                            self.err.get_or_insert(m);
                        }
                    },
                    _ => {}
                }
                Ok(true)
            }
            fn stmt(&mut self, s: &mut Stmt) -> R<Option<Vec<Stmt>>> {
                if let StmtKind::Block { name, raw } = &s.kind {
                    self.blocks.push((name.clone(), raw.clone(), s.line, true));
                }
                Ok(None)
            }
        }
        let mut c = Collect { x: &*self, found: Vec::new(), blocks: Vec::new(), seen: HashSet::new(), err: None };
        // blocks and generated calls interleave in source order; keep the order of the walk
        let mut order: Vec<(bool, usize)> = Vec::new();
        for (_, d) in &decls {
            let DeclKind::Func(fd) = &d.kind else { continue };
            if fd.comptime || has_attr(d, "generated") {
                continue;
            }
            let (nf, nb) = (c.found.len(), c.blocks.len());
            let mut copy = d.clone();
            let _ = walk_decl(&mut copy, &mut c);
            for i in nb..c.blocks.len() {
                order.push((true, i));
            }
            for i in nf..c.found.len() {
                order.push((false, i));
            }
        }
        if let Some(m) = c.err.take() {
            return Err(m);
        }
        let Collect { found, blocks, .. } = c;
        let mut found: Vec<Option<(CallInfoOwned, u32)>> = found.into_iter().map(Some).collect();
        // blocks first in their own order matter (the rewrite consumes them in order); the
        // generated calls only need to be unique
        for (is_block, i) in order {
            if is_block {
                let (name, raw, line, stmt) = blocks[i].clone();
                let Some((m, label)) = self.find_block(&name) else {
                    return Err(format!("{}.br:{}: block `{}` has no #[block] comptime function", self.main_name, line, name));
                };
                self.closure_for(&m, &name, line)?;
                out.push(Pending {
                    kind: Kind::Block,
                    gen: (m, name.clone()),
                    gen_label: label,
                    table: TypeTable::default(),
                    root: 0,
                    raw: Some(raw),
                    site_line: line,
                    site_desc: format!("block `{}`", name),
                    ty_name: name,
                    decl_idx: if stmt { 1 } else { 0 },
                    spec: None,
                });
            } else if let Some((CallInfoOwned(ci), line)) = found[i].take() {
                self.closure_for(&ci.gen.0, &ci.gen.1, line)?;
                let mut r = Reflector::new(&self.uni);
                let root = r.reflect(&ci.arg_ty, &self.main_name);
                let table = r.table;
                let ty_name = table.types[root].name.clone();
                let n = self.counter;
                self.counter += 1;
                let name = format!("__g{}_{}", n, ci.fname);
                let label = if ci.gen.0 == self.main_name { ci.gen.1.clone() } else { format!("{}.{}", ci.gen.0, ci.gen.1) };
                out.push(Pending {
                    kind: Kind::Generated,
                    gen: ci.gen.clone(),
                    gen_label: label,
                    table,
                    root,
                    raw: None,
                    site_line: line,
                    site_desc: format!("call of `{}`", ci.fname),
                    ty_name,
                    decl_idx: 0,
                    spec: Some(SpecInfo {
                        key: ci.key.clone(),
                        module: ci.module.clone(),
                        template: ci.template.clone(),
                        template_attrs: ci.template_attrs.clone(),
                        template_public: ci.template_public,
                        comptime_idx: ci.comptime_idx,
                        arg_ty: ci.arg_ty.clone(),
                        name,
                    }),
                });
            }
        }
        Ok(out)
    }

    fn find_block(&self, name: &str) -> Option<(String, String)> {
        let mut mods = vec![self.main_name.clone()];
        mods.extend(self.main().uses.iter().map(|u| u[0].clone()));
        for m in mods {
            if let Some((d, fd)) = self.func_in(&m, name) {
                if fd.comptime && has_attr(d, "block") {
                    let label = if m == self.main_name { name.to_string() } else { format!("{}.{}", m, name) };
                    return Some((m, label));
                }
            }
        }
        None
    }

    fn execute(&mut self, pend: &[Pending]) -> R<Vec<String>> {
        let mut specs: Vec<CallSpec> = Vec::new();
        for (i, p) in pend.iter().enumerate() {
            let closure = self.closures.get(&p.gen).cloned().unwrap_or_default();
            specs.push(CallSpec {
                wire: WireCall {
                    id: i,
                    generator: p.gen_label.clone(),
                    arg_kind: p.kind.word().to_string(),
                    table: p.table.clone(),
                    root: p.root,
                    raw: p.raw.clone(),
                },
                gen_module: p.gen.0.clone(),
                gen_name: p.gen.1.clone(),
                closure,
                site_module: self.main_name.clone(),
                site_line: p.site_line,
                site_desc: p.site_desc.clone(),
                ty_name: p.ty_name.clone(),
                tag: String::new(),
            });
        }
        let target_name = if self.target == Target::Linux { "linux" } else { "windows" };
        let outcomes = run_calls(&self.uni, &self.main_name, &specs, target_name)?;
        let mut texts = Vec::new();
        for (p, o) in pend.iter().zip(outcomes) {
            match o {
                Outcome::Ok(t) => texts.push(t),
                Outcome::Fail { line, msg } => {
                    let line = if line == 0 { p.site_line } else { user_line(line) };
                    return Err(format!("{}.br:{}: error from comptime function `{}`: {}", self.main_name, line, p.gen_label, msg));
                }
            }
        }
        Ok(texts)
    }

    // ---------------------------------------------------------------------
    // Applying results
    // ---------------------------------------------------------------------

    fn unit_for(&self, p: &Pending, text: &str) -> u32 {
        register_unit(GenUnit {
            base: 0,
            generator: p.gen_label.clone(),
            ty: p.ty_name.clone(),
            site_line: p.site_line,
            site: p.site_desc.clone(),
            module: self.main_name.clone(),
            lines: text.lines().map(|l| l.to_string()).collect(),
        })
    }

    fn parse_at(&self, src: &str, line: u32, base: u32, p: &Pending) -> R<File> {
        let toks = Lexer::at(src, line, 1).with_blocks(self.blocks.clone()).tokenize().map_err(|d| self.gen_error(&d.msg, d.line, base, p))?;
        Parser::new(toks).parse_file().map_err(|d| self.gen_error(&d.msg, d.line, base, p))
    }

    fn gen_error(&self, msg: &str, line: u32, base: u32, _p: &Pending) -> String {
        map_error(format!("{}.br:{}: {}", self.main_name, line.max(base), msg))
    }

    fn apply(&mut self, pend: Vec<Pending>, texts: Vec<String>) -> R<()> {
        if super::settings().dump {
            for (p, t) in pend.iter().zip(&texts) {
                eprintln!("--- comptime {} for `{}` ---\n{}\n--- end ---", p.gen_label, p.ty_name, t);
            }
        }
        let n_old = self.main().decls.len();
        let mut new_decls: Vec<Decl> = Vec::new();
        let mut derived_from: Vec<(usize, String)> = Vec::new();
        let mut block_results: Vec<BlockResult> = Vec::new();
        for (p, text) in pend.iter().zip(&texts) {
            let base = self.unit_for(p, text);
            match p.kind {
                Kind::Derive => {
                    let src = format!("package __g\n{}", text);
                    let file = self.parse_at(&src, base, base, p)?;
                    let n = self.counter;
                    self.counter += 1;
                    let mut decls = file.decls;
                    for u in file.uses {
                        if !self.main().uses.contains(&u) {
                            self.main_mut().uses.push(u);
                        }
                    }
                    if p.gen.0 != self.main_name && !self.main().uses.iter().any(|u| u[0] == p.gen.0) {
                        self.main_mut().uses.push(vec![p.gen.0.clone()]);
                    }
                    self.check_collisions(&decls, p)?;
                    mangle(&mut decls, n);
                    // a generated type that derives again continues the chain
                    let mut chain = self.chains.get(&p.ty_name).cloned().unwrap_or_default();
                    chain.push(p.gen_label.clone());
                    for d in &decls {
                        if let DeclKind::Type(td) = &d.kind {
                            if d.attrs.iter().any(|a| a.name == "derive") {
                                self.chains.insert(td.name.clone(), chain.clone());
                            }
                        }
                    }
                    derived_from.push((p.decl_idx, p.gen_label.clone()));
                    new_decls.extend(decls);
                }
                Kind::Generated => {
                    let sp = p.spec.clone().unwrap();
                    let src = format!("package __g\nfunc __b()\n{}\nend\n", text);
                    let file = self.parse_at(&src, base.saturating_sub(1), base, p)?;
                    let body = match file.decls.into_iter().next().map(|d| d.kind) {
                        Some(DeclKind::Func(f)) => f.body,
                        _ => return Err("internal error: generated body".into()),
                    };
                    let mut fd = sp.template.clone();
                    fd.name = sp.name.clone();
                    fd.params.remove(sp.comptime_idx);
                    fd.body = body;
                    fd.comptime = false;
                    let tname = sp.template.params[sp.comptime_idx].name.clone();
                    let arg = self.qualify_for(&sp.arg_ty, &sp.module);
                    subst_type(&mut fd, &tname, &arg);
                    let attrs: Vec<Attr> = sp.template_attrs.iter().filter(|a| a.name != "generated").cloned().collect();
                    let decl = Decl { attrs, public: sp.template_public, kind: DeclKind::Func(fd), line: base };
                    self.specs.insert(sp.key.clone(), sp.name.clone());
                    if sp.module == self.main_name {
                        new_decls.push(decl);
                    } else {
                        self.extra.entry(sp.module.clone()).or_default().push(decl);
                    }
                }
                Kind::Block => {
                    let stmt_form = p.decl_idx == 1;
                    if stmt_form {
                        let src = format!("package __g\nfunc __b()\n{}\nend\n", text);
                        let file = self.parse_at(&src, base.saturating_sub(1), base, p)?;
                        let body = match file.decls.into_iter().next().map(|d| d.kind) {
                            Some(DeclKind::Func(f)) => f.body,
                            _ => return Err("internal error: generated block".into()),
                        };
                        block_results.push(BlockResult::Stmts(body));
                    } else {
                        let src = format!("package __g\nfunc __b()\n    let __e = (\n{}\n)\nend\n", text);
                        let file = self.parse_at(&src, base.saturating_sub(2), base, p)?;
                        let expr = match file.decls.into_iter().next().map(|d| d.kind) {
                            Some(DeclKind::Func(mut f)) if !f.body.is_empty() => match f.body.remove(0).kind {
                                StmtKind::Var(v) => v.value,
                                _ => None,
                            },
                            _ => None,
                        };
                        match expr {
                            Some(e) => block_results.push(BlockResult::Expr(e)),
                            None => {
                                return Err(format!(
                                    "{}.br:{}: block `{}` did not produce an expression",
                                    self.main_name, p.site_line, p.ty_name
                                ))
                            }
                        }
                    }
                }
            }
        }
        // the derive attributes are used up
        for (idx, _) in &derived_from {
            if let Some(d) = self.main_mut().decls.get_mut(*idx) {
                d.attrs.retain(|a| a.name != "derive");
            }
        }
        self.main_mut().decls.extend(new_decls);
        self.rewrite(block_results, n_old)
    }

    /// A generated method may not replace an existing one.
    fn check_collisions(&mut self, decls: &[Decl], p: &Pending) -> R<()> {
        for d in decls {
            let DeclKind::Func(fd) = &d.kind else { continue };
            let Some(r) = &fd.receiver else { continue };
            let Type::Named { path, .. } = &r.ty else { continue };
            let ty = path.last().unwrap().clone();
            let key = (ty.clone(), fd.name.clone());
            if let Some(other) = self.gen_methods.get(&key) {
                return Err(format!(
                    "{}.br:{}: `{}` generates the method `{}` for `{}`, but it was already generated by `{}`",
                    self.main_name, p.site_line, p.gen_label, fd.name, ty, other
                ));
            }
            for e in &self.main().decls {
                if let DeclKind::Func(ef) = &e.kind {
                    if ef.name == fd.name {
                        if let Some(er) = &ef.receiver {
                            if matches!(&er.ty, Type::Named { path: ep, .. } if ep.last() == Some(&ty)) {
                                return Err(format!(
                                    "{}.br:{}: `{}` generates the method `{}` for `{}`, but `{}` is already declared for `{}` at line {}",
                                    self.main_name,
                                    p.site_line,
                                    p.gen_label,
                                    fd.name,
                                    ty,
                                    fd.name,
                                    ty,
                                    user_line(e.line)
                                ));
                            }
                        }
                    }
                }
            }
            self.gen_methods.insert(key, p.gen_label.clone());
        }
        Ok(())
    }

    /// Replaces blocks with their results, in order, and calls of generated functions with
    /// calls of their specializations.
    fn rewrite(&mut self, blocks: Vec<BlockResult>, n_old: usize) -> R<()> {
        struct Rewrite<'a> {
            x: &'a Expander,
            blocks: std::vec::IntoIter<BlockResult>,
            err: Option<String>,
        }
        impl<'a> Visit for Rewrite<'a> {
            fn expr(&mut self, e: &mut Expr) -> R<bool> {
                if let ExprKind::Block { .. } = &e.kind {
                    if let Some(BlockResult::Expr(new)) = self.blocks.next() {
                        *e = new;
                    }
                    return Ok(false);
                }
                if let ExprKind::Call { .. } = &e.kind {
                    match self.x.call_info(e) {
                        Ok(Some(ci)) => {
                            if let Some(spec) = self.x.specs.get(&ci.key) {
                                let ExprKind::Call { callee, args, targs } = &mut e.kind else { unreachable!() };
                                let line = callee.line;
                                if ci.targ_form {
                                    targs.clear();
                                } else if ci.comptime_idx < args.len() {
                                    args.remove(ci.comptime_idx);
                                }
                                let ident = Expr { kind: ExprKind::Ident(spec.clone()), line };
                                **callee = if ci.module == self.x.main_name {
                                    ident
                                } else {
                                    Expr {
                                        kind: ExprKind::Field { expr: Box::new(Expr { kind: ExprKind::Ident(ci.module.clone()), line }), name: spec.clone(), safe: false },
                                        line,
                                    }
                                };
                            }
                        }
                        Ok(None) => {}
                        Err(m) => {
                            self.err.get_or_insert(m);
                        }
                    }
                }
                Ok(true)
            }
            fn stmt(&mut self, s: &mut Stmt) -> R<Option<Vec<Stmt>>> {
                if let StmtKind::Block { .. } = &s.kind {
                    if let Some(BlockResult::Stmts(b)) = self.blocks.next() {
                        return Ok(Some(b));
                    }
                }
                Ok(None)
            }
        }
        let mut decls = self.main().decls.clone();
        let result = {
            let mut rw = Rewrite { x: &*self, blocks: blocks.into_iter(), err: None };
            for d in decls.iter_mut().take(n_old) {
                let skip = match &d.kind {
                    DeclKind::Func(fd) => fd.comptime || d.attrs.iter().any(|a| a.name == "generated"),
                    _ => true,
                };
                if skip {
                    continue;
                }
                let _ = walk_decl(d, &mut rw);
            }
            rw.err.take()
        };
        self.main_mut().decls = decls;
        match result {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

enum BlockResult {
    Expr(Expr),
    Stmts(Vec<Stmt>),
}

/// Renames the top-level functions, types and constants of generated code to `__g<n>_<name>`.
fn mangle(decls: &mut [Decl], n: usize) {
    let mut map: HashMap<String, String> = HashMap::new();
    for d in decls.iter() {
        let name = match &d.kind {
            DeclKind::Func(f) if f.receiver.is_none() => Some(&f.name),
            DeclKind::Type(t) => Some(&t.name),
            DeclKind::Var(v) => Some(&v.name),
            _ => None,
        };
        if let Some(name) = name {
            map.insert(name.clone(), format!("__g{}_{}", n, name));
        }
    }
    if map.is_empty() {
        return;
    }
    for d in decls.iter_mut() {
        match &mut d.kind {
            DeclKind::Func(f) if f.receiver.is_none() => {
                if let Some(m) = map.get(&f.name) {
                    f.name = m.clone();
                }
            }
            DeclKind::Type(t) => {
                if let Some(m) = map.get(&t.name) {
                    t.name = m.clone();
                }
            }
            DeclKind::Var(v) => {
                if let Some(m) = map.get(&v.name) {
                    v.name = m.clone();
                }
            }
            _ => {}
        }
        struct Rename<'a> {
            map: &'a HashMap<String, String>,
            bound: HashSet<String>,
        }
        impl<'a> Visit for Rename<'a> {
            fn ty(&mut self, t: &mut Type) {
                if let Type::Named { path, .. } = t {
                    if path.len() == 1 {
                        if let Some(m) = self.map.get(&path[0]) {
                            path[0] = m.clone();
                        }
                    }
                }
            }
            fn expr(&mut self, e: &mut Expr) -> R<bool> {
                match &mut e.kind {
                    ExprKind::Ident(n) if !self.bound.contains(n) => {
                        if let Some(m) = self.map.get(n) {
                            *n = m.clone();
                        }
                    }
                    ExprKind::Record { path, .. } if path.len() == 1 => {
                        if let Some(m) = self.map.get(&path[0]) {
                            path[0] = m.clone();
                        }
                    }
                    _ => {}
                }
                Ok(true)
            }
        }
        let bound = match &d.kind {
            DeclKind::Func(f) => bound_names(f),
            _ => HashSet::new(),
        };
        let mut r = Rename { map: &map, bound };
        let _ = walk_decl(d, &mut r);
    }
}

/// Binds the type parameter `t` of a specialization to the type argument.
fn subst_type(fd: &mut FuncDecl, t: &str, arg: &Type) {
    struct S<'a>(&'a str, &'a Type);
    impl<'a> Visit for S<'a> {
        fn ty(&mut self, ty: &mut Type) {
            if matches!(ty, Type::Named { path, args } if path.len() == 1 && path[0] == self.0 && args.is_empty()) {
                *ty = self.1.clone();
            }
        }
        fn expr(&mut self, e: &mut Expr) -> R<bool> {
            if let ExprKind::Record { path, .. } = &mut e.kind {
                if path.len() == 1 && path[0] == self.0 {
                    if let Type::Named { path: p, .. } = self.1 {
                        *path = p.clone();
                    }
                }
            }
            Ok(true)
        }
    }
    let _ = walk_func(fd, &mut S(t, arg));
}
