//! The position index: every name in a program with its range, whether it declares or uses, and
//! what it refers to. Hover, definition, references, rename and completion are lookups in it.
//!
//! It is built after checking, by a walk over the syntax tree that keeps its own scopes (so every
//! name resolves, also in code the body checker leaves `Opaque`) and uses the declaration tables
//! for global names. Where a name's meaning depends on a type (the member of a value, the
//! variants a `match` names), the type comes from `check_expr`; when that cannot say, the name
//! gets `Referent::Unknown` rather than a guess.
//!
//! Only the packages of the project are indexed (not the standard library); lines in a package
//! of several files count its merged text (gap 11 in the change notes).

use super::*;
use std::collections::BTreeMap;

/// What a name refers to. The declaration of a thing and every use of it carry the same target.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Referent {
    /// A parameter or a local: the range of its name where it is declared.
    Local { decl: Span },
    Function { module: String, name: String },
    /// A method (or an interface method) of the type with this key (`module.Type`).
    Method { type_key: String, name: String },
    /// A type written by the program: a record, enum, interface or alias.
    Type { module: String, name: String },
    Field { type_key: String, name: String },
    Variant { enum_key: String, name: String },
    /// A top-level `let` or `var`.
    Global { module: String, name: String },
    /// A package named in a `use`.
    Package { name: String },
    /// A type parameter of a generic function or type.
    TypeParameter { name: String },
    /// Something the language provides: a built-in function, type or member.
    Builtin { name: String },
    /// Resolution failed or needs a type that is not known.
    Unknown,
}

/// One name in the text.
#[derive(Debug, Clone)]
pub struct IndexEntry {
    pub span: Span,
    pub name: String,
    /// The name is where the thing is declared (its target is the thing itself).
    pub is_declaration: bool,
    pub target: Referent,
    /// The type, or for a function its signature, when known.
    pub ty: Option<String>,
    /// The key of the type of a value (a key of `PositionIndex::members`), when it is a type with
    /// members.
    pub type_key: Option<String>,
    /// For a local or parameter: where it can be used, from its declaration to the end of its
    /// block. Empty (see `Span::is_none`) for everything else.
    pub visible: Span,
}

/// A member of a type or package, for completion.
#[derive(Debug, Clone)]
pub struct Member {
    pub name: String,
    pub kind: MemberKind,
    /// The type of a field, the signature of a method or function, the type of a global.
    pub ty: String,
    /// The key of the type of the member (of a field; of the result of a method) when it has members.
    pub type_key: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberKind {
    Field,
    Method,
    Function,
    Global,
    Type,
    Variant,
}

/// The entries of every indexed module, in text order.
#[derive(Debug, Clone, Default)]
pub struct PositionIndex {
    modules: BTreeMap<String, Vec<IndexEntry>>,
    /// Fields and methods of each type, by type key (a record's `module.Type`, or the key
    /// methods of a built-in type are stored under).
    pub members: HashMap<String, Vec<Member>>,
    /// The variants of each enum, by enum key.
    pub variants: HashMap<String, Vec<Member>>,
    /// The public functions, globals and types of each module, standard library included.
    pub package_members: HashMap<String, Vec<Member>>,
    /// The packages each indexed module imports.
    pub imports: HashMap<String, Vec<String>>,
    /// The files of each package of the project other than the main one (see
    /// `package::module_files`), to turn a line of the merged text into a file and line.
    pub files: HashMap<String, Vec<(std::path::PathBuf, u32, u32)>>,
}

impl PositionIndex {
    /// The file and line (from 1) a line of the merged text of the package `module` comes from.
    pub fn file_of(&self, module: &str, line: u32) -> Option<(&std::path::Path, u32)> {
        self.files.get(module)?.iter().find(|(_, start, n)| line >= *start && line < start + n).map(|(p, start, _)| (p.as_path(), line - start + 1))
    }

    /// The entries of `module`.
    pub fn entries(&self, module: &str) -> &[IndexEntry] {
        self.modules.get(module).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// The names of the indexed modules.
    pub fn modules(&self) -> impl Iterator<Item = &str> {
        self.modules.keys().map(|k| k.as_str())
    }

    /// The name at a position (line and column count from 1).
    pub fn at(&self, module: &str, line: u32, col: u32) -> Option<&IndexEntry> {
        self.entries(module).iter().find(|e| e.span.contains(line, col))
    }

    /// The entry that declares `target`, with its module.
    pub fn declaration_of(&self, target: &Referent) -> Option<(&str, &IndexEntry)> {
        self.modules.iter().find_map(|(m, v)| v.iter().find(|e| e.is_declaration && &e.target == target).map(|e| (m.as_str(), e)))
    }

    /// Every entry, declaration and uses, that refers to `target`.
    pub fn references(&self, target: &Referent) -> Vec<(&str, &IndexEntry)> {
        self.modules.iter().flat_map(|(m, v)| v.iter().filter(move |e| &e.target == target).map(move |e| (m.as_str(), e))).collect()
    }
}

/// State of one walk.
#[derive(Default)]
struct Walk {
    out: Vec<IndexEntry>,
    /// The locals in scope: name -> where it is declared.
    scopes: Vec<HashMap<String, Span>>,
    /// Type parameters in scope.
    generics: Vec<String>,
    /// Where the blocks being walked end (the innermost last), for the extent of locals.
    ends: Vec<(u32, u32)>,
}

/// The type of a local whose type is not known: it stops `check_expr` from resolving the name to
/// an outer variable of the same name.
fn unknown_ty() -> Ty {
    Ty::Param { name: "?".into(), bound: None }
}

fn is_unknown(t: &Ty) -> bool {
    matches!(t, Ty::Param { name, .. } if name == "?")
}

/// Names of the functions and types the language provides without a `use`.
pub const BUILTIN_NAMES: &[&str] = &["abort", "assert", "assert_eq", "bool", "channel", "char", "error", "f32", "f64", "float", "heap_bytes", "len", "live_objects", "print", "println", "sleep", "text", "timeout", "weak"];

/// The key under which the members of a value of type `t` are listed, for the types that have some.
fn member_key(t: &Ty) -> Option<String> {
    let view = rc_view(t);
    matches!(view, Ty::Rec(_) | Ty::Enum(_) | Ty::List(_) | Ty::Text | Ty::Map(..) | Ty::Err | Ty::Kind).then(|| method_key(&view))
}

/// The range of the first `chars` characters of `s`, when `s` is on one line.
fn head(s: Span, chars: usize) -> Span {
    Span::new(s.line, s.col, s.line, s.col + chars as u32)
}

/// The range of the `chars` characters that end `s`.
fn tail(s: Span, chars: usize) -> Span {
    Span::new(s.end_line, s.end_col.saturating_sub(chars as u32).max(1), s.end_line, s.end_col)
}

impl Compiler {
    /// Indexes the modules of the project (everything but the standard library).
    pub(super) fn index_program(&mut self) -> PositionIndex {
        let mods: Vec<(String, Vec<Decl>)> = self.modules.iter().map(|m| (m.name.clone(), m.file.decls.clone())).collect();
        let mut index = PositionIndex::default();
        for (m, decls) in mods {
            if std_source(&m).is_some() {
                continue;
            }
            self.cur_mod = m.clone();
            let mut w = Walk::default();
            for d in &decls {
                w.ends = vec![(d.span.end_line, d.span.end_col)];
                self.ix_decl(&m, d, &mut w);
            }
            w.out.sort_by_key(|e| (e.span.line, e.span.col));
            let mut imports: Vec<String> = self.imports.get(&m).map(|s| s.iter().cloned().collect()).unwrap_or_default();
            imports.sort();
            index.imports.insert(m.clone(), imports);
            index.modules.insert(m, w.out);
        }
        self.ix_members(&mut index);
        index
    }

    /// The tables completion lists members from.
    fn ix_members(&mut self, index: &mut PositionIndex) {
        let key_of = member_key;
        for (key, info) in &self.tp.records {
            let list = index.members.entry(key.clone()).or_default();
            for (name, ty, _) in &info.fields {
                list.push(Member { name: name.clone(), kind: MemberKind::Field, ty: ty.show(), type_key: key_of(ty) });
            }
        }
        for ((key, name), sig) in &self.tp.methods {
            if !sig.public && std_source(&sig.module).is_some() {
                continue;
            }
            index.members.entry(key.clone()).or_default().push(Member { name: name.clone(), kind: MemberKind::Method, ty: sig_text(name, sig), type_key: key_of(&sig.ret) });
        }
        for (q, info) in &self.tp.enums {
            index.variants.insert(q.clone(), info.variants.iter().map(|(n, tys)| Member { name: n.clone(), kind: MemberKind::Variant, ty: tys.iter().map(|t| t.show()).collect::<Vec<_>>().join(", "), type_key: None }).collect());
        }
        for ((module, name), sig) in &self.tp.funcs {
            if sig.public {
                index.package_members.entry(module.clone()).or_default().push(Member { name: name.clone(), kind: MemberKind::Function, ty: sig_text(name, sig), type_key: key_of(&sig.ret) });
            }
        }
        for ((module, name), g) in &self.tp.globals {
            if g.public {
                index.package_members.entry(module.clone()).or_default().push(Member { name: name.clone(), kind: MemberKind::Global, ty: g.ty.show(), type_key: key_of(&g.ty) });
            }
        }
        for q in &self.tp.pub_types {
            if let Some((module, name)) = q.split_once('.') {
                index.package_members.entry(module.to_string()).or_default().push(Member { name: name.to_string(), kind: MemberKind::Type, ty: format!("type {}", name), type_key: None });
            }
        }
        for v in index.members.values_mut().chain(index.variants.values_mut()).chain(index.package_members.values_mut()) {
            v.sort_by(|a, b| a.name.cmp(&b.name));
        }
    }

    fn ix_push(&mut self, w: &mut Walk, e: IndexEntry) {
        w.out.push(e);
    }

    fn ix_name(&mut self, w: &mut Walk, span: Span, name: &str, is_declaration: bool, target: Referent, ty: Option<String>) {
        self.ix_entry(w, span, name, is_declaration, target, ty, None, Span::default());
    }

    #[allow(clippy::too_many_arguments)]
    fn ix_entry(&mut self, w: &mut Walk, span: Span, name: &str, is_declaration: bool, target: Referent, ty: Option<String>, type_key: Option<String>, visible: Span) {
        if span.is_none() {
            return;
        }
        self.ix_push(w, IndexEntry { span, name: name.to_string(), is_declaration, target, ty, type_key, visible });
    }

    fn ix_decl(&mut self, m: &str, d: &Decl, w: &mut Walk) {
        match &d.kind {
            DeclKind::Func(fd) => self.ix_func(m, fd, w),
            DeclKind::Type(td) => self.ix_type_decl(m, td, w),
            DeclKind::Var(vd) => {
                let g = self.tp.globals.get(&(m.to_string(), vd.name.clone()));
                let (ty, key) = (g.map(|g| g.ty.show()), g.and_then(|g| member_key(&g.ty)));
                self.ix_entry(w, vd.name_span, &vd.name, true, Referent::Global { module: m.to_string(), name: vd.name.clone() }, ty, key, Span::default());
                if let Some(t) = &vd.ty {
                    self.ix_type_use(t, w);
                }
                if let Some(e) = &vd.value {
                    self.ix_expr(e, w);
                }
            }
            DeclKind::Extern(x) => {
                let ty = self.tp.funcs.get(&(m.to_string(), x.name.clone())).map(|s| sig_text(&x.name, s));
                self.ix_name(w, x.name_span, &x.name, true, Referent::Function { module: m.to_string(), name: x.name.clone() }, ty);
                for p in &x.params {
                    if let Some(t) = &p.ty {
                        self.ix_type_use(t, w);
                    }
                }
                if let Some(t) = &x.ret {
                    self.ix_type_use(t, w);
                }
            }
            DeclKind::Embed(_) => {}
        }
    }

    fn ix_type_decl(&mut self, m: &str, td: &TypeDecl, w: &mut Walk) {
        let key = format!("{}.{}", m, td.name);
        w.generics = td.generics.iter().map(|g| g.name.clone()).collect();
        self.ix_name(w, td.name_span, &td.name, true, Referent::Type { module: m.to_string(), name: td.name.clone() }, None);
        match &td.body {
            TypeBody::Record(fields) => {
                for f in fields {
                    let ty = self.field_of(&key, &f.name).map(|x| x.0.show());
                    self.ix_name(w, f.name_span, &f.name, true, Referent::Field { type_key: key.clone(), name: f.name.clone() }, ty);
                    self.ix_type_use(&f.ty, w);
                }
            }
            TypeBody::Enum(variants) => {
                for v in variants {
                    self.ix_name(w, v.name_span, &v.name, true, Referent::Variant { enum_key: key.clone(), name: v.name.clone() }, None);
                    for t in &v.fields {
                        self.ix_type_use(t, w);
                    }
                }
            }
            TypeBody::Interface(sigs) => {
                for s in sigs {
                    self.ix_name(w, s.name_span, &s.name, true, Referent::Method { type_key: key.clone(), name: s.name.clone() }, None);
                    for p in &s.params {
                        if let Some(t) = &p.ty {
                            self.ix_type_use(t, w);
                        }
                    }
                    if let Some(r) = &s.ret {
                        if let Some(t) = &r.ty {
                            self.ix_type_use(t, w);
                        }
                    }
                }
            }
            TypeBody::Alias(t) => self.ix_type_use(t, w),
        }
        w.generics.clear();
    }

    fn ix_func(&mut self, m: &str, fd: &FuncDecl, w: &mut Walk) {
        let generic = !fd.generics.is_empty() || self.generic_receiver(m, fd).is_some();
        w.generics = fd.generics.iter().map(|g| g.name.clone()).collect();
        if let Some(r) = &fd.receiver {
            if let Type::Named { args, .. } = &r.ty {
                for a in args {
                    if let Type::Named { path, .. } = a {
                        if path.len() == 1 {
                            w.generics.push(path[0].clone());
                        }
                    }
                }
            }
        }
        // the signature, for a function that is not a template
        let sig: Option<Sig> = if generic {
            None
        } else {
            match &fd.receiver {
                Some(r) => self.resolve(&r.ty, m).ok().and_then(|rt| self.tp.methods.get(&(method_key(&rt), fd.name.clone())).cloned()),
                None => self.tp.funcs.get(&(m.to_string(), fd.name.clone())).cloned(),
            }
        };
        let target = match &fd.receiver {
            Some(r) => {
                let key = match (&sig, &r.ty) {
                    (Some(s), _) if s.receiver.is_some() => method_key(s.receiver.as_ref().unwrap()),
                    (_, Type::Named { path, .. }) => self.qname(m, path),
                    _ => String::new(),
                };
                Referent::Method { type_key: key, name: fd.name.clone() }
            }
            None => Referent::Function { module: m.to_string(), name: fd.name.clone() },
        };
        self.ix_name(w, fd.name_span, &fd.name, true, target, sig.as_ref().map(|s| sig_text(&fd.name, s)));

        let ret = sig.as_ref().map(|s| s.ret.clone()).unwrap_or(Ty::Void);
        self.fs = FnState::new(ret, String::new());
        w.scopes = vec![HashMap::new()];
        if let Some(r) = &fd.receiver {
            self.ix_type_use(&r.ty, w);
            let ty = sig.as_ref().and_then(|s| s.receiver.clone());
            self.ix_local(w, &r.name, r.name_span, ty);
        }
        for (i, p) in fd.params.iter().enumerate() {
            if let Some(t) = &p.ty {
                self.ix_type_use(t, w);
            }
            let ty = sig.as_ref().and_then(|s| s.params.get(i)).map(|p| p.ty.clone());
            self.ix_local(w, &p.name, p.name_span, ty);
            if let Some(d) = &p.default {
                self.ix_expr(d, w);
            }
        }
        if let Some(r) = &fd.ret {
            if let Some(t) = &r.ty {
                self.ix_type_use(t, w);
            }
        }
        self.ix_stmts(&fd.body, w);
        w.generics.clear();
    }

    /// Declares a local (or parameter): the entry, the scope of the walk and the scope of the
    /// type check. A type that is not known goes in as the unknown type.
    fn ix_local(&mut self, w: &mut Walk, name: &str, span: Span, ty: Option<Ty>) {
        let text = ty.as_ref().filter(|t| !is_unknown(t)).map(|t| t.show());
        let type_key = ty.as_ref().filter(|t| !is_unknown(t)).and_then(member_key);
        let visible = match w.ends.last() {
            Some(end) if !span.is_none() => Span::new(span.line, span.col, end.0, end.1),
            _ => Span::default(),
        };
        self.ix_entry(w, span, name, true, Referent::Local { decl: span }, text, type_key, visible);
        if let Some(s) = w.scopes.last_mut() {
            s.insert(name.to_string(), span);
        }
        let ty = ty.unwrap_or_else(unknown_ty);
        if let Some(sc) = self.fs.scopes.last_mut() {
            sc.vars.insert(name.to_string(), Local { off: 0, ty, mutable: true, captured: false, reg: None, imm: None, is_ref: false, ind: false });
        }
    }

    fn ix_push_scope(&mut self, w: &mut Walk) {
        w.scopes.push(HashMap::new());
        self.fs.scopes.push(Scope::default());
    }

    fn ix_pop_scope(&mut self, w: &mut Walk) {
        w.scopes.pop();
        self.fs.scopes.pop();
    }

    /// The type of `e` as the checker sees it, when it can say.
    fn ix_type_of(&mut self, e: &Expr, want: Option<&Ty>) -> Option<Ty> {
        self.checking_body = true;
        let r = self.check_expr(e, want).map(|x| x.ty);
        self.checking_body = false;
        self.lambda_vars.clear();
        r.filter(|t| !is_unknown(t))
    }

    fn ix_stmts(&mut self, b: &[Stmt], w: &mut Walk) {
        for s in b {
            self.ix_stmt(s, w);
        }
    }

    fn ix_block(&mut self, b: &[Stmt], w: &mut Walk) {
        self.ix_push_scope(w);
        self.ix_stmts(b, w);
        self.ix_pop_scope(w);
    }

    fn ix_stmt(&mut self, s: &Stmt, w: &mut Walk) {
        match &s.kind {
            StmtKind::Var(vd) => {
                let ann = match &vd.ty {
                    Some(t) => {
                        self.ix_type_use(t, w);
                        let m = self.cur_mod.clone();
                        self.resolve(t, &m).ok()
                    }
                    None => None,
                };
                let mut ty = ann.clone();
                if let Some(e) = &vd.value {
                    self.ix_expr(e, w);
                    if ty.is_none() {
                        ty = self.ix_type_of(e, None);
                    }
                }
                self.ix_local(w, &vd.name, vd.name_span, ty);
            }
            StmtKind::Assign { target, value, .. } => {
                self.ix_expr(target, w);
                self.ix_expr(value, w);
            }
            StmtKind::Return(e) => {
                if let Some(e) = e {
                    self.ix_expr(e, w);
                }
            }
            StmtKind::Defer(e) | StmtKind::Spawn(e) | StmtKind::Expr(e) => self.ix_expr(e, w),
            StmtKind::Unsafe(b) => {
                w.ends.push((s.span.end_line, s.span.end_col));
                self.ix_block(b, w);
                w.ends.pop();
            }
            StmtKind::Asm(a) => {
                for o in &a.operands {
                    self.ix_expr(&o.value, w);
                }
            }
            StmtKind::For { vars, var_spans, iter, body } => {
                self.ix_expr(iter, w);
                let types = self.ix_for_types(iter, vars.len());
                w.ends.push((s.span.end_line, s.span.end_col));
                self.ix_push_scope(w);
                for (i, v) in vars.iter().enumerate() {
                    let span = var_spans.get(i).copied().unwrap_or_default();
                    self.ix_local(w, v, span, types.get(i).cloned());
                }
                self.ix_stmts(body, w);
                self.ix_pop_scope(w);
                w.ends.pop();
            }
            StmtKind::While { cond, body } => {
                self.ix_expr(cond, w);
                w.ends.push((s.span.end_line, s.span.end_col));
                self.ix_block(body, w);
                w.ends.pop();
            }
            StmtKind::Break | StmtKind::Continue | StmtKind::Block { .. } => {}
        }
    }

    /// The types of the variables of a `for` over `iter`.
    fn ix_for_types(&mut self, iter: &Expr, n: usize) -> Vec<Ty> {
        if let ExprKind::Binary { op: "..", lhs, .. } = &iter.kind {
            return self.ix_type_of(lhs, None).into_iter().collect();
        }
        match (self.ix_type_of(iter, None), n) {
            (Some(Ty::Map(k, _)), 1) => vec![*k],
            (Some(Ty::Map(k, v)), 2) => vec![*k, *v],
            (Some(Ty::Text), 1) => vec![Ty::Char],
            (Some(Ty::List(el)), 1) => vec![*el],
            (Some(Ty::List(el)), 2) => vec![Ty::int(), *el],
            _ => Vec::new(),
        }
    }

    fn ix_expr(&mut self, e: &Expr, w: &mut Walk) {
        match &e.kind {
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Char(_) | ExprKind::Bool(_) | ExprKind::Nil | ExprKind::Block { .. } | ExprKind::Sql { .. } => {}
            ExprKind::Text(parts) => {
                for p in parts {
                    if let TextPart::Expr(x) = p {
                        self.ix_expr(x, w);
                    }
                }
            }
            ExprKind::Ident(name) => {
                let (target, ty) = self.ix_resolve_name(name, w);
                self.ix_name(w, e.span, name, false, target, ty);
            }
            ExprKind::List(items) => items.iter().for_each(|x| self.ix_expr(x, w)),
            ExprKind::Map(pairs) => {
                for (k, v) in pairs {
                    self.ix_expr(k, w);
                    self.ix_expr(v, w);
                }
            }
            ExprKind::Record { path, fields, field_spans } => self.ix_record(e, path, fields, field_spans, w),
            ExprKind::Lambda { params, body } => {
                w.ends.push((e.span.end_line, e.span.end_col));
                self.ix_push_scope(w);
                for p in params {
                    let ty = match &p.ty {
                        Some(t) => {
                            self.ix_type_use(t, w);
                            let m = self.cur_mod.clone();
                            self.resolve(t, &m).ok()
                        }
                        None => None,
                    };
                    self.ix_local(w, &p.name, p.name_span, ty);
                }
                self.ix_stmts(body, w);
                self.ix_pop_scope(w);
                w.ends.pop();
            }
            ExprKind::If { branches, else_body } => {
                w.ends.push((e.span.end_line, e.span.end_col));
                for (c, b) in branches {
                    self.ix_expr(c, w);
                    self.ix_block(b, w);
                }
                if let Some(b) = else_body {
                    self.ix_block(b, w);
                }
                w.ends.pop();
            }
            ExprKind::Match { scrutinee, arms } => {
                w.ends.push((e.span.end_line, e.span.end_col));
                self.ix_match(scrutinee, arms, w);
                w.ends.pop();
            }
            ExprKind::New(x) | ExprKind::Shared(x) | ExprKind::SpawnThread(x) | ExprKind::Deref(x) | ExprKind::Propagate(x) | ExprKind::Await(x) => self.ix_expr(x, w),
            ExprKind::Unary { expr, .. } => self.ix_expr(expr, w),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.ix_expr(lhs, w);
                self.ix_expr(rhs, w);
            }
            ExprKind::Index { expr, index } => {
                self.ix_expr(expr, w);
                self.ix_expr(index, w);
            }
            ExprKind::Field { expr, name, .. } => self.ix_field(e, expr, name, false, w),
            ExprKind::Call { callee, args, targs } => {
                match &callee.kind {
                    ExprKind::Field { expr, name, .. } => self.ix_field(callee, expr, name, true, w),
                    _ => self.ix_expr(callee, w),
                }
                for t in targs {
                    self.ix_type_use(t, w);
                }
                for a in args {
                    self.ix_expr(&a.value, w);
                }
            }
        }
    }

    /// A name used as a value or callee.
    fn ix_resolve_name(&mut self, name: &str, w: &Walk) -> (Referent, Option<String>) {
        for s in w.scopes.iter().rev() {
            if let Some(decl) = s.get(name) {
                let ty = self.lookup(name).map(|l| l.ty).filter(|t| !is_unknown(t)).map(|t| t.show());
                return (Referent::Local { decl: *decl }, ty);
            }
        }
        let m = self.cur_mod.clone();
        let key = (m.clone(), name.to_string());
        if let Some(sig) = self.tp.funcs.get(&key) {
            return (Referent::Function { module: m, name: name.to_string() }, Some(sig_text(name, sig)));
        }
        if let Some(g) = self.tp.globals.get(&key) {
            return (Referent::Global { module: m, name: name.to_string() }, Some(g.ty.show()));
        }
        if self.type_exists(&format!("{}.{}", m, name)) {
            return (Referent::Type { module: m, name: name.to_string() }, None);
        }
        if let Some((q, _)) = self.tp.variants.get(&key) {
            return (Referent::Variant { enum_key: q.clone(), name: name.to_string() }, None);
        }
        if self.imports.get(&m).is_some_and(|i| i.contains(name)) {
            return (Referent::Package { name: self.modkey(&m, name) }, None);
        }
        if w.generics.iter().any(|g| g == name) {
            return (Referent::TypeParameter { name: name.to_string() }, None);
        }
        if BUILTIN_NAMES.contains(&name) || int_type(name).is_some() {
            return (Referent::Builtin { name: name.to_string() }, None);
        }
        (Referent::Unknown, None)
    }

    /// `recv.name`: `field` is the whole expression, so its end is the end of the name.
    fn ix_field(&mut self, field: &Expr, recv: &Expr, name: &str, is_call: bool, w: &mut Walk) {
        self.ix_expr(recv, w);
        let span = tail(field.span, name.chars().count());
        let (target, ty) = self.ix_member(recv, name, is_call, w);
        self.ix_name(w, span, name, false, target, ty);
    }

    fn ix_member(&mut self, recv: &Expr, name: &str, is_call: bool, w: &Walk) -> (Referent, Option<String>) {
        let m = self.cur_mod.clone();
        if let ExprKind::Ident(r) = &recv.kind {
            let local = w.scopes.iter().any(|s| s.contains_key(r));
            if !local {
                // `package.member`
                if self.imports.get(&m).is_some_and(|i| i.contains(r)) && !self.tp.globals.contains_key(&(m.clone(), r.clone())) {
                    let mk = self.modkey(&m, r);
                    let key = (mk.clone(), name.to_string());
                    if let Some(sig) = self.tp.funcs.get(&key) {
                        return (Referent::Function { module: mk, name: name.to_string() }, Some(sig_text(name, sig)));
                    }
                    if let Some(g) = self.tp.globals.get(&key) {
                        return (Referent::Global { module: mk, name: name.to_string() }, Some(g.ty.show()));
                    }
                    if self.type_exists(&format!("{}.{}", mk, name)) {
                        return (Referent::Type { module: mk, name: name.to_string() }, None);
                    }
                    return (Referent::Unknown, None);
                }
                // `Enum.Variant`
                if let Some(q) = self.enum_type_named(r) {
                    if self.tp.enums.get(&q).is_some_and(|i| i.variants.iter().any(|v| v.0 == name)) {
                        return (Referent::Variant { enum_key: q, name: name.to_string() }, None);
                    }
                }
            }
        }
        let Some(t) = self.ix_type_of(recv, None) else { return (Referent::Unknown, None) };
        let view = rc_view(&t);
        let key = method_key(&view);
        let field = |c: &Compiler| match &view {
            Ty::Rec(r) => c.field_of(r, name).map(|(ft, _)| (Referent::Field { type_key: r.clone(), name: name.to_string() }, Some(ft.show()))),
            _ => None,
        };
        let method = |c: &Compiler| c.tp.methods.get(&(key.clone(), name.to_string())).map(|s| (Referent::Method { type_key: key.clone(), name: name.to_string() }, Some(sig_text(name, s))));
        let found = if is_call { method(self).or_else(|| field(self)) } else { field(self).or_else(|| method(self)) };
        if let Some(f) = found {
            return f;
        }
        // members the language gives to its own types: `len`, `message`, `code`, ...
        match (&view, name) {
            (Ty::List(_) | Ty::Text | Ty::Map(..), _) | (Ty::Char, "code") | (Ty::Err | Ty::Kind, _) => (Referent::Builtin { name: name.to_string() }, None),
            _ => (Referent::Unknown, None),
        }
    }

    fn ix_record(&mut self, e: &Expr, path: &[String], fields: &[(String, Expr)], field_spans: &[Span], w: &mut Walk) {
        let m = self.cur_mod.clone();
        let key = self.qname(&m, path);
        // the type name starts the literal (after `new` the parser starts the span at the name)
        let mut at = head(e.span, 0);
        for (i, part) in path.iter().enumerate() {
            let n = part.chars().count();
            let span = Span::new(at.line, at.col, at.line, at.col + n as u32);
            let last = i + 1 == path.len();
            let target = if last {
                let module = key.rsplit_once('.').map(|x| x.0.to_string()).unwrap_or_default();
                if self.type_exists(&key) {
                    Referent::Type { module, name: part.clone() }
                } else {
                    Referent::Unknown
                }
            } else {
                Referent::Package { name: self.modkey(&m, part) }
            };
            self.ix_name(w, span, part, false, target, None);
            at = Span::new(at.line, at.col + n as u32 + 1, at.line, at.col + n as u32 + 1);
        }
        for (i, (fname, value)) in fields.iter().enumerate() {
            let span = field_spans.get(i).copied().unwrap_or_default();
            let target = if self.field_of(&key, fname).is_some() || self.tp.rec_decls.get(&key).is_some_and(|d| d.1.iter().any(|f| &f.name == fname)) {
                Referent::Field { type_key: key.clone(), name: fname.clone() }
            } else {
                Referent::Unknown
            };
            let ty = self.field_of(&key, fname).map(|x| x.0.show());
            self.ix_name(w, span, fname, false, target, ty);
            self.ix_expr(value, w);
        }
    }

    fn ix_match(&mut self, scrutinee: &Expr, arms: &[MatchArm], w: &mut Walk) {
        self.ix_expr(scrutinee, w);
        let st = self.ix_type_of(scrutinee, None);
        for arm in arms {
            self.ix_push_scope(w);
            for atom in &arm.pattern {
                match atom {
                    PatAtom::Wildcard => {}
                    PatAtom::Literal(x) => self.ix_expr(x, w),
                    PatAtom::Name { name, bindings, span, binding_spans } => self.ix_pattern(&st, name, bindings.as_deref(), *span, binding_spans, w),
                }
            }
            self.ix_stmts(&arm.body, w);
            self.ix_pop_scope(w);
        }
    }

    fn ix_pattern(&mut self, st: &Option<Ty>, name: &str, bindings: Option<&[String]>, span: Span, binding_spans: &[Span], w: &mut Walk) {
        let mut payload: Vec<Ty> = Vec::new();
        let mut known = false;
        match st {
            Some(Ty::Enum(q)) => {
                if let Some(v) = self.tp.enums.get(q).and_then(|i| i.variants.iter().find(|v| v.0 == name)) {
                    payload = v.1.clone();
                    known = true;
                    self.ix_name(w, span, name, false, Referent::Variant { enum_key: q.clone(), name: name.to_string() }, None);
                }
            }
            Some(Ty::Result(inner)) if name == "ok" || name == "err" => {
                payload = vec![if name == "ok" { (**inner).clone() } else { Ty::Err }];
                known = true;
                self.ix_name(w, span, name, false, Referent::Builtin { name: name.to_string() }, None);
            }
            _ => {}
        }
        if known {
            for (i, b) in bindings.unwrap_or(&[]).iter().enumerate() {
                if b != "_" {
                    let sp = binding_spans.get(i).copied().unwrap_or_default();
                    self.ix_local(w, b, sp, payload.get(i).cloned());
                }
            }
        } else if bindings.is_none() && name != "_" {
            // a name that is no variant binds the matched value
            self.ix_local(w, name, span, st.clone());
        } else {
            // the type is not known: the variant is not resolved, the bindings are still names
            self.ix_name(w, span, name, false, Referent::Unknown, None);
            for (i, b) in bindings.unwrap_or(&[]).iter().enumerate() {
                if b != "_" {
                    let sp = binding_spans.get(i).copied().unwrap_or_default();
                    self.ix_local(w, b, sp, None);
                }
            }
        }
    }

    /// The names inside a written type.
    fn ix_type_use(&mut self, t: &Type, w: &mut Walk) {
        match t {
            Type::Named { path, args, span } => {
                let m = self.cur_mod.clone();
                if !span.is_none() && span.line == span.end_line {
                    let mut col = span.col;
                    for (i, part) in path.iter().enumerate() {
                        let n = part.chars().count() as u32;
                        let s = Span::new(span.line, col, span.line, col + n);
                        let last = i + 1 == path.len();
                        let target = if !last {
                            Referent::Package { name: self.modkey(&m, part) }
                        } else if path.len() == 1 && w.generics.iter().any(|g| g == part) {
                            Referent::TypeParameter { name: part.clone() }
                        } else {
                            let module = if path.len() == 1 { m.clone() } else { self.modkey(&m, &path[0]) };
                            if self.type_exists(&format!("{}.{}", module, part)) {
                                Referent::Type { module, name: part.clone() }
                            } else if path.len() == 1 && (int_type(part).is_some() || ["Map", "Future", "Weak", "Rc", "Chan"].contains(&part.as_str()) || self.resolve(&Type::Named { path: path.clone(), args: Vec::new(), span: Span::default() }, &m).is_ok()) {
                                Referent::Builtin { name: part.clone() }
                            } else {
                                Referent::Unknown
                            }
                        };
                        self.ix_name(w, s, part, false, target, None);
                        col += n + 1;
                    }
                }
                for a in args {
                    self.ix_type_use(a, w);
                }
            }
            Type::Optional(x) | Type::Pointer(x) | Type::Shared(x) | Type::Slice(x) | Type::Array(_, x) => self.ix_type_use(x, w),
            Type::Func { params, ret, .. } => {
                for p in params {
                    self.ix_type_use(p, w);
                }
                if let Some(r) = ret {
                    if let Some(t) = &r.ty {
                        self.ix_type_use(t, w);
                    }
                }
            }
        }
    }
}

/// `func name(a: int, b: text) -> int`
fn sig_text(name: &str, s: &Sig) -> String {
    let params: Vec<String> = s.params.iter().map(|p| format!("{}: {}", p.name, p.ty.show())).collect();
    if s.ret == Ty::Void {
        format!("func {}({})", name, params.join(", "))
    } else {
        format!("func {}({}) -> {}", name, params.join(", "), s.ret.show())
    }
}

impl IndexEntry {
    /// Whether the name starts on `line`.
    pub fn line_is(&self, line: u32) -> bool {
        self.span.line == line
    }
}
