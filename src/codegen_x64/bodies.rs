//! Checking whole function bodies before they are emitted.
//!
//! `check_function_body` walks the statements of a function with its own scopes (types only, no
//! slots, no assembly) and builds a `TypedBody`: each statement the checker understands becomes a
//! typed statement; what it does not understand stays `TypedStmtKind::Opaque` and makes the body
//! `complete: false`. A local whose type cannot be settled ends the walk (everything after it is
//! opaque), so a name is never looked up in a scope that lacks one of its locals.
//!
//! Emission checks the type it gives each `let`/`var` against the one recorded here (in builds with
//! debug assertions), the same way it checks `check_expr`.

use super::*;

/// State of one walk over a body.
#[derive(Default)]
pub(super) struct Walk {
    /// A local of unknown type was met: the rest of the function is opaque.
    stopped: bool,
    /// Type of each local declared by a `let`/`var`, by (line, name).
    vars: HashMap<(u32, String), Ty>,
    /// Taken the first time a type annotation is resolved (resolving can instantiate generics).
    snap: Option<Snapshot>,
    complete: bool,
}

impl Compiler {
    /// Checks the body of `fd` (whose signature is `sig`) and attaches the result to the typed
    /// program under the function's symbol. Emission state is left untouched.
    pub(super) fn check_function_body(&mut self, fd: &FuncDecl, sig: &Sig) {
        let inst_count = self.tp.inst_count;
        // an `async` body returns the type inside its future
        let ret = match (&sig.ret, fd.is_async) {
            (Ty::Future(inner), true) => (**inner).clone(),
            (r, _) => r.clone(),
        };
        let saved = std::mem::replace(&mut self.fs, FnState::new(ret, String::new()));
        let mut incoming: Vec<(String, Ty, bool)> = Vec::new();
        if let (Some(r), Some(rt)) = (&fd.receiver, &sig.receiver) {
            incoming.push((r.name.clone(), rt.clone(), r.by_ref && !matches!(rt, Ty::List(_) | Ty::Map(..))));
        }
        for p in &sig.params {
            incoming.push((p.name.clone(), p.ty.clone(), p.by_ref));
        }
        for (name, ty, by_ref) in incoming {
            self.declare_checked(&name, ty, true, by_ref);
        }
        let mut w = Walk { complete: true, ..Walk::default() };
        self.checking_body = true;
        self.lambda_vars.clear();
        let stmts = self.check_stmts(&fd.body, &mut w);
        self.checking_body = false;
        w.vars.extend(std::mem::take(&mut self.lambda_vars));
        if let Some(snap) = w.snap.take() {
            self.restore(snap);
        }
        self.tp.inst_count = inst_count;
        self.fs = saved;
        if fd.is_async {
            self.next_lambda_vars = Some(Rc::new(w.vars));
        } else {
            self.fs.checked_vars = Rc::new(w.vars);
        }
        self.tp.bodies.insert(sig.sym.clone(), TypedBody { stmts, ret: sig.ret.clone(), complete: w.complete });
    }

    /// Runs `f` and undoes what it did to the instantiation state (resolving a type can instantiate
     /// generics). Only while a body is being checked; otherwise the answer is `None`.
    pub(super) fn speculative<T>(&mut self, f: impl FnOnce(&mut Self) -> Option<T>) -> Option<T> {
        if !self.checking_body {
            return None;
        }
        let inst_count = self.tp.inst_count;
        let snap = self.snapshot();
        let r = f(self);
        self.restore(snap);
        self.tp.inst_count = inst_count;
        r
    }

    /// Walks the body of a closure whose parameter and result types are known: its own scope over
    /// the enclosing ones (which it captures from), its own `return` type. The types of its locals
    /// are recorded for emission to assert.
    pub(super) fn check_lambda_body(&mut self, params: &[Param], ptys: &[Ty], ret: &Ty, body: &[Stmt]) -> Vec<TypedStmt> {
        let before = self.fs.clone();
        self.fs.scopes.push(Scope::default());
        for (p, t) in params.iter().zip(ptys) {
            self.declare_checked(&p.name, t.clone(), true, false);
        }
        let saved_ret = std::mem::replace(&mut self.fs.ret, ret.clone());
        let mut w = Walk { complete: true, ..Walk::default() };
        let stmts = self.check_stmts(body, &mut w);
        self.fs.ret = saved_ret;
        self.fs.scopes.pop();
        if let Some(snap) = w.snap.take() {
            self.restore(snap);
            self.fs = before;
        }
        self.lambda_vars.extend(w.vars);
        stmts
    }

    fn declare_checked(&mut self, name: &str, ty: Ty, mutable: bool, is_ref: bool) {
        let local = Local { off: 0, ty, mutable, captured: false, reg: None, imm: None, is_ref, ind: false };
        self.fs.scopes.last_mut().unwrap().vars.insert(name.to_string(), local);
    }

    /// The statements of a block, in a scope of their own.
    fn check_block(&mut self, b: &[Stmt], w: &mut Walk) -> Vec<TypedStmt> {
        self.fs.scopes.push(Scope::default());
        let out = self.check_stmts(b, w);
        self.fs.scopes.pop();
        out
    }

    fn check_stmts(&mut self, b: &[Stmt], w: &mut Walk) -> Vec<TypedStmt> {
        let mut out = Vec::new();
        for s in b {
            if w.stopped {
                w.complete = false;
                out.push(TypedStmt { kind: TypedStmtKind::Opaque(Box::new(s.clone())), line: s.line });
                continue;
            }
            match self.check_stmt(s, w) {
                Some(t) => out.push(t),
                None => {
                    w.complete = false;
                    out.push(TypedStmt { kind: TypedStmtKind::Opaque(Box::new(s.clone())), line: s.line });
                }
            }
        }
        out
    }

    fn resolve_ann(&mut self, t: &Type, w: &mut Walk) -> Option<Ty> {
        if w.snap.is_none() {
            w.snap = Some(self.snapshot());
        }
        let m = self.cur_mod.clone();
        self.resolve(t, &m).ok()
    }

    /// `None` when the statement is not covered; the walk then goes on, unless the statement
    /// declares locals (`w.stopped` is set in that case).
    fn check_stmt(&mut self, s: &Stmt, w: &mut Walk) -> Option<TypedStmt> {
        let line = s.line;
        let kind = match &s.kind {
            StmtKind::Var(vd) => {
                let ann = match &vd.ty {
                    Some(t) => match self.resolve_ann(t, w) {
                        Some(a) => Some(a),
                        None => {
                            w.stopped = true;
                            return None;
                        }
                    },
                    None => None,
                };
                let value = match &vd.value {
                    Some(e) => self.check_value(e, ann.as_ref(), w),
                    None => None,
                };
                let ty = match (&ann, &vd.value, &value) {
                    (Some(a), _, _) => a.clone(),
                    (None, Some(_), Some(x)) if x.ty != Ty::Nil && x.ty != Ty::Void => x.ty.clone(),
                    _ => {
                        w.stopped = true;
                        return None;
                    }
                };
                // a value that does not have exactly the declared type (a coercion) stays opaque
                if ann.is_some() && vd.value.is_some() && value.as_ref().is_none_or(|x| x.ty != ty && !(x.ty == Ty::Nil && ty.nullable())) {
                    self.declare_checked(&vd.name, ty.clone(), vd.mutable, false);
                    w.vars.insert((line, vd.name.clone()), ty);
                    return None;
                }
                self.declare_checked(&vd.name, ty.clone(), vd.mutable, false);
                w.vars.insert((line, vd.name.clone()), ty.clone());
                TypedStmtKind::Var { name: vd.name.clone(), ty, value, mutable: vd.mutable }
            }
            StmtKind::Assign { target, op, value } => {
                let t = self.check_expr(target, None)?;
                let v = self.check_value(value, Some(&t.ty), w)?;
                if v.ty != t.ty && !(v.ty == Ty::Nil && t.ty.nullable()) {
                    return None;
                }
                TypedStmtKind::Assign { target: t, op, value: v }
            }
            StmtKind::Return(None) => TypedStmtKind::Return(None),
            StmtKind::Return(Some(e)) => {
                let want = match &self.fs.ret {
                    Ty::Result(inner) => (**inner).clone(),
                    t => t.clone(),
                };
                let x = self.check_value(e, Some(&want), w)?;
                // in a function that can fail, an error value is returned as the failure
                let fails = matches!(self.fs.ret, Ty::Result(_)) && x.ty == Ty::Err;
                if x.ty != want && !fails && !(x.ty == Ty::Nil && want.nullable()) {
                    return None;
                }
                TypedStmtKind::Return(Some(x))
            }
            // the call a `defer` or `spawn` runs later: typed like any call, nothing is declared
            StmtKind::Defer(e) => TypedStmtKind::Defer(self.check_expr(e, None)?),
            StmtKind::Spawn(e) => TypedStmtKind::Spawn(self.check_expr(e, None)?),
            StmtKind::Asm(a) => TypedStmtKind::Asm(a.clone()),
            StmtKind::Break => TypedStmtKind::Break,
            StmtKind::Continue => TypedStmtKind::Continue,
            StmtKind::While { cond, body } => {
                let c = self.check_expr(cond, None)?;
                if c.ty != Ty::Bool {
                    return None;
                }
                let body = self.check_block(body, w);
                TypedStmtKind::While { cond: c, body }
            }
            StmtKind::Unsafe(b) => TypedStmtKind::Unsafe(self.check_block(b, w)),
            StmtKind::For { vars, iter, body } => {
                self.fs.scopes.push(Scope::default());
                let r = self.check_for(vars, iter, body, w);
                self.fs.scopes.pop();
                r?
            }
            StmtKind::Expr(e) => match &e.kind {
                ExprKind::If { .. } | ExprKind::Match { .. } => TypedStmtKind::Expr(self.check_branching(e, None, false, w)?),
                ExprKind::Lambda { .. } => return None,
                ExprKind::Call { callee, .. } if matches!(&callee.kind, ExprKind::Ident(n) if n == "assert" || n == "assert_eq") => return None,
                _ => TypedStmtKind::Expr(self.check_expr(e, None)?),
            },
            _ => return None,
        };
        Some(TypedStmt { kind, line })
    }

    /// An expression in a value position (a `let` or `return` value, an assigned value, the last
    /// expression of a block used as a value): an `if` or `match` is walked with scopes of its
    /// own, anything else is typed by `check_expr`.
    fn check_value(&mut self, e: &Expr, want: Option<&Ty>, w: &mut Walk) -> Option<TypedExpr> {
        match &e.kind {
            ExprKind::If { .. } | ExprKind::Match { .. } => {
                let before = w.stopped;
                let x = self.check_branching(e, want, true, w);
                // a walk that lost track of a local cannot give the value a trustworthy type
                if w.stopped && !before {
                    return None;
                }
                x
            }
            _ => self.check_expr(e, want),
        }
    }

    /// An `if` or `match`, as a statement (`as_value` false: its type is `Void`) or as a value
    /// (the type of its first branch; the branches must agree, and an `if` needs its `else`).
    fn check_branching(&mut self, e: &Expr, want: Option<&Ty>, as_value: bool, w: &mut Walk) -> Option<TypedExpr> {
        match &e.kind {
            ExprKind::If { branches, else_body } => {
                if as_value && else_body.is_none() {
                    return None;
                }
                let mut typed = Vec::new();
                let mut ty: Option<Ty> = None;
                for (c, b) in branches {
                    let cx = self.check_expr(c, None)?;
                    if cx.ty != Ty::Bool {
                        return None;
                    }
                    let (body, t) = self.check_arm(b, want, as_value, w)?;
                    if ty.get_or_insert_with(|| t.clone()) != &t {
                        return None;
                    }
                    typed.push((cx, body));
                }
                let else_typed = match else_body {
                    Some(b) => {
                        let (body, t) = self.check_arm(b, want, as_value, w)?;
                        if ty.as_ref().is_some_and(|f| *f != t) {
                            return None;
                        }
                        Some(body)
                    }
                    None => None,
                };
                let ty = if as_value { ty.unwrap_or(Ty::Void) } else { Ty::Void };
                Some(TypedExpr { kind: TypedExprKind::If { branches: typed, else_body: else_typed }, ty, line: e.line })
            }
            ExprKind::Match { scrutinee, arms } => {
                let (kind, ty) = self.check_match(scrutinee, arms, want, as_value, w)?;
                Some(TypedExpr { kind, ty, line: e.line })
            }
            _ => None,
        }
    }

    /// The body of a branch, in a scope of its own. As a value, its last expression gives the type
    /// (`Void` when the block does not end in an expression), as emission's `block_value` does.
    fn check_arm(&mut self, b: &[Stmt], want: Option<&Ty>, as_value: bool, w: &mut Walk) -> Option<(Vec<TypedStmt>, Ty)> {
        self.fs.scopes.push(Scope::default());
        let r = self.check_arm_in_scope(b, want, as_value, w);
        self.fs.scopes.pop();
        r
    }

    fn check_arm_in_scope(&mut self, b: &[Stmt], want: Option<&Ty>, as_value: bool, w: &mut Walk) -> Option<(Vec<TypedStmt>, Ty)> {
        let last_value = match (as_value, b.last()) {
            (true, Some(Stmt { kind: StmtKind::Expr(_), .. })) => b.len() - 1,
            _ => b.len(),
        };
        let mut out = self.check_stmts(&b[..last_value], w);
        let mut ty = Ty::Void;
        if last_value < b.len() {
            let s = &b[last_value];
            let StmtKind::Expr(e) = &s.kind else { return None };
            if w.stopped {
                return None;
            }
            let x = self.check_value(e, want, w)?;
            ty = x.ty.clone();
            out.push(TypedStmt { kind: TypedStmtKind::Expr(x), line: s.line });
        }
        Some((out, ty))
    }

    /// A `match`, as a statement or a value. The type of each variable an arm binds is the one
    /// emission gives it; a pattern emission would reject is not covered. The type of each variable an arm binds is the one emission
    /// gives it; a pattern emission would reject is not covered.
    fn check_match(&mut self, scrut: &Expr, arms: &[MatchArm], want: Option<&Ty>, as_value: bool, w: &mut Walk) -> Option<(TypedExprKind, Ty)> {
        let s = self.check_expr(scrut, None)?;
        let st = s.ty.clone();
        let mut typed = Vec::new();
        let mut result = Ty::Void;
        for arm in arms {
            let mut binds: Option<(String, Vec<String>)> = None;
            let mut catch_name: Option<String> = None;
            for atom in &arm.pattern {
                let PatAtom::Name { name, bindings } = atom else { continue };
                match &st {
                    Ty::Enum(q) => {
                        let info = self.tp.enums.get(q)?;
                        match info.variants.iter().find(|v| &v.0 == name) {
                            Some(v) => {
                                if v.1.len() != bindings.as_ref().map_or(0, |b| b.len()) {
                                    return None;
                                }
                                if let Some(b) = bindings {
                                    binds = Some((name.clone(), b.clone()));
                                }
                            }
                            None if bindings.is_none() => catch_name = Some(name.clone()),
                            None => return None,
                        }
                    }
                    Ty::Result(inner) => {
                        let n = bindings.as_ref().map_or(0, |b| b.len());
                        match name.as_str() {
                            "ok" if n > 1 || (n == 1 && **inner == Ty::Void) => return None,
                            "err" if n > 1 => return None,
                            "ok" | "err" => {}
                            _ => return None,
                        }
                        if let Some(b) = bindings {
                            binds = Some((name.clone(), b.clone()));
                        }
                    }
                    _ => {
                        if bindings.is_some() {
                            return None;
                        }
                        catch_name = Some(name.clone());
                    }
                }
            }
            self.fs.scopes.push(Scope::default());
            if let Some(n) = catch_name {
                if n != "_" {
                    self.declare_checked(&n, st.clone(), false, false);
                }
            }
            if let Some((vname, names)) = binds {
                let tys: Vec<Ty> = match &st {
                    Ty::Enum(q) => self.tp.enums[q].variants.iter().find(|v| v.0 == vname).map(|v| v.1.clone()).unwrap_or_default(),
                    Ty::Result(inner) if vname == "ok" => vec![(**inner).clone()],
                    _ => vec![Ty::Err],
                };
                for (n, ty) in names.iter().zip(tys) {
                    if n != "_" {
                        self.declare_checked(n, ty, false, false);
                    }
                }
            }
            let arm_value = self.check_arm_in_scope(&arm.body, want, as_value, w);
            self.fs.scopes.pop();
            let (body, t) = arm_value?;
            // the value is the first arm's that has one; the others must agree
            if t != Ty::Void {
                if result == Ty::Void {
                    result = t;
                } else if result != t {
                    return None;
                }
            }
            typed.push(TypedMatchArm { pattern: arm.pattern.clone(), body });
        }
        Some((TypedExprKind::Match { scrutinee: Box::new(s), arms: typed }, if as_value { result } else { Ty::Void }))
    }

    /// A `for` loop over a range, a map, a text or a list; its variables are declared in the
    /// scope the caller opened.
    fn check_for(&mut self, vars: &[String], iter: &Expr, body: &[Stmt], w: &mut Walk) -> Option<TypedStmtKind> {
        let (typed_iter, decl): (TypedExpr, Vec<(String, Ty)>) = if let ExprKind::Binary { op: "..", lhs, rhs } = &iter.kind {
            let l = self.check_expr(lhs, None)?;
            if !l.ty.is_int() || vars.is_empty() {
                return None;
            }
            let r = self.check_expr(rhs, Some(&l.ty))?;
            if r.ty != l.ty {
                w.stopped = true;
                return None;
            }
            let t = l.ty.clone();
            let line = iter.line;
            let range = TypedExpr { kind: TypedExprKind::Binary { op: "..", lhs: Box::new(l), rhs: Box::new(r) }, ty: t.clone(), line };
            (range, vec![(vars[0].clone(), t)])
        } else {
            let x = self.check_expr(iter, None)?;
            let decl = match (&x.ty, vars.len()) {
                (Ty::Map(k, _), 1) => vec![(vars[0].clone(), (**k).clone())],
                (Ty::Map(k, v), 2) => vec![(vars[0].clone(), (**k).clone()), (vars[1].clone(), (**v).clone())],
                (Ty::Text, 1) => vec![(vars[0].clone(), Ty::Char)],
                (Ty::List(el), 1) => vec![(vars[0].clone(), (**el).clone())],
                (Ty::List(el), 2) => vec![(vars[0].clone(), Ty::int()), (vars[1].clone(), (**el).clone())],
                _ => {
                    w.stopped = true;
                    return None;
                }
            };
            (x, decl)
        };
        let names = decl.iter().map(|d| d.0.clone()).collect();
        for (n, t) in decl {
            self.declare_checked(&n, t, false, false);
        }
        let body = self.check_block(body, w);
        Some(TypedStmtKind::For { vars: names, iter: typed_iter, body })
    }
}
