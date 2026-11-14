//! Part of the code generator (`codegen_x64`), methods of `Compiler`: calls, inlining and C intrinsics.

use super::*;

impl Compiler {
    /// Loads argument temps into registers / stack and emits the call.
    pub(super) fn emit_call(&mut self, temps: &[i32], call: &str, closure: Option<(i32, bool)>) {
        self.fs.max_args = self.fs.max_args.max(temps.len());
        for (i, t) in temps.iter().enumerate().skip(4) {
            self.e(format!("mov rax, qword ptr [rbp - {}]", t));
            self.e(format!("mov qword ptr [rsp + {}], rax", 32 + 8 * (i - 4)));
        }
        for (i, t) in temps.iter().enumerate().take(4) {
            self.e(format!("mov {}, qword ptr [rbp - {}]", ARG_REGS[i], t));
        }
        match closure {
            Some((c, false)) => {
                self.e(format!("mov rax, qword ptr [rbp - {}]", c));
                self.e("mov r10, qword ptr [rax + 8]");
                self.e("call qword ptr [rax]");
            }
            Some((c, true)) => {
                self.e(format!("mov rax, qword ptr [rbp - {}]", c));
                self.e("call rax");
            }
            None => self.e(call),
        }
    }

    /// A call of a C function on Linux: the System V convention. Integers and pointers go to
    /// rdi, rsi, rdx, rcx, r8, r9, floats to xmm0-7 (counted separately), the rest to the stack
    /// from [rsp]; `al` holds the number of vector registers used (for variadic functions). The
    /// function is reached through its slot in the import table filled in by the dynamic loader.
    pub(super) fn sysv_call(&mut self, temps: &[i32], floats: &[(usize, u8)], sym: &str) {
        const IREGS: [&str; 6] = ["rdi", "rsi", "rdx", "rcx", "r8", "r9"];
        self.fs.max_args = self.fs.max_args.max(temps.len());
        let (mut ni, mut nf, mut ns) = (0, 0, 0);
        let mut moves: Vec<String> = Vec::new();
        for (i, t) in temps.iter().enumerate() {
            let float = floats.iter().find(|(p, _)| *p == i).map(|(_, b)| *b);
            match float {
                Some(bits) if nf < 8 => {
                    moves.push(format!("mov rax, qword ptr [rbp - {}]", t));
                    moves.push(if bits == 32 { format!("movd xmm{}, eax", nf) } else { format!("movq xmm{}, rax", nf) });
                    nf += 1;
                }
                None if ni < 6 => {
                    moves.push(format!("mov {}, qword ptr [rbp - {}]", IREGS[ni], t));
                    ni += 1;
                }
                _ => {
                    moves.push(format!("mov rax, qword ptr [rbp - {}]", t));
                    moves.push(format!("mov qword ptr [rsp + {}], rax", 8 * ns));
                    ns += 1;
                }
            }
        }
        for m in moves {
            self.e(m);
        }
        self.e(format!("mov eax, {}", nf));
        self.e(format!("call qword ptr GOT_{}", sym));
    }

    /// Emits `abort(message)` (arguments already known to fit the form).
    pub(super) fn emit_abort(&mut self, args: &[Arg]) -> R<Ty> {
        let t = self.expr(&args[0].value, Some(&Ty::Text))?;
        self.check(&Ty::Text, &t, "abort message")?;
        self.e("mov rcx, rax");
        self.at("rdx");
        self.e("call rt_abort");
        Ok(Ty::Void)
    }

    /// Emits `print(...)` and `println(...)` (arguments already known to fit the form).
    pub(super) fn emit_print(&mut self, args: &[Arg]) -> R<Ty> {
        let mut first = true;
        for a in args {
            if !first {
                let sp = self.string_lit(" ");
                self.e(format!("lea rcx, {}", sp));
                self.e("call rt_write_out");
            }
            first = false;
            let t = self.expr(&a.value, None)?;
            self.to_text(&t)?;
            let keep = self.temp();
            self.e(format!("mov qword ptr [rbp - {}], rax", keep));
            self.e("mov rcx, rax");
            self.e("call rt_write_out");
            self.release_slot(keep);
            self.free(keep);
        }
        let nl = self.string_lit("\n");
        self.e(format!("lea rcx, {}", nl));
        self.e("call rt_write_out");
        Ok(Ty::Void)
    }

    /// Emits `error(message)`, `error(kind, message)` and `error(kind, message, cause)` (arguments already known to fit the form).
    pub(super) fn emit_error(&mut self, args: &[Arg]) -> R<Ty> {
        let (k, m, c) = (self.temp(), self.temp(), self.temp());
        if args.len() == 1 {
            let t = self.expr(&args[0].value, Some(&Ty::Text))?;
            self.check(&Ty::Text, &t, "error message")?;
            self.e(format!("mov qword ptr [rbp - {}], rax", m));
            self.e("call rt_kind_other");
            self.e(format!("mov qword ptr [rbp - {}], rax", k));
        } else {
            let kt = self.expr(&args[0].value, Some(&Ty::Kind))?;
            if kt == Ty::Text {
                return self.err("error(kind, message): expected an ErrorKind, found text; make a kind with errors.kind(\"package.Name\")");
            }
            self.check(&Ty::Kind, &kt, "error kind")?;
            self.e(format!("mov qword ptr [rbp - {}], rax", k));
            let t = self.expr(&args[1].value, Some(&Ty::Text))?;
            self.check(&Ty::Text, &t, "error message")?;
            self.e(format!("mov qword ptr [rbp - {}], rax", m));
        }
        if args.len() == 3 {
            let ct = self.expr(&args[2].value, Some(&Ty::Err))?;
            self.check(&Ty::Err, &ct, "error cause")?;
            self.e(format!("mov qword ptr [rbp - {}], rax", c));
        } else {
            self.e(format!("mov qword ptr [rbp - {}], 0", c));
        }
        self.alloc(24, "rt_drop_error");
        self.e(format!("mov rcx, qword ptr [rbp - {}]", m));
        self.e("mov qword ptr [rax], rcx");
        self.e(format!("mov rcx, qword ptr [rbp - {}]", k));
        self.e("mov qword ptr [rax + 8], rcx");
        self.e(format!("mov rcx, qword ptr [rbp - {}]", c));
        self.e("mov qword ptr [rax + 16], rcx");
        self.free(c);
        self.free(m);
        self.free(k);
        Ok(Ty::Err)
    }

    /// Emits `char(n)` (arguments already known to fit the form).
    pub(super) fn emit_char(&mut self, args: &[Arg]) -> R<Ty> {
        let t = self.expr(&args[0].value, None)?;
        if !t.is_int() {
            return self.err(format!("char() needs an integer code point, found {}", t.show()));
        }
        let (bad, ok) = (self.lbl("charbad"), self.lbl("charok"));
        self.e("cmp rax, 1114111");
        self.e(format!("ja {}", bad));
        self.e("lea rcx, [rax - 55296]");
        self.e("cmp rcx, 2047");
        self.e(format!("ja {}", ok));
        self.label(&bad);
        self.e("mov rdx, rax");
        self.e("lea rcx, rt_msg_char1");
        self.e("lea r8, rt_msg_char2");
        self.at("r9");
        self.e("call rt_abort_val");
        self.label(&ok);
        self.e("mov eax, eax");
        Ok(Ty::Char)
    }

    /// Emits `sleep(ms)`.
    pub(super) fn emit_sleep(&mut self, args: &[Arg]) -> R<Ty> {
            let t = self.expr(&args[0].value, Some(&Ty::i64()))?;
            if !t.is_int() {
                return self.err(format!("sleep() needs a number of milliseconds, found {}", t.show()));
            }
            let s = self.temp();
            self.e(format!("mov qword ptr [rbp - {}], rax", s));
            let sig = self.future_fn("sleep_future")?;
            self.emit_call(&[s], &format!("call {}", sig.sym), None);
            self.free(s);
            Ok(Ty::Future(Box::new(Ty::Void)))
    }

    /// Emits `live_objects()`.
    pub(super) fn emit_live_objects(&mut self) -> R<Ty> {
            self.e("mov rax, qword ptr rt_live");
            Ok(Ty::int())
    }

    /// Emits `int(x)`, `u8(x)`, `bool(x)`... (a conversion to the type `t`, written `name`).
    pub(super) fn emit_int_conversion(&mut self, name: &str, t: Ty, args: &[Arg]) -> R<Ty> {
        if args.len() != 1 {
            return self.err(format!("{}() takes one argument", name));
        }
        let at = self.expr(&args[0].value, None)?;
        if let Ty::Float { bits } = at {
            // truncate toward zero; NaN, infinities and out-of-range values abort
            let ok = self.lbl("f2iok");
            if bits == 32 {
                self.e("movd xmm0, eax");
                self.e("cvtss2sd xmm0, xmm0");
            } else {
                self.e("movq xmm0, rax");
            }
            self.e("cvttsd2si rax, xmm0");
            self.e("mov rcx, 8000000000000000h");
            self.e("cmp rax, rcx");
            self.e(format!("jne {}", ok));
            self.e("movq rdx, xmm0");
            self.e("mov rcx, 0C3E0000000000000h");
            self.e("cmp rdx, rcx");
            self.e(format!("je {}", ok));
            self.e("lea rcx, rt_msg_floatrange");
            self.at("rdx");
            self.e("call rt_abort");
            self.label(&ok);
            if t == Ty::Bool {
                return self.err("cannot convert float to bool");
            }
            if let (Ty::Int { bits, .. }, Some(_)) = (&t, norm_insn(&t)) {
                // a narrower target: the truncated value must be the converted one
                if *bits < 64 {
                    let fits = self.lbl("f2ifit");
                    self.e("mov rdx, rax");
                    self.norm(&t);
                    self.e("cmp rax, rdx");
                    self.e(format!("je {}", fits));
                    self.e("lea rcx, rt_msg_floatrange");
                    self.at("rdx");
                    self.e("call rt_abort");
                    self.label(&fits);
                }
            }
            self.norm(&t);
            return Ok(t);
        }
        if !(at.is_int() || matches!(at, Ty::Ptr(_) | Ty::Bool | Ty::Func { ext: true, .. })) {
            return self.err(format!("cannot convert {} to {}", at.show(), name));
        }
        if t == Ty::Bool {
            self.e("test rax, rax");
            self.e("setnz al");
            self.e("movzx eax, al");
        }
        self.norm(&t);
        Ok(t)
    }

    /// Emits `float(x)`, `f64(x)` and `f32(x)` (written `name`).
    pub(super) fn emit_float_conversion(&mut self, name: &str, args: &[Arg]) -> R<Ty> {
        if args.len() != 1 {
            return self.err(format!("{}() takes one argument", name));
        }
        let tb = if name == "f32" { 32 } else { 64 };
        let t = Ty::Float { bits: tb };
        let at = self.expr(&args[0].value, None)?;
        match at {
            Ty::Int { bits: 64, signed: false } => {
                // values above i64::MAX: halve (keeping the low bit), convert, double
                let (big, end) = (self.lbl("ubig"), self.lbl("uend"));
                let cvt = if tb == 32 { "cvtsi2ss" } else { "cvtsi2sd" };
                let mv = if tb == 32 { "movd" } else { "movq" };
                let (add, ra) = if tb == 32 { ("addss", "eax") } else { ("addsd", "rax") };
                self.e("test rax, rax");
                self.e(format!("js {}", big));
                self.e(format!("{} xmm0, rax", cvt));
                self.e(format!("jmp {}", end));
                self.label(&big);
                self.e("mov rcx, rax");
                self.e("shr rax, 1");
                self.e("and ecx, 1");
                self.e("or rax, rcx");
                self.e(format!("{} xmm0, rax", cvt));
                self.e(format!("{} xmm0, xmm0", add));
                self.label(&end);
                self.e(format!("{} {}, xmm0", mv, ra));
            }
            Ty::Int { .. } | Ty::Bool => {
                self.e(format!("{} xmm0, rax", if tb == 32 { "cvtsi2ss" } else { "cvtsi2sd" }));
                self.e(if tb == 32 { "movd eax, xmm0" } else { "movq rax, xmm0" });
            }
            Ty::Float { bits: sb } if sb == tb => {}
            Ty::Float { bits: 64 } => {
                self.e("movq xmm0, rax");
                self.e("cvtsd2ss xmm0, xmm0");
                self.e("movd eax, xmm0");
            }
            Ty::Float { .. } => {
                self.e("movd xmm0, eax");
                self.e("cvtss2sd xmm0, xmm0");
                self.e("movq rax, xmm0");
            }
            other => return self.err(format!("cannot convert {} to {}", other.show(), name)),
        }
        Ok(t)
    }

    /// Emits the call of a typed call node whose target the checker settled: a function by name,
    /// an enum variant, a method or one of the simple built-ins. `None` when the node is not one of those (the caller then resolves the
    /// callee itself). Visibility of a function of another module is still checked here.
    pub(super) fn emit_resolved_call(&mut self, t: &TypedExpr, callee: &Expr, args: &[Arg]) -> Option<R<Ty>> {
        let TypedExprKind::Call { callee: target, .. } = &t.kind else { return None };
        match target {
            TypedCallee::Func { module, name } => {
                let sig = self.tp.funcs.get(&(module.clone(), name.clone()))?.clone();
                if matches!(callee.kind, ExprKind::Field { .. }) {
                    if let Err(e) = self.check_member_vis(module, name, sig.public) {
                        return Some(Err(e));
                    }
                }
                Some(self.call_sig(&sig, None, args))
            }
            TypedCallee::Variant { enum_key, index } => Some(self.enum_construct(enum_key, *index, args)),
            // a built-in function the checker typed (it is not shadowed by a local or a function)
            TypedCallee::Builtin(name) => match name.as_str() {
                "print" | "println" => Some(self.emit_print(args)),
                "error" => Some(self.emit_error(args)),
                "abort" => Some(self.emit_abort(args)),
                "char" => Some(self.emit_char(args)),
                "float" | "f64" | "f32" => Some(self.emit_float_conversion(name, args)),
                "sleep" => Some(self.emit_sleep(args)),
                "live_objects" => Some(self.emit_live_objects()),
                _ => {
                    let target = int_type(name).or(if name == "bool" { Some(Ty::Bool) } else { None })?;
                    Some(self.emit_int_conversion(name, target, args))
                }
            },
            TypedCallee::Method { rec_key, name } => {
                let ExprKind::Field { expr: obj, .. } = &callee.kind else { return None };
                let sig = self.tp.methods.get(&(rec_key.clone(), name.clone()))?.clone();
                Some(self.emit_method_call(obj, rec_key, name, &sig, args))
            }
            _ => None,
        }
    }

    /// `obj.name(args)` where the checker found the method `sig` for the receiver's type: the
    /// receiver is evaluated (the method owns it), then the call goes to the method.
    fn emit_method_call(&mut self, obj: &Expr, key: &str, name: &str, sig: &Sig, args: &[Arg]) -> R<Ty> {
        let ot = self.expr(obj, None)?;
        debug_assert!(method_key(&rc_view(&ot)) == key, "the checker found a method of {} but the receiver is {}", key, ot.show());
        self.check_method_vis(sig, name)?;
        let t = self.temp();
        self.e(format!("mov qword ptr [rbp - {}], rax", t));
        let res = self.call_sig(sig, Some(t), args);
        self.free(t);
        res
    }

    pub(super) fn call(&mut self, callee: &Expr, args: &[Arg], targs: &[Type], want: Option<&Ty>) -> R<Ty> {
        match &callee.kind {
            // test hook: lowers the element ceiling of lists, maps and texts so a test can reach it
            ExprKind::Ident(name) if self.lookup(name).is_none() && name == "__set_max_len" && args.len() == 1 => {
                let at = self.expr(&args[0].value, Some(&Ty::i64()))?;
                if !at.is_int() {
                    return self.err("__set_max_len() takes a number");
                }
                self.e("mov qword ptr rt_max_len, rax");
                Ok(Ty::Void)
            }
            ExprKind::Ident(name) if self.lookup(name).is_none() && matches!(name.as_str(), "__eq" | "__cmp" | "__hash" | "__tag" | "__payload" | "__mix") => {
                self.std_intrinsic(name, args)
            }
            ExprKind::Ident(name) if self.lookup(name).is_none() && matches!(name.as_str(), "__from_bits" | "__bits") => {
                if args.len() != 1 {
                    return self.err(format!("{}() takes one argument", name));
                }
                // reinterpret the 64 bits: a float and an int share the register
                if name == "__from_bits" {
                    let at = self.expr(&args[0].value, Some(&Ty::i64()))?;
                    self.check(&Ty::i64(), &at, name)?;
                    Ok(Ty::Float { bits: 64 })
                } else {
                    let at = self.expr(&args[0].value, Some(&Ty::Float { bits: 64 }))?;
                    self.check(&Ty::Float { bits: 64 }, &at, name)?;
                    Ok(Ty::i64())
                }
            }
            ExprKind::Ident(name) if self.lookup(name).is_none() && matches!(name.as_str(), "__sqrt" | "__floor" | "__ceil" | "__trunc") => {
                if args.len() != 1 {
                    return self.err(format!("{}() takes one argument", name));
                }
                let at = self.expr(&args[0].value, Some(&Ty::Float { bits: 64 }))?;
                self.check(&Ty::Float { bits: 64 }, &at, name)?;
                self.e("movq xmm0, rax");
                self.e(match name.as_str() {
                    "__sqrt" => "sqrtsd xmm0, xmm0",
                    "__floor" => "roundsd xmm0, xmm0, 9",
                    "__ceil" => "roundsd xmm0, xmm0, 10",
                    _ => "roundsd xmm0, xmm0, 11",
                });
                self.e("movq rax, xmm0");
                Ok(Ty::Float { bits: 64 })
            }
            ExprKind::Ident(name) if self.lookup(name).is_none() && matches!(name.as_str(), "float" | "f64" | "f32") => self.emit_float_conversion(name, args),
            ExprKind::Ident(name) if self.lookup(name).is_none() => {
                if let Some(t) = int_type(name).or(if name == "bool" { Some(Ty::Bool) } else { None }) {
                    return self.emit_int_conversion(name, t, args);
                }
                if (name == "__abort" || name == "abort" && !self.tp.funcs.contains_key(&(self.cur_mod.clone(), name.clone()))) && args.len() == 1 {
                    return self.emit_abort(args);
                }
                if name == "print" || name == "println" {
                    return self.emit_print(args);
                }
                if name == "__kind" && args.len() == 1 {
                    // the interned kind for a name: the one way a kind is made at run time
                    let t = self.expr(&args[0].value, Some(&Ty::Text))?;
                    self.check(&Ty::Text, &t, "__kind")?;
                    let keep = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", keep));
                    self.e("mov rcx, rax");
                    self.e("call rt_kind_intern");
                    let res = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", res));
                    self.release_slot(keep);
                    self.e(format!("mov rax, qword ptr [rbp - {}]", res));
                    self.free(res);
                    self.free(keep);
                    return Ok(Ty::Kind);
                }
                if name == "error" && (1..=3).contains(&args.len()) {
                    return self.emit_error(args);
                }
                if name == "char" && args.len() == 1 && !self.tp.funcs.contains_key(&(self.cur_mod.clone(), name.clone())) {
                    return self.emit_char(args);
                }
                if name == "heap_bytes" && args.is_empty() {
                    if self.custom_allocator.is_some() {
                        return self.err("heap_bytes() needs the built-in heap");
                    }
                    if self.target != Target::Bare {
                        return self.err("heap_bytes() is only available in a bare-metal kernel");
                    }
                    self.e("mov rax, qword ptr rt_brk");
                    self.e("sub rax, 2097152");
                    return Ok(Ty::i64());
                }
                if name == "weak" && !self.tp.funcs.contains_key(&(self.cur_mod.clone(), name.clone())) {
                    return self.weak_of(args);
                }
                if name == "live_objects" && args.is_empty() {
                    return self.emit_live_objects();
                }
                if name == "channel" {
                    return self.channel_new(args);
                }
                if name == "timeout" && args.len() == 2 && !self.tp.funcs.contains_key(&(self.cur_mod.clone(), name.clone())) {
                    return self.timeout_future(&args[0].value, &args[1].value);
                }
                if name == "sleep" && args.len() == 1 && !self.tp.funcs.contains_key(&(self.cur_mod.clone(), name.clone())) {
                return self.emit_sleep(args);
                }
                let key = (self.cur_mod.clone(), name.clone());
                if let Some(sig) = self.tp.funcs.get(&key).cloned() {
                    return self.call_sig(&sig, None, args);
                }
                if let Some((fd, line)) = self.tmpl_funcs.get(&key).cloned() {
                    return self.call_generic(&key.0, &fd, line, args, targs, want);
                }
                if let Some((q, idx)) = self.tp.variants.get(&key).cloned() {
                    return self.enum_construct(&q, idx, args);
                }
                if self.tp.globals.contains_key(&key) {
                    let t = self.expr(callee, None)?;
                    return self.call_value(t, args);
                }
                if name == "chr" {
                    return self.err("unknown function chr: use char(n).to_text() to make a text from a code point");
                }
                self.err(format!("undefined function '{}'", name))
            }
            ExprKind::Field { expr: obj, name, .. } => {
                if let ExprKind::Ident(m) = &obj.kind {
                    if self.is_module(m) {
                        if m == "c" {
                            return self.c_intrinsic(name, args);
                        }
                        let mk = self.modkey(&self.cur_mod, m);
                        let m = &mk;
                        // `format.fmt(template, a, b)` is `format.fmt2(template, a, b)`
                        let renamed;
                        let name = if m == "format" && name == "fmt" && !self.tp.funcs.contains_key(&(m.clone(), name.clone())) {
                            if args.is_empty() {
                                return self.err("format.fmt needs a template");
                            }
                            if args.len() > 9 {
                                return self.err("format.fmt takes at most eight values");
                            }
                            renamed = format!("fmt{}", args.len() - 1);
                            &renamed
                        } else {
                            name
                        };
                        let key = (m.clone(), name.clone());
                        if let Some((fd, line)) = self.tmpl_funcs.get(&key).cloned() {
                            self.check_member_vis(m, name, fd.public)?;
                            return self.call_generic(&key.0, &fd, line, args, targs, want);
                        }
                        return match self.tp.funcs.get(&key).cloned() {
                            Some(sig) => {
                                self.check_member_vis(m, name, sig.public)?;
                                self.call_sig(&sig, None, args)
                            }
                            None if self.modules.iter().any(|x| &x.name == m) => {
                                if let Some((q, idx)) = self.tp.variants.get(&key).cloned() {
                                    self.check_member_vis(m, name, self.tp.pub_types.contains(&q))?;
                                    return self.enum_construct(&q, idx, args);
                                }
                                if self.tp.globals.contains_key(&key) {
                                    let t = self.member(&m.clone(), name, None)?;
                                    return self.call_value(t, args);
                                }
                                self.err(format!("module '{}' has no function '{}'", m, name))
                            }
                            None => self.err(format!("module '{}' is not available in the native backend", m)),
                        };
                    }
                    if let Some(q) = self.enum_type_named(m) {
                        let Some(idx) = self.tp.enums[&q].variants.iter().position(|v| &v.0 == name) else {
                            return self.err(format!("enum {} has no variant '{}'", m, name));
                        };
                        return self.enum_construct(&q, idx, args);
                    }
                }
                if let Some(pt) = self.path_ty(obj) {
                    if let Some(res) = self.cow_method(obj, &pt, name, args)? {
                        return Ok(res);
                    }
                }
                let ot = self.expr(obj, None)?;
                if let Ty::Weak(inner) = &ot {
                    if name == "upgrade" {
                        return self.weak_upgrade(inner, args);
                    }
                    return self.err(format!("{} has no method '{}'; use upgrade()", ot.show(), name));
                }
                let ot = rc_view(&ot);
                if let Ty::Param { name: tname, bound } = &ot {
                    return self.param_method_call("", bound, tname, name, args);
                }
                if !matches!(ot, Ty::Rec(_)) {
                    let mut found = self.tp.methods.get(&(method_key(&ot), name.clone())).cloned();
                    if let Some(sg) = &found {
                        self.check_method_vis(sg, name)?;
                    }
                    if found.is_none() {
                        found = self.std_sig(&ot, name)?;
                    }
                    if let Some(sig) = found {
                        let t = self.temp();
                        self.e(format!("mov qword ptr [rbp - {}], rax", t));
                        let res = self.call_sig(&sig, Some(t), args);
                        self.free(t);
                        return res;
                    }
                }
                let r = match &ot {
                    Ty::Rec(r) => Some(r.clone()),
                    _ => None,
                };
                if let Some(r) = r {
                    let mut found = self.tp.methods.get(&(r.clone(), name.clone())).cloned();
                    if let Some(sg) = &found {
                        self.check_method_vis(sg, name)?;
                    }
                    if found.is_none() {
                        found = self.std_sig(&ot, name)?;
                    }
                    if let Some(sig) = found {
                        // the method owns its receiver, which expr() handed over
                        let t = self.temp();
                        self.e(format!("mov qword ptr [rbp - {}], rax", t));
                        let res = self.call_sig(&sig, Some(t), args);
                        self.free(t);
                        return res;
                    }
                    if let Some((fty @ Ty::Func { .. }, off)) = self.fld(&r, name)? {
                        // closure stored in a field: keep it alive for the call
                        let s = self.temp();
                        self.e(format!("mov qword ptr [rbp - {}], rax", s));
                        self.e(format!("mov rax, qword ptr [rax + {}]", off));
                        self.retain_if(&fty);
                        let res = self.call_value(fty, args);
                        self.release_slot(s);
                        self.free(s);
                        return res;
                    }
                    return self.err(format!("{} has no method '{}'", r, name));
                }
                if ot == Ty::Char && matches!(name.as_str(), "to_text" | "len_utf8" | "is_ascii") {
                    if !args.is_empty() {
                        return self.err(format!("{}() takes no arguments", name));
                    }
                    match name.as_str() {
                        "to_text" => {
                            self.e("mov rcx, rax");
                            self.e("call rt_text_from_char");
                            return Ok(Ty::Text);
                        }
                        "len_utf8" => {
                            self.e("mov ecx, eax");
                            self.e("mov eax, 1");
                            for (lim, n) in [(128, 2), (2048, 3), (65536, 4)] {
                                self.e(format!("mov edx, {}", n));
                                self.e(format!("cmp ecx, {}", lim));
                                self.e("cmovae eax, edx");
                            }
                            return Ok(Ty::int());
                        }
                        _ => {
                            self.e("cmp eax, 128");
                            self.e("setb al");
                            self.e("movzx eax, al");
                            return Ok(Ty::Bool);
                        }
                    }
                }
                if ot == Ty::Text && matches!(name.as_str(), "bytes" | "char_count") {
                    if !args.is_empty() {
                        return self.err(format!("{}() takes no arguments", name));
                    }
                    let t = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", t));
                    self.e("mov rcx, rax");
                    self.e(format!("call {}", if name == "bytes" { "rt_text_bytes" } else { "rt_text_char_count" }));
                    let keep = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", keep));
                    self.release_slot(t);
                    self.e(format!("mov rax, qword ptr [rbp - {}]", keep));
                    self.free(keep);
                    self.free(t);
                    return Ok(if name == "bytes" { Ty::List(Box::new(Ty::Int { bits: 8, signed: false })) } else { Ty::int() });
                }
                if ot == Ty::Text && name == "byte" {
                    if args.len() != 1 {
                        return self.err("byte(i) takes one position");
                    }
                    let t = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", t));
                    let it = self.expr(&args[0].value, Some(&Ty::i64()))?;
                    if !it.is_int() {
                        return self.err(format!("byte position must be a number, found {}", it.show()));
                    }
                    self.e("mov rdx, rax");
                    self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
                    self.at("r9");
                    self.e("call rt_text_byte_at");
                    let keep = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", keep));
                    self.release_slot(t);
                    self.e(format!("mov rax, qword ptr [rbp - {}]", keep));
                    self.free(keep);
                    self.free(t);
                    return Ok(Ty::Int { bits: 8, signed: false });
                }
                if ot == Ty::Text && (name == "slice" || name == "__raw_slice") {
                    if args.len() != 2 {
                        return self.err("slice(start, end) takes two numbers");
                    }
                    let t = self.temp();
                    let a = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", t));
                    self.expr(&args[0].value, Some(&Ty::i64()))?;
                    self.e(format!("mov qword ptr [rbp - {}], rax", a));
                    self.expr(&args[1].value, Some(&Ty::i64()))?;
                    self.e("mov r8, rax");
                    self.e(format!("mov rdx, qword ptr [rbp - {}]", a));
                    self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
                    self.at("r9");
                    self.e(if name == "slice" { "call rt_text_slice" } else { "call rt_text_slice_raw" });
                    let keep = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", keep));
                    self.release_slot(t);
                    self.e(format!("mov rax, qword ptr [rbp - {}]", keep));
                    self.free(keep);
                    self.free(a);
                    self.free(t);
                    return Ok(Ty::Text);
                }
                if ot == Ty::Err && (name == "context" || name == "is") {
                    if args.len() != 1 {
                        return self.err(format!("'{}' takes one argument", name));
                    }
                    let t = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", t));
                    if name == "context" {
                        // a new error with the kind of this one; this one becomes its cause
                        let mt = self.expr(&args[0].value, Some(&Ty::Text))?;
                        self.check(&Ty::Text, &mt, "context message")?;
                        let m = self.temp();
                        self.e(format!("mov qword ptr [rbp - {}], rax", m));
                        self.alloc(24, "rt_drop_error");
                        self.e(format!("mov rcx, qword ptr [rbp - {}]", m));
                        self.e("mov qword ptr [rax], rcx");
                        self.e(format!("mov rdx, qword ptr [rbp - {}]", t));
                        self.e("mov rcx, qword ptr [rdx + 8]");
                        self.e("mov qword ptr [rax + 8], rcx");
                        self.e("call rt_retain");
                        self.e("mov qword ptr [rax + 16], rdx");
                        self.free(m);
                        self.free(t);
                        return Ok(Ty::Err);
                    }
                    let kt = self.expr(&args[0].value, Some(&Ty::Kind))?;
                    if kt == Ty::Text {
                        return self.err("is(kind): expected an ErrorKind, found text; make a kind with errors.kind(\"package.Name\")");
                    }
                    self.check(&Ty::Kind, &kt, "is")?;
                    let k = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", k));
                    self.e("mov rdx, rax");
                    self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
                    self.e("call rt_error_is");
                    self.release_slot(k);
                    self.release_slot(t);
                    self.free(k);
                    self.free(t);
                    return Ok(Ty::Bool);
                }
                if let Ty::Chan(el) = &ot {
                    return self.chan_method(el, name, args);
                }
                if let Ty::List(el) = &ot {
                    let t = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", t));
                    let res = (|| -> R<Ty> {
                        if name == "add" || name == "push" {
                            if args.len() != 1 {
                                return self.err(format!("'{}' takes one argument", name));
                            }
                            let vt = self.value(&args[0].value, Some(el))?;
                            self.check(el, &vt, "list element")?;
                            self.e("mov rdx, rax");
                            self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
                            self.e("call rt_list_push");
                            return Ok(Ty::Void);
                        }
                        if name == "clear" {
                            self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
                            self.e("call rt_list_clear");
                            return Ok(Ty::Void);
                        }
                        if name == "get" {
                            // the element at an index, or nothing when there is none (negative
                            // included): `T?` for a type that can be nil, `T or error` otherwise
                            if args.len() != 1 || args[0].name.is_some() {
                                return self.err("'get' takes one index");
                            }
                            let it = self.expr(&args[0].value, Some(&Ty::i64()))?;
                            if !it.is_int() {
                                return self.err(format!("list index must be a number, found {}", it.show()));
                            }
                            let (miss, done) = (self.lbl("getmiss"), self.lbl("getdone"));
                            self.e("mov rdx, rax");
                            self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
                            self.e("cmp rdx, qword ptr [rcx]");
                            self.e(format!("jae {}", miss));
                            self.e("mov rax, qword ptr [rcx + 16]");
                            self.e("mov rax, qword ptr [rax + rdx * 8]");
                            self.retain_if(el);
                            if el.nullable() {
                                self.e(format!("jmp {}", done));
                                self.label(&miss);
                                self.e("xor eax, eax");
                                self.label(&done);
                                return Ok((**el).clone());
                            }
                            self.mk_ok(el);
                            self.e(format!("jmp {}", done));
                            self.label(&miss);
                            let at = self.line;
                            let mk = |kind: ExprKind| Expr { kind, line: at };
                            let lit = |s: &str| Arg { name: None, by_ref: false, spread: false, value: mk(ExprKind::Text(vec![TextPart::Lit(s.into())])) };
                            let kind = mk(ExprKind::Call { callee: Box::new(mk(ExprKind::Ident("__kind".into()))), args: vec![lit("std.InvalidInput")], targs: Vec::new() });
                            let failed = mk(ExprKind::Call {
                                callee: Box::new(mk(ExprKind::Ident("error".into()))),
                                args: vec![Arg { name: None, by_ref: false, spread: false, value: kind }, lit("index out of bounds")],
                                targs: Vec::new(),
                            });
                            self.expr(&failed, None)?;
                            self.mk_err(el);
                            self.label(&done);
                            return Ok(Ty::Result(el.clone()));
                        }
                        if name == "take" {
                            self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
                            self.at("rdx");
                            self.e("call rt_list_take");
                            return Ok((**el).clone());
                        }
                        self.err(format!("{} has no method '{}'", ot.show(), name))
                    })();
                    self.release_slot(t);
                    self.free(t);
                    return res;
                }
                if let Ty::Map(k, v) = &ot {
                    let t = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", t));
                    let res = self.map_method(t, k, v, name, args);
                    self.release_slot(t);
                    self.free(t);
                    return res;
                }
                self.err(format!("{} has no method '{}'", ot.show(), name))
            }
            _ => {
                let _ = want;
                let t = self.expr(callee, None)?;
                self.call_value(t, args)
            }
        }
    }

    /// `has`, `get`, `remove` and `clear` on the map in slot `t` (borrowed).
    pub(super) fn map_method(&mut self, t: i32, k: &Ty, v: &Ty, name: &str, args: &[Arg]) -> R<Ty> {
        let argc = match name {
            "has" | "remove" => 1,
            "get" => 2,
            "clear" => 0,
            _ => return self.err(format!("{} has no method '{}'", Ty::Map(Box::new(k.clone()), Box::new(v.clone())).show(), name)),
        };
        if args.len() != argc {
            return self.err(format!("'{}' takes {} argument(s)", name, argc));
        }
        if name == "clear" {
            self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
            self.e("call rt_map_clear");
            return Ok(Ty::Void);
        }
        let kty = self.value(&args[0].value, Some(k))?;
        self.check(k, &kty, "map key")?;
        let ks = self.temp();
        self.e(format!("mov qword ptr [rbp - {}], rax", ks));
        let res = match name {
            "has" | "remove" => {
                self.e("mov rdx, rax");
                self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
                self.e(if name == "has" { "call rt_map_has" } else { "call rt_map_remove" });
                if name == "has" { Ty::Bool } else { Ty::Void }
            }
            _ => {
                let dty = self.value(&args[1].value, Some(v))?;
                self.check(v, &dty, "default value")?;
                let ds = self.temp();
                self.e(format!("mov qword ptr [rbp - {}], rax", ds));
                let (miss, end) = (self.lbl("mgmiss"), self.lbl("mgend"));
                self.e(format!("mov rcx, qword ptr [rbp - {}]", t));
                self.e(format!("mov rdx, qword ptr [rbp - {}]", ks));
                self.e("call rt_map_find");
                self.e("test rax, rax");
                self.e(format!("jz {}", miss));
                self.e("mov rax, qword ptr [rax + 24]");
                self.retain_if(v);
                if self.is_rc(v) {
                    self.release_slot(ds);
                }
                self.e(format!("jmp {}", end));
                self.label(&miss);
                self.e(format!("mov rax, qword ptr [rbp - {}]", ds));
                self.label(&end);
                self.free(ds);
                v.clone()
            }
        };
        if self.is_rc(k) {
            self.release_slot(ks);
        }
        self.free(ks);
        Ok(res)
    }

    /// Calls the function value in rax (owned); it is released after the call.
    pub(super) fn call_value(&mut self, t: Ty, args: &[Arg]) -> R<Ty> {
        let Ty::Func { params, ret, ext } = t else { return self.err(format!("cannot call a value of type {}", t.show())) };
        if params.len() != args.len() {
            return self.err(format!("expected {} argument(s), found {}", params.len(), args.len()));
        }
        let f = self.temp();
        self.e(format!("mov qword ptr [rbp - {}], rax", f));
        let mut temps = Vec::new();
        for (a, p) in args.iter().zip(&params) {
            let at = self.value(&a.value, Some(p))?;
            self.check(p, &at, "argument")?;
            self.norm(p);
            let t = self.temp();
            self.e(format!("mov qword ptr [rbp - {}], rax", t));
            temps.push(t);
        }
        self.emit_call(&temps, "", Some((f, ext)));
        if !ext {
            let keep = self.temp();
            self.e(format!("mov qword ptr [rbp - {}], rax", keep));
            self.release_slot(f);
            self.e(format!("mov rax, qword ptr [rbp - {}]", keep));
            self.free(keep);
        }
        for t in temps {
            self.free(t);
        }
        self.free(f);
        self.norm(&ret);
        Ok(*ret)
    }

    pub(super) fn call_sig(&mut self, sig: &Sig, receiver: Option<i32>, args: &[Arg]) -> R<Ty> {
        if sig.is_extern {
            self.require_unsafe(&format!("calling extern function '{}'", sig.sym))?;
        }
        // Bind arguments to parameters.
        let n = sig.params.len();
        let mut slots: Vec<Option<&Expr>> = vec![None; n];
        let mut rest: Vec<&Expr> = Vec::new();
        let fixed = if sig.variadic { n - 1 } else { n };
        let mut pos = 0;
        for a in args {
            if a.spread {
                return self.err("spread arguments ('...') are not supported by the native backend yet");
            }
            if let Some(name) = &a.name {
                let Some(i) = sig.params.iter().position(|p| &p.name == name) else {
                    return self.err(format!("no parameter named '{}'", name));
                };
                slots[i] = Some(&a.value);
            } else if pos < fixed {
                slots[pos] = Some(&a.value);
                pos += 1;
            } else if sig.variadic || sig.c_variadic {
                rest.push(&a.value);
            } else {
                return self.err(format!("too many arguments: expected {}", n));
            }
        }
        if self.opt && receiver.is_none() && rest.is_empty() && self.inline_depth < INLINE_DEPTH {
            if let Some((body, bm)) = self.inline_bodies.get(&sig.sym).cloned() {
                return self.inline_call(sig, &slots, &body, &bm);
            }
        }
        let mut temps = Vec::new();
        if let Some(r) = receiver {
            temps.push(r);
        }
        // (slot, type) of owned heap arguments that extern callees do not take over
        let mut extern_owned: Vec<i32> = Vec::new();
        let mut owned = Vec::new();
        // (argument position, bits) of float arguments of an extern call: Windows x64 also
        // wants them in XMM<position>
        let mut float_args: Vec<(usize, u8)> = Vec::new();
        for (i, p) in sig.params.iter().enumerate() {
            if sig.variadic && i == n - 1 {
                let Ty::List(el) = &p.ty else { unreachable!() };
                let lt = self.temp();
                let mut created = false;
                for x in &rest {
                    let vt = self.value(x, Some(el))?;
                    self.check(el, &vt, &format!("argument '{}'", p.name))?;
                    let item = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", item));
                    if !created {
                        self.new_list(&el.clone());
                        self.e(format!("mov qword ptr [rbp - {}], rax", lt));
                        created = true;
                    }
                    self.e(format!("mov rdx, qword ptr [rbp - {}]", item));
                    self.e(format!("mov rcx, qword ptr [rbp - {}]", lt));
                    self.e("call rt_list_push");
                    self.free(item);
                }
                if !created {
                    self.new_list(&el.clone());
                    self.e(format!("mov qword ptr [rbp - {}], rax", lt));
                }
                temps.push(lt);
                owned.push(lt);
                continue;
            }
            let at = match (slots[i], &p.default) {
                (Some(x), _) => {
                    if p.by_ref && matches!(p.ty, Ty::List(_) | Ty::Map(..)) {
                        // the callee gets the address of the caller's own slot and writes through it
                        if self.path_ty(x).is_some() {
                            self.write_place(x)?
                        } else {
                            let vt = self.expr(x, Some(&p.ty))?;
                            self.temp_place();
                            vt
                        }
                    } else if p.by_ref {
                        self.expr(x, Some(&p.ty))?
                    } else {
                        self.value(x, Some(&p.ty))?
                    }
                }
                (None, Some(d)) => {
                    let saved = std::mem::replace(&mut self.cur_mod, sig.module.clone());
                    let r = self.value(d, Some(&p.ty));
                    self.cur_mod = saved;
                    r?
                }
                (None, None) => return self.err(format!("missing argument '{}'", p.name)),
            };
            self.check(&p.ty, &at, &format!("argument '{}'", p.name))?;
            self.norm(&p.ty);
            let t = self.temp();
            self.e(format!("mov qword ptr [rbp - {}], rax", t));
            if let Ty::Float { bits } = p.ty {
                float_args.push((temps.len(), bits));
            }
            temps.push(t);
            owned.push(t);
            if sig.is_extern && self.is_rc(&at) {
                extern_owned.push(t);
            }
        }
        for x in &rest {
            if sig.c_variadic {
                let mut t0 = self.value(x, None)?;
                if t0 == (Ty::Float { bits: 32 }) {
                    // C promotes float to double in variadic calls
                    self.e("movd xmm0, eax");
                    self.e("cvtss2sd xmm0, xmm0");
                    self.e("movq rax, xmm0");
                    t0 = Ty::Float { bits: 64 };
                }
                if t0.is_float() {
                    float_args.push((temps.len(), 64));
                }
                let t = self.temp();
                if t0 == Ty::Text {
                    let own = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", own));
                    extern_owned.push(own);
                    self.e("add rax, 8");
                }
                self.e(format!("mov qword ptr [rbp - {}], rax", t));
                temps.push(t);
                owned.push(t);
            }
        }
        if sig.is_extern {
            self.extern_syms.insert(sig.sym.clone());
        }
        if sig.is_extern && self.target == Target::Linux {
            self.sysv_call(&temps, &float_args, &sig.sym);
        } else {
        for (idx, bits) in float_args.iter().filter(|(i, _)| *i < 4) {
            let reg = ARG_REGS[*idx];
            self.e(format!("mov {}, qword ptr [rbp - {}]", reg, temps[*idx]));
            if *bits == 32 {
                self.e(format!("movd xmm{}, {}", idx, ["ecx", "edx", "r8d", "r9d"][*idx]));
            } else {
                self.e(format!("movq xmm{}, {}", idx, reg));
            }
        }
        let target = if sig.is_extern { format!("call qword ptr GOT_{}", sig.sym) } else { format!("call {}", sig.sym) };
        self.emit_call(&temps, &target, None);
        }
        if sig.is_extern {
            match sig.ret {
                Ty::Float { bits: 32 } => self.e("movd eax, xmm0"),
                Ty::Float { .. } => self.e("movq rax, xmm0"),
                _ => {}
            }
        }
        // C does not release what it is given
        for o in extern_owned {
            self.release_slot(o);
        }
        for t in owned {
            self.free(t);
        }
        self.norm(&sig.ret);
        Ok(sig.ret.clone())
    }

    /// A call of a small function compiled in place: the arguments are evaluated once, in order
    /// (literals and plain local variables are used as they are), then the function's expression
    /// is compiled in a scope holding only its parameters, in its own module.
    pub(super) fn inline_call(&mut self, sig: &Sig, slots: &[Option<&Expr>], body: &Expr, module: &str) -> R<Ty> {
        let mut scope = Scope::default();
        let mut temps = Vec::new();
        for (i, p) in sig.params.iter().enumerate() {
            let Some(x) = slots[i] else { return self.err(format!("missing argument '{}'", p.name)) };
            let aliased = if let Some(v) = literal_bits(x, &p.ty) {
                Some(Local { off: 0, ty: p.ty.clone(), mutable: false, captured: false, reg: None, imm: Some(v), is_ref: false, ind: false })
            } else if let ExprKind::Ident(n) = &x.kind {
                // a later argument that could change the variable forces a copy
                let later_ok = slots[i + 1..].iter().flatten().all(|y| reads_only(y));
                match self.lookup(n) {
                    Some(l) if later_ok && !l.captured && l.ty == p.ty => Some(Local { mutable: false, ..l }),
                    _ => None,
                }
            } else {
                None
            };
            let local = match aliased {
                Some(l) => l,
                None => {
                    let at = self.value(x, Some(&p.ty))?;
                    self.check(&p.ty, &at, &format!("argument '{}'", p.name))?;
                    self.norm(&p.ty);
                    let t = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", t));
                    temps.push(t);
                    Local { off: t, ty: p.ty.clone(), mutable: false, captured: false, reg: None, imm: None, is_ref: false, ind: false }
                }
            };
            scope.vars.insert(p.name.clone(), local);
        }
        let saved_scopes = std::mem::replace(&mut self.fs.scopes, vec![scope]);
        let saved_mod = std::mem::replace(&mut self.cur_mod, module.to_string());
        let saved_bindings = std::mem::take(&mut self.bindings);
        self.inline_depth += 1;
        let r = self.value(body, Some(&sig.ret)).and_then(|t| {
            self.check(&sig.ret, &t, "return value")?;
            self.norm(&sig.ret);
            Ok(())
        });
        self.inline_depth -= 1;
        self.bindings = saved_bindings;
        self.cur_mod = saved_mod;
        // heap values the body's calls left to be released belong to the caller's scope now
        let inner = std::mem::replace(&mut self.fs.scopes, saved_scopes);
        if let Some(sc) = self.fs.scopes.last_mut() {
            sc.owned.extend(inner.into_iter().flat_map(|s| s.owned));
        }
        for t in temps {
            self.free(t);
        }
        r?;
        Ok(sig.ret.clone())
    }

    /// Built-in `c` module: `c.cstr`, `c.from_cstr`, `c.alloc`, `c.free`.
    pub(super) fn c_intrinsic(&mut self, name: &str, args: &[Arg]) -> R<Ty> {
        let one = |s: &Self| -> R<()> {
            if args.len() == 1 {
                Ok(())
            } else {
                s.err(format!("c.{}() takes one argument", name))
            }
        };
        let u8p = Ty::Ptr(Box::new(Ty::Int { bits: 8, signed: false }));
        match name {
            "cstr" => {
                one(self)?;
                let t = self.expr(&args[0].value, Some(&Ty::Text))?;
                self.check(&Ty::Text, &t, "c.cstr")?;
                // the text must outlive the call it is passed to: it is
                // released at the end of the statement
                let keep = self.slot();
                self.e(format!("mov qword ptr [rbp - {}], rax", keep));
                self.fs.stmt_temps.push(keep);
                self.fs.may_own = true;
                self.e("add rax, 8");
                Ok(u8p)
            }
            "from_cstr" => {
                one(self)?;
                let (t, _) = self.operand(&args[0].value, None)?;
                self.check(&u8p, &t, "c.from_cstr")?;
                self.e("mov rcx, rax");
                self.e("call rt_text_from_cstr");
                Ok(Ty::Text)
            }
            "from_bytes" => {
                if args.len() != 2 {
                    return self.err("c.from_bytes(pointer, length) takes two arguments");
                }
                let (pt, _) = self.operand(&args[0].value, None)?;
                self.check(&u8p, &pt, "c.from_bytes")?;
                let p = self.temp();
                self.e(format!("mov qword ptr [rbp - {}], rax", p));
                let lt = self.expr(&args[1].value, Some(&Ty::i64()))?;
                if !lt.is_int() {
                    return self.err("c.from_bytes needs a number of bytes");
                }
                self.e("mov rdx, rax");
                self.e(format!("mov rcx, qword ptr [rbp - {}]", p));
                self.e("call rt_text_from_bytes");
                self.free(p);
                Ok(Ty::Text)
            }
            "alloc" => {
                one(self)?;
                self.expr(&args[0].value, Some(&Ty::Int { bits: 64, signed: false }))?;
                self.e("mov rcx, rax");
                self.e("xor edx, edx");
                self.e("call rt_alloc");
                Ok(u8p)
            }
            "free" => {
                one(self)?;
                self.expr(&args[0].value, None)?;
                self.e("mov rcx, rax");
                self.e("call rt_release");
                Ok(Ty::Void)
            }
            _ => self.err(format!("unknown function 'c.{}'", name)),
        }
    }

    // ---------------- defer ----------------

}
