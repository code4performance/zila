//! A walker over the syntax tree, used by the expansion and the subset check.

use crate::ast::*;
use std::collections::HashSet;

type R<T> = Result<T, String>;

/// Visits the nodes of a tree. `expr` runs before an expression's children and says whether to
/// go into them; `stmt` runs before a statement and may replace it with other statements, which
/// are not visited; `ty` runs on every type.
pub trait Visit {
    fn expr(&mut self, _e: &mut Expr) -> R<bool> {
        Ok(true)
    }
    fn stmt(&mut self, _s: &mut Stmt) -> R<Option<Vec<Stmt>>> {
        Ok(None)
    }
    fn ty(&mut self, _t: &mut Type) {}
}

pub fn walk_block(b: &mut Block, v: &mut dyn Visit) -> R<()> {
    let mut i = 0;
    while i < b.len() {
        if let Some(new) = v.stmt(&mut b[i])? {
            let n = new.len();
            b.splice(i..i + 1, new);
            i += n;
            continue;
        }
        walk_stmt(&mut b[i], v)?;
        i += 1;
    }
    Ok(())
}

fn walk_stmt(s: &mut Stmt, v: &mut dyn Visit) -> R<()> {
    match &mut s.kind {
        StmtKind::Var(vd) => {
            if let Some(t) = &mut vd.ty {
                walk_ty(t, v);
            }
            if let Some(e) = &mut vd.value {
                walk_expr(e, v)?;
            }
        }
        StmtKind::Assign { target, value, .. } => {
            walk_expr(target, v)?;
            walk_expr(value, v)?;
        }
        StmtKind::Return(Some(e)) | StmtKind::Defer(e) | StmtKind::Spawn(e) | StmtKind::Expr(e) => walk_expr(e, v)?,
        StmtKind::Return(None) | StmtKind::Break | StmtKind::Continue | StmtKind::Block { .. } => {}
        StmtKind::Unsafe(b) => walk_block(b, v)?,
        StmtKind::Asm(a) => {
            for op in &mut a.operands {
                walk_expr(&mut op.value, v)?;
            }
        }
        StmtKind::For { iter, body, .. } => {
            walk_expr(iter, v)?;
            walk_block(body, v)?;
        }
        StmtKind::While { cond, body } => {
            walk_expr(cond, v)?;
            walk_block(body, v)?;
        }
    }
    Ok(())
}

pub fn walk_ty(t: &mut Type, v: &mut dyn Visit) {
    v.ty(t);
    match t {
        Type::Named { args, .. } => {
            for a in args {
                walk_ty(a, v);
            }
        }
        Type::Optional(i) | Type::Shared(i) | Type::Pointer(i) | Type::Slice(i) | Type::Array(_, i) => walk_ty(i, v),
        Type::Func { params, ret, .. } => {
            for p in params {
                walk_ty(p, v);
            }
            if let Some(r) = ret {
                if let Some(rt) = &mut r.ty {
                    walk_ty(rt, v);
                }
            }
        }
    }
}

fn walk_params(ps: &mut [Param], v: &mut dyn Visit) -> R<()> {
    for p in ps {
        if let Some(t) = &mut p.ty {
            walk_ty(t, v);
        }
        if let Some(d) = &mut p.default {
            walk_expr(d, v)?;
        }
    }
    Ok(())
}

pub fn walk_expr(e: &mut Expr, v: &mut dyn Visit) -> R<()> {
    if !v.expr(e)? {
        return Ok(());
    }
    match &mut e.kind {
        ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Char(_)
        | ExprKind::Bool(_)
        | ExprKind::Nil
        | ExprKind::Ident(_)
        | ExprKind::Block { .. } => {}
        ExprKind::Text(parts) => {
            for p in parts {
                if let TextPart::Expr(x) = p {
                    walk_expr(x, v)?;
                }
            }
        }
        ExprKind::List(xs) => {
            for x in xs {
                walk_expr(x, v)?;
            }
        }
        ExprKind::Map(kv) => {
            for (k, x) in kv {
                walk_expr(k, v)?;
                walk_expr(x, v)?;
            }
        }
        ExprKind::Record { fields, .. } => {
            for (_, x) in fields {
                walk_expr(x, v)?;
            }
        }
        ExprKind::Lambda { params, body } => {
            walk_params(params, v)?;
            walk_block(body, v)?;
        }
        ExprKind::If { branches, else_body } => {
            for (c, b) in branches {
                walk_expr(c, v)?;
                walk_block(b, v)?;
            }
            if let Some(b) = else_body {
                walk_block(b, v)?;
            }
        }
        ExprKind::Match { scrutinee, arms } => {
            walk_expr(scrutinee, v)?;
            for a in arms {
                for p in &mut a.pattern {
                    if let PatAtom::Literal(x) = p {
                        walk_expr(x, v)?;
                    }
                }
                walk_block(&mut a.body, v)?;
            }
        }
        ExprKind::New(e) | ExprKind::Shared(e) | ExprKind::SpawnThread(e) => walk_expr(e, v)?,
        ExprKind::Unary { expr, .. } | ExprKind::Deref(expr) | ExprKind::Propagate(expr) | ExprKind::Await(expr) => walk_expr(expr, v)?,
        ExprKind::Binary { lhs, rhs, .. } => {
            walk_expr(lhs, v)?;
            walk_expr(rhs, v)?;
        }
        ExprKind::Field { expr, .. } => walk_expr(expr, v)?,
        ExprKind::Index { expr, index } => {
            walk_expr(expr, v)?;
            walk_expr(index, v)?;
        }
        ExprKind::Call { callee, args, targs } => {
            walk_expr(callee, v)?;
            for t in targs {
                walk_ty(t, v);
            }
            for a in args {
                walk_expr(&mut a.value, v)?;
            }
        }
    }
    Ok(())
}

pub fn walk_func(fd: &mut FuncDecl, v: &mut dyn Visit) -> R<()> {
    if let Some(r) = &mut fd.receiver {
        walk_ty(&mut r.ty, v);
    }
    walk_params(&mut fd.params, v)?;
    if let Some(r) = &mut fd.ret {
        if let Some(t) = &mut r.ty {
            walk_ty(t, v);
        }
    }
    walk_block(&mut fd.body, v)
}

pub fn walk_decl(d: &mut Decl, v: &mut dyn Visit) -> R<()> {
    match &mut d.kind {
        DeclKind::Func(fd) => walk_func(fd, v)?,
        DeclKind::Type(td) => match &mut td.body {
            TypeBody::Record(fs) => {
                for f in fs {
                    walk_ty(&mut f.ty, v);
                }
            }
            TypeBody::Enum(vs) => {
                for var in vs {
                    for t in &mut var.fields {
                        walk_ty(t, v);
                    }
                }
            }
            TypeBody::Interface(sigs) => {
                for s in sigs {
                    walk_params(&mut s.params, v)?;
                    if let Some(r) = &mut s.ret {
                        if let Some(t) = &mut r.ty {
                            walk_ty(t, v);
                        }
                    }
                }
            }
            TypeBody::Alias(t) => walk_ty(t, v),
        },
        DeclKind::Var(vd) => {
            if let Some(t) = &mut vd.ty {
                walk_ty(t, v);
            }
            if let Some(e) = &mut vd.value {
                walk_expr(e, v)?;
            }
        }
        DeclKind::Extern(ed) => {
            walk_params(&mut ed.params, v)?;
            if let Some(t) = &mut ed.ret {
                walk_ty(t, v);
            }
        }
        DeclKind::Embed(_) => {}
    }
    Ok(())
}

/// The names a function binds itself: parameters, `let`/`var`, `for` variables, lambda
/// parameters and match bindings.
pub fn bound_names(fd: &FuncDecl) -> HashSet<String> {
    struct B(HashSet<String>);
    impl Visit for B {
        fn expr(&mut self, e: &mut Expr) -> R<bool> {
            match &e.kind {
                ExprKind::Lambda { params, .. } => {
                    for p in params {
                        self.0.insert(p.name.clone());
                    }
                }
                ExprKind::Match { arms, .. } => {
                    for a in arms {
                        for p in &a.pattern {
                            if let PatAtom::Name { bindings: Some(bs), .. } = p {
                                self.0.extend(bs.iter().cloned());
                            }
                        }
                    }
                }
                _ => {}
            }
            Ok(true)
        }
        fn stmt(&mut self, s: &mut Stmt) -> R<Option<Vec<Stmt>>> {
            match &s.kind {
                StmtKind::Var(vd) => {
                    self.0.insert(vd.name.clone());
                }
                StmtKind::For { vars, .. } => self.0.extend(vars.iter().cloned()),
                _ => {}
            }
            Ok(None)
        }
    }
    let mut b = B(HashSet::new());
    for p in &fd.params {
        b.0.insert(p.name.clone());
    }
    let mut body = fd.body.clone();
    let _ = walk_block(&mut body, &mut b);
    b.0
}
