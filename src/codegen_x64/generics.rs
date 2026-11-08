//! Part of the code generator (`codegen_x64`): see the section comment below.

use super::*;

// ---------------------------------------------------------------------------
// Generics and interfaces
//
// A generic function or record is a template. Using it with concrete types makes an *instance*:
// the template's declaration compiled with its type parameters bound to those types
// (`Compiler::bindings`), under a name that includes the type arguments. Each template is also
// compiled once with opaque `Ty::Param` types, so mistakes are reported at the definition.
// ---------------------------------------------------------------------------

/// Full spelling of a type, with module prefixes: the key of an instance.
pub(super) fn ty_full(t: &Ty) -> String {
    match t {
        Ty::Rec(n) | Ty::Enum(n) => n.clone(),
        Ty::List(e) => format!("[]{}", ty_full(e)),
        Ty::Map(k, v) => format!("Map<{},{}>", ty_full(k), ty_full(v)),
        Ty::Ptr(e) => format!("*{}", ty_full(e)),
        Ty::Chan(e) => format!("Channel<{}>", ty_full(e)),
        Ty::Future(e) => format!("Future<{}>", ty_full(e)),
        Ty::Rc(e) => format!("Rc<{}>", ty_full(e)),
        Ty::Weak(e) => format!("Weak<{}>", ty_full(e)),
        Ty::Result(e) => format!("{}!", ty_full(e)),
        other => other.show(),
    }
}

/// Does the type mention one of the type parameters?
pub(super) fn mentions_param(t: &Type, names: &[String]) -> bool {
    match t {
        Type::Named { path, args } => {
            (path.len() == 1 && args.is_empty() && names.contains(&path[0])) || args.iter().any(|a| mentions_param(a, names))
        }
        Type::Optional(i) | Type::Shared(i) | Type::Pointer(i) | Type::Slice(i) | Type::Array(_, i) => mentions_param(i, names),
        Type::Func { params, ret, .. } => {
            params.iter().any(|p| mentions_param(p, names))
                || ret.as_ref().map_or(false, |r| r.ty.as_ref().map_or(false, |t| mentions_param(t, names)))
        }
    }
}

pub(super) const INSTANCE_LIMIT: usize = 2000;

impl Compiler {
    /// Qualified name of the generic record whose methods `fd` belongs to.
    pub(super) fn generic_receiver(&self, m: &str, fd: &FuncDecl) -> Option<String> {
        let r = fd.receiver.as_ref()?;
        let Type::Named { path, args } = &r.ty else { return None };
        if args.is_empty() {
            return None;
        }
        let q = self.qname(m, path);
        self.tmpl_recs.contains_key(&q).then_some(q)
    }

    pub(super) fn snapshot(&mut self) -> Snapshot {
        Snapshot {
            fs: self.fs.clone(),
            procs: self.procs.len(),
            data: self.data.len(),
            strings: self.strings.clone(),
            records: self.tp.records.clone(),
            rec_decls: self.tp.rec_decls.clone(),
            shared_recs: self.tp.shared_recs.clone(),
            methods: self.tp.methods.clone(),
            funcs: self.tp.funcs.clone(),
            inst_args: self.inst_args.clone(),
            inst_funcs: self.inst_funcs.clone(),
            pending: std::mem::take(&mut self.pending),
            fn_closures: self.fn_closures.clone(),
            bindings: self.bindings.clone(),
            cur_mod: self.cur_mod.clone(),
            line: self.line,
            derived: self.tp.derived.clone(),
            map_thunks: self.map_thunks.clone(),
        }
    }

    pub(super) fn restore(&mut self, s: Snapshot) {
        self.fs = s.fs;
        self.procs.truncate(s.procs);
        self.data.truncate(s.data);
        self.strings = s.strings;
        self.tp.records = s.records;
        self.tp.rec_decls = s.rec_decls;
        self.tp.shared_recs = s.shared_recs;
        self.tp.methods = s.methods;
        self.tp.funcs = s.funcs;
        self.inst_args = s.inst_args;
        self.inst_funcs = s.inst_funcs;
        self.pending = s.pending;
        self.fn_closures = s.fn_closures;
        self.bindings = s.bindings;
        self.cur_mod = s.cur_mod;
        self.line = s.line;
        self.tp.derived = s.derived;
        self.map_thunks = s.map_thunks;
    }

    pub(super) fn resolve_ret(&mut self, r: &Option<RetType>, m: &str) -> R<Ty> {
        match r {
            Some(r) => {
                let inner = match &r.ty {
                    Some(t) => self.resolve(t, m)?,
                    None => Ty::Void,
                };
                Ok(if r.fallible { Ty::Result(Box::new(inner)) } else { inner })
            }
            None => Ok(Ty::Void),
        }
    }

    // ---------------- interfaces ----------------

    /// The qualified interface a bound (`Printable`, `pkg.Printable`) written in module `m` names.
    pub(super) fn bound_name(&self, bound: &[String], m: &str) -> R<String> {
        let q = if bound.len() == 1 { format!("{}.{}", m, bound[0]) } else { bound.join(".") };
        if self.tp.interfaces.contains_key(&q) {
            Ok(q)
        } else if self.tp.rec_decls.contains_key(&q) || self.tp.enum_decls.contains_key(&q) || self.tmpl_recs.contains_key(&q) {
            self.err(format!("the bound '{}' must be an interface", bound.join(".")))
        } else if bound.len() == 1 && matches!(bound[0].as_str(), "Eq" | "Ord" | "Hash") {
            Ok(format!("std.{}", bound[0]))
        } else {
            self.err(format!("unknown interface '{}'", bound.join(".")))
        }
    }

    /// Parameter and result types of the interface method `sg`; `Self` is `self_ty`.
    pub(super) fn interface_method(&mut self, iface_mod: &str, sg: &MethodSig, self_ty: &Ty) -> R<(Vec<Ty>, Ty)> {
        let mut b = HashMap::new();
        b.insert("Self".to_string(), self_ty.clone());
        let saved = std::mem::replace(&mut self.bindings, b);
        let mut ps = Vec::new();
        let mut result = Ok(());
        for p in &sg.params {
            match &p.ty {
                Some(t) => match self.resolve(t, iface_mod) {
                    Ok(t) => ps.push(t),
                    Err(e) => result = Err(e),
                },
                None => result = self.err(format!("parameter '{}' of '{}' needs a type", p.name, sg.name)),
            }
        }
        let ret = self.resolve_ret(&sg.ret, iface_mod);
        self.bindings = saved;
        result?;
        Ok((ps, ret?))
    }

    /// The method `mname` a bound provides: its own, or `eq` when the bound is `Ord` or `Hash`.
    pub(super) fn bound_sig(&self, bound: &str, mname: &str) -> Option<(String, MethodSig)> {
        let (bm, bsigs) = self.tp.interfaces.get(bound)?;
        if let Some(x) = bsigs.iter().find(|x| x.name == mname) {
            return Some((bm.clone(), x.clone()));
        }
        if mname == "eq" && matches!(bound, "std.Ord" | "std.Hash") {
            let (em, esigs) = self.tp.interfaces.get("std.Eq")?;
            return esigs.iter().find(|x| x.name == "eq").map(|x| (em.clone(), x.clone()));
        }
        None
    }

    /// Checks that `ty` has every method of the interface `iface`, with the same signature.
    pub(super) fn satisfies(&mut self, ty: &Ty, iface: &str) -> R<()> {
        let (im, sigs) = self.tp.interfaces[iface].clone();
        let iname = display_name(iface);
        let standard = matches!(iface, "std.Eq" | "std.Ord" | "std.Hash");
        for sg in &sigs {
            let (want_params, want_ret) = self.interface_method(&im, sg, ty)?;
            let have: Option<(Vec<Ty>, Ty)> = match ty {
                Ty::Param { bound: Some(b), .. } => match self.bound_sig(b, &sg.name) {
                    Some((bm, x)) => Some(self.interface_method(&bm, &x, ty)?),
                    None => None,
                },
                Ty::Param { .. } => None,
                _ => {
                    let found = self.tp.methods.get(&(method_key(ty), sg.name.clone())).cloned();
                    if let Some(m) = &found {
                        if !standard && !m.public && m.module != im {
                            return self.private_name(&sg.name, &m.module);
                        }
                    }
                    let found = match found {
                        Some(m) => Some(m),
                        None => self.std_sig(ty, &sg.name)?,
                    };
                    found.map(|m| (m.params.iter().map(|p| p.ty.clone()).collect(), m.ret.clone()))
                }
            };
            let Some((have_params, have_ret)) = have else {
                if standard {
                    let cap = Cap::of(&sg.name);
                    let why = self.lacks(ty, cap).map(|(_, w)| format!(": {}", w)).unwrap_or_default();
                    return self.err(format!("{} does not satisfy {}{}", ty.show(), iname, why));
                }
                return self.err(format!("{} does not satisfy {}: it has no method '{}'", ty.show(), iname, sg.name));
            };
            if have_params != want_params {
                let show = |v: &[Ty]| v.iter().map(|t| t.show()).collect::<Vec<_>>().join(", ");
                return self.err(format!(
                    "{} does not satisfy {}: method '{}' {}takes ({}) where {} needs ({})",
                    ty.show(),
                    iname,
                    sg.name,
                    if standard { "has the wrong signature for the interface: it " } else { "" },
                    show(&have_params),
                    iname,
                    show(&want_params)
                ));
            }
            if have_ret != want_ret {
                let r = |t: &Ty| if *t == Ty::Void { "nothing".to_string() } else { t.show() };
                return self.err(format!(
                    "{} does not satisfy {}: method '{}' {}returns {} where {} needs {}",
                    ty.show(),
                    iname,
                    sg.name,
                    if standard { "has the wrong signature for the interface: it " } else { "" },
                    r(&have_ret),
                    iname,
                    r(&want_ret)
                ));
            }
        }
        Ok(())
    }

    pub(super) fn check_bound(&mut self, ty: &Ty, iface: &str) -> R<()> {
        if !self.declared {
            self.deferred_bounds.push((ty.clone(), iface.to_string(), self.line));
            return Ok(());
        }
        self.satisfies(ty, iface)
    }

    /// A method call on a value whose type is a type parameter: only the bound's methods exist.
    pub(super) fn param_method_call(&mut self, name: &str, bound: &Option<String>, tname: &str, mname: &str, args: &[Arg]) -> R<Ty> {
        let Some(b) = bound else {
            return self.err(format!("{} has no method '{}': add a bound, for example <{}: Printable>", tname, mname, tname));
        };
        let Some((im, sg)) = self.bound_sig(b, mname) else {
            return self.err(format!("{} has no method '{}': its bound {} does not list it", tname, mname, display_name(b)));
        };
        let _ = name;
        let self_ty = Ty::Param { name: tname.to_string(), bound: bound.clone() };
        let (ps, ret) = self.interface_method(&im, &sg, &self_ty)?;
        if args.len() != ps.len() {
            return self.err(format!("'{}' takes {} argument{}", mname, ps.len(), if ps.len() == 1 { "" } else { "s" }));
        }
        for (a, pt) in args.iter().zip(&ps) {
            let at = self.expr(&a.value, Some(pt))?;
            self.check(pt, &at, &format!("argument of '{}'", mname))?;
        }
        self.e("xor eax, eax");
        Ok(ret)
    }

    // ---------------- generic records ----------------

    pub(super) fn instantiate_record(&mut self, q: &str, args: &[Type], m: &str) -> R<Ty> {
        let (tm, td, shared) = self.tmpl_recs[q].clone();
        let TypeBody::Record(fields) = &td.body else { unreachable!() };
        let n = td.generics.len();
        if args.len() != n {
            return self.err(format!(
                "{} takes {} type argument{}, found {}",
                td.name,
                n,
                if n == 1 { "" } else { "s" },
                args.len()
            ));
        }
        let mut tys = Vec::new();
        for a in args {
            tys.push(self.resolve(a, m)?);
        }
        for (tp, ty) in td.generics.iter().zip(&tys) {
            if let Some(b) = &tp.bound {
                let bq = self.bound_name(b, &tm)?;
                self.check_bound(ty, &bq)?;
            }
        }
        let full: Vec<String> = tys.iter().map(ty_full).collect();
        let name = format!("{}<{}>", q, full.join(","));
        if self.tp.rec_decls.contains_key(&name) {
            return Ok(Ty::Rec(name));
        }
        self.bump_instances()?;
        // registered before the layout, so a record may mention itself (`next: Node<T>?`)
        self.tp.rec_decls.insert(name.clone(), (tm.clone(), Vec::new()));
        self.inst_args.insert(name.clone(), (q.to_string(), tys.clone()));
        if shared {
            self.tp.shared_recs.insert(name.clone());
        }
        let binds: HashMap<String, Ty> = td.generics.iter().map(|g| g.name.clone()).zip(tys.iter().cloned()).collect();
        let saved = std::mem::replace(&mut self.bindings, binds);
        let info = self.layout(fields, &tm);
        self.bindings = saved;
        self.tp.records.insert(name.clone(), info?);
        let methods = self.tmpl_methods.get(q).cloned().unwrap_or_default();
        for (mm, fd, line) in methods {
            self.instantiate_method(&name, &mm, &fd, line, &tys)?;
        }
        Ok(Ty::Rec(name))
    }

    pub(super) fn instantiate_method(&mut self, rec_inst: &str, module: &str, fd: &FuncDecl, line: u32, tys: &[Ty]) -> R<()> {
        if self.tp.methods.contains_key(&(rec_inst.to_string(), fd.name.clone())) {
            return Ok(());
        }
        let Some(Type::Named { args: rargs, .. }) = fd.receiver.as_ref().map(|r| r.ty.clone()) else { unreachable!() };
        let mut binds = HashMap::new();
        for (a, t) in rargs.iter().zip(tys) {
            match a {
                Type::Named { path, args } if path.len() == 1 && args.is_empty() => {
                    binds.insert(path[0].clone(), t.clone());
                }
                _ => return self.err("the receiver of a method of a generic record must list its type parameters, like Stack<T>"),
            }
        }
        if rargs.len() != tys.len() {
            return self.err(format!("the receiver of '{}' must have {} type parameter(s)", fd.name, tys.len()));
        }
        let saved = std::mem::replace(&mut self.bindings, binds.clone());
        let saved_mod = std::mem::replace(&mut self.cur_mod, module.to_string());
        let saved_line = std::mem::replace(&mut self.line, line);
        let sig = self.func_sig(module, fd);
        self.bindings = saved;
        self.cur_mod = saved_mod;
        self.line = saved_line;
        let sig = sig?;
        self.tp.methods.insert((rec_inst.to_string(), fd.name.clone()), sig);
        self.pending.push(Pending { module: module.to_string(), decl: fd.clone(), bindings: binds, line });
        Ok(())
    }

    pub(super) fn bump_instances(&mut self) -> R<()> {
        self.inst_count += 1;
        if self.inst_count > INSTANCE_LIMIT {
            return self.err("too many generic instances: does a generic function or record use itself with ever larger types?");
        }
        Ok(())
    }

    /// The generic record a literal `Name { ... }` builds: the expected type decides the arguments.
    pub(super) fn generic_literal(&mut self, path: &[String], want: Option<&Ty>) -> R<Option<String>> {
        let m = self.cur_mod.clone();
        let q = self.qname(&m, path);
        if !self.tmpl_recs.contains_key(&q) {
            return Ok(None);
        }
        if let Some(Ty::Rec(inst)) = want {
            if self.inst_args.get(inst).map_or(false, |(tq, _)| *tq == q) {
                return Ok(Some(inst.clone()));
            }
        }
        let name = path.last().unwrap();
        let params: Vec<String> = self.tmpl_recs[&q].1.generics.iter().map(|g| g.name.clone()).collect();
        self.err(format!(
            "cannot infer the type arguments of {}: give the expected type, for example `let x: {}<{}> = ...`",
            name,
            name,
            params.iter().map(|_| "int").collect::<Vec<_>>().join(", ")
        ))
    }

    // ---------------- generic functions ----------------

    pub(super) fn unify(&mut self, pat: &Type, actual: &Ty, params: &[String], b: &mut HashMap<String, Ty>, m: &str) -> R<()> {
        match pat {
            Type::Shared(inner) => return self.unify(inner, actual, params, b, m),
            Type::Named { path, args } => {
                if path.len() == 1 && args.is_empty() && params.contains(&path[0]) {
                    match b.get(&path[0]) {
                        Some(prev) if prev != actual => {
                            return self.err(format!(
                                "type parameter {} is both {} and {} in this call",
                                path[0],
                                prev.show(),
                                actual.show()
                            ));
                        }
                        Some(_) => {}
                        None => {
                            b.insert(path[0].clone(), actual.clone());
                        }
                    }
                    return Ok(());
                }
                if path.len() == 1 {
                    match (path[0].as_str(), actual) {
                        ("List", Ty::List(el)) if args.len() == 1 => return self.unify(&args[0], el, params, b, m),
                        ("Map", Ty::Map(k, v)) if args.len() == 2 => {
                            self.unify(&args[0], k, params, b, m)?;
                            return self.unify(&args[1], v, params, b, m);
                        }
                        ("Channel", Ty::Chan(el)) if args.len() == 1 => return self.unify(&args[0], el, params, b, m),
                        ("Future", Ty::Future(el)) if args.len() == 1 => return self.unify(&args[0], el, params, b, m),
                        ("Rc", Ty::Rc(el)) | ("Weak", Ty::Weak(el)) if args.len() == 1 => return self.unify(&args[0], el, params, b, m),
                        _ => {}
                    }
                }
                let q = self.qname(m, path);
                if self.tmpl_recs.contains_key(&q) {
                    if let Ty::Rec(inst) = actual {
                        if let Some((tq, targs)) = self.inst_args.get(inst).cloned() {
                            if tq == q && targs.len() == args.len() {
                                for (a, t) in args.iter().zip(&targs) {
                                    self.unify(a, t, params, b, m)?;
                                }
                            }
                        }
                    }
                }
                Ok(())
            }
            Type::Optional(i) => self.unify(i, actual, params, b, m),
            Type::Pointer(i) => match actual {
                Ty::Ptr(e) => self.unify(i, e, params, b, m),
                _ => Ok(()),
            },
            Type::Slice(i) => match actual {
                Ty::List(e) => self.unify(i, e, params, b, m),
                _ => Ok(()),
            },
            Type::Func { params: ps, ret, .. } => {
                if let Ty::Func { params: aps, ret: ar, .. } = actual {
                    for (p, a) in ps.iter().zip(aps) {
                        self.unify(p, a, params, b, m)?;
                    }
                    if let Some(t) = ret.as_ref().and_then(|r| r.ty.as_ref()) {
                        self.unify(t, ar, params, b, m)?;
                    }
                }
                Ok(())
            }
            Type::Array(..) => Ok(()),
        }
    }

    /// A call to a generic function: infer the type arguments, make (or reuse) the instance, call it.
    pub(super) fn call_generic(&mut self, module: &str, fd: &FuncDecl, decl_line: u32, args: &[Arg], targs: &[Type], want: Option<&Ty>) -> R<Ty> {
        let names: Vec<String> = fd.generics.iter().map(|g| g.name.clone()).collect();
        let mut b: HashMap<String, Ty> = HashMap::new();
        if targs.len() > names.len() {
            return self.err(format!("'{}' takes {} type argument(s), but {} were given", fd.name, names.len(), targs.len()));
        }
        let caller = self.cur_mod.clone();
        for (n, t) in names.iter().zip(targs) {
            let ty = self.resolve(t, &caller)?;
            b.insert(n.clone(), ty);
        }
        let mut next = 0;
        let mut lambdas: Vec<(Type, &Expr)> = Vec::new();
        let mut late_lits: Vec<(Type, &Expr)> = Vec::new();
        for a in args {
            if a.spread {
                return self.err("spread arguments ('...') are not supported by the native backend yet");
            }
            let pi = match &a.name {
                Some(n) => fd.params.iter().position(|p| &p.name == n),
                None => {
                    next += 1;
                    Some(next - 1)
                }
            };
            let Some(p) = pi.and_then(|i| fd.params.get(i)) else { continue };
            let Some(pt) = &p.ty else { continue };
            // a lambda with untyped parameters is typed from the parameter type once that is known
            if matches!(a.value.kind, ExprKind::Lambda { .. }) {
                if !p.variadic && mentions_param(pt, &names) {
                    lambdas.push((pt.clone(), &a.value));
                }
                continue;
            }
            if p.variadic || !mentions_param(pt, &names) {
                continue;
            }
            // a bare number literal adapts to a type another argument fixed: it only binds an open parameter
            if is_int_literal(&a.value) {
                if let Type::Named { path, args: pargs } = pt {
                    if path.len() == 1 && pargs.is_empty() && b.contains_key(&path[0]) {
                        continue;
                    }
                }
                late_lits.push((pt.clone(), &a.value));
                continue;
            }
            // type the argument by compiling it into a scratch buffer
            let snap = self.snapshot();
            let r = self.expr(&a.value, None);
            self.restore(snap);
            let aty = r?;
            self.unify(pt, &aty, &names, &mut b, module)?;
        }
        for (pt, lit) in late_lits {
            if let Type::Named { path, args: pargs } = &pt {
                if path.len() == 1 && pargs.is_empty() && b.contains_key(&path[0]) {
                    continue;
                }
            }
            let aty = self.expr(lit, None)?;
            self.unify(&pt, &aty, &names, &mut b, module)?;
        }
        // lambdas: their parameter types come from the bindings so far; their body gives the rest
        for (pt, lam) in lambdas {
            let Type::Func { params: fps, ret: fret, .. } = &pt else { continue };
            let saved = std::mem::replace(&mut self.bindings, b.clone());
            let mut ptys = Vec::new();
            let mut open = None;
            for fp in fps {
                if mentions_param(fp, &names.iter().filter(|n| !b.contains_key(*n)).cloned().collect::<Vec<_>>()) {
                    open = Some(fp.clone());
                    break;
                }
                match self.resolve(fp, module) {
                    Ok(t) => ptys.push(t),
                    Err(e) => {
                        self.bindings = saved;
                        return Err(e);
                    }
                }
            }
            self.bindings = saved;
            if open.is_some() {
                continue;
            }
            let _ = fret;
            let want_fn = Ty::Func { params: ptys, ret: Box::new(Ty::Param { name: "?".into(), bound: None }), ext: false };
            let snap = self.snapshot();
            let r = self.expr(lam, Some(&want_fn));
            self.restore(snap);
            let lt = r?;
            self.unify(&pt, &lt, &names, &mut b, module)?;
        }
        if let (Some(w), Some(t)) = (want, fd.ret.as_ref().and_then(|r| r.ty.as_ref())) {
            let mut extra = HashMap::new();
            if self.unify(t, w, &names, &mut extra, module).is_ok() {
                for (k, v) in extra {
                    b.entry(k).or_insert(v);
                }
            }
        }
        for n in &names {
            if !b.contains_key(n) {
                return self.err(format!(
                    "cannot infer the type of {} in the call to '{}': no argument determines it; give the result an annotated type or pass a value of that type",
                    n, fd.name
                ));
            }
        }
        for n in &names {
            if b[n] == Ty::Void {
                return self.err(format!(
                    "the type {} in the call to '{}' would be 'nothing': a function argument must return a value here",
                    n, fd.name
                ));
            }
        }
        for g in &fd.generics {
            if let Some(bq) = &g.bound {
                let q = self.bound_name(bq, module)?;
                let ty = b[&g.name].clone();
                self.check_bound(&ty, &q)?;
            }
        }
        let argkey = names.iter().map(|n| ty_full(&b[n])).collect::<Vec<_>>().join(",");
        let key = (module.to_string(), fd.name.clone(), argkey.clone());
        let inst = match self.inst_funcs.get(&key) {
            Some(n) => n.clone(),
            None => {
                self.bump_instances()?;
                let inst = format!("{}__{}", fd.name, sym_of(&argkey));
                let mut decl = fd.clone();
                decl.generics.clear();
                decl.name = inst.clone();
                let saved = std::mem::replace(&mut self.bindings, b.clone());
                let saved_mod = std::mem::replace(&mut self.cur_mod, module.to_string());
                let sig = self.func_sig(module, &decl);
                self.bindings = saved;
                self.cur_mod = saved_mod;
                self.tp.funcs.insert((module.to_string(), inst.clone()), sig?);
                self.inst_funcs.insert(key, inst.clone());
                self.pending.push(Pending { module: module.to_string(), decl, bindings: b, line: decl_line });
                inst
            }
        };
        let sig = self.tp.funcs[&(module.to_string(), inst)].clone();
        self.call_sig(&sig, None, args)
    }

    // ---------------- driving ----------------

    /// Compiles every instance requested so far (and those they request).
    pub(super) fn drain_pending(&mut self) -> R<()> {
        while let Some(p) = self.pending.pop() {
            self.cur_mod = p.module.clone();
            self.line = p.line;
            self.bindings = p.bindings.clone();
            let r = self.gen_func(&p.module, &p.decl, false);
            self.bindings.clear();
            r?;
        }
        Ok(())
    }

    /// Checks each template once, with its type parameters as opaque types, and runs the bound
    /// checks that had to wait until every method was known.
    pub(super) fn check_templates(&mut self) -> R<()> {
        let deferred = std::mem::take(&mut self.deferred_bounds);
        for (ty, iface, line) in deferred {
            self.line = line;
            self.satisfies(&ty, &iface)?;
        }
        let mut fkeys: Vec<(String, String)> = self.tmpl_funcs.keys().cloned().collect();
        fkeys.sort();
        for key in fkeys {
            let (fd, line) = self.tmpl_funcs[&key].clone();
            let snap = self.snapshot();
            let r = self.check_function(&key.0, &fd, line);
            self.restore(snap);
            r?;
        }
        let mut rkeys: Vec<String> = self.tmpl_recs.keys().cloned().collect();
        rkeys.sort();
        for q in rkeys {
            let snap = self.snapshot();
            let r = self.check_record(&q);
            self.restore(snap);
            r?;
        }
        Ok(())
    }

    pub(super) fn param_bindings(&self, generics: &[TypeParam], m: &str) -> R<HashMap<String, Ty>> {
        let mut b = HashMap::new();
        for g in generics {
            let bound = match &g.bound {
                Some(bq) => Some(self.bound_name(bq, m)?),
                None => None,
            };
            b.insert(g.name.clone(), Ty::Param { name: g.name.clone(), bound });
        }
        Ok(b)
    }

    pub(super) fn check_function(&mut self, m: &str, fd: &FuncDecl, line: u32) -> R<()> {
        self.cur_mod = m.to_string();
        self.line = line;
        self.bindings = self.param_bindings(&fd.generics, m)?;
        let mut decl = fd.clone();
        decl.generics.clear();
        decl.name = format!("__check_{}", fd.name);
        let sig = self.func_sig(m, &decl)?;
        self.tp.funcs.insert((m.to_string(), decl.name.clone()), sig);
        let r = self.gen_func(m, &decl, false);
        self.bindings.clear();
        r
    }

    pub(super) fn check_record(&mut self, q: &str) -> R<()> {
        let (tm, td, _) = self.tmpl_recs[q].clone();
        self.cur_mod = tm.clone();
        self.line = 0;
        let binds = self.param_bindings(&td.generics, &tm)?;
        self.bindings = binds;
        let args: Vec<Type> = td.generics.iter().map(|g| Type::Named { path: vec![g.name.clone()], args: vec![] }).collect();
        self.instantiate_record(q, &args, &tm)?;
        // the methods of this instance are the template's own
        let todo = std::mem::take(&mut self.pending);
        for p in todo {
            self.cur_mod = p.module.clone();
            self.line = p.line;
            self.bindings = p.bindings.clone();
            self.gen_func(&p.module, &p.decl, false)?;
        }
        self.bindings.clear();
        Ok(())
    }
}

