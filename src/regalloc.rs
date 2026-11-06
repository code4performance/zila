//! Chooses which variables of a function live in processor registers.
//!
//! The plan is made from the syntax alone: variables are counted by name, uses inside loops count
//! more, and only names that are bound exactly once in the function (by a `let`/`var` or a plain
//! parameter) are candidates, so two variables never need the same register. The code generator
//! then checks the type of each candidate (integers, booleans and pointers only).

use crate::ast::*;
use std::collections::{HashMap, HashSet};

/// Callee-saved registers the compiler uses for nothing else.
pub const REGS: [&str; 5] = ["rbx", "r12", "r13", "r14", "r15"];

/// A name needs at least this score to be worth a register.
const MIN_SCORE: u64 = 8;

#[derive(Default)]
struct Walk {
    score: HashMap<String, u64>,
    /// Number of `let`/`var`/parameter bindings of each name.
    binds: HashMap<String, u32>,
    /// Names bound some other way (loop variables, patterns, lambda parameters) or whose
    /// address is taken: they stay in memory.
    bad: HashSet<String>,
    /// The function contains something that makes every variable stay in memory.
    blocked: bool,
}

fn weight(depth: u32) -> u64 {
    8u64.pow(depth.min(4))
}

impl Walk {
    fn block(&mut self, b: &[Stmt], depth: u32) {
        for s in b {
            self.stmt(s, depth);
        }
    }

    fn stmt(&mut self, s: &Stmt, depth: u32) {
        match &s.kind {
            StmtKind::Var(vd) => {
                *self.binds.entry(vd.name.clone()).or_default() += 1;
                *self.score.entry(vd.name.clone()).or_default() += weight(depth);
                if let Some(e) = &vd.value {
                    self.expr(e, depth);
                }
            }
            StmtKind::Assign { target, value, .. } => {
                self.expr(target, depth);
                self.expr(value, depth);
            }
            StmtKind::Return(e) => {
                if let Some(e) = e {
                    self.expr(e, depth);
                }
            }
            StmtKind::Defer(_) | StmtKind::Spawn(_) | StmtKind::Asm(_) => self.blocked = true,
            StmtKind::Unsafe(b) => self.block(b, depth),
            StmtKind::For { vars, iter, body } => {
                if matches!(&iter.kind, ExprKind::Binary { op: "..", .. }) && vars.len() == 1 {
                    // a counting loop: its variable is tested and stepped on every turn
                    *self.binds.entry(vars[0].clone()).or_default() += 1;
                    *self.score.entry(vars[0].clone()).or_default() += 2 * weight(depth + 1);
                } else {
                    for v in vars {
                        self.bad.insert(v.clone());
                    }
                }
                self.expr(iter, depth);
                self.block(body, depth + 1);
            }
            StmtKind::While { cond, body } => {
                self.expr(cond, depth + 1);
                self.block(body, depth + 1);
            }
            StmtKind::Break | StmtKind::Continue => {}
            StmtKind::Expr(e) => self.expr(e, depth),
        }
    }

    fn mark_bad(&mut self, e: &Expr) {
        let mut names = Vec::new();
        crate::regalloc::idents(e, &mut names);
        for n in names {
            self.bad.insert(n);
        }
    }

    fn expr(&mut self, e: &Expr, depth: u32) {
        match &e.kind {
            ExprKind::Ident(n) => *self.score.entry(n.clone()).or_default() += weight(depth),
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Char(_) | ExprKind::Bool(_) | ExprKind::Nil | ExprKind::New(_) => {}
            ExprKind::Text(parts) => {
                for p in parts {
                    if let TextPart::Expr(x) = p {
                        self.expr(x, depth);
                    }
                }
            }
            ExprKind::List(xs) => xs.iter().for_each(|x| self.expr(x, depth)),
            ExprKind::Map(kv) => kv.iter().for_each(|(k, v)| {
                self.expr(k, depth);
                self.expr(v, depth);
            }),
            ExprKind::Record { fields, .. } => fields.iter().for_each(|(_, v)| self.expr(v, depth)),
            // A lambda copies the variables it uses when it is created, and its own names are
            // its own: its body is not part of this function.
            ExprKind::Lambda { .. } => {}
            ExprKind::If { branches, else_body } => {
                for (c, b) in branches {
                    self.expr(c, depth);
                    self.block(b, depth);
                }
                if let Some(b) = else_body {
                    self.block(b, depth);
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                self.expr(scrutinee, depth);
                for a in arms {
                    for p in &a.pattern {
                        match p {
                            PatAtom::Wildcard => {}
                            PatAtom::Literal(x) => self.expr(x, depth),
                            PatAtom::Name { name, bindings } => {
                                self.bad.insert(name.clone());
                                for b in bindings.iter().flatten() {
                                    self.bad.insert(b.clone());
                                }
                            }
                        }
                    }
                    self.block(&a.body, depth);
                }
            }
            ExprKind::Unary { op, expr } => {
                if *op == "&" {
                    self.mark_bad(expr);
                }
                self.expr(expr, depth);
            }
            ExprKind::Deref(x) | ExprKind::Propagate(x) => self.expr(x, depth),
            ExprKind::Await(_) => self.blocked = true,
            ExprKind::Field { expr, .. } => self.expr(expr, depth),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.expr(lhs, depth);
                self.expr(rhs, depth);
            }
            ExprKind::Index { expr, index } => {
                self.expr(expr, depth);
                self.expr(index, depth);
            }
            ExprKind::Call { callee, args } => {
                self.expr(callee, depth);
                for a in args {
                    if a.by_ref || a.spread {
                        self.mark_bad(&a.value);
                    }
                    self.expr(&a.value, depth);
                }
            }
        }
    }
}

/// Every identifier mentioned in `e` (lambda bodies included).
pub fn idents(e: &Expr, out: &mut Vec<String>) {
    let mut w = Walk::default();
    w.expr(e, 0);
    out.extend(w.score.into_keys());
    out.extend(w.bad.into_iter());
}

/// The registers for the variables of `fd`, by name.
pub fn plan(fd: &FuncDecl) -> HashMap<String, &'static str> {
    let mut w = Walk::default();
    if fd.is_async || fd.receiver.is_some() {
        return HashMap::new();
    }
    for p in &fd.params {
        if p.by_ref || p.variadic || p.default.is_some() {
            w.bad.insert(p.name.clone());
        }
        *w.binds.entry(p.name.clone()).or_default() += 1;
        *w.score.entry(p.name.clone()).or_default() += 1;
    }
    w.block(&fd.body, 0);
    if w.blocked {
        return HashMap::new();
    }
    let mut cands: Vec<(&String, u64)> = w
        .score
        .iter()
        .filter(|(n, s)| **s >= MIN_SCORE && w.binds.get(*n) == Some(&1) && !w.bad.contains(*n))
        .map(|(n, s)| (n, *s))
        .collect();
    cands.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    cands.into_iter().zip(REGS.iter()).map(|((n, _), r)| (n.clone(), *r)).collect()
}
