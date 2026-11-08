//! Part of the code generator (`codegen_x64`), methods of `Compiler`: loading the program, declarations, layout and types.

use super::*;

impl Compiler {
    pub(super) fn load(&mut self, main: &File) -> R<()> {
        // generators run, derives, generated functions and blocks are expanded (comptime-reflection)
        let expansion = if self.driver { None } else { Some(crate::comptime::expand(main, self.target)?) };
        let main = match &expansion {
            Some(x) => &x.main,
            None => main,
        };
        let mut queue = vec![(main.package.clone(), main.clone())];
        let mut seen = HashSet::new();
        seen.insert(main.package.clone());
        while let Some((name, file)) = queue.pop() {
            let mut imps = HashSet::new();
            let mut wanted: Vec<Vec<String>> = file.uses.clone();
            if file_uses_concurrency(&file) {
                if self.target == Target::Bare {
                    return Err("'spawn', 'channel', 'async' and 'await' need the task library, which needs Windows".into());
                }
                wanted.push(vec!["task".into()]);
                wanted.push(vec!["future".into()]);
            }
            let (uses_shared, uses_spawn) = file_uses_threads(&file);
            if uses_spawn {
                if self.target == Target::Bare {
                    return Err("'spawn_thread' needs operating-system threads: it is not available in a bare-metal kernel".into());
                }
                wanted.push(vec!["thread".into()]);
            }
            if (uses_shared || uses_spawn) && self.target != Target::Bare {
                self.threads = true;
            }
            if name != "float" && file_uses_floats(&file) {
                if self.target == Target::Bare {
                    return Err("floating point is not available in a bare-metal kernel".into());
                }
                wanted.push(vec!["float".into()]);
            }
            for segs in wanted {
                let mut std_name = crate::codegen_x64::std_source(&segs[0]).is_some();
                // a package of the project may take the name of a standard library that is not core
                // (spec `package-manager`); the standard libraries themselves keep their own
                let importer_is_std = name != main.package && crate::codegen_x64::std_source(&name).is_some();
                let mut shadowing: Option<(String, &'static str)> = None;
                if std_name && !is_core_std(&segs[0]) && !importer_is_std {
                    if let Some(r) = crate::package::resolve_use(&name, &segs)? {
                        if !self.driver {
                            crate::warn::push(format!("the package '{}' shadows the standard library's '{}'", segs.join("."), segs[0]));
                        }
                        shadowing = Some((r.key, r.source));
                        std_name = false;
                    }
                }
                let m = if std_name { segs[0].clone() } else { segs.last().unwrap().clone() };
                if m == "ws" && name == main.package {
                    return Err("'ws' is internal to 'net' and cannot be used by programs; use 'net' instead".into());
                }
                if std_name && self.target == Target::Bare && (matches!(m.as_str(), "ui" | "task" | "future" | "fs" | "net" | "sys" | "ws" | "tls" | "thread" | "sync" | "web" | "crypto" | "db" | "postgres" | "sqlserver" | "ai" | "mcp" | "rag")) {
                    return Err(format!("'{}' needs Windows and cannot be used in a bare-metal kernel", m));
                }
                if std_name
                    && self.target == Target::Linux
                    && (m == "ws" || (OS_LIBRARIES.contains(&m.as_str()) && std_linux_source(&m).is_none()))
                {
                    return Err(format!("'{}' is not available on Linux yet (it uses the Windows API)", m));
                }
                // where the package is: the standard library, the project (zila.toml and its
                // directories), then the files registered next to the source
                let found: Option<(String, &'static str)> = if shadowing.is_some() {
                    shadowing
                } else if std_name {
                    std_source_for(&m, self.target).map(|s| (m.clone(), s))
                } else {
                    match crate::package::resolve_use(&name, &segs)? {
                        Some(r) => Some((r.key, r.source)),
                        None => match crate::comptime::registered_source(&segs[0]) {
                            Some(s) if segs.len() == 1 => Some((segs[0].clone(), s)),
                            _ if segs[0] == "c" => None,
                            _ => {
                                return Err(format!(
                                    "unknown package '{}' (not in the standard library, zila.toml or the project directory); to add one: zila-parse --add <git-url>",
                                    segs.join(".")
                                ))
                            }
                        },
                    }
                };
                imps.insert(m.clone());
                if let Some((k, _)) = &found {
                    if *k != m {
                        self.mod_aliases.insert((name.clone(), m.clone()), k.clone());
                    }
                }
                if m == "thread" {
                    self.uses_thread_lib = true;
                }
                if m == "ui" {
                    self.uses_ui = true;
                }
                if let Some((key, src)) = found {
                    if !seen.contains(&key) {
                        seen.insert(key.clone());
                        let mut f = parse_source(src).map_err(|d| format!("{}{}.zila:{}:{}: {}", if std_name { "std/" } else { "package " }, key, d.line, d.col, d.msg))?;
                        // generators run at build time only; generated templates have no body here
                        let driver = self.driver;
                        f.decls.retain(|d| match &d.kind {
                            DeclKind::Func(fd) => !(d.attrs.iter().any(|a| a.name == "generated") || (fd.comptime && !driver)),
                            _ => true,
                        });
                        if let Some(extra) = expansion.as_ref().and_then(|x| x.extra.get(&key)) {
                            f.decls.extend(extra.iter().cloned());
                        }
                        queue.push((key.clone(), f));
                    }
                }
            }
            self.imports.insert(name.clone(), imps);
            self.modules.push(Module { name, file });
        }
        Ok(())
    }

    /// The standard interfaces `Eq`, `Ord` and `Hash`, visible in every package without `use`.
    pub(super) fn install_prelude(&mut self) -> R<()> {
        let src = "package std
type Eq = interface
    func eq(self, other: Self) -> bool
end
type Ord = interface
    func compare(self, other: Self) -> int
end
type Hash = interface
    func hash(self) -> long
end
";
        let f = parse_source(src).map_err(|d| format!("prelude: {}", d.msg))?;
        for d in f.decls {
            if let DeclKind::Type(td) = d.kind {
                if let TypeBody::Interface(sigs) = td.body {
                    self.interfaces.insert(format!("std.{}", td.name), ("std".to_string(), sigs));
                }
            }
        }
        Ok(())
    }

    pub(super) fn declare(&mut self) -> R<()> {
        self.install_prelude()?;
        // Type names first, so records can refer to each other.
        let mods: Vec<(String, File)> = self.modules.iter().map(|m| (m.name.clone(), m.file.clone())).collect();
        for (mname, file) in &mods {
            self.cur_mod = mname.clone();
            for d in &file.decls {
                self.line = d.line;
                if let DeclKind::Type(td) = &d.kind {
                    let q = format!("{}.{}", mname, td.name);
                    if d.public {
                        self.pub_types.insert(q.clone());
                    }
                    if td.name == "Rc" || td.name == "Weak" {
                        return self.err(format!("type '{}' is defined twice: '{}' is a built-in type (shared references)", td.name, td.name));
                    }
                    if !td.generics.is_empty() {
                        match &td.body {
                            TypeBody::Record(_) => {
                                let shared = d.attrs.iter().any(|a| a.name == "shared");
                                self.tmpl_recs.insert(q, (mname.clone(), td.clone(), shared));
                                continue;
                            }
                            TypeBody::Enum(_) => return self.err(format!("generic enums are not supported ('{}')", td.name)),
                            TypeBody::Alias(_) => return self.err(format!("generic type aliases are not supported ('{}')", td.name)),
                            TypeBody::Interface(_) => return self.err(format!("generic interfaces are not supported ('{}')", td.name)),
                        }
                    }
                    match &td.body {
                        TypeBody::Record(fields) => {
                            if d.attrs.iter().any(|a| a.name == "shared") {
                                self.shared_recs.insert(q.clone());
                            }
                            self.rec_decls.insert(q, (mname.clone(), fields.clone()));
                        }
                        TypeBody::Interface(sigs) => {
                            let mut seen = HashSet::new();
                            for sg in sigs {
                                if !seen.insert(sg.name.clone()) {
                                    return self.err(format!("interface '{}' declares method '{}' twice", td.name, sg.name));
                                }
                            }
                            self.interfaces.insert(q, (mname.clone(), sigs.clone()));
                        }
                        TypeBody::Enum(vs) => {
                            if vs.iter().any(|v| !v.fields.is_empty()) {
                                self.enum_payload.insert(q.clone());
                            }
                            for (i, v) in vs.iter().enumerate() {
                                let key = (mname.clone(), v.name.clone());
                                if self.variants.contains_key(&key) {
                                    return self.err(format!("variant name '{}' is used by two enums in this module", v.name));
                                }
                                self.variants.insert(key, (q.clone(), i));
                            }
                            self.enum_decls.insert(q, (mname.clone(), vs.clone()));
                        }
                        TypeBody::Alias(t) => {
                            self.aliases.insert(q, (mname.clone(), t.clone()));
                        }
                    }
                }
            }
        }
        // Generic functions and the methods of generic records are templates.
        for (mname, file) in &mods {
            self.cur_mod = mname.clone();
            for d in &file.decls {
                let DeclKind::Func(fd) = &d.kind else { continue };
                self.line = d.line;
                if let Some(rq) = self.generic_receiver(mname, fd) {
                    if !fd.generics.is_empty() {
                        return self.err("methods with their own type parameters are not supported yet");
                    }
                    self.tmpl_methods.entry(rq).or_default().push((mname.clone(), fd.clone(), d.line));
                } else if !fd.generics.is_empty() {
                    if fd.is_async {
                        return self.err("async generic functions are not supported");
                    }
                    if fd.receiver.is_some() {
                        return self.err("methods with their own type parameters are not supported yet");
                    }
                    self.tmpl_funcs.insert((mname.clone(), fd.name.clone()), (fd.clone(), d.line));
                }
            }
        }
        // Record layouts.
        let decls: Vec<(String, String, Vec<Field>)> =
            self.rec_decls.iter().map(|(q, (m, f))| (q.clone(), m.clone(), f.clone())).collect();
        for (q, m, fields) in decls {
            self.cur_mod = m.clone();
            let info = self.layout(&fields, &m)?;
            self.records.insert(q, info);
        }
        // Enum layouts.
        let edecls: Vec<(String, String, Vec<Variant>)> =
            self.enum_decls.iter().map(|(q, (m, v))| (q.clone(), m.clone(), v.clone())).collect();
        for (q, m, vs) in edecls {
            self.cur_mod = m.clone();
            let mut variants = Vec::new();
            let mut maxf = 0;
            for v in &vs {
                let mut tys = Vec::new();
                for f in &v.fields {
                    tys.push(self.resolve(f, &m)?);
                }
                maxf = maxf.max(tys.len());
                variants.push((v.name.clone(), tys));
            }
            let payload = self.enum_payload.contains(&q);
            self.enums.insert(q, EnumInfo { variants, payload, size: 8 + 8 * maxf as u32 });
        }
        // Functions, methods, externs, globals.
        let mods: Vec<(String, Vec<Decl>)> = self.modules.iter().map(|m| (m.name.clone(), m.file.decls.clone())).collect();
        for (m, decls) in &mods {
            self.cur_mod = m.clone();
            // a `#[link]` names the library of the externs that follow it, up to the next `#[link]`
            let mut cur_lib: Option<String> = None;
            for d in decls {
                self.line = d.line;
                match &d.kind {
                    DeclKind::Func(fd) => {
                        if !fd.generics.is_empty() || self.generic_receiver(m, fd).is_some() {
                            continue;
                        }
                        let sig = self.func_sig(m, fd)?;
                        if fd.name == "drop" && sig.receiver.is_some() && (!sig.params.is_empty() || sig.ret != Ty::Void) {
                            return self.err("a `drop` method takes no parameters and returns nothing");
                        }
                        if let Some(rt) = &sig.receiver {
                            if matches!(rt, Ty::Void | Ty::Nil | Ty::Err) {
                                return self.err(format!("methods cannot be declared on {}", rt.show()));
                            }
                            let key = method_key(rt);
                            if self.methods.contains_key(&(key.clone(), fd.name.clone())) {
                                return self.err(format!("{} already has a method '{}'", rt.show(), fd.name));
                            }
                            self.methods.insert((key, fd.name.clone()), sig);
                        } else {
                            if self.opt && !d.attrs.iter().any(|a| a.name == "naked") {
                                if let Some(body) = inlinable(fd, &sig) {
                                    self.inline_bodies.insert(sig.sym.clone(), (body, m.clone()));
                                }
                            }
                            self.funcs.insert((m.clone(), fd.name.clone()), sig);
                        }
                    }
                    DeclKind::Extern(ed) => {
                        for a in &d.attrs {
                            if a.name == "link_src" && self.target == Target::Windows {
                                self.line = d.line;
                                return self.err("#[link_src] is not supported on Windows: the compiler no longer builds C source files there (build the C code into a DLL and use #[link])");
                            }
                            if a.name == "link" {
                                if let Some(Expr { kind: ExprKind::Text(parts), .. }) = a.args.first() {
                                    if let Some(TextPart::Lit(l)) = parts.first() {
                                        self.libs.insert(l.clone());
                                        cur_lib = Some(l.clone());
                                    }
                                }
                            }
                        }
                        if let Some(l) = &cur_lib {
                            self.extern_lib.entry(ed.name.clone()).or_insert_with(|| l.clone());
                        }
                        let mut params = Vec::new();
                        for p in &ed.params {
                            let ty = match &p.ty {
                                Some(t) => self.resolve(t, m)?,
                                None => return self.err("extern parameters need a type"),
                            };
                            params.push(ParamInfo { name: p.name.clone(), ty, by_ref: false, default: None });
                        }
                        let ret = match &ed.ret {
                            Some(t) => self.resolve(t, m)?,
                            None => Ty::Void,
                        };
                        self.funcs.insert(
                            (m.clone(), ed.name.clone()),
                            Sig {
                                sym: ed.name.clone(),
                                module: m.clone(),
                                params,
                                ret,
                                variadic: false,
                                c_variadic: ed.c_variadic,
                                is_extern: true,
                                receiver: None,
                                public: d.public,
                            },
                        );
                    }
                    DeclKind::Var(vd) => {
                        let konst = if vd.mutable { None } else { vd.value.as_ref().and_then(|e| self.const_eval(e, m)) };
                        let ty = match (&vd.ty, &konst) {
                            (Some(t), _) => self.resolve(t, m)?,
                            (None, Some(k)) => {
                                let t = Ty::int();
                                if !lit_fits(k.unsigned_abs(), *k < 0, &t) {
                                    return self.err(format!("constant {} is out of range for int; write `let {}: long = ...`", k, vd.name));
                                }
                                t
                            }
                            (None, None) => self.infer_global(vd.value.as_ref(), m)?,
                        };
                        self.globals.insert(
                            (m.clone(), vd.name.clone()),
                            Global { sym: format!("G_{}_{}", m, vd.name), ty, konst, mutable: vd.mutable, public: d.public },
                        );
                    }
                    _ => {}
                }
            }
        }
        self.check_public_signatures()?;
        self.declared = true;
        let keys = std::mem::take(&mut self.deferred_keys);
        for (k, line, m) in keys {
            self.cur_mod = m;
            self.line = line;
            self.check_key(&k)?;
        }
        let mods: Vec<(String, File)> = self.modules.iter().map(|m| (m.name.clone(), m.file.clone())).collect();
        let mut ifaces = HashSet::new();
        for k in self.interfaces.keys() {
            ifaces.insert(k.clone());
        }
        self.custom_allocator = crate::noalloc::check_program(&mods, &ifaces)?;
        Ok(())
    }

    pub(super) fn layout(&mut self, fields: &[Field], m: &str) -> R<RecInfo> {
        let mut off = 0u32;
        let mut align = 1u32;
        let mut out = Vec::new();
        for f in fields {
            let ty = self.resolve(&f.ty, m)?;
            let sz = ty.size();
            off = (off + sz - 1) / sz * sz;
            align = align.max(sz);
            out.push((f.name.clone(), ty, off));
            off += sz;
        }
        let size = ((off + align - 1) / align * align).max(8);
        Ok(RecInfo { size, fields: out })
    }

    pub(super) fn func_sig(&mut self, m: &str, fd: &FuncDecl) -> R<Sig> {
        if fd.is_async {
            // spec `ref-parameters`: a task may outlive the caller, so it cannot borrow from it
            if fd.params.iter().any(|p| p.by_ref) || fd.receiver.as_ref().map_or(false, |r| r.by_ref) {
                return self.err(format!(
                    "async func '{}' cannot take `ref` parameters: the task works on copies; pass a value or a channel",
                    fd.name
                ));
            }
            if !fd.generics.is_empty() {
                return self.err("async generic functions are not supported");
            }
        }
        let mut params = Vec::new();
        let mut variadic = false;
        for p in &fd.params {
            let mut ty = match &p.ty {
                Some(t) => self.resolve(t, m)?,
                None => return self.err(format!("parameter '{}' needs a type", p.name)),
            };
            if p.variadic {
                variadic = true;
                ty = Ty::List(Box::new(ty));
            }
            params.push(ParamInfo { name: p.name.clone(), ty, by_ref: p.by_ref, default: p.default.clone() });
        }
        let ret = match &fd.ret {
            Some(r) => {
                let inner = match &r.ty {
                    Some(t) => self.resolve(t, m)?,
                    None => Ty::Void,
                };
                if r.fallible {
                    Ty::Result(Box::new(inner))
                } else {
                    inner
                }
            }
            None => Ty::Void,
        };
        let ret = if fd.is_async { Ty::Future(Box::new(ret)) } else { ret };
        let receiver = match &fd.receiver {
            Some(r) => Some(self.resolve(&r.ty, m)?),
            None => None,
        };
        let sym = match &receiver {
            Some(Ty::Rec(r)) => format!("B_{}_{}", sym_of(r), fd.name),
            Some(t) => format!("B_T{}_{}", mangle(&ty_full(t)), fd.name),
            _ => format!("B_{}_{}", m, fd.name),
        };
        if fd_has_await(fd) {
            self.await_syms.insert(sym.clone());
        }
        Ok(Sig { sym, module: m.to_string(), params, ret, variadic, c_variadic: false, is_extern: false, receiver, public: fd.public })
    }

    pub(super) fn infer_global(&mut self, e: Option<&Expr>, m: &str) -> R<Ty> {
        let Some(e) = e else { return self.err("global variable needs a type or a value") };
        Ok(match &e.kind {
            ExprKind::Bool(_) => Ty::Bool,
            ExprKind::Text(_) => Ty::Text,
            ExprKind::Int(_) => Ty::int(),
            ExprKind::Char(_) => Ty::Char,
            ExprKind::Float(_) => Ty::Float { bits: 64 },
            ExprKind::Record { path, .. } => Ty::Rec(self.record_name(path, m)?),
            ExprKind::Call { callee, .. } => match &callee.kind {
                ExprKind::Ident(f) => match self.funcs.get(&(m.to_string(), f.clone())) {
                    Some(s) => s.ret.clone(),
                    None => return self.err("cannot infer the type of this global; add a type annotation"),
                },
                _ => return self.err("cannot infer the type of this global; add a type annotation"),
            },
            _ => return self.err("cannot infer the type of this global; add a type annotation"),
        })
    }

    pub(super) fn const_eval(&self, e: &Expr, m: &str) -> Option<i64> {
        match &e.kind {
            ExprKind::Int(n) => Some(*n as i64),
            ExprKind::Char(c) => Some(*c as i64),
            ExprKind::Ident(n) => self.globals.get(&(m.to_string(), n.clone())).and_then(|g| g.konst),
            ExprKind::Unary { op: "-", expr } => self.const_eval(expr, m).map(|v| v.wrapping_neg()),
            ExprKind::Unary { op: "bitnot", expr } => self.const_eval(expr, m).map(|v| !v),
            ExprKind::Binary { op, lhs, rhs } => {
                let (a, b) = (self.const_eval(lhs, m)?, self.const_eval(rhs, m)?);
                Some(match *op {
                    "+" => a.wrapping_add(b),
                    "-" => a.wrapping_sub(b),
                    "*" => a.wrapping_mul(b),
                    "/" if b != 0 => a / b,
                    "%" if b != 0 => a % b,
                    "bitor" => a | b,
                    "bitand" => a & b,
                    "bitxor" => a ^ b,
                    "<<" => a << b,
                    ">>" => a >> b,
                    _ => return None,
                })
            }
            _ => None,
        }
    }

    pub(super) fn record_name(&mut self, path: &[String], m: &str) -> R<String> {
        let q = self.qname(m, path);
        if path.len() > 1 && !self.type_visible(&q, m) && self.type_exists(&q) {
            return self.private_name(path.last().unwrap(), type_owner(&q));
        }
        if self.rec_decls.contains_key(&q) {
            return Ok(q);
        }
        if let Some((am, t)) = self.aliases.get(&q).cloned() {
            if let Ty::Rec(r) = self.resolve(&t, &am)? {
                return Ok(r);
            }
        }
        self.err(format!("unknown record type '{}'", path.join(".")))
    }

    /// A `pub` declaration may not name a private type of its own package.
    pub(super) fn check_public_signatures(&mut self) -> R<()> {
        let mods: Vec<(String, Vec<Decl>)> = self.modules.iter().map(|m| (m.name.clone(), m.file.decls.clone())).collect();
        for (m, decls) in &mods {
            self.cur_mod = m.clone();
            for d in decls {
                if !d.public {
                    continue;
                }
                self.line = d.line;
                let mut tys: Vec<&Type> = Vec::new();
                let mut generics: Vec<&str> = Vec::new();
                let mut bounds: Vec<&Vec<String>> = Vec::new();
                let (kind, name) = match &d.kind {
                    DeclKind::Func(fd) => {
                        for p in &fd.params {
                            tys.extend(p.ty.iter());
                        }
                        if let Some(r) = &fd.ret {
                            tys.extend(r.ty.iter());
                        }
                        generics.extend(fd.generics.iter().map(|g| g.name.as_str()));
                        bounds.extend(fd.generics.iter().filter_map(|g| g.bound.as_ref()));
                        (if fd.receiver.is_some() { "method" } else { "func" }, fd.name.as_str())
                    }
                    DeclKind::Extern(ed) => {
                        for p in &ed.params {
                            tys.extend(p.ty.iter());
                        }
                        tys.extend(ed.ret.iter());
                        ("extern func", ed.name.as_str())
                    }
                    DeclKind::Var(vd) => {
                        tys.extend(vd.ty.iter());
                        (if vd.mutable { "var" } else { "let" }, vd.name.as_str())
                    }
                    DeclKind::Type(td) => {
                        generics.extend(td.generics.iter().map(|g| g.name.as_str()));
                        bounds.extend(td.generics.iter().filter_map(|g| g.bound.as_ref()));
                        match &td.body {
                            TypeBody::Record(fs) => tys.extend(fs.iter().filter(|f| f.public).map(|f| &f.ty)),
                            TypeBody::Enum(vs) => tys.extend(vs.iter().flat_map(|v| v.fields.iter())),
                            TypeBody::Alias(t) => tys.push(t),
                            TypeBody::Interface(sigs) => {
                                for sg in sigs {
                                    for p in &sg.params {
                                        tys.extend(p.ty.iter());
                                    }
                                    if let Some(r) = &sg.ret {
                                        tys.extend(r.ty.iter());
                                    }
                                }
                            }
                        }
                        ("type", td.name.as_str())
                    }
                    DeclKind::Embed(_) => continue,
                };
                let mut names: Vec<String> = Vec::new();
                for t in tys {
                    named_types(t, &generics, &mut names);
                }
                for b in bounds {
                    if b.len() == 1 {
                        names.push(b[0].clone());
                    }
                }
                for n in names {
                    let q = format!("{}.{}", m, n);
                    if self.type_exists(&q) && !self.pub_types.contains(&q) {
                        return self.err(format!(
                            "pub {} '{}' exposes private type '{}'; mark '{}' pub or make '{}' private",
                            kind, name, n, n, name
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    pub(super) fn type_exists(&self, q: &str) -> bool {
        self.tmpl_recs.contains_key(q)
            || self.rec_decls.contains_key(q)
            || self.interfaces.contains_key(q)
            || self.enum_decls.contains_key(q)
            || self.aliases.contains_key(q)
    }

    pub(super) fn resolve(&mut self, t: &Type, m: &str) -> R<Ty> {
        match t {
            Type::Shared(inner) => return self.resolve(inner, m),
            Type::Named { path, args } => {
                if path.len() == 1 {
                    let n = path[0].as_str();
                    if args.is_empty() {
                        if let Some(t) = self.bindings.get(n) {
                            return Ok(t.clone());
                        }
                    }
                    if let Some(i) = int_type(n) {
                        return Ok(i);
                    }
                    match n {
                        "bool" => return Ok(Ty::Bool),
                        "text" => return Ok(Ty::Text),
                        "char" => return Ok(Ty::Char),
                        "error" => return Ok(Ty::Err),
                        "ErrorKind" => return Ok(Ty::Kind),
                        "float" | "f64" => return Ok(Ty::Float { bits: 64 }),
                        "f32" => return Ok(Ty::Float { bits: 32 }),
                        "List" if args.len() == 1 => return Ok(Ty::List(Box::new(self.resolve(&args[0], m)?))),
                        "Map" if args.len() == 2 => {
                            let k = self.resolve(&args[0], m)?;
                            let v = self.resolve(&args[1], m)?;
                            return self.map_type(k, v);
                        }
                        "Channel" if args.len() == 1 => return Ok(Ty::Chan(Box::new(self.resolve(&args[0], m)?))),
                        "Future" if args.len() == 1 => return Ok(Ty::Future(Box::new(self.resolve(&args[0], m)?))),
                        "Rc" | "Weak" if args.len() == 1 => {
                            let inner = self.resolve(&args[0], m)?;
                            if !matches!(inner, Ty::Rec(_) | Ty::Param { .. }) {
                                return self.err(format!("{}<T> needs a record type, found {}", n, inner.show()));
                            }
                            return Ok(if n == "Rc" { Ty::Rc(Box::new(inner)) } else { Ty::Weak(Box::new(inner)) });
                        }
                        _ => {}
                    }
                }
                if path.len() == 2 && path[0] == "c" {
                    if let Some(t) = c_type(&path[1]) {
                        return Ok(t);
                    }
                }
                let q = self.qname(m, path);
                if path.len() > 1 && !self.type_visible(&q, m) && self.type_exists(&q) {
                    return self.private_name(path.last().unwrap(), type_owner(&q));
                }
                if self.tmpl_recs.contains_key(&q) {
                    return self.instantiate_record(&q, args, m);
                }
                if self.rec_decls.contains_key(&q) {
                    return Ok(Ty::Rec(q));
                }
                if self.interfaces.contains_key(&q) {
                    return self.err(format!(
                        "'{}' is an interface: interfaces can only bound type parameters; write `<T: {}>` and use `T`",
                        path.join("."),
                        path.last().unwrap()
                    ));
                }
                if self.enum_decls.contains_key(&q) {
                    return Ok(Ty::Enum(q));
                }
                if let Some((am, at)) = self.aliases.get(&q).cloned() {
                    return self.resolve(&at, &am);
                }
                if path.len() == 1 && path[0] == "Self" {
                    return self.err("`Self` is only valid inside an interface method signature");
                }
                if path.len() == 1 && path[0] == "Type" {
                    if self.driver && self.rec_decls.contains_key("reflect.Type") {
                        return Ok(Ty::Rec("reflect.Type".to_string()));
                    }
                    return self.err("`Type` exists only at compile time: use it in a `comptime func` or as a `comptime` parameter");
                }
                self.err(format!("type '{}' is not supported by the native backend", path.join(".")))
            }
            Type::Optional(inner) => {
                let t = self.resolve(inner, m)?;
                if t.nullable() {
                    Ok(t)
                } else {
                    self.err(format!("optional '{}?' is only supported for reference types", t.show()))
                }
            }
            Type::Pointer(inner) => Ok(Ty::Ptr(Box::new(self.resolve(inner, m)?))),
            Type::Slice(inner) => Ok(Ty::List(Box::new(self.resolve(inner, m)?))),
            Type::Array(..) => self.err("fixed arrays are not supported by the native backend yet"),
            Type::Func { abi, params, ret } => {
                let mut ps = Vec::new();
                for p in params {
                    ps.push(self.resolve(p, m)?);
                }
                let r = match ret.as_deref() {
                    Some(rt) => {
                        let inner = match &rt.ty {
                            Some(t) => self.resolve(t, m)?,
                            None => Ty::Void,
                        };
                        if rt.fallible {
                            Ty::Result(Box::new(inner))
                        } else {
                            inner
                        }
                    }
                    None => Ty::Void,
                };
                Ok(Ty::Func { params: ps, ret: Box::new(r), ext: abi.is_some() })
            }
        }
    }

    // ---------------- code generation driver ----------------

}
