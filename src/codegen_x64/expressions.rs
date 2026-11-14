//! Part of the code generator (`codegen_x64`), methods of `Compiler`: expressions, operators, records, `match`, closures.

use super::*;

impl Compiler {
    /// Evaluates `e`. The result is in rax; heap values are owned (+1).
    /// Compiles `e`, leaving its value in `rax`, and returns its type. In builds with debug assertions
    /// the type is checked against `check_expr` (see `checker.rs`).
    pub(super) fn expr(&mut self, e: &Expr, want: Option<&Ty>) -> R<Ty> {
        let tree = if cfg!(debug_assertions) { self.check_expr(e, want) } else { None };
        let r = self.expr_emit(e, want);
        if let (Some(t), Ok(got)) = (&tree, &r) {
            assert!(&t.ty == got, "check_expr says {} but emission gives {} (line {})", t.ty.show(), got.show(), e.line);
        }
        r
    }

    fn expr_emit(&mut self, e: &Expr, want: Option<&Ty>) -> R<Ty> {
        self.line = e.line;
        if self.opt && matches!(e.kind, ExprKind::Binary { .. } | ExprKind::Unary { .. }) {
            // literals are typed from the expected type, as the code generator would do
            let t = match want {
                Some(t) if t.is_int() => t.clone(),
                _ => Ty::int(),
            };
            let fbits = if matches!(want, Some(Ty::Float { bits: 32 })) { 32 } else { 64 };
            if let Some(c) = self.fold(e, &t, fbits) {
                return Ok(self.emit_const(c, &t, fbits));
            }
        }
        match &e.kind {
            ExprKind::Int(n) => {
                let t = match want {
                    Some(t) if t.is_int() => t.clone(),
                    _ => Ty::int(),
                };
                self.lit_check(*n, false, &t)?;
                self.e(format!("mov rax, {}", imm64(*n as i64)));
                Ok(t)
            }
            ExprKind::Char(c) => {
                self.e(format!("mov eax, {}", *c as u32));
                Ok(Ty::Char)
            }
            ExprKind::Bool(b) => {
                self.e(format!("mov eax, {}", *b as i32));
                Ok(Ty::Bool)
            }
            ExprKind::Nil => {
                self.e("xor eax, eax");
                Ok(Ty::Nil)
            }
            ExprKind::Float(f) => {
                if matches!(want, Some(Ty::Float { bits: 32 })) {
                    let b = (*f as f32).to_bits();
                    if b == 0 {
                        self.e("xor eax, eax");
                    } else {
                        self.e(format!("mov eax, {}", b));
                    }
                    Ok(Ty::Float { bits: 32 })
                } else {
                    let b = f.to_bits();
                    if b == 0 {
                        self.e("xor eax, eax");
                    } else {
                        self.e(format!("mov rax, {}", b));
                    }
                    Ok(Ty::Float { bits: 64 })
                }
            }
            ExprKind::Text(parts) => self.text(parts),
            ExprKind::Ident(name) => self.ident(name, want),
            ExprKind::List(items) => {
                let mut el_ty = match want {
                    Some(Ty::List(t)) => Some((**t).clone()),
                    _ => None,
                };
                let lt = self.temp();
                let mut created = false;
                for it in items {
                    let t = self.value(it, el_ty.as_ref())?;
                    match &el_ty {
                        Some(w) => self.check(w, &t, "list element")?,
                        None => el_ty = Some(t),
                    }
                    let first = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", first));
                    if !created {
                        let el = el_ty.clone().unwrap();
                        self.new_list(&el);
                        self.e(format!("mov qword ptr [rbp - {}], rax", lt));
                        created = true;
                    }
                    self.e(format!("mov rdx, qword ptr [rbp - {}]", first));
                    self.e(format!("mov rcx, qword ptr [rbp - {}]", lt));
                    self.e("call rt_list_push");
                    self.free(first);
                }
                let Some(el) = el_ty else {
                    return self.err("cannot infer the element type of an empty list; add a type annotation");
                };
                if !created {
                    self.new_list(&el);
                    self.e(format!("mov qword ptr [rbp - {}], rax", lt));
                }
                self.e(format!("mov rax, qword ptr [rbp - {}]", lt));
                self.free(lt);
                Ok(Ty::List(Box::new(el)))
            }
            ExprKind::Map(pairs) => {
                let (mut kt, mut vt) = match want {
                    Some(Ty::Map(k, v)) => (Some((**k).clone()), Some((**v).clone())),
                    _ => (None, None),
                };
                let mt = self.temp();
                let mut created = false;
                if let (Some(k), Some(v)) = (&kt, &vt) {
                    let t = self.map_type(k.clone(), v.clone())?;
                    self.new_map(&t)?;
                    self.e(format!("mov qword ptr [rbp - {}], rax", mt));
                    created = true;
                }
                for (ke, ve) in pairs {
                    let kty = self.value(ke, kt.as_ref())?;
                    let ks = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", ks));
                    let vty = self.value(ve, vt.as_ref())?;
                    match (&kt, &vt) {
                        (Some(k), Some(v)) => {
                            self.check(k, &kty, "map key")?;
                            self.check(v, &vty, "map value")?;
                        }
                        _ => {
                            kt = Some(kty);
                            vt = Some(vty);
                        }
                    }
                    if !created {
                        let t = self.map_type(kt.clone().unwrap(), vt.clone().unwrap())?;
                        let vs = self.temp();
                        self.e(format!("mov qword ptr [rbp - {}], rax", vs));
                        self.new_map(&t)?;
                        self.e(format!("mov qword ptr [rbp - {}], rax", mt));
                        self.e(format!("mov rax, qword ptr [rbp - {}]", vs));
                        self.free(vs);
                        created = true;
                    }
                    self.e("mov r8, rax");
                    self.e(format!("mov rdx, qword ptr [rbp - {}]", ks));
                    self.e(format!("mov rcx, qword ptr [rbp - {}]", mt));
                    self.e("call rt_map_set");
                    if kt.as_ref().is_some_and(|k| self.is_rc(k)) {
                        self.release_slot(ks);
                    }
                    self.free(ks);
                }
                let (Some(k), Some(v)) = (kt, vt) else {
                    return self.err("cannot infer the key and value types of an empty map; add a type annotation");
                };
                self.e(format!("mov rax, qword ptr [rbp - {}]", mt));
                self.free(mt);
                Ok(Ty::Map(Box::new(k), Box::new(v)))
            }
            ExprKind::Record { path, fields } => {
                let r = match self.generic_literal(path, want)? {
                    Some(r) => r,
                    None => self.record_name(path, &self.cur_mod.clone())?,
                };
                self.check_constructible(&r)?;
                self.record_literal(&r, fields)?;
                Ok(Ty::Rec(r))
            }
            ExprKind::Lambda { params, body } => self.lambda(params, body, want),
            ExprKind::If { branches, else_body } => self.if_chain(branches, else_body.as_ref(), want, true),
            ExprKind::Match { scrutinee, arms } => self.match_expr(scrutinee, arms, want, true),
            ExprKind::Shared(inner) => self.shared_expr(inner, want),
            ExprKind::SpawnThread(call) => self.spawn_thread(call, e.line),
            ExprKind::New(inner) => {
                let ExprKind::Record { path, fields } = &inner.kind else {
                    return self.err("'new' needs a record literal such as `new Node{ ... }`");
                };
                let want_rec = want.map(rc_view);
                let r = match self.generic_literal(path, want_rec.as_ref())? {
                    Some(r) => r,
                    None => self.record_name(path, &self.cur_mod.clone())?,
                };
                self.check_constructible(&r)?;
                self.tp.rc_recs.insert(r.clone());
                self.record_alloc(&r, fields, true)?;
                Ok(Ty::Rc(Box::new(Ty::Rec(r))))
            }
            ExprKind::Block { name, .. } => self.err(format!("block '{}' was not expanded", name)),
            ExprKind::Sql { dialect, raw } => self.sql_block(dialect, raw, e.line, want),
            ExprKind::Propagate(inner) => self.try_expr(inner),
            ExprKind::Await(inner) => {
                if let ExprKind::Call { callee, args, .. } = &inner.kind {
                    if let ExprKind::Ident(n) = &callee.kind {
                        if (n == "all" || n == "race")
                            && args.len() == 1
                            && self.lookup(n).is_none()
                            && !self.tp.funcs.contains_key(&(self.cur_mod.clone(), n.clone()))
                        {
                            return self.await_combinator(n, &args[0].value);
                        }
                    }
                }
                let t = self.expr(inner, None)?;
                let Ty::Future(v) = t.clone() else {
                    return self.err(format!("`await` needs a Future, found {}", t.show()));
                };
                let s = self.temp();
                self.e(format!("mov qword ptr [rbp - {}], rax", s));
                let sig = self.future_fn("wait")?;
                self.emit_call(&[s], &format!("call {}", sig.sym), None);
                self.free(s);
                self.norm(&v);
                Ok(*v)
            }
            ExprKind::Unary { op, expr: inner } => match *op {
                "not" => {
                    self.expr(inner, Some(&Ty::Bool))?;
                    self.e("test rax, rax");
                    self.e("sete al");
                    self.e("movzx eax, al");
                    Ok(Ty::Bool)
                }
                "-" => {
                    if let ExprKind::Int(n) = &inner.kind {
                        let t = match want {
                            Some(t) if t.is_int() => t.clone(),
                            _ => Ty::int(),
                        };
                        self.lit_check(*n, true, &t)?;
                        self.e(format!("mov rax, {}", imm64((*n as i64).wrapping_neg())));
                        return Ok(t);
                    }
                    let t = self.expr(inner, want)?;
                    if let Ty::Float { bits } = t {
                        if bits == 32 {
                            self.e("xor eax, 80000000h");
                        } else {
                            self.e("mov rcx, 8000000000000000h");
                            self.e("xor rax, rcx");
                        }
                        return Ok(t);
                    }
                    if !t.is_int() {
                        return self.err(format!("unary '-' needs a number, found {}", t.show()));
                    }
                    self.e("neg rax");
                    self.norm(&t);
                    Ok(t)
                }
                "bitnot" => {
                    let t = self.expr(inner, want)?;
                    self.e("not rax");
                    self.norm(&t);
                    Ok(t)
                }
                "&" => self.address_of(inner),
                "*" => self.deref(inner),
                _ => self.err(format!("unknown operator '{}'", op)),
            },
            ExprKind::Deref(inner) => self.deref(inner),
            ExprKind::Binary { op, lhs, rhs } => self.binary(op, lhs, rhs, want),
            ExprKind::Field { expr: obj, name, safe } => self.field(obj, name, *safe, want),
            ExprKind::Index { expr: obj, index } => {
                let (ot, own) = self.operand(obj, None)?;
                let t = self.temp();
                self.e(format!("mov qword ptr [rbp - {}], rax", t));
                let key_want = if let Ty::Map(k, _) = &ot { Some((**k).clone()) } else { None };
                let ity = self.expr(index, key_want.as_ref())?;
                let res = match &ot {
                    Ty::Map(k, v) => {
                        self.check(k, &ity, "map key")?;
                        let ks = self.temp();
                        self.e(format!("mov qword ptr [rbp - {}], rax", ks));
                        self.e("mov rdx, rax");
                        self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
                        self.at("r8");
                        self.e("call rt_map_get");
                        self.retain_if(v);
                        if self.is_rc(k) {
                            self.release_slot(ks);
                        }
                        self.free(ks);
                        (**v).clone()
                    }
                    Ty::List(el) => {
                        self.e("mov rdx, rax");
                        self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
                        self.at("r9");
                        self.e("call rt_list_get");
                        self.retain_if(el);
                        (**el).clone()
                    }
                    Ty::Text => return self.err(TEXT_INDEX_MSG),
                    Ty::Ptr(el) => {
                        self.require_unsafe("reading through a raw pointer")?;
                        let el = (**el).clone();
                        self.e(format!("imul rax, rax, {}", el.size()));
                        self.e(format!("add rax, qword ptr [rbp - {}]", t));
                        self.load_mem(&el, "rax");
                        self.retain_if(&el);
                        el
                    }
                    _ => return self.err(format!("cannot index {}", ot.show())),
                };
                if own {
                    self.release_slot(t);
                }
                self.free(t);
                Ok(res)
            }
            ExprKind::Call { callee, args, targs } => {
                // a call the checker resolved (a function, or an enum variant used as a
                // constructor) is emitted to its resolved target without looking the callee up again
                if targs.is_empty() {
                    if let Some(t) = self.check_expr(e, want) {
                        if let Some(r) = self.emit_resolved_call(&t, callee, args) {
                            return r;
                        }
                    }
                }
                self.call(callee, args, targs, want)
            }
        }
    }

    pub(super) fn text(&mut self, parts: &[TextPart]) -> R<Ty> {
        if parts.iter().all(|p| matches!(p, TextPart::Lit(_))) {
            let s: String = parts.iter().map(|p| if let TextPart::Lit(l) = p { l.as_str() } else { "" }).collect();
            let l = self.string_lit(&s);
            self.e(format!("lea rax, {}", l));
            return Ok(Ty::Text);
        }
        let acc = self.temp();
        let part = self.temp();
        let nw = self.temp();
        for (i, p) in parts.iter().enumerate() {
            match p {
                TextPart::Lit(l) => {
                    let lb = self.string_lit(l);
                    self.e(format!("lea rax, {}", lb));
                }
                TextPart::Expr(x) => {
                    let t = self.expr(x, None)?;
                    self.to_text(&t)?;
                }
            }
            if i == 0 {
                self.e(format!("mov qword ptr [rbp - {}], rax", acc));
            } else {
                self.e(format!("mov qword ptr [rbp - {}], rax", part));
                self.e(format!("mov rcx, qword ptr [rbp - {}]", acc));
                self.e("mov rdx, rax");
                self.e("call rt_concat");
                self.e(format!("mov qword ptr [rbp - {}], rax", nw));
                self.release_slot(acc);
                self.release_slot(part);
                self.e(format!("mov rax, qword ptr [rbp - {}]", nw));
                self.e(format!("mov qword ptr [rbp - {}], rax", acc));
            }
        }
        self.e(format!("mov rax, qword ptr [rbp - {}]", acc));
        self.free(nw);
        self.free(part);
        self.free(acc);
        Ok(Ty::Text)
    }

    pub(super) fn ident(&mut self, name: &str, want: Option<&Ty>) -> R<Ty> {
        if let Some(l) = self.lookup(name) {
            self.load_local("rax", &l);
            self.retain_if(&l.ty);
            return Ok(l.ty);
        }
        let m = self.cur_mod.clone();
        self.member(&m, name, want)
    }

    /// A global, constant, enum variant or function of module `m`, used as a value.
    pub(super) fn member(&mut self, m: &str, name: &str, want: Option<&Ty>) -> R<Ty> {
        let key = (m.to_string(), name.to_string());
        if let Some(g) = self.tp.globals.get(&key) {
            self.check_member_vis(m, name, g.public)?;
            let ty = g.ty.clone();
            match g.konst {
                Some(k) => {
                    self.e(format!("mov rax, {}", k));
                    return Ok(match want {
                        Some(t) if t.is_int() => t.clone(),
                        _ => ty,
                    });
                }
                None => {
                    let s = g.sym.clone();
                    self.e(format!("mov rax, qword ptr {}", s));
                    self.retain_if(&ty);
                    return Ok(ty);
                }
            }
        }
        if let Some(sig) = self.tp.funcs.get(&key).cloned() {
            self.check_member_vis(m, name, sig.public)?;
            if sig.is_extern {
                return self.err(format!("extern function '{}' cannot be used as a value", name));
            }
            let ext = matches!(want, Some(Ty::Func { ext: true, .. }));
            if ext && self.await_syms.contains(&sig.sym) {
                return self.err("`await` cannot be used in a function called from C");
            }
            if ext && self.target == Target::Linux {
                // C calls with the System V convention: the thunk moves the arguments to ours
                let kind = |t: &Ty| match t {
                    Ty::Float { bits: 32 } => 1u8,
                    Ty::Float { .. } => 2,
                    _ => 0,
                };
                let kinds: Vec<u8> = sig.params.iter().map(|p| kind(&p.ty)).collect();
                self.callbacks.insert(sig.sym.clone(), (kinds, kind(&sig.ret)));
                self.e(format!("lea rax, CB_{}", sig.sym));
            } else if ext {
                self.e(format!("lea rax, {}", sig.sym));
            } else {
                self.fn_closures.insert(sig.sym.clone());
                self.e(format!("lea rax, C_{}", sig.sym));
            }
            return Ok(sig.func_ty(ext));
        }
        if let Some((q, idx)) = self.tp.variants.get(&key).cloned() {
            self.check_member_vis(m, name, self.tp.pub_types.contains(&q))?;
            return self.enum_construct(&q, idx, &[]);
        }
        if self.imports.get(&self.cur_mod).is_some_and(|i| i.contains(name)) {
            return self.err(format!("module '{}' cannot be used as a value", name));
        }
        if m != self.cur_mod {
            return self.err(format!("module '{}' has no member '{}'", m, name));
        }
        self.err(format!("undefined identifier '{}'", name))
    }

    /// Builds a value of enum `q`, variant `idx`, from call arguments.
    pub(super) fn enum_construct(&mut self, q: &str, idx: usize, args: &[Arg]) -> R<Ty> {
        let info = self.tp.enums[q].clone();
        let (vname, tys) = info.variants[idx].clone();
        if args.len() != tys.len() {
            return self.err(format!("'{}' takes {} value(s), found {}", vname, tys.len(), args.len()));
        }
        if !info.payload {
            self.e(format!("mov eax, {}", idx));
        } else if tys.is_empty() {
            self.enum_statics.insert((q.to_string(), idx));
            self.e(format!("lea rax, E_{}_{}", sym_of(q), idx));
        } else {
            let drop = self.enum_drop(q);
            self.alloc(info.size, &drop);
            let t = self.temp();
            self.e(format!("mov qword ptr [rbp - {}], rax", t));
            self.e(format!("mov qword ptr [rax], {}", idx));
            for (i, (a, ty)) in args.iter().zip(&tys).enumerate() {
                let vt = self.value(&a.value, Some(ty))?;
                self.check(ty, &vt, &format!("value {} of '{}'", i + 1, vname))?;
                self.norm(ty);
                self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
                self.e(format!("mov qword ptr [rcx + {}], rax", 8 + 8 * i));
            }
            self.e(format!("mov rax, qword ptr [rbp - {}]", t));
            self.free(t);
        }
        Ok(Ty::Enum(q.to_string()))
    }

    pub(super) fn address_of(&mut self, inner: &Expr) -> R<Ty> {
        match &inner.kind {
            ExprKind::Ident(name) => {
                if let Some(l) = self.lookup(name) {
                    if l.is_ref && self.fs.unsafe_depth == 0 {
                        return self.ref_escape(name);
                    }
                    if let Ty::Rec(_) = &l.ty {
                        self.e(format!("mov rax, qword ptr [rbp - {}]", l.off));
                    } else {
                        if l.ty.size() < 8 {
                            return self.err(format!("'&{}': address of a {} variable is not supported; use a record field", name, l.ty.show()));
                        }
                        self.e(format!("lea rax, [rbp - {}]", l.off));
                    }
                    return Ok(Ty::Ptr(Box::new(l.ty)));
                }
                let key = (self.cur_mod.clone(), name.clone());
                if let Some(g) = self.tp.globals.get(&key) {
                    if g.konst.is_none() {
                        let (s, t) = (g.sym.clone(), g.ty.clone());
                        if let Ty::Rec(_) = &t {
                            self.e(format!("mov rax, qword ptr {}", s));
                        } else {
                            self.e(format!("lea rax, {}", s));
                        }
                        return Ok(Ty::Ptr(Box::new(t)));
                    }
                }
                self.err(format!("cannot take the address of '{}'", name))
            }
            ExprKind::Field { expr: obj, name, .. } => {
                let (ot, _) = self.operand(obj, None)?;
                let r = match &ot {
                    Ty::Rec(r) => r.clone(),
                    Ty::Ptr(p) if matches!(&**p, Ty::Rec(_)) => match &**p {
                        Ty::Rec(r) => r.clone(),
                        _ => unreachable!(),
                    },
                    _ => return self.err("'&' needs a variable or record field"),
                };
                let Some((fty, off)) = self.fld(&r, name)? else { return self.err(format!("{} has no field '{}'", r, name)) };
                if let Ty::Rec(_) = &fty {
                    self.e(format!("mov rax, qword ptr [rax + {}]", off));
                } else {
                    self.e(format!("lea rax, [rax + {}]", off));
                }
                Ok(Ty::Ptr(Box::new(fty)))
            }
            _ => self.err("'&' needs a variable or record field"),
        }
    }

    pub(super) fn deref(&mut self, inner: &Expr) -> R<Ty> {
        let (t, _) = self.operand(inner, None)?;
        let Ty::Ptr(el) = t else { return self.err(format!("'^' needs a pointer, found {}", t.show())) };
        if let Ty::Rec(_) = &*el {
            self.retain();
            return Ok(*el);
        }
        self.require_unsafe("dereferencing a raw pointer")?;
        self.load_mem(&el, "rax");
        self.retain_if(&el);
        Ok(*el)
    }

    /// A number literal (or arithmetic on literals) on the left adapts to an integer type on the
    /// right (`0 - x` with `x: u32`): that type, when it applies.
    pub(super) fn literal_side_type(&mut self, op: &str, lhs: &Expr, rhs: &Expr, want: Option<&Ty>) -> Option<Ty> {
        let is_cmp = matches!(op, "==" | "!=" | "<" | "<=" | ">" | ">=");
        if is_const_int_expr(lhs) && !is_const_int_expr(rhs) && op != "<<" && op != ">>" && (is_cmp || !want.is_some_and(|w| w.is_int())) {
            // the checker's type for the right side; a form it does not cover is typed by
            // compiling it once and undoing that
            let r = match self.check_expr(rhs, None) {
                Some(x) => Ok(x.ty),
                None => {
                    let snap = self.snapshot();
                    let r = self.expr(rhs, None);
                    self.restore(snap);
                    r
                }
            };
            if let Ok(t) = r {
                if t.is_int() {
                    return Some(t);
                }
            }
        }
        None
    }

    pub(super) fn binary(&mut self, op: &str, lhs: &Expr, rhs: &Expr, want: Option<&Ty>) -> R<Ty> {
        match op {
            "and" | "or" => {
                let (skip, end) = (self.lbl("sc"), self.lbl("scend"));
                self.expr(lhs, Some(&Ty::Bool))?;
                self.e("test rax, rax");
                self.e(format!("{} {}", if op == "and" { "jz" } else { "jnz" }, skip));
                self.expr(rhs, Some(&Ty::Bool))?;
                self.e("test rax, rax");
                self.e(format!("{} {}", if op == "and" { "jz" } else { "jnz" }, skip));
                self.e(format!("mov eax, {}", if op == "and" { 1 } else { 0 }));
                self.e(format!("jmp {}", end));
                self.label(&skip);
                self.e(format!("mov eax, {}", if op == "and" { 0 } else { 1 }));
                self.label(&end);
                return Ok(Ty::Bool);
            }
            "??" => return self.coalesce(lhs, rhs, want),
            ".." => return self.err("ranges are only supported in 'for' loops"),
            _ => {}
        }
        let is_cmp = matches!(op, "==" | "!=" | "<" | "<=" | ">" | ">=");
        // A plain local on the left cannot change while the right side runs (an expression cannot
        // assign), so the right side goes first and the local is read last: no temporary.
        if self.opt {
            if let ExprKind::Ident(n) = &lhs.kind {
                if let Some(l) = self.lookup(n) {
                    if !l.captured && !self.is_rc(&l.ty) && !self.rhs_is_simple(rhs) {
                        let lt = l.ty.clone();
                        let (rt, rown) = self.operand(rhs, Some(&lt))?;
                        let owned = if rown && self.is_rc(&rt) {
                            let t = self.temp();
                            self.e(format!("mov qword ptr [rbp - {}], rax", t));
                            Some(t)
                        } else {
                            None
                        };
                        self.e("mov rcx, rax");
                        self.load_local("rax", &l);
                        let res = self.core_for(rhs, op, &lt, &rt, is_cmp)?;
                        if let Some(t) = owned {
                            self.release_slot(t);
                            self.free(t);
                        }
                        return Ok(res);
                    }
                }
            }
        }
        let lit_ty = self.literal_side_type(op, lhs, rhs, want);
        let (lt, lown) = self.operand(lhs, if lit_ty.is_some() { lit_ty.as_ref() } else if is_cmp { None } else { want })?;
        // a literal or a plain local on the right needs no temporary: load it straight into rcx
        if self.opt && !(lown && self.is_rc(&lt)) {
            if let Some(rt) = self.simple_rhs(rhs, &lt) {
                return self.core_for(rhs, op, &lt, &rt, is_cmp);
            }
        }
        let t1 = self.temp();
        self.e(format!("mov qword ptr [rbp - {}], rax", t1));
        let (rt, rown) = self.operand(rhs, Some(&lt))?;
        let t2 = self.temp();
        self.e(format!("mov qword ptr [rbp - {}], rax", t2));
        self.e("mov rcx, rax");
        self.e(format!("mov rax, qword ptr [rbp - {}]", t1));
        let res = self.core_for(rhs, op, &lt, &rt, is_cmp)?;
        if lown && self.is_rc(&lt) {
            self.release_slot(t1);
        }
        if rown && self.is_rc(&rt) {
            self.release_slot(t2);
        }
        self.free(t2);
        self.free(t1);
        Ok(res)
    }

    /// Float arithmetic and comparison; rax = lhs bits, rcx = rhs bits.
    pub(super) fn float_binary(&mut self, op: &str, lt: &Ty, rt: &Ty, is_cmp: bool) -> R<Ty> {
        if lt != rt {
            return if is_cmp {
                self.err(format!("cannot compare {} with {}", lt.show(), rt.show()))
            } else {
                self.err(format!("operator '{}' is not defined for {} and {}; convert one side first", op, lt.show(), rt.show()))
            };
        }
        let Ty::Float { bits } = lt else { unreachable!() };
        let d = *bits == 64;
        let (mv, sfx) = if d { ("movq", "sd") } else { ("movd", "ss") };
        let (a, b) = if d { ("rax", "rcx") } else { ("eax", "ecx") };
        self.e(format!("{} xmm0, {}", mv, a));
        self.e(format!("{} xmm1, {}", mv, b));
        if is_cmp {
            let uc = format!("ucomi{}", sfx);
            match op {
                "==" | "!=" => {
                    self.e(format!("{} xmm0, xmm1", uc));
                    if op == "==" {
                        self.e("sete al");
                        self.e("setnp cl");
                        self.e("and al, cl");
                    } else {
                        self.e("setne al");
                        self.e("setp cl");
                        self.e("or al, cl");
                    }
                }
                ">" | ">=" => {
                    self.e(format!("{} xmm0, xmm1", uc));
                    self.e(format!("set{} al", if op == ">" { "a" } else { "ae" }));
                }
                _ => {
                    // a < b is b > a; unordered sets CF, so `above` stays false
                    self.e(format!("{} xmm1, xmm0", uc));
                    self.e(format!("set{} al", if op == "<" { "a" } else { "ae" }));
                }
            }
            self.e("movzx eax, al");
            return Ok(Ty::Bool);
        }
        let ins = match op {
            "+" => "add",
            "-" => "sub",
            "*" => "mul",
            "/" => "div",
            "%" => return self.err("'%' is not defined for floats"),
            _ => return self.err(format!("operator '{}' is not defined for {}", op, lt.show())),
        };
        self.e(format!("{}{} xmm0, xmm1", ins, sfx));
        self.e(format!("{} {}, xmm0", mv, a));
        Ok(lt.clone())
    }

    /// When `e` is a number literal or a plain local variable, loads it into rcx (as `operand`
    /// would leave it in rax) and returns its type. Nothing else is evaluated, so the left
    /// operand waiting in rax cannot be disturbed.
    pub(super) fn simple_rhs(&mut self, e: &Expr, lt: &Ty) -> Option<Ty> {
        match &e.kind {
            ExprKind::Int(n) => {
                let t = if lt.is_int() { lt.clone() } else { Ty::int() };
                if !lit_fits(*n, false, &t) {
                    return None; // the general path reports the range error
                }
                self.e(format!("mov rcx, {}", imm64(*n as i64)));
                Some(t)
            }
            ExprKind::Char(c) => {
                self.e(format!("mov ecx, {}", *c as u32));
                Some(Ty::Char)
            }
            ExprKind::Float(f) => {
                if matches!(lt, Ty::Float { bits: 32 }) {
                    self.e(format!("mov ecx, {}", imm64((*f as f32).to_bits() as i64)));
                    Some(Ty::Float { bits: 32 })
                } else {
                    self.e(format!("mov rcx, {}", imm64(f.to_bits() as i64)));
                    Some(Ty::Float { bits: 64 })
                }
            }
            ExprKind::Ident(n) => {
                let l = self.lookup(n)?;
                if l.captured {
                    return None;
                }
                self.load_local("rcx", &l);
                Some(l.ty)
            }
            _ => None,
        }
    }

    /// rax = lhs, rcx = rhs. Leaves the result in rax.
    /// `binary_core` for the operands of `lhs op rhs`: a literal divisor that is not zero needs
    /// no zero check.
    pub(super) fn core_for(&mut self, rhs: &Expr, op: &str, lt: &Ty, rt: &Ty, is_cmp: bool) -> R<Ty> {
        self.div_nonzero = self.opt && matches!(&rhs.kind, ExprKind::Int(n) if *n != 0);
        let r = self.binary_core(op, lt, rt, is_cmp);
        self.div_nonzero = false;
        r
    }

    pub(super) fn binary_core(&mut self, op: &str, lt: &Ty, rt: &Ty, is_cmp: bool) -> R<Ty> {
        if op == "+" && ((*lt == Ty::Text && *rt == Ty::Char) || (*lt == Ty::Char && *rt == Ty::Text)) {
            // a char joins a text as its UTF-8 bytes
            let (a, b, c) = (self.temp(), self.temp(), self.temp());
            self.e(format!("mov qword ptr [rbp - {}], rax", a));
            self.e(format!("mov qword ptr [rbp - {}], rcx", b));
            let (cs, other) = if *lt == Ty::Char { (a, b) } else { (b, a) };
            self.e(format!("mov rcx, qword ptr [rbp - {}]", cs));
            self.e("call rt_text_from_char");
            self.e(format!("mov qword ptr [rbp - {}], rax", cs));
            self.e(format!("mov rcx, qword ptr [rbp - {}]", a));
            self.e(format!("mov rdx, qword ptr [rbp - {}]", b));
            self.e("call rt_concat");
            self.e(format!("mov qword ptr [rbp - {}], rax", c));
            let _ = other;
            self.e(format!("mov rcx, qword ptr [rbp - {}]", cs));
            self.e("call rt_release");
            self.e(format!("mov rax, qword ptr [rbp - {}]", c));
            self.free(c);
            self.free(b);
            self.free(a);
            return Ok(Ty::Text);
        }
        if *lt == Ty::Text && *rt == Ty::Text {
            match op {
                "+" => {
                    self.e("mov rdx, rcx");
                    self.e("mov rcx, rax");
                    self.e("call rt_concat");
                    return Ok(Ty::Text);
                }
                "==" | "!=" => {
                    self.e("mov rdx, rcx");
                    self.e("mov rcx, rax");
                    self.e("call rt_text_eq");
                    if op == "!=" {
                        self.e("xor eax, 1");
                    }
                    return Ok(Ty::Bool);
                }
                "<" | "<=" | ">" | ">=" => {
                    self.emit_std(Cap::Ord, &Ty::Text)?;
                    self.e("test rax, rax");
                    let cc = match op {
                        "<" => "l",
                        "<=" => "le",
                        ">" => "g",
                        _ => "ge",
                    };
                    self.e(format!("set{} al", cc));
                    self.e("movzx eax, al");
                    return Ok(Ty::Bool);
                }
                _ => return self.err(format!("operator '{}' is not defined for text", op)),
            }
        }
        if is_cmp {
            if let Some(t) = self.std_operator(op, lt, rt)? {
                return Ok(t);
            }
        }
        if matches!(lt, Ty::Param { .. }) || matches!(rt, Ty::Param { .. }) {
            let (a, b) = (lt.show(), rt.show());
            return self.err(format!(
                "operator '{}' is not available for {}: a type parameter only has the methods of its bound{}",
                op,
                if matches!(lt, Ty::Param { .. }) { a } else { b },
                ""
            ));
        }
        if lt.is_float() || rt.is_float() {
            return self.float_binary(op, lt, rt, is_cmp);
        }
        if is_cmp {
            if (matches!(lt, Ty::Rc(_) | Ty::Weak(_)) || matches!(rt, Ty::Rc(_) | Ty::Weak(_))) && !matches!(op, "==" | "!=") {
                return self.err(format!("operator '{}' is not defined for {}: references only compare with == and !=", op, if matches!(lt, Ty::Rc(_) | Ty::Weak(_)) { lt.show() } else { rt.show() }));
            }
            if !(compatible(lt, rt) || compatible(rt, lt)) {
                return self.err(format!("cannot compare {} with {}", lt.show(), rt.show()));
            }
            if let (Ty::Enum(a), Ty::Enum(_)) = (lt, rt) {
                if self.tp.enum_payload.contains(a) {
                    return self.err("enums with data cannot be compared with '=='; use `match`");
                }
            }
            if (matches!(lt, Ty::Result(_) | Ty::Err) || matches!(rt, Ty::Result(_) | Ty::Err)) && *lt != Ty::Nil && *rt != Ty::Nil {
                return self.err("a result cannot be compared; use `match`, `try` or `??`");
            }
            let unsigned = cmp_unsigned(lt, rt);
            let cc = match (op, unsigned) {
                ("==", _) => "e",
                ("!=", _) => "ne",
                ("<", false) => "l",
                ("<=", false) => "le",
                (">", false) => "g",
                (">=", false) => "ge",
                ("<", true) => "b",
                ("<=", true) => "be",
                (">", true) => "a",
                (">=", true) => "ae",
                _ => unreachable!(),
            };
            self.e("cmp rax, rcx");
            self.e(format!("set{} al", cc));
            self.e("movzx eax, al");
            return Ok(Ty::Bool);
        }
        let ty = match (lt, rt) {
            // a shift keeps the type of the value shifted; the count may be any integer
            (Ty::Int { .. }, Ty::Int { .. }) if op == "<<" || op == ">>" => lt.clone(),
            (Ty::Int { .. }, Ty::Int { .. }) => match wider(lt, rt) {
                Some(t) => t,
                None => {
                    return self.err(format!(
                        "operator '{}' is not defined for {} and {}; convert one side first (for example {}(x))",
                        op,
                        lt.show(),
                        rt.show(),
                        lt.show()
                    ))
                }
            },
            (Ty::Ptr(_), Ty::Int { .. }) if op == "+" || op == "-" => lt.clone(),
            _ => return self.err(format!("operator '{}' is not defined for {} and {}", op, lt.show(), rt.show())),
        };
        let unsigned = ty.is_unsigned();
        match op {
            "+" => {
                if let Ty::Ptr(el) = &ty {
                    self.e(format!("imul rcx, rcx, {}", el.size()));
                }
                self.e("add rax, rcx")
            }
            "-" => {
                if let Ty::Ptr(el) = &ty {
                    self.e(format!("imul rcx, rcx, {}", el.size()));
                }
                self.e("sub rax, rcx")
            }
            "*" => self.e("imul rax, rcx"),
            "/" | "%" => {
                if self.target != Target::Bare && !self.div_nonzero {
                    self.at("rdx");
                    self.e("test rcx, rcx");
                    self.e("jz rt_div_zero");
                }
                if unsigned {
                    self.e("xor edx, edx");
                    self.e("div rcx");
                } else if self.div_nonzero {
                    self.e("cqo");
                    self.e("idiv rcx");
                } else {
                    // MIN / -1 wraps to MIN (and MIN % -1 is 0) instead of faulting
                    let (norm, done) = (self.lbl("divnorm"), self.lbl("divdone"));
                    self.e("cmp rcx, -1");
                    self.e(format!("jne {}", norm));
                    if op == "/" {
                        self.e("neg rax");
                    } else {
                        self.e("xor eax, eax");
                    }
                    self.e(format!("jmp {}", done));
                    self.label(&norm);
                    self.e("cqo");
                    self.e("idiv rcx");
                    if op == "%" {
                        self.e("mov rax, rdx");
                    }
                    self.label(&done);
                    self.norm(&ty);
                    return Ok(ty);
                }
                if op == "%" {
                    self.e("mov rax, rdx");
                }
            }
            "bitand" => self.e("and rax, rcx"),
            "bitor" => self.e("or rax, rcx"),
            "bitxor" => self.e("xor rax, rcx"),
            "<<" => self.e("shl rax, cl"),
            ">>" => self.e(if unsigned { "shr rax, cl" } else { "sar rax, cl" }),
            _ => return self.err(format!("unknown operator '{}'", op)),
        }
        self.norm(&ty);
        Ok(ty)
    }

    /// `a ?? b`: a default for an optional value or for a failed call.
    pub(super) fn coalesce(&mut self, lhs: &Expr, rhs: &Expr, want: Option<&Ty>) -> R<Ty> {
        let (lt, own) = self.operand(lhs, want)?;
        if let Ty::Result(inner) = &lt {
            let inner = (**inner).clone();
            if inner == Ty::Void {
                return self.err("'??' needs a call that returns a value");
            }
            let (other, end) = (self.lbl("coalt"), self.lbl("coalend"));
            let s = self.temp();
            self.e(format!("mov qword ptr [rbp - {}], rax", s));
            self.e("mov rax, qword ptr [rax]");
            self.e("test rax, rax");
            self.e(format!("jnz {}", other));
            self.e(format!("mov rax, qword ptr [rbp - {}]", s));
            self.e("mov rax, qword ptr [rax + 8]");
            self.retain_if(&inner);
            if own {
                self.release_slot(s);
            }
            self.e(format!("jmp {}", end));
            self.label(&other);
            if own {
                self.release_slot(s);
            }
            let rt = self.value(rhs, Some(&inner))?;
            self.check(&inner, &rt, "'??'")?;
            self.label(&end);
            self.free(s);
            return Ok(inner);
        }
        if !lt.nullable() {
            return self.err(format!("'??' needs an optional value, found {}", lt.show()));
        }
        if !own {
            self.retain_if(&lt);
        }
        let end = self.lbl("coal");
        self.e("test rax, rax");
        self.e(format!("jnz {}", end));
        let rt = self.value(rhs, Some(&lt))?;
        self.check(&lt, &rt, "'??'")?;
        self.label(&end);
        Ok(lt)
    }

    pub(super) fn field(&mut self, obj: &Expr, name: &str, safe: bool, want: Option<&Ty>) -> R<Ty> {
        if let ExprKind::Ident(m) = &obj.kind {
            if self.is_module(m) {
                if m == "c" {
                    return self.err(format!("'c.{}' must be called", name));
                }
                // `want` lets a function of the module be passed where the system wants a callback
                let mk = self.modkey(&self.cur_mod, m);
                return self.member(&mk, name, want);
            }
            if let Some(q) = self.enum_type_named(m) {
                let Some(idx) = self.tp.enums[&q].variants.iter().position(|v| v.0 == name) else {
                    return self.err(format!("enum {} has no variant '{}'", m, name));
                };
                return self.enum_construct(&q, idx, &[]);
            }
        }
        let (ot, own) = self.operand(obj, None)?;
        let s = self.temp();
        self.e(format!("mov qword ptr [rbp - {}], rax", s));
        let end = self.lbl("safe");
        if safe {
            self.e("test rax, rax");
            self.e(format!("jz {}", end));
        }
        let ot_rc = ot.clone();
        let ot = rc_view(&ot);
        let res = match &ot {
            Ty::Rec(r) => self.rec_field(&r.clone(), name, own)?,
            Ty::Ptr(p) if matches!(&**p, Ty::Rec(_)) => {
                let Ty::Rec(r) = &**p else { unreachable!() };
                let f = self.fld(r, name)?;
                match f {
                    Some((fty, off)) => {
                        self.load_mem(&fty, &format!("rax + {}", off));
                        self.retain_if(&fty);
                        fty
                    }
                    None => return self.err(format!("{} has no field '{}'", r, name)),
                }
            }
            Ty::List(_) | Ty::Text if name == "len" => {
                self.e("mov rax, qword ptr [rax]");
                Ty::int()
            }
            Ty::Map(..) if name == "len" => {
                self.e("mov rax, qword ptr [rax + 48]");
                Ty::int()
            }
            Ty::Char if name == "code" => Ty::int(),
            Ty::Err if name == "message" => {
                self.e("mov rax, qword ptr [rax]");
                self.retain();
                Ty::Text
            }
            Ty::Err if name == "kind" => {
                self.e("mov rax, qword ptr [rax + 8]");
                self.retain();
                Ty::Kind
            }
            Ty::Err if name == "cause" => {
                self.e("mov rax, qword ptr [rax + 16]");
                self.retain();
                Ty::Err
            }
            Ty::Err if name == "full_message" => {
                self.e("mov rcx, rax");
                self.e("call rt_error_full");
                Ty::Text
            }
            Ty::Kind if name == "name" => {
                self.retain();
                Ty::Text
            }
            _ => return self.err(format!("{} has no field '{}'", ot.show(), name)),
        };
        // property getters consume their receiver themselves
        let consumed = matches!(&ot, Ty::Rec(r) if self.field_of(r, name).is_none());
        let ot = ot_rc;
        if own && self.is_rc(&ot) && !consumed {
            let keep = self.temp();
            self.e(format!("mov qword ptr [rbp - {}], rax", keep));
            self.release_slot(s);
            self.e(format!("mov rax, qword ptr [rbp - {}]", keep));
            self.free(keep);
        }
        if safe {
            self.label(&end);
        }
        self.free(s);
        Ok(res)
    }

    /// Field of the record in rax, or a property getter (a method without
    /// parameters). `own`: the receiver is an owned reference.
    pub(super) fn rec_field(&mut self, r: &str, name: &str, own: bool) -> R<Ty> {
        if let Some((fty, off)) = self.fld(r, name)? {
            self.load_mem(&fty, &format!("rax + {}", off));
            self.retain_if(&fty);
            return Ok(fty);
        }
        if let Some(sig) = self.tp.methods.get(&(r.to_string(), name.to_string())).cloned() {
            if sig.params.is_empty() && sig.ret != Ty::Void {
                self.check_method_vis(&sig, name)?;
                if !own {
                    self.retain();
                }
                let t = self.temp();
                self.e(format!("mov qword ptr [rbp - {}], rax", t));
                self.emit_call(&[t], &format!("call {}", sig.sym), None);
                self.free(t);
                return Ok(sig.ret);
            }
        }
        self.err(format!("{} has no field '{}'", r, name))
    }

    /// Allocates a record and initializes it from `fields`.
    pub(super) fn record_literal(&mut self, r: &str, fields: &[(String, Expr)]) -> R<()> {
        self.record_alloc(r, fields, false)
    }

    /// Offset of the weak-cell word of a shared object (after the fields).
    pub(super) fn cell_off(&self, r: &str) -> u32 {
        (self.rec_size(r) + 7) & !7
    }

    /// `rc`: the object of a `new` (an extra word for the weak cell, dropped by `B_rcdrop_*`).
    pub(super) fn record_alloc(&mut self, r: &str, fields: &[(String, Expr)], rc: bool) -> R<()> {
        let (size, drop) = if rc { (self.cell_off(r) + 8, format!("B_rcdrop_{}", sym_of(r))) } else { (self.rec_size(r), self.rec_drop(r)) };
        self.alloc(size, &drop);
        let t = self.temp();
        self.e(format!("mov qword ptr [rbp - {}], rax", t));
        let decl: Vec<(String, Ty, u32)> = self.tp.records[r].fields.clone();
        for (fname, _) in fields {
            if !decl.iter().any(|d| &d.0 == fname) {
                return self.err(format!("{} has no field '{}'", r, fname));
            }
        }
        for (fname, fty, off) in decl {
            match fields.iter().find(|f| f.0 == fname) {
                Some((_, fe)) => {
                    let vt = self.value(fe, Some(&fty))?;
                    self.check(&fty, &vt, &format!("field '{}'", fname))?;
                    self.norm(&fty);
                }
                None => match &fty {
                    Ty::List(el) => self.new_list(&el.clone()),
                    Ty::Text => {
                        let l = self.string_lit("");
                        self.e(format!("lea rax, {}", l));
                    }
                    _ => continue,
                },
            }
            self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
            self.store_mem(&fty, &format!("rcx + {}", off));
        }
        self.e(format!("mov rax, qword ptr [rbp - {}]", t));
        self.free(t);
        Ok(())
    }

    pub(super) fn if_chain(&mut self, branches: &[(Expr, Block)], else_body: Option<&Block>, want: Option<&Ty>, as_value: bool) -> R<Ty> {
        let end = self.lbl("ifend");
        let mut ty: Option<Ty> = None;
        for (cond, body) in branches {
            let next = self.lbl("ifnext");
            self.cond_jump(cond, &next, false)?;
            let t = self.block_value(body, want, as_value)?;
            if ty.is_none() {
                ty = Some(t);
            }
            self.e(format!("jmp {}", end));
            self.label(&next);
        }
        match else_body {
            Some(b) => {
                self.block_value(b, want, as_value)?;
            }
            None if as_value => self.e("xor eax, eax"),
            None => {}
        }
        self.label(&end);
        Ok(if as_value { ty.unwrap_or(Ty::Void) } else { Ty::Void })
    }

    /// Runs a block; with `as_value`, a trailing expression is its value (in rax).
    pub(super) fn block_value(&mut self, b: &[Stmt], want: Option<&Ty>, as_value: bool) -> R<Ty> {
        self.push_scope();
        let mut ty = Ty::Void;
        for (i, s) in b.iter().enumerate() {
            if as_value && i + 1 == b.len() {
                if let StmtKind::Expr(e) = &s.kind {
                    self.line = s.line;
                    let mark = self.fs.stmt_temps.len();
                    ty = self.value(e, want)?;
                    self.flush_temps(mark);
                    continue;
                }
            }
            self.stmt(s)?;
        }
        self.pop_scope();
        Ok(ty)
    }

    // ---------------- match ----------------

    pub(super) fn match_expr(&mut self, scrut: &Expr, arms: &[MatchArm], want: Option<&Ty>, as_value: bool) -> R<Ty> {
        self.push_scope();
        let (st, own) = self.operand(scrut, None)?;
        let s = if own {
            
            self.hidden_local(st.clone())
        } else {
            self.slot()
        };
        self.e(format!("mov qword ptr [rbp - {}], rax", s));
        let end = self.lbl("mend");
        let mut result = Ty::Void;
        let mut got_result = false;
        let mut covered: Vec<String> = Vec::new();
        let mut catch_all = false;

        for arm in arms {
            let body_l = self.lbl("mbody");
            let next_l = self.lbl("mnext");
            let mut binds: Option<(String, Vec<String>)> = None;
            let mut catch_name: Option<String> = None;
            for atom in &arm.pattern {
                match atom {
                    PatAtom::Wildcard => {
                        catch_all = true;
                        self.e(format!("jmp {}", body_l));
                    }
                    PatAtom::Literal(lit) => {
                        self.pattern_literal(&st, s, lit, &body_l)?;
                    }
                    PatAtom::Name { name, bindings } => match &st {
                        Ty::Enum(q) => {
                            let info = self.tp.enums[q].clone();
                            if let Some(idx) = info.variants.iter().position(|v| &v.0 == name) {
                                let want_n = info.variants[idx].1.len();
                                let got_n = bindings.as_ref().map_or(0, |b| b.len());
                                if got_n != want_n {
                                    return self.err(format!("'{}' has {} value(s), the pattern names {}", name, want_n, got_n));
                                }
                                covered.push(name.clone());
                                self.e(format!("mov rax, qword ptr [rbp - {}]", s));
                                if info.payload {
                                    self.e("mov rax, qword ptr [rax]");
                                }
                                self.e(format!("cmp rax, {}", idx));
                                self.e(format!("je {}", body_l));
                                if let Some(b) = bindings {
                                    binds = Some((name.clone(), b.clone()));
                                }
                            } else if bindings.is_none() {
                                catch_all = true;
                                catch_name = Some(name.clone());
                                self.e(format!("jmp {}", body_l));
                            } else {
                                return self.err(format!("enum {} has no variant '{}'", st.show(), name));
                            }
                        }
                        Ty::Result(inner) => {
                            let n = bindings.as_ref().map_or(0, |b| b.len());
                            match name.as_str() {
                                "ok" => {
                                    if n > 1 || (n == 1 && **inner == Ty::Void) {
                                        return self.err("'ok' binds the value of the call (none for `-> error`)");
                                    }
                                    covered.push("ok".into());
                                    self.e(format!("mov rax, qword ptr [rbp - {}]", s));
                                    self.e("mov rax, qword ptr [rax]");
                                    self.e("test rax, rax");
                                    self.e(format!("jz {}", body_l));
                                }
                                "err" => {
                                    if n > 1 {
                                        return self.err("'err' binds one value: the error");
                                    }
                                    covered.push("err".into());
                                    self.e(format!("mov rax, qword ptr [rbp - {}]", s));
                                    self.e("mov rax, qword ptr [rax]");
                                    self.e("test rax, rax");
                                    self.e(format!("jnz {}", body_l));
                                }
                                _ => return self.err("a result is matched with `ok(value)` and `err(e)`"),
                            }
                            if let Some(b) = bindings {
                                binds = Some((name.clone(), b.clone()));
                            }
                        }
                        _ => {
                            if bindings.is_some() {
                                return self.err(format!("cannot match {} against '{}(...)'", st.show(), name));
                            }
                            catch_all = true;
                            catch_name = Some(name.clone());
                            self.e(format!("jmp {}", body_l));
                        }
                    },
                }
            }
            self.e(format!("jmp {}", next_l));
            self.label(&body_l);
            self.push_scope();
            if let Some(n) = catch_name {
                if n != "_" {
                    self.e(format!("mov rax, qword ptr [rbp - {}]", s));
                    self.retain_if(&st);
                    let off = self.declare_local(&n, st.clone(), false);
                    self.e(format!("mov qword ptr [rbp - {}], rax", off));
                }
            }
            if let Some((vname, names)) = binds {
                let tys: Vec<Ty> = match &st {
                    Ty::Enum(q) => self.tp.enums[q].variants.iter().find(|v| v.0 == vname).map(|v| v.1.clone()).unwrap_or_default(),
                    Ty::Result(inner) => {
                        if vname == "ok" {
                            vec![(**inner).clone()]
                        } else {
                            vec![Ty::Err]
                        }
                    }
                    _ => vec![],
                };
                for (i, n) in names.iter().enumerate() {
                    if n == "_" {
                        continue;
                    }
                    let ty = tys[i].clone();
                    self.e(format!("mov rax, qword ptr [rbp - {}]", s));
                    match &st {
                        Ty::Enum(_) => self.e(format!("mov rax, qword ptr [rax + {}]", 8 + 8 * i)),
                        Ty::Result(_) if vname == "ok" => self.e("mov rax, qword ptr [rax + 8]"),
                        _ => self.e("mov rax, qword ptr [rax]"),
                    }
                    self.retain_if(&ty);
                    let off = self.declare_local(n, ty, false);
                    self.e(format!("mov qword ptr [rbp - {}], rax", off));
                }
            }
            let t = self.block_value(&arm.body, want, as_value)?;
            self.pop_scope();
            if !got_result && t != Ty::Void {
                result = t;
                got_result = true;
            }
            self.e(format!("jmp {}", end));
            self.label(&next_l);
        }

        // exhaustiveness
        if !catch_all {
            let missing: Vec<String> = match &st {
                Ty::Enum(q) => self.tp.enums[q].variants.iter().map(|v| v.0.clone()).filter(|n| !covered.contains(n)).collect(),
                Ty::Result(_) => ["ok", "err"].iter().map(|s| s.to_string()).filter(|n| !covered.contains(n)).collect(),
                _ => vec![],
            };
            if !missing.is_empty() {
                return self.err(format!("'match' is not exhaustive: missing {}; add the cases or `_ ->`", missing.join(", ")));
            }
        }
        if as_value {
            self.e("xor eax, eax");
        }
        self.label(&end);
        self.pop_scope();
        Ok(if as_value { result } else { Ty::Void })
    }

    pub(super) fn pattern_literal(&mut self, st: &Ty, s: i32, lit: &Expr, body: &str) -> R<()> {
        match &lit.kind {
            ExprKind::Text(parts) if *st == Ty::Text => {
                let txt: String = parts.iter().map(|p| if let TextPart::Lit(l) = p { l.as_str() } else { "" }).collect();
                let l = self.string_lit(&txt);
                self.e(format!("mov rcx, qword ptr [rbp - {}]", s));
                self.e(format!("lea rdx, {}", l));
                self.e("call rt_text_eq");
                self.e("test rax, rax");
                self.e(format!("jnz {}", body));
            }
            ExprKind::Nil if st.nullable() => {
                self.e(format!("cmp qword ptr [rbp - {}], 0", s));
                self.e(format!("je {}", body));
            }
            _ => {
                let v = match &lit.kind {
                    ExprKind::Bool(b) => Some(*b as i64),
                    _ => self.const_eval(lit, &self.cur_mod.clone()),
                };
                let Some(v) = v else { return self.err(format!("this pattern cannot be matched against {}", st.show())) };
                let is_char = matches!(&lit.kind, ExprKind::Char(_));
                if is_char != (*st == Ty::Char) || !(st.is_int() || *st == Ty::Bool || *st == Ty::Char) {
                    return self.err(format!("a {} pattern cannot match {}", if is_char { "char" } else { "number" }, st.show()));
                }
                self.e(format!("mov rax, qword ptr [rbp - {}]", s));
                self.e(format!("mov rcx, {}", v));
                self.e("cmp rax, rcx");
                self.e(format!("je {}", body));
            }
        }
        Ok(())
    }

    // ---------------- try ----------------

    pub(super) fn try_expr(&mut self, inner: &Expr) -> R<Ty> {
        let Ty::Result(ret_inner) = self.fs.ret.clone() else {
            return self.err("'try' can only be used inside a function that ends with `or error`");
        };
        let (t, own) = self.operand(inner, None)?;
        let Ty::Result(v) = t.clone() else {
            return self.err(format!("'try' needs a call that can fail, found {}", t.show()));
        };
        let s = self.temp();
        let (ok_l,) = (self.lbl("tryok"),);
        self.e(format!("mov qword ptr [rbp - {}], rax", s));
        self.e("mov rcx, qword ptr [rax]");
        self.e("test rcx, rcx");
        self.e(format!("jz {}", ok_l));
        // failure: hand the same error to our caller
        self.e("mov rax, rcx");
        self.retain();
        let e2 = self.temp();
        self.e(format!("mov qword ptr [rbp - {}], rax", e2));
        if own {
            self.release_slot(s);
        }
        self.e(format!("mov rax, qword ptr [rbp - {}]", e2));
        self.mk_err(&ret_inner);
        self.emit_run_defers();
        self.flush_all_temps_emit();
        self.emit_unwind(0);
        let rl = self.fs.ret_label.clone();
        self.e(format!("jmp {}", rl));
        self.free(e2);
        self.label(&ok_l);
        if *v != Ty::Void {
            self.e(format!("mov rax, qword ptr [rbp - {}]", s));
            self.e("mov rax, qword ptr [rax + 8]");
            self.retain_if(&v);
            if own {
                let keep = self.temp();
                self.e(format!("mov qword ptr [rbp - {}], rax", keep));
                self.release_slot(s);
                self.e(format!("mov rax, qword ptr [rbp - {}]", keep));
                self.free(keep);
            }
        } else if own {
            self.release_slot(s);
        }
        self.free(s);
        Ok(*v)
    }

    // ---------------- closures ----------------

    pub(super) fn lambda(&mut self, params: &[Param], body: &[Stmt], want: Option<&Ty>) -> R<Ty> {
        let share_records = std::mem::take(&mut self.next_lambda_share);
        let ignore_result = std::mem::take(&mut self.next_lambda_ignore_result);
        let checked_vars = self.next_lambda_vars.take();
        let (wparams, wret) = match want {
            Some(Ty::Func { params, ret, ext: false }) => (Some(params.clone()), (**ret).clone()),
            Some(Ty::Func { ext: true, .. }) => return self.err("a closure cannot be used as an extern callback; pass a top-level function"),
            _ => (None, Ty::Void),
        };
        // the result type is worked out from the body when nothing is expected, or when only
        // the parameter types are known (the result is the `?` placeholder of generic inference)
        let infer_ret = !matches!(want, Some(Ty::Func { .. })) || matches!(&wret, Ty::Param { name, .. } if name == "?");
        let wret = if infer_ret { Ty::Void } else { wret };
        let mut ptys = Vec::new();
        for (i, p) in params.iter().enumerate() {
            let t = match (&p.ty, &wparams) {
                (Some(t), _) => self.resolve(t, &self.cur_mod.clone())?,
                (None, Some(wp)) if i < wp.len() => wp[i].clone(),
                _ => return self.err(format!("cannot infer the type of lambda parameter '{}'", p.name)),
            };
            let by_ref = p.by_ref && !matches!(t, Ty::List(_) | Ty::Map(..));
            ptys.push((p.name.clone(), t, by_ref));
        }
        if let Some(wp) = &wparams {
            if wp.len() != params.len() {
                return self.err(format!("expected a function with {} parameter(s), found {}", wp.len(), params.len()));
            }
        }
        let own_refs: Vec<(String, bool)> =
            ptys.iter().filter(|p| p.2).map(|p| (p.0.clone(), plain_scalar(&p.1))).collect();
        self.check_refs(&own_refs, body)?;
        // Free variables that are locals of the enclosing function.
        let mut used = Vec::new();
        collect_block(body, &mut used);
        let pnames: HashSet<&String> = params.iter().map(|p| &p.name).collect();
        let mut caps: Vec<(String, Local)> = Vec::new();
        for n in used {
            if pnames.contains(&n) || caps.iter().any(|c| c.0 == n) {
                continue;
            }
            if let Some(l) = self.lookup(&n) {
                caps.push((n, l));
            }
        }
        let env_rc = caps.iter().any(|c| self.is_rc(&c.1.ty));
        let sym = self.lbl("L");
        let env_drop = if env_rc { format!("{}_env", sym) } else { "0".to_string() };
        // Environment block with captured values (records are copied).
        let env = self.temp();
        if caps.is_empty() {
            self.e(format!("mov qword ptr [rbp - {}], 0", env));
        } else {
            self.alloc(8 * caps.len() as u32, &env_drop);
            self.e(format!("mov qword ptr [rbp - {}], rax", env));
            for (i, (_, l)) in caps.iter().enumerate() {
                self.load_local("rax", l);
                if share_records {
                    self.retain_if(&l.ty);
                } else {
                    self.copy_or_retain(&l.ty);
                }
                self.e(format!("mov rcx, qword ptr [rbp - {}]", env));
                self.e(format!("mov qword ptr [rcx + {}], rax", 8 * i));
            }
        }
        // Compile the body as its own procedure.
        let rl = self.lbl("ret");
        let saved = std::mem::replace(&mut self.fs, FnState::new(wret.clone(), rl));
        // the types the body checker found for the locals of the enclosing function, closure
        // bodies included (an `async` body is handed its own)
        self.fs.checked_vars = checked_vars.unwrap_or_else(|| saved.checked_vars.clone());
        self.fs.unsafe_depth = saved.unsafe_depth;
        self.fs.allow_ignored_result = ignore_result;
        let env_slot = self.slot();
        let mut pro = self.bind_params(&ptys, Some(env_slot));
        for (i, (n, l)) in caps.iter().enumerate() {
            let off = self.slot();
            pro.push(format!("    mov rax, qword ptr [rbp - {}]", env_slot));
            pro.push(format!("    mov rax, qword ptr [rax + {}]", 8 * i));
            if self.is_rc(&l.ty) {
                pro.push("    mov rcx, rax".to_string());
                pro.push("    call rt_retain".to_string());
            }
            pro.push(format!("    mov qword ptr [rbp - {}], rax", off));
            let rc = self.is_rc(&l.ty);
            let sc = &mut self.fs.scopes[0];
            sc.vars.insert(n.clone(), Local { off, ty: l.ty.clone(), mutable: false, captured: true, reg: None, imm: None, is_ref: false, ind: false });
            if rc {
                sc.owned.push((off, l.ty.clone()));
                self.fs.may_own = true;
            }
        }
        let mut inferred: Option<Ty> = None;
        let r = (|| -> R<()> {
            self.init_defers(body)?;
            if infer_ret {
                let t = self.block_value(body, None, true)?;
                if t != Ty::Void {
                    self.norm(&t);
                    inferred = Some(t);
                }
            } else if let Ty::Result(inner) = wret.clone() {
                self.block_value(body, Some(&wret), false)?;
                self.e("xor eax, eax");
                self.mk_ok(&inner);
            } else {
                let t = self.block_value(body, Some(&wret), wret != Ty::Void)?;
                if wret != Ty::Void && t != Ty::Void {
                    self.check(&wret, &t, "lambda result")?;
                    self.norm(&wret);
                }
            }
            self.emit_run_defers();
            self.emit_unwind(0);
            Ok(())
        })();
        let proc_text = self.finish_proc(&sym, pro, false);
        self.fs = saved;
        r?;
        self.procs.push(proc_text);
        if env_rc {
            let mut d = format!("{}_env proc\n    push rbp\n    mov rbp, rsp\n    sub rsp, 48\n    mov qword ptr [rbp - 8], rcx\n", sym);
            for (i, (_, l)) in caps.iter().enumerate() {
                if self.is_rc(&l.ty) {
                    d.push_str(&format!(
                        "    mov rcx, qword ptr [rbp - 8]\n    mov rcx, qword ptr [rcx + {}]\n    call rt_release\n",
                        8 * i
                    ));
                }
            }
            d.push_str(&format!("    mov rsp, rbp\n    pop rbp\n    ret\n{}_env endp\n", sym));
            self.procs.push(d);
        }
        // Closure object [code, env]; it owns the environment.
        self.alloc(16, "rt_drop_closure");
        self.e(format!("lea rcx, {}", sym));
        self.e("mov qword ptr [rax], rcx");
        self.e(format!("mov rcx, qword ptr [rbp - {}]", env));
        self.e("mov qword ptr [rax + 8], rcx");
        self.free(env);
        let wret = inferred.unwrap_or(wret);
        Ok(Ty::Func { params: ptys.into_iter().map(|p| p.1).collect(), ret: Box::new(wret), ext: false })
    }

    // ---------------- calls ----------------

}
