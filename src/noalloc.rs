//! Static analysis for `#[no_alloc]` and `#[allocator]` guarantees.

use crate::ast::*;
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Clone)]
pub struct AllocOp {
    pub description: String,
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Clone)]
pub struct FuncNode {
    pub qname: String,
    pub name: String,
    pub file: String,
    pub line: u32,
    pub is_no_alloc: bool,
    pub is_extern: bool,
    pub direct_allocs: Vec<AllocOp>,
    pub callees: Vec<(String, u32)>, // (qname of callee, call site line)
}

#[derive(Debug, Clone)]
pub struct CustomAllocatorInfo {
    pub alloc_sym: String,
    pub free_sym: String,
    pub realloc_sym: Option<String>,
}

/// Validates attributes, allocator signatures, and computes no-alloc correctness.
pub fn check_program(
    modules: &[(String, File)],
    interfaces: &HashSet<String>,
) -> Result<Option<CustomAllocatorInfo>, String> {
    // 1. Validate attribute targets and allocator roles
    let mut alloc_func: Option<(String, u32, String)> = None; // (file, line, func_name)
    let mut free_func: Option<(String, u32, String)> = None;
    let mut realloc_func: Option<(String, u32, String)> = None;

    let mut func_nodes: HashMap<String, FuncNode> = HashMap::new();
    let mut extern_allocates: HashSet<String> = HashSet::new();

    // Collect all declared functions and methods
    for (mname, file) in modules {
        for d in &file.decls {
            let line = d.line;
            match &d.kind {
                DeclKind::Type(td) => {
                    for a in &d.attrs {
                        if a.name == "no_alloc" {
                            return Err(format!("{}.br:{}: #[no_alloc] only applies to functions ('{}')", mname, line, td.name));
                        }
                        if a.name == "allocates" {
                            return Err(format!("{}.br:{}: #[allocates] only applies to functions ('{}')", mname, line, td.name));
                        }
                        if a.name == "allocator" {
                            return Err(format!("{}.br:{}: #[allocator] only applies to functions ('{}')", mname, line, td.name));
                        }
                    }
                }
                DeclKind::Var(vd) => {
                    for a in &d.attrs {
                        if a.name == "no_alloc" {
                            return Err(format!("{}.br:{}: #[no_alloc] only applies to functions ('{}')", mname, line, vd.name));
                        }
                        if a.name == "allocates" {
                            return Err(format!("{}.br:{}: #[allocates] only applies to functions ('{}')", mname, line, vd.name));
                        }
                        if a.name == "allocator" {
                            return Err(format!("{}.br:{}: #[allocator] only applies to functions ('{}')", mname, line, vd.name));
                        }
                    }
                }
                DeclKind::Extern(ed) => {
                    let has_allocates = d.attrs.iter().any(|a| a.name == "allocates");
                    let qname = format!("{}.{}", mname, ed.name);
                    if has_allocates {
                        extern_allocates.insert(qname.clone());
                    }
                    func_nodes.insert(
                        qname.clone(),
                        FuncNode {
                            qname,
                            name: ed.name.clone(),
                            file: mname.clone(),
                            line,
                            is_no_alloc: false,
                            is_extern: true,
                            direct_allocs: Vec::new(),
                            callees: Vec::new(),
                        },
                    );
                }
                DeclKind::Func(fd) => {
                    let mut is_no_alloc = d.attrs.iter().any(|a| a.name == "no_alloc");
                    let has_allocates = d.attrs.iter().any(|a| a.name == "allocates");
                    if has_allocates {
                        extern_allocates.insert(format!("{}.{}", mname, fd.name));
                    }

                    // Check #[allocator]
                    for a in &d.attrs {
                        if a.name == "allocator" {
                            if mname != "main" && mname.starts_with("std") {
                                return Err(format!("{}.br:{}: #[allocator] is only allowed on top-level functions of the program, not in the standard library", mname, line));
                            }
                            if fd.receiver.is_some() {
                                return Err(format!("{}.br:{}: #[allocator] only applies to top-level functions", mname, line));
                            }
                            if a.args.len() != 1 {
                                return Err(format!("{}.br:{}: #[allocator] requires one argument: 'alloc', 'free' or 'realloc'", mname, line));
                            }
                            let role_str = match a.args.first() {
                                Some(Expr { kind: ExprKind::Ident(id), .. }) => id.as_str(),
                                Some(Expr { kind: ExprKind::Text(parts), .. }) if parts.len() == 1 => {
                                    if let TextPart::Lit(s) = &parts[0] {
                                        s.as_str()
                                    } else {
                                        ""
                                    }
                                }
                                _ => "",
                            };

                            match role_str {
                                "alloc" => {
                                    if let Some((_, prev_line, prev_fn)) = &alloc_func {
                                        return Err(format!("{}.br:{}: #[allocator(alloc)] already declared at line {} ('{}')", mname, line, prev_line, prev_fn));
                                    }
                                    validate_alloc_sig(mname, line, fd)?;
                                    alloc_func = Some((mname.clone(), line, fd.name.clone()));
                                }
                                "free" => {
                                    if let Some((_, prev_line, prev_fn)) = &free_func {
                                        return Err(format!("{}.br:{}: #[allocator(free)] already declared at line {} ('{}')", mname, line, prev_line, prev_fn));
                                    }
                                    validate_free_sig(mname, line, fd)?;
                                    free_func = Some((mname.clone(), line, fd.name.clone()));
                                }
                                "realloc" => {
                                    if let Some((_, prev_line, prev_fn)) = &realloc_func {
                                        return Err(format!("{}.br:{}: #[allocator(realloc)] already declared at line {} ('{}')", mname, line, prev_line, prev_fn));
                                    }
                                    validate_realloc_sig(mname, line, fd)?;
                                    realloc_func = Some((mname.clone(), line, fd.name.clone()));
                                }
                                other => {
                                    return Err(format!("{}.br:{}: unknown allocator role '{}' (expected 'alloc', 'free' or 'realloc')", mname, line, other));
                                }
                            }
                            // Allocator functions are implicitly #[no_alloc]
                            is_no_alloc = true;
                        }
                    }

                    let qname = if let Some(rcv) = &fd.receiver {
                        format!("{}.{}.{}", mname, type_to_string(&rcv.ty), fd.name)
                    } else {
                        format!("{}.{}", mname, fd.name)
                    };

                    func_nodes.insert(
                        qname.clone(),
                        FuncNode {
                            qname,
                            name: fd.name.clone(),
                            file: mname.clone(),
                            line,
                            is_no_alloc,
                            is_extern: false,
                            direct_allocs: Vec::new(),
                            callees: Vec::new(),
                        },
                    );
                }
                _ => {}
            }
        }
    }

    // Verify allocator completeness: if any is declared, both alloc and free are required
    let custom_allocator = match (&alloc_func, &free_func, &realloc_func) {
        (Some(a), Some(f), r) => Some(CustomAllocatorInfo {
            alloc_sym: a.2.clone(),
            free_sym: f.2.clone(),
            realloc_sym: r.as_ref().map(|x| x.2.clone()),
        }),
        (Some(a), None, _) => {
            return Err(format!("{}.br:{}: custom allocator declares 'alloc' but is missing 'free'", a.0, a.1));
        }
        (None, Some(f), _) => {
            return Err(format!("{}.br:{}: custom allocator declares 'free' but is missing 'alloc'", f.0, f.1));
        }
        (None, None, Some(r)) => {
            return Err(format!("{}.br:{}: custom allocator declares 'realloc' but requires both 'alloc' and 'free'", r.0, r.1));
        }
        (None, None, None) => None,
    };

    // 2. Collect operations and callees for every function body
    for (mname, file) in modules {
        for d in &file.decls {
            if let DeclKind::Func(fd) = &d.kind {
                let qname = if let Some(rcv) = &fd.receiver {
                    format!("{}.{}.{}", mname, type_to_string(&rcv.ty), fd.name)
                } else {
                    format!("{}.{}", mname, fd.name)
                };

                let mut direct_allocs = Vec::new();
                let mut callees = Vec::new();

                let mut local_vars = HashSet::new();
                let mut fn_vals = HashSet::new();
                let mut iface_vars = HashMap::new();

                if let Some(rcv) = &fd.receiver {
                    local_vars.insert(rcv.name.clone());
                    if let Type::Named { path, .. } = &rcv.ty {
                        if let Some(last) = path.last() {
                            if interfaces.contains(last) || interfaces.contains(&format!("{}.{}", mname, last)) {
                                iface_vars.insert(rcv.name.clone(), last.clone());
                            }
                        }
                    }
                }
                for p in &fd.params {
                    local_vars.insert(p.name.clone());
                    if let Some(ty) = &p.ty {
                        match ty {
                            Type::Func { .. } => {
                                fn_vals.insert(p.name.clone());
                            }
                            Type::Named { path, .. } => {
                                if let Some(last) = path.last() {
                                    if interfaces.contains(last) || interfaces.contains(&format!("{}.{}", mname, last)) {
                                        iface_vars.insert(p.name.clone(), last.clone());
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }

                collect_body_allocs(
                    &fd.body,
                    mname,
                    &mut local_vars,
                    &mut fn_vals,
                    &mut iface_vars,
                    &mut direct_allocs,
                    &mut callees,
                    interfaces,
                    &extern_allocates,
                );

                if let Some(node) = func_nodes.get_mut(&qname) {
                    node.direct_allocs = direct_allocs;
                    node.callees = callees;
                }
            }
        }
    }

    // 3. Transitive analysis: compute for each #[no_alloc] function if it allocates
    // Run BFS from each #[no_alloc] function to find the shortest path to a direct allocating op
    for (qname, node) in &func_nodes {
        if node.is_no_alloc {
            if let Some(chain) = find_alloc_chain(qname, &func_nodes) {
                return Err(format!(
                    "{}.br:{}: error: '{}' is #[no_alloc] but allocates\n  {}",
                    node.file, node.line, node.name, chain
                ));
            }
        }
    }

    Ok(custom_allocator)
}

fn type_to_string(ty: &Type) -> String {
    match ty {
        Type::Named { path, args } => {
            let base = path.join(".");
            if args.is_empty() {
                base
            } else {
                format!("{}<{}>", base, args.iter().map(type_to_string).collect::<Vec<_>>().join(", "))
            }
        }
        Type::Optional(inner) => format!("{}?", type_to_string(inner)),
        Type::Pointer(inner) => format!("*{}", type_to_string(inner)),
        Type::Slice(inner) => format!("[]{}", type_to_string(inner)),
        Type::Array(sz, inner) => format!("[{}]{}", sz, type_to_string(inner)),
        Type::Func { params, ret, .. } => {
            let p = params.iter().map(type_to_string).collect::<Vec<_>>().join(", ");
            if let Some(r) = ret.as_ref() {
                if let Some(t) = &r.ty {
                    format!("func({}) -> {}{}", p, type_to_string(t), if r.fallible { " or error" } else { "" })
                } else if r.fallible {
                    format!("func({}) -> error", p)
                } else {
                    format!("func({})", p)
                }
            } else {
                format!("func({})", p)
            }
        }
    }
}

fn validate_alloc_sig(mname: &str, line: u32, fd: &FuncDecl) -> Result<(), String> {
    let wrong = || {
        format!(
            "{}.br:{}: wrong signature for #[allocator(alloc)] (expected `func {}(size: u64, align: u64) -> *u8`)",
            mname, line, fd.name
        )
    };
    if fd.params.len() != 2 || fd.ret.is_none() {
        return Err(wrong());
    }
    let p0 = fd.params[0].ty.as_ref().map(type_to_string);
    let p1 = fd.params[1].ty.as_ref().map(type_to_string);
    let ret = fd.ret.as_ref().and_then(|r| r.ty.as_ref()).map(type_to_string);

    if !matches!(p0.as_deref(), Some("u64" | "usize")) || !matches!(p1.as_deref(), Some("u64" | "usize")) {
        return Err(wrong());
    }
    if !matches!(ret.as_deref(), Some("*u8" | "*byte" | "*c.void" | "*void")) {
        return Err(wrong());
    }
    Ok(())
}

fn validate_free_sig(mname: &str, line: u32, fd: &FuncDecl) -> Result<(), String> {
    let wrong = || {
        format!(
            "{}.br:{}: wrong signature for #[allocator(free)] (expected `func {}(ptr: *u8, size: u64, align: u64)`)",
            mname, line, fd.name
        )
    };
    if fd.params.len() != 3 || fd.ret.is_some() {
        return Err(wrong());
    }
    let p0 = fd.params[0].ty.as_ref().map(type_to_string);
    let p1 = fd.params[1].ty.as_ref().map(type_to_string);
    let p2 = fd.params[2].ty.as_ref().map(type_to_string);

    if !matches!(p0.as_deref(), Some("*u8" | "*byte" | "*c.void" | "*void")) {
        return Err(wrong());
    }
    if !matches!(p1.as_deref(), Some("u64" | "usize")) || !matches!(p2.as_deref(), Some("u64" | "usize")) {
        return Err(wrong());
    }
    Ok(())
}

fn validate_realloc_sig(mname: &str, line: u32, fd: &FuncDecl) -> Result<(), String> {
    let wrong = || {
        format!(
            "{}.br:{}: wrong signature for #[allocator(realloc)] (expected `func {}(ptr: *u8, old: u64, size: u64, align: u64) -> *u8`)",
            mname, line, fd.name
        )
    };
    if fd.params.len() != 4 || fd.ret.is_none() {
        return Err(wrong());
    }
    let p0 = fd.params[0].ty.as_ref().map(type_to_string);
    let p1 = fd.params[1].ty.as_ref().map(type_to_string);
    let p2 = fd.params[2].ty.as_ref().map(type_to_string);
    let p3 = fd.params[3].ty.as_ref().map(type_to_string);
    let ret = fd.ret.as_ref().and_then(|r| r.ty.as_ref()).map(type_to_string);

    if !matches!(p0.as_deref(), Some("*u8" | "*byte" | "*c.void" | "*void")) {
        return Err(wrong());
    }
    if !matches!(p1.as_deref(), Some("u64" | "usize")) || !matches!(p2.as_deref(), Some("u64" | "usize")) || !matches!(p3.as_deref(), Some("u64" | "usize")) {
        return Err(wrong());
    }
    if !matches!(ret.as_deref(), Some("*u8" | "*byte" | "*c.void" | "*void")) {
        return Err(wrong());
    }
    Ok(())
}

fn collect_body_allocs(
    block: &[Stmt],
    cur_mod: &str,
    locals: &mut HashSet<String>,
    fn_vals: &mut HashSet<String>,
    iface_vars: &mut HashMap<String, String>,
    direct_allocs: &mut Vec<AllocOp>,
    callees: &mut Vec<(String, u32)>,
    interfaces: &HashSet<String>,
    extern_allocates: &HashSet<String>,
) {
    for s in block {
        collect_stmt_allocs(s, cur_mod, locals, fn_vals, iface_vars, direct_allocs, callees, interfaces, extern_allocates);
    }
}

fn collect_stmt_allocs(
    stmt: &Stmt,
    cur_mod: &str,
    locals: &mut HashSet<String>,
    fn_vals: &mut HashSet<String>,
    iface_vars: &mut HashMap<String, String>,
    direct_allocs: &mut Vec<AllocOp>,
    callees: &mut Vec<(String, u32)>,
    interfaces: &HashSet<String>,
    extern_allocates: &HashSet<String>,
) {
    let line = stmt.line;
    match &stmt.kind {
        StmtKind::Var(vd) => {
            locals.insert(vd.name.clone());
            if let Some(ty) = &vd.ty {
                match ty {
                    Type::Func { .. } => {
                        fn_vals.insert(vd.name.clone());
                    }
                    Type::Named { path, .. } => {
                        if let Some(last) = path.last() {
                            if interfaces.contains(last) || interfaces.contains(&format!("{}.{}", cur_mod, last)) {
                                iface_vars.insert(vd.name.clone(), last.clone());
                            }
                        }
                    }
                    _ => {}
                }
            }
            if let Some(val) = &vd.value {
                if matches!(val.kind, ExprKind::Lambda { .. }) {
                    fn_vals.insert(vd.name.clone());
                }
                collect_expr_allocs(val, cur_mod, locals, fn_vals, iface_vars, direct_allocs, callees, interfaces, extern_allocates);
            }
        }
        StmtKind::Expr(e) => {
            collect_expr_allocs(e, cur_mod, locals, fn_vals, iface_vars, direct_allocs, callees, interfaces, extern_allocates);
        }
        StmtKind::Assign { target, value, .. } => {
            collect_expr_allocs(target, cur_mod, locals, fn_vals, iface_vars, direct_allocs, callees, interfaces, extern_allocates);
            collect_expr_allocs(value, cur_mod, locals, fn_vals, iface_vars, direct_allocs, callees, interfaces, extern_allocates);
        }
        StmtKind::Return(Some(e)) => {
            collect_expr_allocs(e, cur_mod, locals, fn_vals, iface_vars, direct_allocs, callees, interfaces, extern_allocates);
        }
        StmtKind::Spawn(e) => {
            direct_allocs.push(AllocOp {
                description: "spawn".to_string(),
                file: cur_mod.to_string(),
                line,
            });
            collect_expr_allocs(e, cur_mod, locals, fn_vals, iface_vars, direct_allocs, callees, interfaces, extern_allocates);
        }
        StmtKind::Defer(e) => {
            direct_allocs.push(AllocOp {
                description: "defer".to_string(),
                file: cur_mod.to_string(),
                line,
            });
            collect_expr_allocs(e, cur_mod, locals, fn_vals, iface_vars, direct_allocs, callees, interfaces, extern_allocates);
        }
        StmtKind::For { vars, iter, body } => {
            for v in vars {
                locals.insert(v.clone());
            }
            collect_expr_allocs(iter, cur_mod, locals, fn_vals, iface_vars, direct_allocs, callees, interfaces, extern_allocates);
            collect_body_allocs(body, cur_mod, locals, fn_vals, iface_vars, direct_allocs, callees, interfaces, extern_allocates);
        }
        StmtKind::While { cond, body } => {
            collect_expr_allocs(cond, cur_mod, locals, fn_vals, iface_vars, direct_allocs, callees, interfaces, extern_allocates);
            collect_body_allocs(body, cur_mod, locals, fn_vals, iface_vars, direct_allocs, callees, interfaces, extern_allocates);
        }
        StmtKind::Unsafe(body) => {
            collect_body_allocs(body, cur_mod, locals, fn_vals, iface_vars, direct_allocs, callees, interfaces, extern_allocates);
        }
        StmtKind::Asm(_) => {
            // Asm blocks are trusted / skipped
        }
        _ => {}
    }
}

fn collect_expr_allocs(
    expr: &Expr,
    cur_mod: &str,
    locals: &mut HashSet<String>,
    fn_vals: &mut HashSet<String>,
    iface_vars: &mut HashMap<String, String>,
    direct_allocs: &mut Vec<AllocOp>,
    callees: &mut Vec<(String, u32)>,
    interfaces: &HashSet<String>,
    extern_allocates: &HashSet<String>,
) {
    let line = expr.line;
    match &expr.kind {
        ExprKind::Text(parts) => {
            if parts.iter().any(|p| matches!(p, TextPart::Expr(_))) {
                direct_allocs.push(AllocOp {
                    description: "text interpolation".to_string(),
                    file: cur_mod.to_string(),
                    line,
                });
            }
            for p in parts {
                if let TextPart::Expr(e) = p {
                    collect_expr_allocs(e, cur_mod, locals, fn_vals, iface_vars, direct_allocs, callees, interfaces, extern_allocates);
                }
            }
        }
        ExprKind::List(items) => {
            if !items.is_empty() {
                direct_allocs.push(AllocOp {
                    description: "list literal".to_string(),
                    file: cur_mod.to_string(),
                    line,
                });
            }
            for it in items {
                collect_expr_allocs(it, cur_mod, locals, fn_vals, iface_vars, direct_allocs, callees, interfaces, extern_allocates);
            }
        }
        ExprKind::Map(items) => {
            if !items.is_empty() {
                direct_allocs.push(AllocOp {
                    description: "map literal".to_string(),
                    file: cur_mod.to_string(),
                    line,
                });
            }
            for (k, v) in items {
                collect_expr_allocs(k, cur_mod, locals, fn_vals, iface_vars, direct_allocs, callees, interfaces, extern_allocates);
                collect_expr_allocs(v, cur_mod, locals, fn_vals, iface_vars, direct_allocs, callees, interfaces, extern_allocates);
            }
        }
        ExprKind::New(ty) => {
            direct_allocs.push(AllocOp {
                description: format!("new {}", type_to_string(ty)),
                file: cur_mod.to_string(),
                line,
            });
        }
        ExprKind::Lambda { params, body } => {
            // Check for captures
            let param_names: HashSet<String> = params.iter().map(|p| p.name.clone()).collect();
            let mut used_idents = HashSet::new();
            collect_used_idents(body, &mut used_idents);

            for used in used_idents {
                if !param_names.contains(&used) && locals.contains(&used) {
                    direct_allocs.push(AllocOp {
                        description: format!("closure that captures '{}'", used),
                        file: cur_mod.to_string(),
                        line,
                    });
                    break;
                }
            }
        }
        ExprKind::Call { callee, args, .. } => {
            // Check special calls
            if let ExprKind::Ident(name) = &callee.kind {
                if name == "channel" {
                    direct_allocs.push(AllocOp {
                        description: "channel".to_string(),
                        file: cur_mod.to_string(),
                        line,
                    });
                } else if fn_vals.contains(name) {
                    // Call through a function value
                    direct_allocs.push(AllocOp {
                        description: format!("call through function value '{}'", name),
                        file: cur_mod.to_string(),
                        line,
                    });
                } else {
                    let target_qname = format!("{}.{}", cur_mod, name);
                    if extern_allocates.contains(&target_qname) {
                        direct_allocs.push(AllocOp {
                            description: format!("extern '{}' declared #[allocates]", name),
                            file: cur_mod.to_string(),
                            line,
                        });
                    }
                    callees.push((target_qname, line));
                }
            } else if let ExprKind::Field { expr: base, name: field, .. } = &callee.kind {
                if let ExprKind::Ident(base_name) = &base.kind {
                    if base_name == "c" && field == "alloc" {
                        direct_allocs.push(AllocOp {
                            description: "c.alloc".to_string(),
                            file: cur_mod.to_string(),
                            line,
                        });
                    } else if let Some(iface_name) = iface_vars.get(base_name) {
                        direct_allocs.push(AllocOp {
                            description: format!("interface call '{}.{}'", iface_name, field),
                            file: cur_mod.to_string(),
                            line,
                        });
                    } else if interfaces.contains(base_name) || interfaces.contains(&format!("{}.{}", cur_mod, base_name)) {
                        direct_allocs.push(AllocOp {
                            description: format!("interface call '{}.{}'", base_name, field),
                            file: cur_mod.to_string(),
                            line,
                        });
                    } else {
                        let target_qname = format!("{}.{}", base_name, field);
                        if extern_allocates.contains(&target_qname) {
                            direct_allocs.push(AllocOp {
                                description: format!("extern '{}' declared #[allocates]", target_qname),
                                file: cur_mod.to_string(),
                                line,
                            });
                        }
                        callees.push((target_qname, line));
                    }
                } else {
                    direct_allocs.push(AllocOp {
                        description: format!("call through expression at line {}", line),
                        file: cur_mod.to_string(),
                        line,
                    });
                }
            } else {
                direct_allocs.push(AllocOp {
                    description: format!("indirect call at line {}", line),
                    file: cur_mod.to_string(),
                    line,
                });
            }

            for a in args {
                collect_expr_allocs(&a.value, cur_mod, locals, fn_vals, iface_vars, direct_allocs, callees, interfaces, extern_allocates);
            }
        }
        ExprKind::Binary { lhs, rhs, .. } => {
            collect_expr_allocs(lhs, cur_mod, locals, fn_vals, iface_vars, direct_allocs, callees, interfaces, extern_allocates);
            collect_expr_allocs(rhs, cur_mod, locals, fn_vals, iface_vars, direct_allocs, callees, interfaces, extern_allocates);
        }
        ExprKind::Unary { expr: inner, .. } | ExprKind::Propagate(inner) | ExprKind::Await(inner) | ExprKind::Deref(inner) => {
            collect_expr_allocs(inner, cur_mod, locals, fn_vals, iface_vars, direct_allocs, callees, interfaces, extern_allocates);
        }
        ExprKind::If { branches, else_body } => {
            for (cond, b) in branches {
                collect_expr_allocs(cond, cur_mod, locals, fn_vals, iface_vars, direct_allocs, callees, interfaces, extern_allocates);
                collect_body_allocs(b, cur_mod, locals, fn_vals, iface_vars, direct_allocs, callees, interfaces, extern_allocates);
            }
            if let Some(eb) = else_body {
                collect_body_allocs(eb, cur_mod, locals, fn_vals, iface_vars, direct_allocs, callees, interfaces, extern_allocates);
            }
        }
        ExprKind::Match { scrutinee, arms } => {
            collect_expr_allocs(scrutinee, cur_mod, locals, fn_vals, iface_vars, direct_allocs, callees, interfaces, extern_allocates);
            for arm in arms {
                collect_body_allocs(&arm.body, cur_mod, locals, fn_vals, iface_vars, direct_allocs, callees, interfaces, extern_allocates);
            }
        }
        ExprKind::Record { fields, .. } => {
            for (_, val) in fields {
                collect_expr_allocs(val, cur_mod, locals, fn_vals, iface_vars, direct_allocs, callees, interfaces, extern_allocates);
            }
        }
        ExprKind::Index { expr: base, index } => {
            collect_expr_allocs(base, cur_mod, locals, fn_vals, iface_vars, direct_allocs, callees, interfaces, extern_allocates);
            collect_expr_allocs(index, cur_mod, locals, fn_vals, iface_vars, direct_allocs, callees, interfaces, extern_allocates);
        }
        ExprKind::Field { expr: base, .. } => {
            collect_expr_allocs(base, cur_mod, locals, fn_vals, iface_vars, direct_allocs, callees, interfaces, extern_allocates);
        }
        _ => {}
    }
}

fn collect_used_idents(block: &[Stmt], used: &mut HashSet<String>) {
    for s in block {
        match &s.kind {
            StmtKind::Var(vd) => {
                if let Some(v) = &vd.value {
                    collect_used_idents_expr(v, used);
                }
            }
            StmtKind::Expr(e) | StmtKind::Return(Some(e)) | StmtKind::Spawn(e) | StmtKind::Defer(e) => {
                collect_used_idents_expr(e, used);
            }
            StmtKind::Assign { target, value, .. } => {
                collect_used_idents_expr(target, used);
                collect_used_idents_expr(value, used);
            }
            StmtKind::While { cond, body } => {
                collect_used_idents_expr(cond, used);
                collect_used_idents(body, used);
            }
            StmtKind::For { iter, body, .. } => {
                collect_used_idents_expr(iter, used);
                collect_used_idents(body, used);
            }
            StmtKind::Unsafe(body) => {
                collect_used_idents(body, used);
            }
            _ => {}
        }
    }
}

fn collect_used_idents_expr(expr: &Expr, used: &mut HashSet<String>) {
    match &expr.kind {
        ExprKind::Ident(s) => {
            used.insert(s.clone());
        }
        ExprKind::Call { callee, args, .. } => {
            collect_used_idents_expr(callee, used);
            for a in args {
                collect_used_idents_expr(&a.value, used);
            }
        }
        ExprKind::Binary { lhs, rhs, .. } => {
            collect_used_idents_expr(lhs, used);
            collect_used_idents_expr(rhs, used);
        }
        ExprKind::Unary { expr: inner, .. } | ExprKind::Propagate(inner) | ExprKind::Await(inner) | ExprKind::Deref(inner) => {
            collect_used_idents_expr(inner, used);
        }
        ExprKind::Field { expr: base, .. } => {
            collect_used_idents_expr(base, used);
        }
        ExprKind::Index { expr: base, index } => {
            collect_used_idents_expr(base, used);
            collect_used_idents_expr(index, used);
        }
        ExprKind::Text(parts) => {
            for p in parts {
                if let TextPart::Expr(e) = p {
                    collect_used_idents_expr(e, used);
                }
            }
        }
        ExprKind::List(items) => {
            for it in items {
                collect_used_idents_expr(it, used);
            }
        }
        _ => {}
    }
}

/// Runs a breadth-first search starting at `start_qname` to find the shortest
/// path to a direct allocation operation. Returns the formatted chain if found.
fn find_alloc_chain(start_qname: &str, nodes: &HashMap<String, FuncNode>) -> Option<String> {
    let start_node = nodes.get(start_qname)?;

    // If start node directly allocates, return immediate chain
    if let Some(op) = start_node.direct_allocs.first() {
        return Some(format!(
            "{} -> {} at {}.br:{}",
            start_node.name, op.description, op.file, op.line
        ));
    }

    // BFS queue: (current_qname, path_of_display_names)
    let mut queue: VecDeque<(String, Vec<String>)> = VecDeque::new();
    let mut visited: HashSet<String> = HashSet::new();

    queue.push_back((start_qname.to_string(), vec![start_node.name.clone()]));
    visited.insert(start_qname.to_string());

    while let Some((curr_qname, path)) = queue.pop_front() {
        if let Some(curr_node) = nodes.get(&curr_qname) {
            // Check direct allocs
            if let Some(op) = curr_node.direct_allocs.first() {
                let chain = path.join(" -> ");
                return Some(format!("{} -> {} at {}.br:{}", chain, op.description, op.file, op.line));
            }

            // Expand callees
            for (callee_qname, _) in &curr_node.callees {
                if !visited.contains(callee_qname) {
                    visited.insert(callee_qname.clone());
                    let callee_name = if let Some(cnode) = nodes.get(callee_qname) {
                        cnode.name.clone()
                    } else {
                        callee_qname.split('.').last().unwrap_or(callee_qname).to_string()
                    };
                    let mut next_path = path.clone();
                    next_path.push(callee_name);
                    queue.push_back((callee_qname.clone(), next_path));
                }
            }
        }
    }

    None
}
