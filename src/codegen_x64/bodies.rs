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
struct Walk {
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
        let saved = std::mem::replace(&mut self.fs, FnState::new(sig.ret.clone(), String::new()));
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
        let stmts = self.check_stmts(&fd.body, &mut w);
        if let Some(snap) = w.snap.take() {
            self.restore(snap);
        }
        self.tp.inst_count = inst_count;
        self.fs = saved;
        self.fs.checked_vars = w.vars;
        self.tp.bodies.insert(sig.sym.clone(), TypedBody { stmts, ret: sig.ret.clone(), complete: w.complete });
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
                let value = vd.value.as_ref().and_then(|e| self.check_expr(e, ann.as_ref()));
                let ty = match (&ann, &vd.value, &value) {
                    (Some(a), _, _) => a.clone(),
                    (None, Some(_), Some(x)) if x.ty != Ty::Nil && x.ty != Ty::Void => x.ty.clone(),
                    _ => {
                        w.stopped = true;
                        return None;
                    }
                };
                // a value that does not have exactly the declared type (a coercion) stays opaque
                if ann.is_some() && vd.value.is_some() && value.as_ref().is_none_or(|x| x.ty != ty) {
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
                let v = self.check_expr(value, Some(&t.ty))?;
                if v.ty != t.ty {
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
                let x = self.check_expr(e, Some(&want))?;
                if x.ty != want {
                    return None;
                }
                TypedStmtKind::Return(Some(x))
            }
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
                ExprKind::If { branches, else_body } => {
                    let mut typed = Vec::new();
                    for (c, b) in branches {
                        let cx = self.check_expr(c, None)?;
                        if cx.ty != Ty::Bool {
                            return None;
                        }
                        typed.push((cx, self.check_block(b, w)));
                    }
                    let else_body = else_body.as_ref().map(|b| self.check_block(b, w));
                    let x = TypedExpr { kind: TypedExprKind::If { branches: typed, else_body }, ty: Ty::Void, line: e.line };
                    TypedStmtKind::Expr(x)
                }
                ExprKind::Match { scrutinee, arms } => {
                    let kind = self.check_match(scrutinee, arms, w)?;
                    TypedStmtKind::Expr(TypedExpr { kind, ty: Ty::Void, line: e.line })
                }
                ExprKind::Lambda { .. } => return None,
                ExprKind::Call { callee, .. } if matches!(&callee.kind, ExprKind::Ident(n) if n == "assert" || n == "assert_eq") => return None,
                _ => TypedStmtKind::Expr(self.check_expr(e, None)?),
            },
            _ => return None,
        };
        Some(TypedStmt { kind, line })
    }

    /// A `match` used as a statement. The type of each variable an arm binds is the one emission
    /// gives it; a pattern emission would reject is not covered.
    fn check_match(&mut self, scrut: &Expr, arms: &[MatchArm], w: &mut Walk) -> Option<TypedExprKind> {
        let s = self.check_expr(scrut, None)?;
        let st = s.ty.clone();
        let mut typed = Vec::new();
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
            let body = self.check_stmts(&arm.body, w);
            self.fs.scopes.pop();
            typed.push(TypedMatchArm { pattern: arm.pattern.clone(), body });
        }
        Some(TypedExprKind::Match { scrutinee: Box::new(s), arms: typed })
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
