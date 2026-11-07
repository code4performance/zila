//! The standard interfaces `Eq`, `Ord` and `Hash` (spec `standard-interfaces`).
//!
//! A type has `eq`, `compare` or `hash` when the program declares the method or the compiler
//! derives it. Derived methods are ordinary methods written as Brasa source for each type (field
//! by field, variant by variant) and compiled on first need, with the type bound to `__T`. They
//! are built on the intrinsics `__eq`, `__cmp`, `__hash`, `__tag`, `__payload` and `__mix`, which
//! know the built-in types. Section 2 of the design says the derivation is comptime code on top
//! of reflection; reflection has not landed, so the compiler expands it itself with the same
//! behavior.

use super::*;

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Cap {
    Eq,
    Ord,
    Hash,
}

impl Cap {
    pub(super) fn of(method: &str) -> Cap {
        match method {
            "compare" => Cap::Ord,
            "hash" => Cap::Hash,
            _ => Cap::Eq,
        }
    }
    pub(super) fn name(self) -> &'static str {
        match self {
            Cap::Eq => "Eq",
            Cap::Ord => "Ord",
            Cap::Hash => "Hash",
        }
    }
    pub(super) fn method(self) -> &'static str {
        match self {
            Cap::Eq => "eq",
            Cap::Ord => "compare",
            Cap::Hash => "hash",
        }
    }
}

const HASH_K: i64 = 0x9E3779B97F4A7C15u64 as i64;

/// A number for a type name: the starting value of a derived hash.
fn type_seed(name: &str) -> i64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in name.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    (h >> 2) as i64
}

impl Compiler {
    /// The method `name` the program declared on `ty` (not one the compiler derived).
    pub(super) fn user_method(&self, ty: &Ty, name: &str) -> Option<Sig> {
        let key = (method_key(ty), name.to_string());
        if self.derived.contains(&key) {
            return None;
        }
        self.methods.get(&key).cloned()
    }

    fn is_identity(&self, ty: &Ty) -> bool {
        matches!(ty, Ty::Rec(q) if self.shared_recs.contains(q))
    }

    /// Why `ty` is not `cap`, or `None` when it is. The flag is true for a basic reason that reads
    /// after "is not Hash"; false for a complete clause.
    pub(super) fn lacks(&self, ty: &Ty, cap: Cap) -> Option<(bool, String)> {
        self.lacks_in(ty, cap, &mut Vec::new())
    }

    fn lacks_in(&self, ty: &Ty, cap: Cap, seen: &mut Vec<(String, Cap)>) -> Option<(bool, String)> {
        let because = |inner: Option<(bool, String)>| -> String {
            match inner {
                Some((_, w)) => format!(" because {}", w),
                None => String::new(),
            }
        };
        match ty {
            Ty::Int { .. } | Ty::Char | Ty::Bool | Ty::Text | Ty::Param { .. } => return None,
            Ty::Float { .. } => {
                return if cap == Cap::Hash { Some((true, "NaN is not equal to itself".into())) } else { None };
            }
            Ty::Rec(_) | Ty::Enum(_) | Ty::List(_) | Ty::Map(..) => {}
            other => return Some((true, format!("{} has no equality or order", other.show()))),
        }
        // methods the program declared
        if matches!(ty, Ty::Rec(_) | Ty::Enum(_)) {
            if let Some(sg) = self.user_method(ty, cap.method()) {
                let ok_ret = match cap {
                    Cap::Eq => sg.ret == Ty::Bool,
                    Cap::Ord => sg.ret == Ty::int(),
                    Cap::Hash => sg.ret == Ty::i64(),
                };
                let ok_params = if cap == Cap::Hash { sg.params.is_empty() } else { sg.params.len() == 1 && sg.params[0].ty == *ty };
                if !(ok_ret && ok_params) {
                    return Some((false, format!("`{}` has the wrong signature for {}", cap.method(), cap.name())));
                }
                return None;
            }
            if cap == Cap::Hash {
                if let Some(sg) = self.user_method(ty, "eq") {
                    if sg.ret != Ty::Bool || sg.params.len() != 1 || sg.params[0].ty != *ty {
                        return Some((false, "`eq` has the wrong signature for Eq".into()));
                    }
                    return Some((false, "it defines eq but not hash".into()));
                }
            }
        }
        let key = (ty_full(ty), cap);
        if seen.contains(&key) {
            return None;
        }
        seen.push(key);
        let r = match ty {
            Ty::Rec(q) => {
                if self.shared_recs.contains(q) {
                    if cap == Cap::Ord {
                        Some((false, "it is held by reference, so it is only ordered by a `compare` method".to_string()))
                    } else {
                        None
                    }
                } else {
                    let mut found = None;
                    if let Some(info) = self.records.get(q) {
                        for (n, fty, _) in &info.fields {
                            if let Some(inner) = self.lacks_in(fty, cap, seen) {
                                found = Some((false, format!("field `{}: {}` is not {}{}", n, fty.show(), cap.name(), because(Some(inner)))));
                                break;
                            }
                        }
                    }
                    found
                }
            }
            Ty::Enum(q) => {
                let mut found = None;
                if let Some(info) = self.enums.get(q) {
                    'v: for (vn, tys) in &info.variants {
                        for t in tys {
                            if let Some(inner) = self.lacks_in(t, cap, seen) {
                                found = Some((false, format!("variant `{}` holds {} which is not {}{}", vn, t.show(), cap.name(), because(Some(inner)))));
                                break 'v;
                            }
                        }
                    }
                }
                found
            }
            Ty::List(e) => self
                .lacks_in(e, cap, seen)
                .map(|inner| (false, format!("its element type {} is not {}{}", e.show(), cap.name(), because(Some(inner))))),
            Ty::Map(_, v) => {
                if cap == Cap::Ord {
                    Some((false, "a map has no order".to_string()))
                } else {
                    self.lacks_in(v, cap, seen)
                        .map(|inner| (false, format!("its value type {} is not {}{}", v.show(), cap.name(), because(Some(inner)))))
                }
            }
            _ => None,
        };
        seen.pop();
        r
    }

    /// The method `name` (`eq`, `compare` or `hash`) of `ty`: the program's, or the derived one,
    /// written and queued on first need. `None` when the type does not have it.
    pub(super) fn std_sig(&mut self, ty: &Ty, name: &str) -> R<Option<Sig>> {
        if !matches!(name, "eq" | "compare" | "hash") {
            return Ok(None);
        }
        let key = (method_key(ty), name.to_string());
        if let Some(s) = self.methods.get(&key) {
            return Ok(Some(s.clone()));
        }
        if !self.declared {
            return Ok(None);
        }
        let cap = Cap::of(name);
        if !matches!(ty, Ty::Int { .. } | Ty::Char | Ty::Bool | Ty::Text | Ty::Float { .. } | Ty::Rec(_) | Ty::Enum(_) | Ty::List(_) | Ty::Map(..)) {
            return Ok(None);
        }
        if self.lacks(ty, cap).is_some() {
            return Ok(None);
        }
        let module = match ty {
            Ty::Rec(q) => self.rec_decls.get(q).map(|d| d.0.clone()),
            Ty::Enum(q) => self.enum_decls.get(q).map(|d| d.0.clone()),
            _ => None,
        }
        .unwrap_or_else(|| self.modules[0].name.clone());
        let src = self.derive_source(ty, cap);
        let file = parse_source(&format!("package {}\n{}", module, src)).map_err(|d| format!("derived {} for {}: {}", name, ty.show(), d.msg))?;
        let fd = file
            .decls
            .into_iter()
            .find_map(|d| if let DeclKind::Func(f) = d.kind { Some(f) } else { None })
            .expect("derived function");
        let mut binds = HashMap::new();
        binds.insert("__T".to_string(), ty.clone());
        let saved = std::mem::replace(&mut self.bindings, binds.clone());
        let saved_mod = std::mem::replace(&mut self.cur_mod, module.clone());
        let sig = self.func_sig(&module, &fd);
        self.bindings = saved;
        self.cur_mod = saved_mod;
        let mut sig = sig?;
        sig.public = true;
        self.methods.insert(key.clone(), sig.clone());
        self.derived.insert(key);
        self.pending.push(Pending { module, decl: fd, bindings: binds, line: self.line });
        Ok(Some(sig))
    }

    /// The Brasa source of the derived method `cap` for `ty`.
    fn derive_source(&self, ty: &Ty, cap: Cap) -> String {
        let head = match cap {
            Cap::Eq => "func (a: __T) eq(b: __T) -> bool\n",
            Cap::Ord => "func (a: __T) compare(b: __T) -> int\n",
            Cap::Hash => "func (a: __T) hash() -> long\n",
        };
        let seed = type_seed(&ty_full(ty));
        let mut s = String::from(head);
        let scalar = |cap: Cap| match cap {
            Cap::Eq => "    return __eq(a, b)\n",
            Cap::Ord => "    return __cmp(a, b)\n",
            Cap::Hash => "    return __hash(a)\n",
        };
        match ty {
            Ty::Int { .. } | Ty::Char | Ty::Bool | Ty::Text | Ty::Float { .. } => s.push_str(scalar(cap)),
            Ty::Enum(q) if !self.enum_payload.contains(q) => s.push_str(scalar(cap)),
            Ty::Rec(q) if self.shared_recs.contains(q) => {
                s.push_str(match cap {
                    Cap::Ord => "    return 0\n",
                    _ => scalar(cap),
                });
            }
            Ty::Rec(q) => {
                let fields: Vec<String> = self.records.get(q).map(|i| i.fields.iter().map(|f| f.0.clone()).collect()).unwrap_or_default();
                match cap {
                    Cap::Eq => {
                        let parts: Vec<String> = fields.iter().map(|f| format!("__eq(a.{f}, b.{f})")).collect();
                        s.push_str(&format!("    return {}\n", if parts.is_empty() { "true".to_string() } else { parts.join(" and ") }));
                    }
                    Cap::Ord => {
                        s.push_str("    var c = 0\n");
                        for f in &fields {
                            s.push_str(&format!("    c = __cmp(a.{f}, b.{f})\n    if c != 0\n        return c\n    end\n"));
                        }
                        s.push_str("    return 0\n");
                    }
                    Cap::Hash => {
                        s.push_str(&format!("    var h: long = {}\n", seed));
                        for f in &fields {
                            s.push_str(&format!("    h = __mix(h, __hash(a.{f}))\n"));
                        }
                        s.push_str("    return h\n");
                    }
                }
            }
            Ty::Enum(q) => {
                let variants: Vec<Vec<Ty>> = self.enums.get(q).map(|i| i.variants.iter().map(|v| v.1.clone()).collect()).unwrap_or_default();
                match cap {
                    Cap::Eq => {
                        s.push_str("    let t = __tag(a)\n    if t != __tag(b)\n        return false\n    end\n");
                        for (i, tys) in variants.iter().enumerate() {
                            if tys.is_empty() {
                                continue;
                            }
                            let parts: Vec<String> = (0..tys.len()).map(|j| format!("__eq(__payload(a, {i}, {j}), __payload(b, {i}, {j}))")).collect();
                            s.push_str(&format!("    if t == {i}\n        return {}\n    end\n", parts.join(" and ")));
                        }
                        s.push_str("    return true\n");
                    }
                    Cap::Ord => {
                        s.push_str("    let ta = __tag(a)\n    let tb = __tag(b)\n    if ta < tb\n        return -1\n    end\n    if ta > tb\n        return 1\n    end\n    var c = 0\n");
                        for (i, tys) in variants.iter().enumerate() {
                            if tys.is_empty() {
                                continue;
                            }
                            s.push_str(&format!("    if ta == {i}\n"));
                            for j in 0..tys.len() {
                                s.push_str(&format!("        c = __cmp(__payload(a, {i}, {j}), __payload(b, {i}, {j}))\n        if c != 0\n            return c\n        end\n"));
                            }
                            s.push_str("    end\n");
                        }
                        s.push_str("    return 0\n");
                    }
                    Cap::Hash => {
                        s.push_str(&format!("    let t = __tag(a)\n    var h: long = __mix({}, t)\n", seed));
                        for (i, tys) in variants.iter().enumerate() {
                            if tys.is_empty() {
                                continue;
                            }
                            s.push_str(&format!("    if t == {i}\n"));
                            for j in 0..tys.len() {
                                s.push_str(&format!("        h = __mix(h, __hash(__payload(a, {i}, {j})))\n"));
                            }
                            s.push_str("    end\n");
                        }
                        s.push_str("    return h\n");
                    }
                }
            }
            Ty::List(_) => match cap {
                Cap::Eq => s.push_str(
                    "    if a.len != b.len\n        return false\n    end\n    var i = 0\n    while i < a.len\n        if not __eq(a[i], b[i])\n            return false\n        end\n        i += 1\n    end\n    return true\n",
                ),
                Cap::Ord => s.push_str(
                    "    var i = 0\n    while i < a.len and i < b.len\n        let c = __cmp(a[i], b[i])\n        if c != 0\n            return c\n        end\n        i += 1\n    end\n    if a.len < b.len\n        return -1\n    end\n    if a.len > b.len\n        return 1\n    end\n    return 0\n",
                ),
                Cap::Hash => s.push_str(&format!(
                    "    var h: long = {}\n    var i = 0\n    while i < a.len\n        h = __mix(h, __hash(a[i]))\n        i += 1\n    end\n    return __mix(h, a.len)\n",
                    seed
                )),
            },
            Ty::Map(..) => match cap {
                Cap::Eq => s.push_str(
                    "    if a.len != b.len\n        return false\n    end\n    for k, v in a\n        if not b.has(k)\n            return false\n        end\n        if not __eq(v, b[k])\n            return false\n        end\n    end\n    return true\n",
                ),
                _ => s.push_str(&format!(
                    "    var h: long = {}\n    for k, v in a\n        h = h + __mix(__hash(k), __hash(v))\n    end\n    return h\n",
                    seed
                )),
            },
            _ => s.push_str("    return 0\n"),
        }
        s.push_str("end\n");
        s
    }

    /// Emits `eq` (rax, rcx borrowed -> bool), `compare` (-> int) or `hash` (rax -> long) for
    /// values of type `ty`.
    pub(super) fn emit_std(&mut self, cap: Cap, ty: &Ty) -> R<Ty> {
        let result = match cap {
            Cap::Eq => Ty::Bool,
            Cap::Ord => Ty::int(),
            Cap::Hash => Ty::i64(),
        };
        let int_like = ty.is_int() || matches!(ty, Ty::Char | Ty::Bool) || matches!(ty, Ty::Enum(q) if !self.enum_payload.contains(q));
        match ty {
            Ty::Param { .. } => {
                self.e("xor eax, eax");
            }
            _ if int_like && self.user_method(ty, cap.method()).is_none() => match cap {
                Cap::Eq => {
                    self.e("cmp rax, rcx");
                    self.e("sete al");
                    self.e("movzx eax, al");
                }
                Cap::Ord => {
                    self.e("cmp rax, rcx");
                    if ty.is_unsigned() {
                        self.e("seta al");
                        self.e("setb cl");
                    } else {
                        self.e("setg al");
                        self.e("setl cl");
                    }
                    self.e("sub al, cl");
                    self.e("movsx eax, al");
                }
                Cap::Hash => self.mix_int(),
            },
            Ty::Float { bits } => match cap {
                Cap::Eq => {
                    self.float_binary("==", ty, ty, true)?;
                }
                Cap::Hash => return self.err("float is not Hash: NaN is not equal to itself"),
                Cap::Ord => {
                    let d = *bits == 64;
                    let (mv, uc) = if d { ("movq", "ucomisd") } else { ("movd", "ucomiss") };
                    let (x, y) = if d { ("rax", "rcx") } else { ("eax", "ecx") };
                    let (nan, gt, lt, anan, both, end) =
                        (self.lbl("fcn"), self.lbl("fcg"), self.lbl("fcl"), self.lbl("fca"), self.lbl("fcb"), self.lbl("fce"));
                    self.e(format!("{} xmm0, {}", mv, x));
                    self.e(format!("{} xmm1, {}", mv, y));
                    self.e(format!("{} xmm0, xmm1", uc));
                    self.e(format!("jp {}", nan));
                    self.e(format!("ja {}", gt));
                    self.e(format!("jb {}", lt));
                    self.e("xor eax, eax");
                    self.e(format!("jmp {}", end));
                    self.label(&gt);
                    self.e("mov eax, 1");
                    self.e(format!("jmp {}", end));
                    self.label(&lt);
                    self.e("mov eax, -1");
                    self.e(format!("jmp {}", end));
                    self.label(&nan);
                    self.e(format!("{} xmm0, xmm0", uc));
                    self.e(format!("jp {}", anan));
                    self.e("mov eax, -1");
                    self.e(format!("jmp {}", end));
                    self.label(&anan);
                    self.e(format!("{} xmm1, xmm1", uc));
                    self.e(format!("jp {}", both));
                    self.e("mov eax, 1");
                    self.e(format!("jmp {}", end));
                    self.label(&both);
                    self.e("xor eax, eax");
                    self.label(&end);
                    self.norm(&Ty::int());
                }
            },
            Ty::Text => match cap {
                Cap::Eq => {
                    self.e("mov rdx, rcx");
                    self.e("mov rcx, rax");
                    self.e("call rt_text_eq");
                }
                Cap::Ord => {
                    self.e("mov rdx, rcx");
                    self.e("mov rcx, rax");
                    self.e("call rt_dv_text_cmp");
                    self.norm(&Ty::int());
                }
                Cap::Hash => {
                    self.e("mov rcx, rax");
                    self.e("call rt_dv_text_hash");
                }
            },
            Ty::Rec(_) | Ty::Enum(_) | Ty::List(_) | Ty::Map(..) => {
                // `==` of a type with a `compare` only: equal when the order says so
                if cap == Cap::Eq && self.user_method(ty, "eq").is_none() && self.lacks(ty, Cap::Eq).is_some() && self.lacks(ty, Cap::Ord).is_none() {
                    self.emit_std(Cap::Ord, ty)?;
                    self.e("test eax, eax");
                    self.e("sete al");
                    self.e("movzx eax, al");
                    return Ok(result);
                }
                self.emit_method_op(cap, ty)?;
            }
            other => return self.err(format!("{} has no {}", other.show(), cap.method())),
        }
        Ok(result)
    }

    /// The integer hash: a multiplicative mix of the 64-bit value in rax.
    fn mix_int(&mut self) {
        self.e(format!("mov rdx, {}", imm64(HASH_K)));
        self.e("imul rax, rdx");
        self.e("mov rdx, rax");
        self.e("shr rdx, 32");
        self.e("xor rax, rdx");
    }

    /// `eq`, `compare` or `hash` of a record, enum with data, list or map through its method.
    fn emit_method_op(&mut self, cap: Cap, ty: &Ty) -> R<()> {
        if self.is_identity(ty) && self.user_method(ty, cap.method()).is_none() {
            match cap {
                Cap::Eq => {
                    self.e("cmp rax, rcx");
                    self.e("sete al");
                    self.e("movzx eax, al");
                }
                Cap::Hash => self.mix_int(),
                Cap::Ord => return self.err(format!("{} is not Ord: it is held by reference, so it is only ordered by a `compare` method", ty.show())),
            }
            return Ok(());
        }
        let Some(sig) = self.std_sig(ty, cap.method())? else {
            let why = self.lacks(ty, cap).map(|(_, w)| format!(" because {}", w)).unwrap_or_default();
            return self.err(format!("{} is not {}{}", ty.show(), cap.name(), why));
        };
        let (t1, t2) = (self.temp(), self.temp());
        let end = self.lbl("sdend");
        match cap {
            Cap::Hash => {
                let zero = self.lbl("sdzero");
                self.e("test rax, rax");
                self.e(format!("jz {}", zero));
                self.e(format!("mov qword ptr [rbp - {}], rax", t1));
                self.retain();
                self.emit_call(&[t1], &format!("call {}", sig.sym), None);
                self.e(format!("jmp {}", end));
                self.label(&zero);
                self.e("xor eax, eax");
            }
            _ => {
                let (same, left_nil, right_nil) = (self.lbl("sdsame"), self.lbl("sdln"), self.lbl("sdrn"));
                self.e(format!("mov qword ptr [rbp - {}], rax", t1));
                self.e(format!("mov qword ptr [rbp - {}], rcx", t2));
                self.e("cmp rax, rcx");
                self.e(format!("je {}", same));
                self.e("test rax, rax");
                self.e(format!("jz {}", left_nil));
                self.e("test rcx, rcx");
                self.e(format!("jz {}", right_nil));
                self.retain();
                self.e(format!("mov rcx, qword ptr [rbp - {}]", t2));
                self.e("call rt_retain");
                self.emit_call(&[t1, t2], &format!("call {}", sig.sym), None);
                self.e(format!("jmp {}", end));
                self.label(&same);
                self.e(if cap == Cap::Eq { "mov eax, 1" } else { "xor eax, eax" });
                self.e(format!("jmp {}", end));
                self.label(&left_nil);
                self.e(if cap == Cap::Eq { "xor eax, eax" } else { "mov eax, -1" });
                self.e(format!("jmp {}", end));
                self.label(&right_nil);
                self.e(if cap == Cap::Eq { "xor eax, eax" } else { "mov eax, 1" });
            }
        }
        self.label(&end);
        match cap {
            Cap::Eq => self.e("movzx eax, al"),
            Cap::Ord => self.norm(&Ty::int()),
            Cap::Hash => {}
        }
        self.free(t2);
        self.free(t1);
        Ok(())
    }

    /// The hash and equality thunks the runtime map calls for keys of type `k`.
    pub(super) fn map_thunks_for(&mut self, k: &Ty) -> R<(String, String)> {
        let key = ty_full(k);
        if let Some(p) = self.map_thunks.get(&key) {
            return Ok(p.clone());
        }
        let id = mangle(&key);
        let pair = (format!("B_maphash_{}", id), format!("B_mapeq_{}", id));
        self.map_thunks.insert(key, pair.clone());
        self.gen_thunk(&pair.0, k, Cap::Hash)?;
        self.gen_thunk(&pair.1, k, Cap::Eq)?;
        Ok(pair)
    }

    /// A procedure for the runtime: `rcx` (and `rdx`) borrowed keys in, hash or bool out.
    fn gen_thunk(&mut self, sym: &str, k: &Ty, cap: Cap) -> R<()> {
        let rl = self.lbl("thunkret");
        let saved = std::mem::replace(&mut self.fs, FnState::new(Ty::Void, rl));
        self.e("mov rax, rcx");
        if cap == Cap::Eq {
            self.e("mov rcx, rdx");
        }
        let r = self.emit_std(cap, k);
        let p = self.finish_proc(sym, Vec::new(), false);
        self.fs = saved;
        r?;
        self.procs.push(p);
        Ok(())
    }

    /// `__eq`, `__cmp`, `__hash`, `__tag`, `__payload` and `__mix`: what derived methods are made of.
    pub(super) fn std_intrinsic(&mut self, name: &str, args: &[Arg]) -> R<Ty> {
        let want = if matches!(name, "__eq" | "__cmp" | "__mix") {
            2
        } else if name == "__payload" {
            3
        } else {
            1
        };
        if args.len() != want {
            return self.err(format!("{}() takes {} argument(s)", name, want));
        }
        match name {
            "__mix" => {
                let a = self.expr(&args[0].value, Some(&Ty::i64()))?;
                let s = self.temp();
                self.e(format!("mov qword ptr [rbp - {}], rax", s));
                let b = self.expr(&args[1].value, Some(&Ty::i64()))?;
                if !a.is_int() || !b.is_int() {
                    self.free(s);
                    return self.err("__mix() takes two numbers");
                }
                self.e("mov rcx, rax");
                self.e(format!("mov rax, qword ptr [rbp - {}]", s));
                self.free(s);
                self.e("xor rax, rcx");
                self.e(format!("mov rdx, {}", imm64(HASH_K)));
                self.e("imul rax, rdx");
                self.e("mov rdx, rax");
                self.e("shr rdx, 32");
                self.e("xor rax, rdx");
                Ok(Ty::i64())
            }
            "__tag" => {
                let (t, own) = self.operand(&args[0].value, None)?;
                let Ty::Enum(q) = &t else { return self.err("__tag() takes an enum") };
                if self.enum_payload.contains(q) {
                    let s = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", s));
                    self.e("mov rax, qword ptr [rax]");
                    if own {
                        let k = self.temp();
                        self.e(format!("mov qword ptr [rbp - {}], rax", k));
                        self.release_slot(s);
                        self.e(format!("mov rax, qword ptr [rbp - {}]", k));
                        self.free(k);
                    }
                    self.free(s);
                }
                Ok(Ty::int())
            }
            "__payload" => {
                let (t, own) = self.operand(&args[0].value, None)?;
                let (ExprKind::Int(v), ExprKind::Int(i)) = (&args[1].value.kind, &args[2].value.kind) else {
                    return self.err("__payload() takes an enum and two numbers");
                };
                let Ty::Enum(q) = &t else { return self.err("__payload() takes an enum") };
                let ty = self.enums[q].variants[*v as usize].1[*i as usize].clone();
                let s = self.temp();
                self.e(format!("mov qword ptr [rbp - {}], rax", s));
                self.e(format!("mov rax, qword ptr [rax + {}]", 8 + 8 * *i));
                self.retain_if(&ty);
                if own {
                    let k = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", k));
                    self.release_slot(s);
                    self.e(format!("mov rax, qword ptr [rbp - {}]", k));
                    self.free(k);
                }
                self.free(s);
                Ok(ty)
            }
            _ => {
                let cap = match name {
                    "__eq" => Cap::Eq,
                    "__cmp" => Cap::Ord,
                    _ => Cap::Hash,
                };
                let (ta, oa) = self.operand(&args[0].value, None)?;
                let sa = self.temp();
                self.e(format!("mov qword ptr [rbp - {}], rax", sa));
                let mut ob = false;
                let sb = self.temp();
                if cap != Cap::Hash {
                    let (tb, o) = self.operand(&args[1].value, Some(&ta))?;
                    ob = o;
                    if !(compatible(&ta, &tb) || compatible(&tb, &ta)) {
                        return self.err(format!("cannot compare {} with {}", ta.show(), tb.show()));
                    }
                    self.e(format!("mov qword ptr [rbp - {}], rax", sb));
                    self.e("mov rcx, rax");
                    self.e(format!("mov rax, qword ptr [rbp - {}]", sa));
                }
                let res = self.emit_std(cap, &ta)?;
                let rel_a = oa && self.is_rc(&ta);
                let rel_b = ob && self.is_rc(&ta);
                if rel_a || rel_b {
                    let k = self.temp();
                    self.e(format!("mov qword ptr [rbp - {}], rax", k));
                    if rel_a {
                        self.release_slot(sa);
                    }
                    if rel_b {
                        self.release_slot(sb);
                    }
                    self.e(format!("mov rax, qword ptr [rbp - {}]", k));
                    self.free(k);
                }
                self.free(sb);
                self.free(sa);
                Ok(res)
            }
        }
    }

    /// `==`, `!=`, `<`, `<=`, `>`, `>=` on records, enums with data, lists and maps (calling the
    /// type's `eq` or `compare`) and on a type parameter whose bound provides them.
    /// rax = lhs, rcx = rhs. `None` when the operands are not such a case.
    pub(super) fn std_operator(&mut self, op: &str, lt: &Ty, rt: &Ty) -> R<Option<Ty>> {
        let eq_op = matches!(op, "==" | "!=");
        if let (Ty::Param { name: a, bound }, Ty::Param { name: b, .. }) = (lt, rt) {
            if a != b {
                return Ok(None);
            }
            let provides = match bound.as_deref() {
                Some("std.Ord") => true,
                Some("std.Eq") | Some("std.Hash") => eq_op,
                _ => false,
            };
            if !provides {
                return if eq_op {
                    self.err(format!(
                        "operator '{}' is not available for {}: a type parameter only has the methods of its bound; add the bound `{}: Eq`",
                        op, a, a
                    ))
                } else if bound.is_some() {
                    self.err(format!("operator '{}' is not available for {}: its bound does not provide it; it needs `{}: Ord`", op, a, a))
                } else {
                    self.err(format!(
                        "operator '{}' is not available for {}: a type parameter only has the methods of its bound; add the bound `{}: Ord`",
                        op, a, a
                    ))
                };
            }
            self.e("xor eax, eax");
            return Ok(Some(Ty::Bool));
        }
        let composite = match (lt, rt) {
            (Ty::Rec(a), Ty::Rec(b)) => a == b,
            (Ty::Enum(a), Ty::Enum(b)) => a == b && self.enum_payload.contains(a),
            (Ty::List(_), Ty::List(_)) | (Ty::Map(..), Ty::Map(..)) => lt == rt,
            _ => false,
        };
        if !composite {
            return Ok(None);
        }
        let cap = if eq_op { Cap::Eq } else { Cap::Ord };
        let blocked = self.lacks(lt, cap);
        let via_order = cap == Cap::Eq && blocked.is_some() && self.user_method(lt, "eq").is_none() && self.lacks(lt, Cap::Ord).is_none();
        if let (Some((_, why)), false) = (&blocked, via_order) {
            return self.err(format!("operator '{}' is not defined for {}: {} is not {} because {}", op, lt.show(), lt.show(), cap.name(), why));
        }
        if eq_op {
            self.emit_std(Cap::Eq, lt)?;
            if op == "!=" {
                self.e("xor eax, 1");
            }
        } else {
            self.emit_std(Cap::Ord, lt)?;
            self.e("test rax, rax");
            let cc = match op {
                "<" => "l",
                "<=" => "le",
                ">" => "g",
                _ => "ge",
            };
            self.e(format!("set{} al", cc));
            self.e("movzx eax, al");
        }
        Ok(Some(Ty::Bool))
    }
}
