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

    /// The result type of a call of a function declared by name (`f(x)` or `module.f(x)`). Calls of
    /// built-ins, of generic functions, of closures and of methods are not covered yet.
    fn type_of_call(&self, callee: &Expr) -> Option<Ty> {
        const BUILTINS: &[&str] = &["abort", "bool", "char", "error", "print", "println", "weak", "live_objects", "channel", "timeout", "sleep", "heap_bytes", "float", "f64", "f32"];
        let key = match &callee.kind {
            ExprKind::Ident(name) => {
                if self.lookup(name).is_some() || name.starts_with("__") || BUILTINS.contains(&name.as_str()) || int_type(name).is_some() {
                    return None;
                }
                (self.cur_mod.clone(), name.clone())
            }
            ExprKind::Field { expr, name, safe: false } => {
                if let ExprKind::Ident(m) = &expr.kind {
                    if self.is_module(m) {
                        if m == "c" || m == "format" {
                            return None;
                        }
                        let key = (self.modkey(&self.cur_mod, m), name.clone());
                        return self.tp.funcs.get(&key).map(|sig| sig.ret.clone());
                    }
                }
                // a method of a record: found by the type of the receiver
                let Ty::Rec(r) = rc_view(&self.type_of(expr, None)?) else { return None };
                return self.tp.methods.get(&(r, name.clone())).map(|sig| sig.ret.clone());
            }
            _ => return None,
        };
        self.tp.funcs.get(&key).map(|sig| sig.ret.clone())
    }
}
