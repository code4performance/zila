//! The `ref` escape rule (spec `ref-parameters`): a `ref` parameter, receiver or lambda
//! parameter only borrows the caller's value for the duration of the call, so outside
//! `unsafe` it may be used as the base of a field, index or method call, iterated, and passed
//! on as `ref r`. Every other bare mention could outlive the call and is rejected.
//! Scalar ref bindings may also be read as a value (a copy).

use crate::ast::*;

/// Checks `body` for escapes of the ref bindings `names` (name, is scalar).
/// Returns the offending name and its line.
pub fn check_ref_escapes(names: &[(String, bool)], body: &[Stmt]) -> Result<(), (String, u32)> {
    if names.is_empty() {
        return Ok(());
    }
    let mut c = Checker { env: names.iter().map(|(n, s)| (n.clone(), Some(*s))).collect(), strict: 0 };
    c.block(body)
}

type R = Result<(), (String, u32)>;

struct Checker {
    /// Innermost last: `Some(scalar)` is a live ref binding, `None` a shadowing name.
    env: Vec<(String, Option<bool>)>,
    /// Inside a lambda or `spawn` operand: any mention is an error.
    strict: u32,
}

impl Checker {
    fn lookup(&self, n: &str) -> Option<bool> {
        self.env.iter().rev().find(|(x, _)| x == n).and_then(|(_, s)| *s)
    }

    fn shadow(&mut self, n: &str) {
        self.env.push((n.to_string(), None));
    }

    fn block(&mut self, b: &[Stmt]) -> R {
        let mark = self.env.len();
        let r = b.iter().try_for_each(|s| self.stmt(s));
        self.env.truncate(mark);
        r
    }

    fn stmt(&mut self, s: &Stmt) -> R {
        match &s.kind {
            StmtKind::Var(vd) => {
                if let Some(v) = &vd.value {
                    self.val(v)?;
                }
                self.shadow(&vd.name);
                Ok(())
            }
            StmtKind::Assign { target, value, .. } => {
                self.target(target)?;
                self.val(value)
            }
            StmtKind::Return(Some(e)) | StmtKind::Defer(e) | StmtKind::Expr(e) => self.val(e),
            StmtKind::Spawn(e) => {
                self.strict += 1;
                let r = self.val(e);
                self.strict -= 1;
                r
            }
            StmtKind::For { vars, iter, body, .. } => {
                self.base(iter)?;
                let mark = self.env.len();
                for v in vars {
                    self.shadow(v);
                }
                let r = self.block(body);
                self.env.truncate(mark);
                r
            }
            StmtKind::While { cond, body } => {
                self.val(cond)?;
                self.block(body)
            }
            // the rule is off inside `unsafe` (and inline asm needs it)
            StmtKind::Unsafe(_) | StmtKind::Asm(_) => Ok(()),
            StmtKind::Return(None) | StmtKind::Break | StmtKind::Continue | StmtKind::Block { .. } => Ok(()),
        }
    }

    /// The target of an assignment: `r.x = v`, `r[i] = v`, and `v += 1` for a scalar.
    fn target(&mut self, e: &Expr) -> R {
        match &e.kind {
            ExprKind::Ident(n) if self.strict == 0 && self.lookup(n).is_some() => Ok(()),
            _ => self.base(e),
        }
    }

    /// A borrowing position: the base of a field, index or method call, or a repeated `ref` argument.
    fn base(&mut self, e: &Expr) -> R {
        match &e.kind {
            ExprKind::Ident(n) => {
                if self.strict > 0 && self.lookup(n).is_some() {
                    return Err((n.clone(), e.line));
                }
                Ok(())
            }
            ExprKind::Field { expr, .. } => self.base(expr),
            ExprKind::Index { expr, index } => {
                self.base(expr)?;
                self.val(index)
            }
            _ => self.val(e),
        }
    }

    fn val(&mut self, e: &Expr) -> R {
        match &e.kind {
            ExprKind::Ident(n) => match self.lookup(n) {
                Some(scalar) if self.strict > 0 || !scalar => Err((n.clone(), e.line)),
                _ => Ok(()),
            },
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Char(_) | ExprKind::Bool(_) | ExprKind::Nil => Ok(()),
            ExprKind::New(e) | ExprKind::Shared(e) | ExprKind::SpawnThread(e) => self.val(e),
            ExprKind::Text(parts) => {
                for p in parts {
                    if let TextPart::Expr(x) = p {
                        self.val(x)?;
                    }
                }
                Ok(())
            }
            ExprKind::List(xs) => xs.iter().try_for_each(|x| self.val(x)),
            ExprKind::Map(kv) => kv.iter().try_for_each(|(k, v)| {
                self.val(k)?;
                self.val(v)
            }),
            ExprKind::Record { fields, .. } => fields.iter().try_for_each(|(_, v)| self.val(v)),
            ExprKind::Lambda { params, body } => {
                self.strict += 1;
                let mark = self.env.len();
                for p in params {
                    self.shadow(&p.name);
                }
                let r = self.block(body);
                self.env.truncate(mark);
                self.strict -= 1;
                r
            }
            ExprKind::If { branches, else_body } => {
                for (c, b) in branches {
                    self.val(c)?;
                    self.block(b)?;
                }
                match else_body {
                    Some(b) => self.block(b),
                    None => Ok(()),
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                self.val(scrutinee)?;
                for arm in arms {
                    let mark = self.env.len();
                    for p in &arm.pattern {
                        match p {
                            PatAtom::Name { bindings: Some(bs), .. } => bs.iter().for_each(|b| self.shadow(b)),
                            PatAtom::Literal(l) => self.val(l)?,
                            _ => {}
                        }
                    }
                    let r = self.block(&arm.body);
                    self.env.truncate(mark);
                    r?;
                }
                Ok(())
            }
            ExprKind::Unary { op, expr } => {
                if *op == "&" {
                    if let ExprKind::Ident(n) = &expr.kind {
                        if self.lookup(n).is_some() {
                            return Err((n.clone(), expr.line));
                        }
                    }
                }
                self.val(expr)
            }
            ExprKind::Binary { lhs, rhs, .. } => {
                self.val(lhs)?;
                self.val(rhs)
            }
            ExprKind::Field { expr, .. } => self.base(expr),
            ExprKind::Index { expr, index } => {
                self.base(expr)?;
                self.val(index)
            }
            ExprKind::Call { callee, args, .. } => {
                match &callee.kind {
                    ExprKind::Field { expr, .. } => self.base(expr)?,
                    ExprKind::Ident(_) if self.strict == 0 => {}
                    _ => self.val(callee)?,
                }
                for a in args {
                    if a.by_ref {
                        self.base(&a.value)?;
                    } else {
                        self.val(&a.value)?;
                    }
                }
                Ok(())
            }
            ExprKind::Deref(x) | ExprKind::Propagate(x) | ExprKind::Await(x) => self.val(x),
            ExprKind::Block { .. } | ExprKind::Sql { .. } => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_source;

    /// Parses `body` as the body of `func f(...)` and checks `r` (record ref) and `v` (scalar ref).
    fn check(body: &str) -> Result<(), (String, u32)> {
        let src = format!("package main\ntype R = record\n    x: int\nend\nfunc f()\n{}\nend\n", body);
        let file = parse_source(&src).unwrap_or_else(|d| panic!("{}: {}", body, d.msg));
        for d in &file.decls {
            if let DeclKind::Func(fd) = &d.kind {
                return check_ref_escapes(&[("r".into(), false), ("v".into(), true)], &fd.body);
            }
        }
        unreachable!()
    }

    #[test]
    fn escapes_are_rejected() {
        for b in [
            "return r",
            "h.last = r",
            "xs.add(r)",
            "m[k] = r",
            "let a = [r]",
            "let a = Holder{r: r}",
            "let c = r",
            "var c = r",
            "c = r",
            "let g = func()\n g.f(r.x)\nend\n let h = func()\n r.x = 1\nend",
            "spawn work(r)",
            "spawn work(ref r)",
            "let p = &r",
            "f(r)",
            "let g = func()\n return v\nend",
        ] {
            assert!(check(b).is_err(), "{}", b);
        }
    }

    #[test]
    fn borrows_are_accepted() {
        for b in [
            "r.x = 1\nlet n = r.x\nr.items.add(1)\nfor a in r.items\n g(a)\nend",
            "r[0] = 1",
            "g(ref r)\nr.m()",
            "v += 10\nreturn v",
            "let r = 1\nlet c = r",
            "let g = func(r: int)\n return r\nend",
            "unsafe\n let p = &r\n return r\nend",
            "return r.x",
        ] {
            assert!(check(b).is_ok(), "{}: {:?}", b, check(b));
        }
    }
}
