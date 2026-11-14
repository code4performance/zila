//! Typing without emitting code.
//!
//! `type_of` answers "what type does this expression have?" from the declarations and the locals in
//! scope, without writing any assembly and without changing the compiler's state. It covers the
//! forms migrated so far and answers `None` for the rest (and for anything that would be an error:
//! reporting errors stays with emission for now).
//!
//! While the migration lasts, `Compiler::expr` checks, in builds with debug assertions (so in every
//! test), that the type emission returns is the one `type_of` gave. A form is migrated when the two
//! agree on the whole suite; the old typing inside emission is deleted at the end (task 5.1).

use super::*;

impl Compiler {
    pub(super) fn type_of(&self, e: &Expr, want: Option<&Ty>) -> Option<Ty> {
        match &e.kind {
            ExprKind::Int(_) => Some(match want {
                Some(t) if t.is_int() => t.clone(),
                _ => Ty::int(),
            }),
            ExprKind::Char(_) => Some(Ty::Char),
            ExprKind::Bool(_) => Some(Ty::Bool),
            ExprKind::Nil => Some(Ty::Nil),
            ExprKind::Float(_) => Some(Ty::Float { bits: if matches!(want, Some(Ty::Float { bits: 32 })) { 32 } else { 64 } }),
            ExprKind::Text(_) => Some(Ty::Text),
            ExprKind::Binary { op, .. } if matches!(*op, "==" | "!=" | "<" | "<=" | ">" | ">=" | "and" | "or") => Some(Ty::Bool),
            ExprKind::Unary { op: "not", .. } => Some(Ty::Bool),
            ExprKind::Unary { op: "-", expr } => {
                let t = self.type_of(expr, want)?;
                (t.is_int() || matches!(t, Ty::Float { .. })).then_some(t)
            }
            // arithmetic is typed only when both sides have the same number type (or both are
            // text for `+`); mixed widths and the literal rules stay with emission for now
            ExprKind::Binary { op, lhs, rhs } if matches!(*op, "+" | "-" | "*" | "/" | "%") => {
                let w = want.filter(|t| t.is_int() || matches!(t, Ty::Float { .. }));
                let l = self.type_of(lhs, w)?;
                let r = self.type_of(rhs, Some(&l))?;
                let number = l.is_int() || matches!(l, Ty::Float { .. });
                (l == r && (number || (*op == "+" && l == Ty::Text))).then_some(l)
            }
            ExprKind::Index { expr, .. } => match rc_view(&self.type_of(expr, None)?) {
                Ty::List(el) => Some(*el),
                Ty::Map(_, v) => Some(*v),
                _ => None,
            },
            ExprKind::Ident(name) => self.type_of_ident(name, want),
            ExprKind::Field { expr, name, safe: false } => {
                let Ty::Rec(r) = rc_view(&self.type_of(expr, None)?) else { return None };
                self.field_of(&r, name).map(|f| f.0)
            }
            ExprKind::Call { callee, targs, .. } if targs.is_empty() => self.type_of_call(callee),
            _ => None,
        }
    }

    /// The typed tree of `e`, for the forms migrated so far (`None` for the rest, and when any
    /// part of `e` is not covered). Its type is the one `type_of` and emission give: emission
    /// asserts it in builds with debug assertions.
    pub(super) fn check_expr(&self, e: &Expr, want: Option<&Ty>) -> Option<TypedExpr> {
        let (kind, ty) = match &e.kind {
            ExprKind::Int(n) => (TypedExprKind::Int(*n), self.type_of(e, want)?),
            ExprKind::Float(f) => (TypedExprKind::Float(*f), self.type_of(e, want)?),
            ExprKind::Char(c) => (TypedExprKind::Char(*c), Ty::Char),
            ExprKind::Bool(b) => (TypedExprKind::Bool(*b), Ty::Bool),
            ExprKind::Nil => (TypedExprKind::Nil, Ty::Nil),
            ExprKind::Text(parts) => (TypedExprKind::Text(parts.clone()), Ty::Text),
            ExprKind::Ident(name) => (TypedExprKind::Ident(name.clone()), self.type_of_ident(name, want)?),
            ExprKind::Unary { op: "not", expr } => {
                let x = self.check_expr(expr, None)?;
                (TypedExprKind::Unary { op: "not", expr: Box::new(x) }, Ty::Bool)
            }
            ExprKind::Unary { op: "-", expr } => {
                let x = self.check_expr(expr, want)?;
                let t = x.ty.clone();
                if !(t.is_int() || matches!(t, Ty::Float { .. })) {
                    return None;
                }
                (TypedExprKind::Unary { op: "-", expr: Box::new(x) }, t)
            }
            ExprKind::Binary { op, lhs, rhs } if matches!(*op, "and" | "or") => {
                let l = self.check_expr(lhs, None)?;
                let r = self.check_expr(rhs, None)?;
                (TypedExprKind::Binary { op, lhs: Box::new(l), rhs: Box::new(r) }, Ty::Bool)
            }
            ExprKind::Binary { op, lhs, rhs } if matches!(*op, "==" | "!=" | "<" | "<=" | ">" | ">=") => {
                let l = self.check_expr(lhs, None)?;
                let r = self.check_expr(rhs, Some(&l.ty))?;
                (TypedExprKind::Binary { op, lhs: Box::new(l), rhs: Box::new(r) }, Ty::Bool)
            }
            ExprKind::Binary { op, lhs, rhs } if matches!(*op, "+" | "-" | "*" | "/" | "%") => {
                let w = want.filter(|t| t.is_int() || matches!(t, Ty::Float { .. }));
                let l = self.check_expr(lhs, w)?;
                let r = self.check_expr(rhs, Some(&l.ty))?;
                let number = l.ty.is_int() || matches!(l.ty, Ty::Float { .. });
                if l.ty != r.ty || !(number || (*op == "+" && l.ty == Ty::Text)) {
                    return None;
                }
                let t = l.ty.clone();
                (TypedExprKind::Binary { op, lhs: Box::new(l), rhs: Box::new(r) }, t)
            }
            ExprKind::Index { expr, index } => {
                let x = self.check_expr(expr, None)?;
                let i = self.check_expr(index, None)?;
                let t = match rc_view(&x.ty) {
                    Ty::List(el) => *el,
                    Ty::Map(_, v) => *v,
                    _ => return None,
                };
                (TypedExprKind::Index { expr: Box::new(x), index: Box::new(i) }, t)
            }
            ExprKind::Field { expr, name, safe: false } => {
                let x = self.check_expr(expr, None)?;
                let Ty::Rec(r) = rc_view(&x.ty) else { return None };
                let t = self.field_of(&r, name)?.0;
                (TypedExprKind::Field { expr: Box::new(x), name: name.clone(), safe: false }, t)
            }
            ExprKind::Call { callee, args, targs } if targs.is_empty() => {
                let (callee, t) = self.resolve_call(callee)?;
                (TypedExprKind::Call { callee, args: args.clone() }, t)
            }
            ExprKind::List(items) => {
                let mut el = match want {
                    Some(Ty::List(t)) => Some((**t).clone()),
                    _ => None,
                };
                let mut typed = Vec::new();
                for it in items {
                    let x = self.check_expr(it, el.as_ref())?;
                    match &el {
                        Some(w) if *w != x.ty => return None,
                        Some(_) => {}
                        None => el = Some(x.ty.clone()),
                    }
                    typed.push(x);
                }
                (TypedExprKind::List(typed), Ty::List(Box::new(el?)))
            }
            ExprKind::Map(pairs) => {
                let (mut kt, mut vt) = match want {
                    Some(Ty::Map(k, v)) => (Some((**k).clone()), Some((**v).clone())),
                    _ => (None, None),
                };
                let mut typed = Vec::new();
                for (ke, ve) in pairs {
                    let k = self.check_expr(ke, kt.as_ref())?;
                    let v = self.check_expr(ve, vt.as_ref())?;
                    match (&kt, &vt) {
                        (Some(a), Some(b)) if *a != k.ty || *b != v.ty => return None,
                        (Some(_), Some(_)) => {}
                        _ => {
                            kt = Some(k.ty.clone());
                            vt = Some(v.ty.clone());
                        }
                    }
                    typed.push((k, v));
                }
                (TypedExprKind::Map(typed), Ty::Map(Box::new(kt?), Box::new(vt?)))
            }
            ExprKind::Record { .. } => {
                let (kind, rec) = self.check_record_literal(e)?;
                (kind, Ty::Rec(rec))
            }
            ExprKind::New(inner) => {
                let ExprKind::Record { .. } = &inner.kind else { return None };
                let (kind, rec) = self.check_record_literal(inner)?;
                let x = TypedExpr { kind, ty: Ty::Rec(rec), line: inner.line };
                let t = Ty::Rc(Box::new(x.ty.clone()));
                (TypedExprKind::New(Box::new(x)), t)
            }
            ExprKind::Shared(inner) => {
                let x = self.check_expr(inner, want)?;
                if !matches!(x.ty, Ty::Rec(_) | Ty::List(_) | Ty::Map(..) | Ty::Text) {
                    return None;
                }
                let t = x.ty.clone();
                (TypedExprKind::Shared(Box::new(x)), t)
            }
            ExprKind::Deref(inner) => {
                let x = self.check_expr(inner, None)?;
                let Ty::Ptr(el) = x.ty.clone() else { return None };
                (TypedExprKind::Deref(Box::new(x)), *el)
            }
            ExprKind::Await(inner) => {
                let x = self.check_expr(inner, None)?;
                let Ty::Future(v) = x.ty.clone() else { return None };
                (TypedExprKind::Await(Box::new(x)), *v)
            }
            _ => return None,
        };
        Some(TypedExpr { kind, ty, line: e.line })
    }

    /// A record literal of a record that is not generic: its typed fields (each typed from the
    /// declared field type) and the record's key. Unknown fields and mistyped values are not typed.
    fn check_record_literal(&self, e: &Expr) -> Option<(TypedExprKind, String)> {
        let ExprKind::Record { path, fields } = &e.kind else { return None };
        let q = self.qname(&self.cur_mod, path);
        if !self.tp.rec_decls.contains_key(&q) {
            return None;
        }
        let decl = &self.tp.records.get(&q)?.fields;
        let mut typed = Vec::new();
        for (name, value) in fields {
            let fty = &decl.iter().find(|d| &d.0 == name)?.1;
            let x = self.check_expr(value, Some(fty))?;
            if x.ty != *fty {
                return None;
            }
            typed.push((name.clone(), x));
        }
        Some((TypedExprKind::Record { rec_key: q.clone(), fields: typed }, q))
    }

    /// A local, or a global, constant or function of the current module used as a value.
    fn type_of_ident(&self, name: &str, want: Option<&Ty>) -> Option<Ty> {
        if let Some(l) = self.lookup(name) {
            return Some(l.ty);
        }
        let key = (self.cur_mod.clone(), name.to_string());
        if let Some(g) = self.tp.globals.get(&key) {
            return Some(match g.konst {
                Some(_) => match want {
                    Some(t) if t.is_int() => t.clone(),
                    _ => g.ty.clone(),
                },
                None => g.ty.clone(),
            });
        }
        if let Some(sig) = self.tp.funcs.get(&key) {
            if sig.is_extern {
                return None;
            }
            return Some(sig.func_ty(matches!(want, Some(Ty::Func { ext: true, .. }))));
        }
        None
    }

    /// The result type of a call of a function declared by name (`f(x)` or `module.f(x)`).
    fn type_of_call(&self, callee: &Expr) -> Option<Ty> {
        self.resolve_call(callee).map(|(_, ty)| ty)
    }

    /// The destination and the result type of a call of a function declared by name (`f(x)` or
    /// `module.f(x)`) or of a method of a record. Calls of built-ins, of generic functions, of
    /// closures and of other methods are not covered yet.
    fn resolve_call(&self, callee: &Expr) -> Option<(TypedCallee, Ty)> {
        const BUILTINS: &[&str] = &["abort", "bool", "char", "error", "print", "println", "weak", "live_objects", "channel", "timeout", "sleep", "heap_bytes", "float", "f64", "f32"];
        let func = |key: (String, String)| {
            let ret = self.tp.funcs.get(&key)?.ret.clone();
            Some((TypedCallee::Func { module: key.0, name: key.1 }, ret))
        };
        match &callee.kind {
            ExprKind::Ident(name) => {
                if self.lookup(name).is_some() || name.starts_with("__") || BUILTINS.contains(&name.as_str()) || int_type(name).is_some() {
                    return None;
                }
                func((self.cur_mod.clone(), name.clone()))
            }
            ExprKind::Field { expr, name, safe: false } => {
                if let ExprKind::Ident(m) = &expr.kind {
                    if self.is_module(m) {
                        if m == "c" || m == "format" {
                            return None;
                        }
                        return func((self.modkey(&self.cur_mod, m), name.clone()));
                    }
                }
                // a method of a record: found by the type of the receiver
                let Ty::Rec(r) = rc_view(&self.type_of(expr, None)?) else { return None };
                let ret = self.tp.methods.get(&(r.clone(), name.clone()))?.ret.clone();
                Some((TypedCallee::Method { rec_key: r, name: name.clone() }, ret))
            }
            _ => None,
        }
    }
}
