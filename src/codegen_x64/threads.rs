//! Part of the code generator (`codegen_x64`): see the section comment below.

use super::*;

// ---------------------------------------------------------------------------
// Threads: transferable types, `shared`, `spawn_thread`
// ---------------------------------------------------------------------------

/// Names declared by `let`/`var`/`for` in a block (at any depth).
pub(super) fn declared_names(b: &[Stmt], out: &mut HashSet<String>) {
    for s in b {
        match &s.kind {
            StmtKind::Var(vd) => {
                out.insert(vd.name.clone());
            }
            StmtKind::For { vars, body, .. } => {
                out.extend(vars.iter().cloned());
                declared_names(body, out);
            }
            StmtKind::While { body, .. } | StmtKind::Unsafe(body) => declared_names(body, out),
            StmtKind::Expr(e) => {
                if let ExprKind::If { branches, else_body } = &e.kind {
                    for (_, b) in branches {
                        declared_names(b, out);
                    }
                    if let Some(b) = else_body {
                        declared_names(b, out);
                    }
                }
            }
            _ => {}
        }
    }
}

impl Compiler {
    /// The first part of `ty` that cannot cross a thread boundary, as (path, type as written).
    /// Numbers, bool, text, char, enums, closures and `Thread` are transferable; records, lists,
    /// maps, channels and results are when everything inside is; `new T`, weak references,
    /// raw pointers and futures are not. Recursive types are visited once.
    pub(super) fn transfer_problem(&self, ty: &Ty, path: &str, seen: &mut HashSet<String>) -> Option<(String, String)> {
        match ty {
            Ty::Rc(inner) => Some((path.to_string(), format!("new {}", inner.show()))),
            Ty::Weak(inner) => Some((path.to_string(), format!("a weak reference to new {}", inner.show()))),
            Ty::Ptr(_) | Ty::Future(_) => Some((path.to_string(), ty.show())),
            Ty::List(t) => self.transfer_problem(t, &format!("{}[]", path), seen),
            Ty::Map(k, v) => self
                .transfer_problem(k, &format!("{}[key]", path), seen)
                .or_else(|| self.transfer_problem(v, &format!("{}[]", path), seen)),
            Ty::Chan(t) | Ty::Result(t) => self.transfer_problem(t, path, seen),
            Ty::Rec(name) => {
                if !seen.insert(name.clone()) {
                    return None;
                }
                let info = self.tp.records.get(name)?;
                for (f, fty, _) in &info.fields {
                    if let Some(p) = self.transfer_problem(fty, &format!("{}.{}", path, f), seen) {
                        return Some(p);
                    }
                }
                None
            }
            Ty::Enum(name) => {
                if !seen.insert(name.clone()) {
                    return None;
                }
                let info = self.tp.enums.get(name)?;
                for (v, tys) in &info.variants {
                    for t in tys {
                        if let Some(p) = self.transfer_problem(t, &format!("{}.{}", path, v), seen) {
                            return Some(p);
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// An error naming `what` (an argument or a captured variable) when `ty` is not transferable.
    pub(super) fn transfer_check(&self, what: &str, root: &str, ty: &Ty) -> R<()> {
        let root = if root.starts_with('$') { "value" } else { root };
        if let Some((path, shown)) = self.transfer_problem(ty, root, &mut HashSet::new()) {
            let hint = "it cannot be used by another thread; share data between threads with `shared`, `Mutex<T>` or `Atomic`";
            if path == root {
                return self.err(format!("{} has type '{}', which is not transferable between threads: {}", what, shown, hint));
            }
            return self.err(format!("{}: field '{}' has type '{}', which is not transferable between threads: {}", what, path, shown, hint));
        }
        Ok(())
    }

    pub(super) fn thread_safe_global(&self, ty: &Ty) -> bool {
        matches!(ty, Ty::Rec(n) if n.contains("sync.Mutex") || n.contains("sync.Atomic"))
    }

    /// A mutable global `var` that the function started with `spawn_thread` (or a function it calls
    /// by name) uses must be a `Mutex` or an `Atomic`. Calls through function values, methods and
    /// qualified names are not followed; library code keeps its own per-thread state.
    pub(super) fn check_thread_globals(&self, callee: &Expr) -> R<()> {
        let ExprKind::Ident(start) = &callee.kind else { return Ok(()) };
        let mut todo = vec![(self.cur_mod.clone(), start.clone())];
        let mut done = HashSet::new();
        while let Some((m, f)) = todo.pop() {
            if !done.insert((m.clone(), f.clone())) || crate::comptime::package_source(&m, self.target).is_some() {
                continue;
            }
            let Some(module) = self.modules.iter().find(|x| x.name == m) else { continue };
            let Some(fd) = module.file.decls.iter().find_map(|d| match &d.kind {
                DeclKind::Func(fd) if fd.name == f && fd.receiver.is_none() => Some(fd),
                _ => None,
            }) else {
                continue;
            };
            let mut names = Vec::new();
            collect_block(&fd.body, &mut names);
            let mut locals = HashSet::new();
            declared_names(&fd.body, &mut locals);
            locals.extend(fd.params.iter().map(|p| p.name.clone()));
            for n in names {
                if locals.contains(&n) {
                    continue;
                }
                if let Some(g) = self.tp.globals.get(&(m.clone(), n.clone())) {
                    if g.mutable && g.konst.is_none() && !self.thread_safe_global(&g.ty) {
                        return self.err(format!(
                            "global variable '{}' is mutable and is used by '{}', which runs on another thread: threads cannot share a plain global; use `shared Atomic` or `shared Mutex<{}>` instead",
                            n,
                            f,
                            g.ty.show()
                        ));
                    }
                } else if self.tp.funcs.contains_key(&(m.clone(), n.clone())) {
                    todo.push((m.clone(), n));
                }
            }
        }
        Ok(())
    }

    /// The `shared` local that the place `e` is rooted at.
    pub(super) fn shared_root(&self, e: &Expr, include_root: bool) -> Option<String> {
        match &e.kind {
            ExprKind::Ident(n) if include_root && self.fs.shared_locals.contains(n) && self.lookup(n).is_some() => Some(n.clone()),
            ExprKind::Field { expr, .. } | ExprKind::Index { expr, .. } => self.shared_root(expr, true),
            _ => None,
        }
    }

    /// Writing through a `shared` value is an error: its content is immutable.
    pub(super) fn shared_write_check(&self, target: &Expr, include_root: bool) -> R<()> {
        if let Some(n) = self.shared_root(target, include_root) {
            let ty = self.lookup(&n).map(|l| l.ty.show()).unwrap_or_default();
            return self.err(format!(
                "'{}' is a `shared` value: its content is immutable; to change it from several threads put it in a `Mutex<{}>`",
                n, ty
            ));
        }
        Ok(())
    }

    /// `shared e`: the value of `e`, which every thread may read (atomic counts, immutable content).
    pub(super) fn shared_expr(&mut self, inner: &Expr, want: Option<&Ty>) -> R<Ty> {
        let t = self.value(inner, want)?;
        if !matches!(t, Ty::Rec(_) | Ty::List(_) | Ty::Map(..) | Ty::Text) {
            return self.err(format!("`shared` needs a record, list, map or text, found {}", t.show()));
        }
        self.transfer_check("the value after `shared`", "value", &t)?;
        Ok(t)
    }

    /// `spawn_thread f(args)`: the arguments are evaluated here, then the call runs on a new thread.
    pub(super) fn spawn_thread(&mut self, call: &Expr, line: u32) -> R<Ty> {
        let Some(sig) = self.tp.funcs.get(&("thread".to_string(), "start".to_string())).cloned() else {
            return self.err("'spawn_thread' needs the thread library");
        };
        let ExprKind::Call { callee, args, targs } = &call.kind else {
            return self.err("'spawn_thread' needs a function call, like `spawn_thread work(x)`");
        };
        self.push_scope();
        let mut new_args: Vec<Arg> = Vec::new();
        let mut labels: HashMap<String, String> = HashMap::new();
        for (i, a) in args.iter().enumerate() {
            if a.by_ref {
                return self.err("'spawn_thread' cannot pass an argument by reference");
            }
            if let ExprKind::Lambda { params, body } = &a.value.kind {
                // what the closure captures must be transferable too
                let mut names = Vec::new();
                collect_block(body, &mut names);
                let own: HashSet<&String> = params.iter().map(|p| &p.name).collect();
                let mut seen_caps = HashSet::new();
                for n in names {
                    if own.contains(&n) || !seen_caps.insert(n.clone()) {
                        continue;
                    }
                    if let Some(l) = self.lookup(&n) {
                        self.transfer_check(&format!("captured variable '{}'", n), &n, &l.ty)?;
                    }
                }
            }
            let plain = matches!(
                a.value.kind,
                ExprKind::Ident(_) | ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Char(_) | ExprKind::Bool(_) | ExprKind::Nil
            );
            if plain {
                if let ExprKind::Ident(n) = &a.value.kind {
                    labels.insert(n.clone(), format!("argument '{}'", n));
                }
                new_args.push(a.clone());
                continue;
            }
            let name = format!("$thr{}", i);
            let decl = VarDecl { name: name.clone(), name_span: Span::default(), ty: None, value: Some(a.value.clone()), mutable: false };
            self.stmt_inner(&Stmt { kind: StmtKind::Var(decl), line, span: a.value.span })?;
            labels.insert(name.clone(), format!("argument {}", i + 1));
            let mut na = a.clone();
            na.value = Expr { kind: ExprKind::Ident(name), line, span: a.value.span };
            new_args.push(na);
        }
        let rewritten = Expr { kind: ExprKind::Call { callee: callee.clone(), args: new_args, targs: targs.clone() }, line, span: Span::default() };
        let mut used = Vec::new();
        collect_expr(&rewritten, &mut used);
        let mut seen = HashSet::new();
        for n in used {
            if !seen.insert(n.clone()) {
                continue;
            }
            if let Some(l) = self.lookup(&n) {
                let what = labels.get(&n).cloned().unwrap_or_else(|| format!("captured variable '{}'", n));
                self.transfer_check(&what, &n, &l.ty)?;
            }
        }
        self.check_thread_globals(callee)?;
        let body = vec![Stmt { kind: StmtKind::Expr(rewritten), line, span: Span::default() }];
        let want = Ty::Func { params: vec![], ret: Box::new(Ty::Void), ext: false };
        self.lambda(&[], &body, Some(&want))?;
        let t = self.temp();
        self.e(format!("mov qword ptr [rbp - {}], rax", t));
        self.emit_call(&[t], &format!("call {}", sig.sym), None);
        self.free(t);
        self.pop_scope();
        Ok(sig.ret.clone())
    }
}

/// The package part of a qualified type name (`text.Builder`, `list.Stack<int>`).
pub(super) fn type_owner(q: &str) -> &str {
    let base = q.split('<').next().unwrap_or(q);
    base.rsplit_once('.').map_or(base, |x| x.0)
}

/// Single-segment type names written in `t` (the package's own types), except type parameters.
pub(super) fn named_types(t: &Type, generics: &[&str], out: &mut Vec<String>) {
    match t {
        Type::Named { path, args, .. } => {
            if path.len() == 1 && !generics.contains(&path[0].as_str()) {
                out.push(path[0].clone());
            }
            for a in args {
                named_types(a, generics, out);
            }
        }
        Type::Optional(i) | Type::Pointer(i) | Type::Shared(i) | Type::Slice(i) | Type::Array(_, i) => named_types(i, generics, out),
        Type::Func { params, ret, .. } => {
            for p in params {
                named_types(p, generics, out);
            }
            if let Some(r) = ret {
                if let Some(t) = &r.ty {
                    named_types(t, generics, out);
                }
            }
        }
    }
}

