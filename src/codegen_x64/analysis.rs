//! Part of the code generator (`codegen_x64`): see the section comment below.

use super::*;

// ---- defer: does a body contain a `defer` of its own (not inside a nested lambda)? ----

pub(super) fn block_has_defer(b: &[Stmt]) -> bool {
    b.iter().any(stmt_has_defer)
}

pub(super) fn stmt_has_defer(s: &Stmt) -> bool {
    match &s.kind {
        StmtKind::Defer(_) => true,
        StmtKind::Unsafe(b) => block_has_defer(b),
        StmtKind::For { body, .. } | StmtKind::While { body, .. } => block_has_defer(body),
        StmtKind::Expr(e) => expr_has_defer(e),
        StmtKind::Var(vd) => vd.value.as_ref().map_or(false, expr_has_defer),
        _ => false,
    }
}

pub(super) fn expr_has_defer(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::If { branches, else_body } => {
            branches.iter().any(|(_, b)| block_has_defer(b)) || else_body.as_ref().map_or(false, |b| block_has_defer(b))
        }
        ExprKind::Match { arms, .. } => arms.iter().any(|a| block_has_defer(&a.body)),
        _ => false,
    }
}

// ---- free-variable collection for closures ----

/// True when the program mentions a float literal or a float type (the printing
/// library is only linked then). Searching the debug text over-approximates, which is harmless.
pub(super) fn file_uses_floats(f: &File) -> bool {
    let d = format!("{:?}", f.decls);
    d.contains("Float(") || d.contains("\"float\"") || d.contains("\"f32\"") || d.contains("\"f64\"")
}

pub(super) fn fd_has_await(fd: &FuncDecl) -> bool {
    let mut names = Vec::new();
    collect_block(&fd.body, &mut names);
    names.iter().any(|n| n == "$await")
}

pub(super) fn file_uses_concurrency(f: &File) -> bool {
    let mut names = Vec::new();
    for d in &f.decls {
        if let DeclKind::Func(fd) = &d.kind {
            if fd.is_async {
                names.push("$await".to_string());
            }
            collect_block(&fd.body, &mut names);
        }
    }
    names.iter().any(|n| n == "$spawn" || n == "$await" || n == "channel" || n == "sleep" || n == "timeout")
}

/// Whether a file uses `shared` or `spawn_thread` (the runtime then counts references atomically).
pub(super) fn file_uses_threads(f: &File) -> (bool, bool) {
    let mut names = Vec::new();
    for d in &f.decls {
        if let DeclKind::Func(fd) = &d.kind {
            collect_block(&fd.body, &mut names);
        }
    }
    let spawn = names.iter().any(|n| n == "$spawn_thread");
    (spawn || names.iter().any(|n| n == "$shared"), spawn)
}

pub(super) fn collect_block(b: &[Stmt], out: &mut Vec<String>) {
    for s in b {
        match &s.kind {
            StmtKind::Var(vd) => {
                if let Some(e) = &vd.value {
                    collect_expr(e, out);
                }
            }
            StmtKind::Assign { target, value, .. } => {
                collect_expr(target, out);
                collect_expr(value, out);
            }
            StmtKind::Spawn(e) => {
                out.push("$spawn".into());
                collect_expr(e, out);
            }
            StmtKind::Return(Some(e)) | StmtKind::Defer(e) | StmtKind::Expr(e) => collect_expr(e, out),
            StmtKind::Unsafe(b) => collect_block(b, out),
            StmtKind::For { iter, body, .. } => {
                collect_expr(iter, out);
                collect_block(body, out);
            }
            StmtKind::While { cond, body } => {
                collect_expr(cond, out);
                collect_block(body, out);
            }
            StmtKind::Asm(a) => {
                for o in &a.operands {
                    collect_expr(&o.value, out);
                }
            }
            _ => {}
        }
    }
}

pub(super) fn collect_expr(e: &Expr, out: &mut Vec<String>) {
    match &e.kind {
        ExprKind::Ident(n) => out.push(n.clone()),
        ExprKind::Text(parts) => {
            for p in parts {
                if let TextPart::Expr(x) = p {
                    collect_expr(x, out);
                }
            }
        }
        ExprKind::List(xs) => xs.iter().for_each(|x| collect_expr(x, out)),
        ExprKind::Map(kv) => kv.iter().for_each(|(k, v)| {
            collect_expr(k, out);
            collect_expr(v, out);
        }),
        ExprKind::Record { fields, .. } => fields.iter().for_each(|(_, v)| collect_expr(v, out)),
        ExprKind::Lambda { body, .. } => collect_block(body, out),
        ExprKind::If { branches, else_body } => {
            for (c, b) in branches {
                collect_expr(c, out);
                collect_block(b, out);
            }
            if let Some(b) = else_body {
                collect_block(b, out);
            }
        }
        ExprKind::Match { scrutinee, arms } => {
            collect_expr(scrutinee, out);
            for a in arms {
                collect_block(&a.body, out);
            }
        }
        ExprKind::Shared(expr) => {
            out.push("$shared".into());
            collect_expr(expr, out)
        }
        ExprKind::SpawnThread(expr) => {
            out.push("$spawn_thread".into());
            collect_expr(expr, out)
        }
        ExprKind::New(expr) => collect_expr(expr, out),
        ExprKind::Await(expr) => {
            out.push("$await".into());
            collect_expr(expr, out)
        }
        ExprKind::Unary { expr, .. } | ExprKind::Deref(expr) | ExprKind::Propagate(expr) => collect_expr(expr, out),
        ExprKind::Field { expr, .. } => collect_expr(expr, out),
        ExprKind::Binary { lhs, rhs, .. } => {
            collect_expr(lhs, out);
            collect_expr(rhs, out);
        }
        ExprKind::Index { expr, index } => {
            collect_expr(expr, out);
            collect_expr(index, out);
        }
        ExprKind::Call { callee, args, .. } => {
            collect_expr(callee, out);
            args.iter().for_each(|a| collect_expr(&a.value, out));
        }
        _ => {}
    }
}

