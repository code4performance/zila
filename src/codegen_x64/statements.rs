//! Part of the code generator (`codegen_x64`), methods of `Compiler`: blocks and statements.

use super::*;

impl Compiler {
    pub(super) fn block(&mut self, b: &[Stmt]) -> R<()> {
        self.push_scope();
        for s in b {
            self.stmt(s)?;
        }
        self.pop_scope();
        Ok(())
    }

    pub(super) fn stmt(&mut self, s: &Stmt) -> R<()> {
        self.line = s.line;
        let mark = self.fs.stmt_temps.len();
        self.stmt_inner(s)?;
        self.flush_temps(mark);
        Ok(())
    }

    pub(super) fn stmt_inner(&mut self, s: &Stmt) -> R<()> {
        match &s.kind {
            StmtKind::Var(vd) => {
                let ann = match &vd.ty {
                    Some(t) => Some(self.resolve(t, &self.cur_mod.clone())?),
                    None => None,
                };
                let ty = match &vd.value {
                    Some(e) => {
                        let t = self.value(e, ann.as_ref())?;
                        if let Some(a) = &ann {
                            self.check(a, &t, &format!("'{}'", vd.name))?;
                            a.clone()
                        } else if t == Ty::Nil || t == Ty::Void {
                            return self.err(format!("cannot infer the type of '{}'", vd.name));
                        } else {
                            t
                        }
                    }
                    None => {
                        let a = ann.clone().unwrap();
                        self.zero_value(&a)?;
                        a
                    }
                };
                self.norm(&ty);
                let off = self.declare_local(&vd.name, ty, vd.mutable);
                let is_shared = matches!(vd.ty, Some(Type::Shared(_)))
                    || matches!(&vd.value, Some(Expr { kind: ExprKind::Shared(_), .. }))
                    || matches!(&vd.value, Some(Expr { kind: ExprKind::Ident(n), .. }) if self.fs.shared_locals.contains(n));
                if is_shared {
                    self.fs.shared_locals.insert(vd.name.clone());
                } else {
                    self.fs.shared_locals.remove(&vd.name);
                }
                match self.lookup(&vd.name).and_then(|l| l.reg) {
                    Some(r) => self.e(format!("mov {}, rax", r)),
                    None => self.e(format!("mov qword ptr [rbp - {}], rax", off)),
                }
            }
            StmtKind::Assign { target, op, value } => self.assign(target, op, value)?,
            StmtKind::Return(v) => {
                let ret = self.fs.ret.clone();
                match (&ret, v) {
                    (Ty::Result(inner), Some(e)) => {
                        let t = self.value(e, Some(inner))?;
                        match &t {
                            Ty::Err => self.mk_err(inner),
                            Ty::Result(u) => {
                                if !compatible(inner, u) {
                                    return self.err(format!("return value: expected {}, found {}", ret.show(), t.show()));
                                }
                            }
                            _ => {
                                if **inner == Ty::Void {
                                    return self.err("this function returns no value; write `return` or `return error(\"...\")`");
                                }
                                self.check(inner, &t, "return value")?;
                                self.norm(inner);
                                self.mk_ok(inner);
                            }
                        }
                    }
                    (Ty::Result(inner), None) => {
                        self.e("xor eax, eax");
                        self.mk_ok(inner);
                    }
                    (Ty::Void, Some(_)) => return self.err("this function does not return a value"),
                    (_, Some(e)) => {
                        let t = self.value(e, Some(&ret))?;
                        self.check(&ret, &t, "return value")?;
                        self.norm(&ret);
                    }
                    (_, None) => {}
                }
                self.emit_run_defers();
                self.flush_all_temps_emit();
                self.emit_unwind(0);
                let l = self.fs.ret_label.clone();
                self.e(format!("jmp {}", l));
            }
            StmtKind::Block { name, .. } => return self.err(format!("block '{}' was not expanded", name)),
            StmtKind::Break | StmtKind::Continue => {
                let Some((cont, brk, depth)) = self.fs.loops.last().map(|l| (l.cont.clone(), l.brk.clone(), l.depth)) else {
                    return self.err("'break'/'continue' outside a loop");
                };
                self.emit_unwind(depth);
                let target = if matches!(s.kind, StmtKind::Break) { brk } else { cont };
                self.e(format!("jmp {}", target));
            }
            StmtKind::While { cond, body } => {
                let (start, end) = (self.lbl("while"), self.lbl("wend"));
                self.label(&start);
                self.cond_jump(cond, &end, false)?;
                let depth = self.fs.scopes.len();
                self.fs.loops.push(LoopCtx { cont: start.clone(), brk: end.clone(), depth });
                self.block(body)?;
                self.fs.loops.pop();
                self.e(format!("jmp {}", start));
                self.label(&end);
            }
            StmtKind::For { vars, iter, body } => self.for_loop(vars, iter, body)?,
            StmtKind::Unsafe(b) => {
                self.fs.unsafe_depth += 1;
                self.block(b)?;
                self.fs.unsafe_depth -= 1;
            }
            StmtKind::Asm(a) => self.asm_block(a)?,
            StmtKind::Expr(e) => match &e.kind {
                ExprKind::If { branches, else_body } => {
                    self.if_chain(branches, else_body.as_ref(), None, false)?;
                }
                ExprKind::Match { scrutinee, arms } => {
                    self.match_expr(scrutinee, arms, None, false)?;
                }
                ExprKind::Call { callee, args, .. }
                    if matches!(&callee.kind, ExprKind::Ident(n) if (n == "assert" || n == "assert_eq")
                        && !self.tp.funcs.contains_key(&(self.cur_mod.clone(), n.clone()))) =>
                {
                    let ExprKind::Ident(name) = &callee.kind else { unreachable!() };
                    self.assert_stmt(name, args, e.line)?;
                }
                _ => {
                    let t = self.expr(e, None)?;
                    if matches!(t, Ty::Result(_)) && !self.fs.allow_ignored_result {
                        return self.err("this call can fail and its result is ignored; use `try`, `match` or `??`");
                    }
                    if self.is_rc(&t) {
                        self.release_rax();
                    }
                }
            },
            StmtKind::Defer(e) => self.defer_stmt(e, s.line)?,
            StmtKind::Spawn(call) => {
                let task_spawn = self.tp.funcs.get(&("task".to_string(), "start".to_string())).cloned();
                let Some(sig) = task_spawn else { return self.err("'spawn' needs the task library") };
                if !matches!(call.kind, ExprKind::Call { .. }) {
                    return self.err("'spawn' needs a function call, like `spawn work(x)`");
                }
                let body = vec![Stmt { kind: StmtKind::Expr(call.clone()), line: s.line }];
                let want = Ty::Func { params: vec![], ret: Box::new(Ty::Void), ext: false };
                self.lambda(&[], &body, Some(&want))?;
                let t = self.temp();
                self.e(format!("mov qword ptr [rbp - {}], rax", t));
                self.emit_call(&[t], &format!("call {}", sig.sym), None);
                self.free(t);
            }
        }
        Ok(())
    }

    // ---------------- copy on write ----------------
    //
    // A list or map is shared by reference counting, so a write first makes sure the writer's own
    // place holds the only reference (`rt_list_unique`, `rt_map_unique`): when someone else holds it
    // too, it is copied one level and the place gets the copy. Writes therefore work on the place
    // (the address of the slot that owns the list), never on a retained temporary.

    /// The static type of a variable, field or element path, or `None` for anything else.
    pub(super) fn path_ty(&self, e: &Expr) -> Option<Ty> {
        match &e.kind {
            ExprKind::Ident(n) => {
                if let Some(l) = self.lookup(n) {
                    return Some(l.ty);
                }
                let g = self.tp.globals.get(&(self.cur_mod.clone(), n.clone()))?;
                if g.konst.is_some() {
                    None
                } else {
                    Some(g.ty.clone())
                }
            }
            ExprKind::Field { expr, name, safe: false } => {
                let Ty::Rec(r) = rc_view(&self.path_ty(expr)?) else { return None };
                self.field_of(&r, name).map(|f| f.0)
            }
            ExprKind::Index { expr, .. } => match self.path_ty(expr)? {
                Ty::List(el) => Some(*el),
                Ty::Map(_, v) => Some(*v),
                _ => None,
            },
            _ => None,
        }
    }

    /// `lea rdx, <copy function>` for the elements of a list or the values of a map, or `xor edx, edx`
    /// when they are retained as they are.
    pub(super) fn clone_fn_for(&self, el: &Ty) -> String {
        match el {
            Ty::Rec(r) if !self.tp.shared_recs.contains(r) => format!("lea rdx, B_copy_{}", sym_of(r)),
            _ => "xor edx, edx".to_string(),
        }
    }

    /// rax = the address of a slot holding a list or map of type `ty`  ->  rax = that list or map,
    /// copied into the slot first when it is shared.
    pub(super) fn unique_rax(&mut self, ty: &Ty) {
        self.e("mov rcx, rax");
        match ty {
            Ty::List(el) => {
                let c = self.clone_fn_for(el);
                self.e(c);
                self.e("call rt_list_unique");
            }
            Ty::Map(_, v) => {
                let c = self.clone_fn_for(v);
                self.e(c);
                self.e("call rt_map_unique");
            }
            _ => {}
        }
    }

    /// Keeps the owned value in rax alive until the end of the statement and leaves the address of
    /// the slot that holds it in rax.
    pub(super) fn temp_place(&mut self) {
        let keep = self.slot();
        self.e(format!("mov qword ptr [rbp - {}], rax", keep));
        self.fs.stmt_temps.push(keep);
        self.fs.may_own = true;
        self.e(format!("lea rax, [rbp - {}]", keep));
    }

    /// rax = the address of the element `obj[index]` (a list or map element slot), after the
    /// container was made unique. A bad index stops the program before anything is copied.
    pub(super) fn index_place(&mut self, obj: &Expr, index: &Expr) -> R<Ty> {
        let Some(ct) = self.path_ty(obj) else { return self.err("invalid assignment target") };
        match ct {
            Ty::List(el) => {
                let ity = self.expr(index, Some(&Ty::i64()))?;
                if !ity.is_int() {
                    return self.err(format!("list index must be a number, found {}", ity.show()));
                }
                let (it, st) = (self.temp(), self.temp());
                self.e(format!("mov qword ptr [rbp - {}], rax", it));
                let lt = self.write_place(obj)?;
                self.e(format!("mov qword ptr [rbp - {}], rax", st));
                let ok = self.lbl("cowok");
                self.e("mov rcx, qword ptr [rax]");
                self.e(format!("mov rdx, qword ptr [rbp - {}]", it));
                self.e("cmp rdx, qword ptr [rcx]");
                self.e(format!("jb {}", ok));
                self.at("r9");
                self.e("jmp rt_index_oob");
                self.label(&ok);
                self.e(format!("mov rax, qword ptr [rbp - {}]", st));
                self.unique_rax(&lt);
                self.e(format!("mov rdx, qword ptr [rbp - {}]", it));
                self.e("mov rax, qword ptr [rax + 16]");
                self.e("lea rax, [rax + rdx * 8]");
                self.free(st);
                self.free(it);
                Ok(*el)
            }
            Ty::Map(k, v) => {
                let kt = self.value(index, Some(&k))?;
                self.check(&k, &kt, "map key")?;
                let (ks, st) = (self.temp(), self.temp());
                self.e(format!("mov qword ptr [rbp - {}], rax", ks));
                let mt = self.write_place(obj)?;
                self.e(format!("mov qword ptr [rbp - {}], rax", st));
                // a missing key stops the program before anything is copied
                self.e("mov rcx, qword ptr [rax]");
                self.e(format!("mov rdx, qword ptr [rbp - {}]", ks));
                self.at("r8");
                self.e("call rt_map_get");
                self.e(format!("mov rax, qword ptr [rbp - {}]", st));
                self.unique_rax(&mt);
                self.e("mov rcx, rax");
                self.e(format!("mov rdx, qword ptr [rbp - {}]", ks));
                self.e("call rt_map_find");
                self.e("add rax, 24");
                if self.is_rc(&k) {
                    self.release_slot(ks);
                }
                self.free(st);
                self.free(ks);
                Ok(*v)
            }
            Ty::Text => self.err(TEXT_INDEX_MSG),
            _ => self.err("invalid assignment target"),
        }
    }

    /// rax = the address of the slot that owns the list or map `e`; every list and map on the way
    /// there has been made unique, the one in the slot itself has not.
    pub(super) fn write_place(&mut self, e: &Expr) -> R<Ty> {
        match &e.kind {
            ExprKind::Ident(n) => {
                if let Some(l) = self.lookup(n) {
                    if l.ind {
                        self.e(format!("mov rax, qword ptr [rbp - {}]", l.off));
                    } else {
                        self.e(format!("lea rax, [rbp - {}]", l.off));
                    }
                    return Ok(l.ty);
                }
                let key = (self.cur_mod.clone(), n.clone());
                if let Some(g) = self.tp.globals.get(&key) {
                    let (sym, ty) = (g.sym.clone(), g.ty.clone());
                    self.e(format!("lea rax, {}", sym));
                    return Ok(ty);
                }
            }
            ExprKind::Field { expr: obj, name, safe: false } if self.path_ty(e).is_some() => {
                let ot = self.rec_ptr(obj)?;
                if let Ty::Rec(r) = &rc_view(&ot) {
                    if let Some((fty, off)) = self.fld(r, name)? {
                        self.e(format!("add rax, {}", off));
                        return Ok(fty);
                    }
                }
            }
            ExprKind::Index { expr: obj, index } if self.path_ty(e).is_some() => {
                return self.index_place(obj, index);
            }
            _ => {}
        }
        // anything else is a value of its own: a temporary holds it
        let t = self.expr(e, None)?;
        if !matches!(t, Ty::List(_) | Ty::Map(..)) {
            return self.err(format!("cannot write to a {}", t.show()));
        }
        self.temp_place();
        Ok(t)
    }

    /// rax = the record that the path `e` names (a pointer), reached for a write: lists and maps on
    /// the way are made unique first.
    pub(super) fn rec_ptr(&mut self, e: &Expr) -> R<Ty> {
        match &e.kind {
            ExprKind::Ident(n) => {
                if let Some(l) = self.lookup(n) {
                    self.load_local("rax", &l);
                    return Ok(l.ty);
                }
                let key = (self.cur_mod.clone(), n.clone());
                if let Some(g) = self.tp.globals.get(&key) {
                    let (sym, ty) = (g.sym.clone(), g.ty.clone());
                    self.e(format!("mov rax, qword ptr {}", sym));
                    return Ok(ty);
                }
            }
            ExprKind::Field { expr: obj, name, safe: false } if self.path_ty(e).is_some() => {
                let ot = self.rec_ptr(obj)?;
                if let Ty::Rec(r) = &rc_view(&ot) {
                    if let Some((fty, off)) = self.fld(r, name)? {
                        self.e(format!("mov rax, qword ptr [rax + {}]", off));
                        return Ok(fty);
                    }
                }
            }
            ExprKind::Index { expr: obj, index } if self.path_ty(e).is_some() => {
                let el = self.index_place(obj, index)?;
                self.e("mov rax, qword ptr [rax]");
                return Ok(el);
            }
            _ => {}
        }
        let (t, own) = self.operand(e, None)?;
        if own {
            let keep = self.slot();
            self.e(format!("mov qword ptr [rbp - {}], rax", keep));
            self.fs.stmt_temps.push(keep);
            self.fs.may_own = true;
        }
        Ok(t)
    }

    /// `xs.push(v)`, `xs.clear()`, `xs.take()` on a list and `m.remove(k)`, `m.clear()` on a map,
    /// where the receiver is a variable, field or element path: the write goes to the receiver's own
    /// place. `Ok(None)` when the call is not one of these.
    pub(super) fn cow_method(&mut self, obj: &Expr, pt: &Ty, name: &str, args: &[Arg]) -> R<Option<Ty>> {
        if matches!(name, "push" | "add" | "clear" | "take" | "remove" | "insert" | "pop") && !self.tp.methods.contains_key(&(method_key(pt), name.to_string())) {
            self.shared_write_check(obj, true)?;
        }
        if self.tp.methods.contains_key(&(method_key(pt), name.to_string())) {
            return Ok(None);
        }
        match (pt, name) {
            (Ty::List(el), "push" | "add") => {
                if args.len() != 1 {
                    return self.err(format!("'{}' takes one argument", name));
                }
                let vt = self.value(&args[0].value, Some(el))?;
                self.check(el, &vt, "list element")?;
                let v = self.temp();
                self.e(format!("mov qword ptr [rbp - {}], rax", v));
                let lt = self.write_place(obj)?;
                self.unique_rax(&lt);
                self.e("mov rcx, rax");
                self.e(format!("mov rdx, qword ptr [rbp - {}]", v));
                self.e("call rt_list_push");
                self.free(v);
                Ok(Some(Ty::Void))
            }
            (Ty::List(_), "clear") | (Ty::Map(..), "clear") => {
                if !args.is_empty() {
                    return self.err("'clear' takes no arguments");
                }
                let lt = self.write_place(obj)?;
                self.unique_rax(&lt);
                self.e("mov rcx, rax");
                self.e(if matches!(pt, Ty::List(_)) { "call rt_list_clear" } else { "call rt_map_clear" });
                Ok(Some(Ty::Void))
            }
            (Ty::List(el), "take") => {
                let lt = self.write_place(obj)?;
                self.unique_rax(&lt);
                self.e("mov rcx, rax");
                self.at("rdx");
                self.e("call rt_list_take");
                Ok(Some((**el).clone()))
            }
            (Ty::Map(k, _), "remove") => {
                if args.len() != 1 {
                    return self.err("'remove' takes 1 argument(s)");
                }
                let kt = self.value(&args[0].value, Some(k))?;
                self.check(k, &kt, "map key")?;
                let ks = self.temp();
                self.e(format!("mov qword ptr [rbp - {}], rax", ks));
                let mt = self.write_place(obj)?;
                self.unique_rax(&mt);
                self.e("mov rcx, rax");
                self.e(format!("mov rdx, qword ptr [rbp - {}]", ks));
                self.e("call rt_map_remove");
                if self.is_rc(k) {
                    self.release_slot(ks);
                }
                self.free(ks);
                Ok(Some(Ty::Void))
            }
            _ => Ok(None),
        }
    }

    /// `obj[index] = rhs` where `obj` is a list or map path.
    pub(super) fn assign_index_cow(&mut self, obj: &Expr, index: &Expr, rhs: &Expr, ct: &Ty) -> R<()> {
        match ct {
            Ty::List(el) => {
                let ity = self.expr(index, Some(&Ty::i64()))?;
                if !ity.is_int() {
                    return self.err(format!("list index must be a number, found {}", ity.show()));
                }
                let (it, v, st) = (self.temp(), self.temp(), self.temp());
                self.e(format!("mov qword ptr [rbp - {}], rax", it));
                let vt = self.value(rhs, Some(el))?;
                self.check(el, &vt, "list element")?;
                self.e(format!("mov qword ptr [rbp - {}], rax", v));
                let lt = self.write_place(obj)?;
                self.e(format!("mov qword ptr [rbp - {}], rax", st));
                let ok = self.lbl("cowok");
                self.e("mov rcx, qword ptr [rax]");
                self.e(format!("mov rdx, qword ptr [rbp - {}]", it));
                self.e("cmp rdx, qword ptr [rcx]");
                self.e(format!("jb {}", ok));
                self.at("r9");
                self.e("jmp rt_index_oob");
                self.label(&ok);
                self.e(format!("mov rax, qword ptr [rbp - {}]", st));
                self.unique_rax(&lt);
                self.e("mov rcx, rax");
                self.e(format!("mov rdx, qword ptr [rbp - {}]", it));
                self.e(format!("mov r8, qword ptr [rbp - {}]", v));
                self.at("r9");
                self.e("call rt_list_set");
                self.free(st);
                self.free(v);
                self.free(it);
            }
            Ty::Map(k, v) => {
                let kt = self.value(index, Some(k))?;
                self.check(k, &kt, "map key")?;
                let (ks, vs) = (self.temp(), self.temp());
                self.e(format!("mov qword ptr [rbp - {}], rax", ks));
                let vt = self.value(rhs, Some(v))?;
                self.check(v, &vt, "map value")?;
                self.e(format!("mov qword ptr [rbp - {}], rax", vs));
                let mt = self.write_place(obj)?;
                self.unique_rax(&mt);
                self.e("mov rcx, rax");
                self.e(format!("mov rdx, qword ptr [rbp - {}]", ks));
                self.e(format!("mov r8, qword ptr [rbp - {}]", vs));
                self.e("call rt_map_set");
                if self.is_rc(k) {
                    self.release_slot(ks);
                }
                self.free(vs);
                self.free(ks);
            }
            _ => unreachable!(),
        }
        Ok(())
    }

    pub(super) fn assign(&mut self, target: &Expr, op: &str, value: &Expr) -> R<()> {
        self.shared_write_check(target, false)?;
        // `a op= b` is compiled as `a = a op b`.
        let rhs_owned;
        let rhs = if op == "=" {
            value
        } else {
            rhs_owned = Expr {
                kind: ExprKind::Binary { op: binop_static(&op[..op.len() - 1]), lhs: Box::new(target.clone()), rhs: Box::new(value.clone()) },
                line: value.line,
            };
            &rhs_owned
        };
        match &target.kind {
            ExprKind::Ident(name) => {
                if let Some(l) = self.lookup(name) {
                    if l.captured {
                        return self.err(format!("cannot assign to '{}': closures capture variables by value", name));
                    }
                    if !l.mutable {
                        return self.err(format!("cannot assign to '{}': declared with 'let'", name));
                    }
                    let t = self.value(rhs, Some(&l.ty))?;
                    self.check(&l.ty, &t, &format!("'{}'", name))?;
                    self.norm(&l.ty);
                    if l.ind {
                        // a `ref` list or map: the caller's own variable is replaced
                        self.e(format!("mov rdx, qword ptr [rbp - {}]", l.off));
                        self.e("mov rcx, qword ptr [rdx]");
                        self.e("mov qword ptr [rdx], rax");
                        self.e("call rt_release");
                    } else if self.is_rc(&l.ty) {
                        self.e(format!("mov rcx, qword ptr [rbp - {}]", l.off));
                        self.e(format!("mov qword ptr [rbp - {}], rax", l.off));
                        self.e("call rt_release");
                    } else {
                        self.store_local(&l);
                    }
                    return Ok(());
                }
                let key = (self.cur_mod.clone(), name.clone());
                let Some(g) = self.tp.globals.get(&key) else { return self.err(format!("undefined variable '{}'", name)) };
                if !g.mutable {
                    return self.err(format!("cannot assign to '{}': declared with 'let'", name));
                }
                let (sym, ty) = (g.sym.clone(), g.ty.clone());
                let t = self.value(rhs, Some(&ty))?;
                self.check(&ty, &t, &format!("'{}'", name))?;
                self.norm(&ty);
                if self.is_rc(&ty) {
                    self.e(format!("mov rcx, qword ptr {}", sym));
                    self.e(format!("mov qword ptr {}, rax", sym));
                    self.e("call rt_release");
                } else {
                    self.e(format!("mov qword ptr {}, rax", sym));
                }
            }
            ExprKind::Field { expr: obj, name, .. } => {
                // a path into lists and maps makes each of them unique before the store
                let (ot, own) = if matches!(self.path_ty(obj).map(|t| rc_view(&t)), Some(Ty::Rec(_))) { (self.rec_ptr(obj)?, false) } else { self.operand(obj, None)? };
                let r = match &rc_view(&ot) {
                    Ty::Rec(r) => r.clone(),
                    Ty::Ptr(p) => match &**p {
                        Ty::Rec(r) => r.clone(),
                        _ => return self.err(format!("cannot assign field '{}' on {}", name, ot.show())),
                    },
                    _ => return self.err(format!("cannot assign field '{}' on {}", name, ot.show())),
                };
                let t = self.temp();
                self.e(format!("mov qword ptr [rbp - {}], rax", t));
                if let Some((fty, off)) = self.fld(&r, name)? {
                    let vt = self.value(rhs, Some(&fty))?;
                    self.check(&fty, &vt, &format!("field '{}'", name))?;
                    self.norm(&fty);
                    self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
                    if self.is_rc(&fty) {
                        let old = self.temp();
                        self.e(format!("mov rdx, qword ptr [rcx + {}]", off));
                        self.e(format!("mov qword ptr [rbp - {}], rdx", old));
                        self.store_mem(&fty, &format!("rcx + {}", off));
                        self.release_slot(old);
                        self.free(old);
                    } else {
                        self.store_mem(&fty, &format!("rcx + {}", off));
                    }
                } else if let Some(sig) = self.tp.methods.get(&(r.clone(), format!("set_{}", name))).cloned() {
                    self.check_method_vis(&sig, &format!("set_{}", name))?;
                    // property setter: `w.text = v` -> `w.set_text(v)`
                    let Some(p) = sig.params.first().cloned() else {
                        return self.err(format!("'set_{}' must take one parameter", name));
                    };
                    let vt = self.value(rhs, Some(&p.ty))?;
                    self.check(&p.ty, &vt, &format!("'{}'", name))?;
                    self.norm(&p.ty);
                    let vtmp = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", vtmp));
                    // the method takes ownership of its receiver
                    self.e(format!("mov rax, qword ptr [rbp - {}]", t));
                    if !own {
                        self.retain();
                    }
                    self.e(format!("mov qword ptr [rbp - {}], rax", t));
                    self.emit_call(&[t, vtmp], &format!("call {}", sig.sym), None);
                    self.free(vtmp);
                    self.free(t);
                    return Ok(());
                } else {
                    return self.err(format!("{} has no field '{}'", r, name));
                }
                if own {
                    self.release_slot(t);
                }
                self.free(t);
            }
            ExprKind::Index { expr: obj, index } => {
                if let Some(ct @ (Ty::List(_) | Ty::Map(..))) = self.path_ty(obj) {
                    return self.assign_index_cow(obj, index, rhs, &ct);
                }
                let (ot, own) = self.operand(obj, None)?;
                let t = self.temp();
                self.e(format!("mov qword ptr [rbp - {}], rax", t));
                let key_want = if let Ty::Map(k, _) = &ot { Some((**k).clone()) } else { None };
                let ity = if key_want.is_some() { self.value(index, key_want.as_ref())? } else { self.expr(index, key_want.as_ref())? };
                let it = self.temp();
                self.e(format!("mov qword ptr [rbp - {}], rax", it));
                match &ot {
                    Ty::Map(k, v) => {
                        self.check(k, &ity, "map key")?;
                        let vt = self.value(rhs, Some(v))?;
                        self.check(v, &vt, "map value")?;
                        self.e("mov r8, rax");
                        self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
                        self.e(format!("mov rdx, qword ptr [rbp - {}]", it));
                        self.e("call rt_map_set");
                        if self.is_rc(k) {
                            self.release_slot(it);
                        }
                    }
                    Ty::List(el) => {
                        let vt = self.value(rhs, Some(el))?;
                        self.check(el, &vt, "list element")?;
                        self.e("mov r8, rax");
                        self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
                        self.e(format!("mov rdx, qword ptr [rbp - {}]", it));
                        self.at("r9");
                        self.e("call rt_list_set");
                    }
                    Ty::Ptr(el) => {
                        self.require_unsafe("writing through a raw pointer")?;
                        let el = (**el).clone();
                        let vt = self.value(rhs, Some(&el))?;
                        self.check(&el, &vt, "pointer element")?;
                        self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
                        self.e(format!("mov rdx, qword ptr [rbp - {}]", it));
                        self.e(format!("imul rdx, rdx, {}", el.size()));
                        self.e("add rcx, rdx");
                        self.store_mem(&el, "rcx");
                    }
                    _ => return self.err(format!("cannot assign into {}", ot.show())),
                }
                if own {
                    self.release_slot(t);
                }
                self.free(it);
                self.free(t);
            }
            ExprKind::Deref(p) => {
                self.require_unsafe("writing through a raw pointer")?;
                let (pt, _) = self.operand(p, None)?;
                let Ty::Ptr(el) = pt else { return self.err("'^' needs a pointer") };
                let t = self.temp();
                self.e(format!("mov qword ptr [rbp - {}], rax", t));
                let vt = self.value(rhs, Some(&el))?;
                self.check(&el, &vt, "pointer target")?;
                self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
                self.store_mem(&el, "rcx");
                self.free(t);
            }
            _ => return self.err("invalid assignment target"),
        }
        Ok(())
    }

    pub(super) fn for_loop(&mut self, vars: &[String], iter: &Expr, body: &[Stmt]) -> R<()> {
        let (start, cont, end) = (self.lbl("for"), self.lbl("fnext"), self.lbl("fend"));
        self.push_scope();
        if let ExprKind::Binary { op: "..", lhs, rhs } = &iter.kind {
            let lt = self.expr(lhs, None)?;
            let ty = if lt.is_int() { lt } else { return self.err("range bounds must be integers") };
            let var = self.declare_local(&vars[0], ty.clone(), false);
            let lreg = self.lookup(&vars[0]).and_then(|l| l.reg);
            match lreg {
                Some(r) => self.e(format!("mov {}, rax", r)),
                None => self.e(format!("mov qword ptr [rbp - {}], rax", var)),
            }
            let rt = self.expr(rhs, Some(&ty))?;
            if rt != ty {
                // `0..n` with a `long` n counts in `long`; the bounds must otherwise agree
                match wider(&ty, &rt) {
                    Some(w) if rt.is_int() => {
                        for sc in self.fs.scopes.iter_mut().rev() {
                            if let Some(l) = sc.vars.get_mut(&vars[0]) {
                                l.ty = w.clone();
                                break;
                            }
                        }
                    }
                    _ => return self.err(format!("range bounds must have the same integer type, found {} and {}", ty.show(), rt.show())),
                }
            }
            let hi = self.slot();
            self.e(format!("mov qword ptr [rbp - {}], rax", hi));
            self.label(&start);
            match lreg {
                Some(r) => self.e(format!("cmp {}, qword ptr [rbp - {}]", r, hi)),
                None => {
                    self.e(format!("mov rax, qword ptr [rbp - {}]", var));
                    self.e(format!("cmp rax, qword ptr [rbp - {}]", hi));
                }
            }
            self.e(format!("jge {}", end));
            let depth = self.fs.scopes.len();
            self.fs.loops.push(LoopCtx { cont: cont.clone(), brk: end.clone(), depth });
            self.block(body)?;
            self.fs.loops.pop();
            self.label(&cont);
            match lreg {
                Some(r) => self.e(format!("inc {}", r)),
                None => self.e(format!("inc qword ptr [rbp - {}]", var)),
            }
            self.e(format!("jmp {}", start));
            self.label(&end);
        } else {
            let lt = self.expr(iter, None)?;
            if let Ty::Map(k, v) = lt.clone() {
                let map = self.hidden_local(lt);
                self.e(format!("mov qword ptr [rbp - {}], rax", map));
                let idx = self.slot();
                self.e(format!("mov qword ptr [rbp - {}], 0", idx));
                let ent = self.slot();
                let kv = self.declare_local(&vars[0], (*k).clone(), false);
                let vv = if vars.len() >= 2 { Some(self.declare_local(&vars[1], (*v).clone(), false)) } else { None };
                self.e(format!("mov qword ptr [rbp - {}], 0", kv));
                if let Some(vv) = vv {
                    self.e(format!("mov qword ptr [rbp - {}], 0", vv));
                }
                self.label(&start);
                self.e(format!("mov rcx, qword ptr [rbp - {}]", map));
                self.e(format!("mov rax, qword ptr [rbp - {}]", idx));
                self.e("cmp rax, qword ptr [rcx]");
                self.e(format!("jae {}", end));
                self.e("shl rax, 5");
                self.e("add rax, qword ptr [rcx + 16]");
                self.e(format!("mov qword ptr [rbp - {}], rax", ent));
                self.e("cmp qword ptr [rax], 0");
                self.e(format!("je {}", cont));
                self.e("mov rax, qword ptr [rax + 16]");
                self.bind_loop_var(&k, kv);
                if let Some(vv) = vv {
                    self.e(format!("mov rax, qword ptr [rbp - {}]", ent));
                    self.e("mov rax, qword ptr [rax + 24]");
                    self.bind_loop_var(&v, vv);
                }
                let depth = self.fs.scopes.len();
                self.fs.loops.push(LoopCtx { cont: cont.clone(), brk: end.clone(), depth });
                self.block(body)?;
                self.fs.loops.pop();
                self.label(&cont);
                self.e(format!("inc qword ptr [rbp - {}]", idx));
                self.e(format!("jmp {}", start));
                self.label(&end);
                self.pop_scope();
                return Ok(());
            }
            if lt == Ty::Text {
                if vars.len() != 1 {
                    return self.err("a loop over a text has one variable: the character");
                }
                // the text is held for the whole loop, so the body may reassign the variable that named it
                let txt = self.hidden_local(lt);
                self.e(format!("mov qword ptr [rbp - {}], rax", txt));
                let off = self.slot();
                self.e(format!("mov qword ptr [rbp - {}], 0", off));
                let cv = self.declare_local(&vars[0], Ty::Char, false);
                self.e(format!("mov qword ptr [rbp - {}], 0", cv));
                self.label(&start);
                self.e(format!("mov rcx, qword ptr [rbp - {}]", txt));
                self.e(format!("mov rdx, qword ptr [rbp - {}]", off));
                self.e("cmp rdx, qword ptr [rcx]");
                self.e(format!("jae {}", end));
                self.e("call rt_text_next_char");
                self.e(format!("mov qword ptr [rbp - {}], rdx", off));
                self.e(format!("mov qword ptr [rbp - {}], rax", cv));
                let depth = self.fs.scopes.len();
                self.fs.loops.push(LoopCtx { cont: cont.clone(), brk: end.clone(), depth });
                self.block(body)?;
                self.fs.loops.pop();
                self.label(&cont);
                self.e(format!("jmp {}", start));
                self.label(&end);
                self.pop_scope();
                return Ok(());
            }
            let Ty::List(el) = lt.clone() else { return self.err(format!("cannot iterate over {}", lt.show())) };
            let list = self.hidden_local(lt);
            self.e(format!("mov qword ptr [rbp - {}], rax", list));
            let idx = self.slot();
            self.e(format!("mov qword ptr [rbp - {}], 0", idx));
            let (iv, vv) = if vars.len() >= 2 {
                (Some(self.declare_local(&vars[0], Ty::int(), false)), self.declare_local(&vars[1], (*el).clone(), false))
            } else {
                (None, self.declare_local(&vars[0], (*el).clone(), false))
            };
            // the slots may hold values from a previous run of this loop
            self.e(format!("mov qword ptr [rbp - {}], 0", vv));
            self.label(&start);
            self.e(format!("mov rdx, qword ptr [rbp - {}]", idx));
            self.e(format!("mov rcx, qword ptr [rbp - {}]", list));
            self.e("cmp rdx, qword ptr [rcx]");
            self.e(format!("jge {}", end));
            self.e("call rt_list_get");
            if matches!(&*el, Ty::Rec(r) if !self.tp.shared_recs.contains(r)) {
                let Ty::Rec(r) = &*el else { unreachable!() };
                self.e("mov rcx, rax");
                self.e(format!("call B_copy_{}", sym_of(r)));
            } else {
                self.retain_if(&el);
            }
            if self.is_rc(&el) {
                self.e(format!("mov rcx, qword ptr [rbp - {}]", vv));
                self.e(format!("mov qword ptr [rbp - {}], rax", vv));
                self.e("call rt_release");
            } else {
                self.e(format!("mov qword ptr [rbp - {}], rax", vv));
            }
            if let Some(iv) = iv {
                self.e(format!("mov rax, qword ptr [rbp - {}]", idx));
                self.e(format!("mov qword ptr [rbp - {}], rax", iv));
            }
            let depth = self.fs.scopes.len();
            self.fs.loops.push(LoopCtx { cont: cont.clone(), brk: end.clone(), depth });
            self.block(body)?;
            self.fs.loops.pop();
            self.label(&cont);
            self.e(format!("inc qword ptr [rbp - {}]", idx));
            self.e(format!("jmp {}", start));
            self.label(&end);
        }
        self.pop_scope();
        Ok(())
    }

    /// Stores the borrowed element in rax into the loop variable at `slot`: a copy for plain
    /// records, a retained reference for other heap values; the previous value is released.
    pub(super) fn bind_loop_var(&mut self, el: &Ty, slot: i32) {
        if matches!(el, Ty::Rec(r) if !self.tp.shared_recs.contains(r)) {
            let Ty::Rec(r) = el else { unreachable!() };
            self.e("mov rcx, rax");
            self.e(format!("call B_copy_{}", sym_of(r)));
        } else {
            self.retain_if(el);
        }
        if self.is_rc(el) {
            self.e(format!("mov rcx, qword ptr [rbp - {}]", slot));
            self.e(format!("mov qword ptr [rbp - {}], rax", slot));
            self.e("call rt_release");
        } else {
            self.e(format!("mov qword ptr [rbp - {}], rax", slot));
        }
    }

    pub(super) fn require_unsafe(&self, what: &str) -> R<()> {
        if self.fs.unsafe_depth == 0 {
            self.err(format!("{} requires an 'unsafe' block", what))
        } else {
            Ok(())
        }
    }

    pub(super) fn asm_block(&mut self, a: &AsmBlock) -> R<()> {
        self.fs.has_asm = true;
        if self.fs.unsafe_depth == 0 && !self.fs.code.is_empty() {
            self.require_unsafe("inline 'asm'")?;
        }
        // lets an assembler error be reported at the Zila Lang line of the block
        self.e(format!("; asm {}.zila:{}", self.cur_mod, self.line));
        const POOL: [&str; 4] = ["r10", "r11", "r8", "r9"];
        let mut regs: HashMap<String, String> = HashMap::new();
        let mut next = 0;
        for o in &a.operands {
            let r = if is_register(&o.name) {
                o.name.clone()
            } else {
                if next >= POOL.len() {
                    return self.err("too many asm operands");
                }
                next += 1;
                POOL[next - 1].to_string()
            };
            regs.insert(o.name.clone(), r);
        }
        // Inputs: evaluate all, then load registers.
        let mut loads = Vec::new();
        for o in &a.operands {
            if o.dir != AsmDir::Out {
                self.expr(&o.value, None)?;
                let t = self.temp();
                self.e(format!("mov qword ptr [rbp - {}], rax", t));
                loads.push((t, reg64(&regs[&o.name])));
            }
        }
        for (t, r) in &loads {
            self.e(format!("mov {}, qword ptr [rbp - {}]", r, t));
        }
        for (t, _) in loads {
            self.free(t);
        }
        for line in &a.lines {
            let mut l = line.clone();
            let mut names: Vec<&String> = regs.keys().collect();
            names.sort_by_key(|n| std::cmp::Reverse(n.len()));
            for n in names {
                l = l.replace(&format!("@{}", n), &regs[n]);
            }
            self.e(l);
        }
        // Outputs: copy registers back into variables.
        for o in &a.operands {
            if o.dir != AsmDir::In {
                let ExprKind::Ident(v) = &o.value.kind else { return self.err("asm output must be a variable") };
                let Some(l) = self.lookup(v) else { return self.err(format!("unknown variable '{}'", v)) };
                self.e(format!("mov rax, {}", reg64(&regs[&o.name])));
                self.norm(&l.ty);
                self.e(format!("mov qword ptr [rbp - {}], rax", l.off));
            }
        }
        Ok(())
    }

    // ---------------- expressions ----------------

}
