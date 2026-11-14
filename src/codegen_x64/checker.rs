//! Typing without emitting code.
//!
//! `check_expr` answers "what is this expression, and what type does it have?" from the
//! declarations and the locals in scope, without writing any assembly and without changing the
//! compiler's state: it builds the `TypedExpr`. It covers the forms migrated so far and answers
//! `None` for the rest (and for anything that would be an error: reporting errors stays with
//! emission). `bodies.rs` walks whole function bodies with it.
//!
//! In builds with debug assertions (so in every test), `Compiler::expr` checks that the type
//! emission returns is the one `check_expr` gave.

use super::*;

/// A parameter as call binding sees it: its name (none for a variant's values) and its type.
type BoundParam = (Option<String>, Ty);

/// What binding the arguments of a call needs: the destination, the parameters (the last one a
/// list when `variadic`) and the result type.
struct CallShape {
    callee: TypedCallee,
    params: Vec<BoundParam>,
    variadic: bool,
    ret: Ty,
}

impl Compiler {
    /// The typed tree of `e`, for the forms migrated so far (`None` for the rest, and when any
    /// part of `e` is not covered). Its type is the one emission gives: emission
    /// asserts it in builds with debug assertions.
    pub(super) fn check_expr(&mut self, e: &Expr, want: Option<&Ty>) -> Option<TypedExpr> {
        let (kind, ty) = match &e.kind {
            ExprKind::Int(n) => {
                let t = match want {
                    Some(t) if t.is_int() => t.clone(),
                    _ => Ty::int(),
                };
                (TypedExprKind::Int(*n), t)
            }
            ExprKind::Float(f) => (TypedExprKind::Float(*f), Ty::Float { bits: if matches!(want, Some(Ty::Float { bits: 32 })) { 32 } else { 64 } }),
            ExprKind::Char(c) => (TypedExprKind::Char(*c), Ty::Char),
            ExprKind::Bool(b) => (TypedExprKind::Bool(*b), Ty::Bool),
            ExprKind::Nil => (TypedExprKind::Nil, Ty::Nil),
            ExprKind::Text(parts) => (TypedExprKind::Text(parts.clone()), Ty::Text),
            ExprKind::Ident(name) => {
                let t = match self.type_of_ident(name, want) {
                    Some(t) => t,
                    // a variant without values used as a value
                    None => {
                        let (q, idx) = self.tp.variants.get(&(self.cur_mod.clone(), name.clone()))?;
                        if !self.tp.enums.get(q)?.variants.get(*idx)?.1.is_empty() {
                            return None;
                        }
                        Ty::Enum(q.clone())
                    }
                };
                (TypedExprKind::Ident(name.clone()), t)
            }
            ExprKind::Unary { op: "not", expr } => {
                let x = self.check_expr(expr, None)?;
                (TypedExprKind::Unary { op: "not", expr: Box::new(x) }, Ty::Bool)
            }
            ExprKind::Unary { op: "-", expr } => {
                let x = self.check_expr(expr, want)?;
                let t = x.ty.clone();
                if !(t.is_int() || matches!(t, Ty::Float { .. })) {
                    return None;
                }
                (TypedExprKind::Unary { op: "-", expr: Box::new(x) }, t)
            }
            ExprKind::Binary { op, lhs, rhs } if matches!(*op, "and" | "or") => {
                let l = self.check_expr(lhs, None)?;
                let r = self.check_expr(rhs, None)?;
                (TypedExprKind::Binary { op, lhs: Box::new(l), rhs: Box::new(r) }, Ty::Bool)
            }
            ExprKind::Binary { op, lhs, rhs } if matches!(*op, "==" | "!=" | "<" | "<=" | ">" | ">=") => {
                let l = self.check_expr(lhs, None)?;
                let r = self.check_expr(rhs, Some(&l.ty))?;
                (TypedExprKind::Binary { op, lhs: Box::new(l), rhs: Box::new(r) }, Ty::Bool)
            }
            // bitwise operators: integers of one type; a shift keeps the type of the value shifted
            ExprKind::Binary { op, lhs, rhs } if matches!(*op, "bitand" | "bitor" | "bitxor" | "<<" | ">>") => {
                let w = want.filter(|t| t.is_int());
                let l = self.check_expr(lhs, w)?;
                let r = self.check_expr(rhs, Some(&l.ty))?;
                let shift = matches!(*op, "<<" | ">>");
                if !l.ty.is_int() || !r.ty.is_int() || (!shift && l.ty != r.ty) {
                    return None;
                }
                let t = l.ty.clone();
                (TypedExprKind::Binary { op, lhs: Box::new(l), rhs: Box::new(r) }, t)
            }
            ExprKind::Binary { op, lhs, rhs } if matches!(*op, "+" | "-" | "*" | "/" | "%") => {
                let w = want.filter(|t| t.is_int() || matches!(t, Ty::Float { .. }));
                let l = self.check_expr(lhs, w)?;
                let r = self.check_expr(rhs, Some(&l.ty))?;
                // pointer arithmetic: a pointer plus or minus an integer is a pointer
                if matches!(l.ty, Ty::Ptr(_)) && r.ty.is_int() && matches!(*op, "+" | "-") {
                    let t = l.ty.clone();
                    return Some(TypedExpr { kind: TypedExprKind::Binary { op, lhs: Box::new(l), rhs: Box::new(r) }, ty: t, line: e.line });
                }
                let number = l.ty.is_int() || matches!(l.ty, Ty::Float { .. });
                if l.ty != r.ty || !(number || (*op == "+" && l.ty == Ty::Text)) {
                    return None;
                }
                let t = l.ty.clone();
                (TypedExprKind::Binary { op, lhs: Box::new(l), rhs: Box::new(r) }, t)
            }
            ExprKind::Index { expr, index } => {
                let x = self.check_expr(expr, None)?;
                let i = self.check_expr(index, None)?;
                let t = match rc_view(&x.ty) {
                    Ty::List(el) => *el,
                    Ty::Map(_, v) => *v,
                    _ => match &x.ty {
                        // reading through a raw pointer (emission requires `unsafe`)
                        Ty::Ptr(el) => (**el).clone(),
                        _ => return None,
                    },
                };
                (TypedExprKind::Index { expr: Box::new(x), index: Box::new(i) }, t)
            }
            ExprKind::Field { expr, name, safe: false } if matches!(&expr.kind, ExprKind::Ident(m) if self.lookup(m).is_none() && self.is_module(m)) => {
                let ExprKind::Ident(m) = &expr.kind else { return None };
                let module = self.modkey(&self.cur_mod, m);
                let g = self.tp.globals.get(&(module.clone(), name.clone()))?;
                let t = match (&g.konst, want) {
                    (Some(_), Some(w)) if w.is_int() => w.clone(),
                    _ => g.ty.clone(),
                };
                (TypedExprKind::Global { module, name: name.clone() }, t)
            }
            ExprKind::Field { expr, name, safe: false } => {
                let x = self.check_expr(expr, None)?;
                let t = match rc_view(&x.ty) {
                    Ty::Rec(r) => self.field_of(&r, name)?.0,
                    Ty::List(_) | Ty::Text | Ty::Map(..) if name == "len" => Ty::int(),
                    Ty::Char if name == "code" => Ty::int(),
                    _ => return None,
                };
                (TypedExprKind::Field { expr: Box::new(x), name: name.clone(), safe: false }, t)
            }
            ExprKind::Call { callee, args, targs } if targs.is_empty() => {
                if let Some(shape) = self.resolve_call_sig(callee) {
                    let args = self.bind_args(&shape.params, shape.variadic, args)?;
                    (TypedExprKind::Call { callee: shape.callee, args }, shape.ret)
                } else if let Some(x) = self.check_builtin_call(callee, args) {
                    x
                } else if let Some(x) = self.check_generic_call(callee, args) {
                    x
                } else {
                    self.check_member_call(callee, args)?
                }
            }
            ExprKind::List(items) => {
                let mut el = match want {
                    Some(Ty::List(t)) => Some((**t).clone()),
                    _ => None,
                };
                let mut typed = Vec::new();
                for it in items {
                    let x = self.check_expr(it, el.as_ref())?;
                    match &el {
                        Some(w) if *w != x.ty => return None,
                        Some(_) => {}
                        None => el = Some(x.ty.clone()),
                    }
                    typed.push(x);
                }
                (TypedExprKind::List(typed), Ty::List(Box::new(el?)))
            }
            ExprKind::Map(pairs) => {
                let (mut kt, mut vt) = match want {
                    Some(Ty::Map(k, v)) => (Some((**k).clone()), Some((**v).clone())),
                    _ => (None, None),
                };
                let mut typed = Vec::new();
                for (ke, ve) in pairs {
                    let k = self.check_expr(ke, kt.as_ref())?;
                    let v = self.check_expr(ve, vt.as_ref())?;
                    match (&kt, &vt) {
                        (Some(a), Some(b)) if *a != k.ty || *b != v.ty => return None,
                        (Some(_), Some(_)) => {}
                        _ => {
                            kt = Some(k.ty.clone());
                            vt = Some(v.ty.clone());
                        }
                    }
                    typed.push((k, v));
                }
                (TypedExprKind::Map(typed), Ty::Map(Box::new(kt?), Box::new(vt?)))
            }
            ExprKind::Record { .. } => {
                let (kind, rec) = self.check_record_literal(e)?;
                (kind, Ty::Rec(rec))
            }
            ExprKind::New(inner) => {
                let ExprKind::Record { .. } = &inner.kind else { return None };
                let (kind, rec) = self.check_record_literal(inner)?;
                let x = TypedExpr { kind, ty: Ty::Rec(rec), line: inner.line };
                let t = Ty::Rc(Box::new(x.ty.clone()));
                (TypedExprKind::New(Box::new(x)), t)
            }
            ExprKind::Shared(inner) => {
                let x = self.check_expr(inner, want)?;
                if !matches!(x.ty, Ty::Rec(_) | Ty::List(_) | Ty::Map(..) | Ty::Text) {
                    return None;
                }
                let t = x.ty.clone();
                (TypedExprKind::Shared(Box::new(x)), t)
            }
            ExprKind::Deref(inner) => {
                let x = self.check_expr(inner, None)?;
                let Ty::Ptr(el) = x.ty.clone() else { return None };
                (TypedExprKind::Deref(Box::new(x)), *el)
            }
            // a closure whose type is fully given by the expected function type; its body is
            // not walked (it has scopes of its own), so it declares nothing here
            ExprKind::Lambda { params, body } => {
                let Some(Ty::Func { params: wp, ret, ext: false }) = want else { return None };
                if wp.len() != params.len() || params.iter().any(|p| p.ty.is_some() || p.by_ref) || matches!(&**ret, Ty::Param { name, .. } if name == "?") {
                    return None;
                }
                let t = want?.clone();
                (TypedExprKind::Lambda { params: params.clone(), body: body.clone() }, t)
            }
            ExprKind::SpawnThread(call) => {
                let ExprKind::Call { .. } = &call.kind else { return None };
                let x = self.check_expr(call, None)?;
                let t = self.tp.funcs.get(&("thread".to_string(), "start".to_string()))?.ret.clone();
                (TypedExprKind::SpawnThread(Box::new(x)), t)
            }
            ExprKind::Propagate(inner) => {
                if !matches!(self.fs.ret, Ty::Result(_)) {
                    return None;
                }
                let x = self.check_expr(inner, None)?;
                let Ty::Result(v) = x.ty.clone() else { return None };
                (TypedExprKind::Propagate(Box::new(x)), *v)
            }
            ExprKind::Await(inner) => {
                let x = self.check_expr(inner, None)?;
                let Ty::Future(v) = x.ty.clone() else { return None };
                (TypedExprKind::Await(Box::new(x)), *v)
            }
            _ => return None,
        };
        Some(TypedExpr { kind, ty, line: e.line })
    }

    /// The parameter types of a callee found by `resolve_call` (a function, a method or an enum
    /// variant used as a constructor), with its destination and result type. Variadic and
    /// extern-variadic callees are not covered.
    fn resolve_call_sig(&mut self, callee: &Expr) -> Option<CallShape> {
        let params = |sig: &Sig| {
            (!sig.c_variadic).then(|| sig.params.iter().map(|p| (Some(p.name.clone()), p.ty.clone())).collect::<Vec<_>>())
        };
        if let Some((c, t)) = self.resolve_call(callee) {
            let sig = match &c {
                TypedCallee::Func { module, name } => self.tp.funcs.get(&(module.clone(), name.clone()))?,
                TypedCallee::Method { rec_key, name } => self.tp.methods.get(&(rec_key.clone(), name.clone()))?,
                _ => return None,
            };
            return Some(CallShape { params: params(sig)?, variadic: sig.variadic, callee: c, ret: t });
        }
        // an enum variant used as a constructor (a function of the same name wins, as in emission)
        let ExprKind::Ident(name) = &callee.kind else { return None };
        let key = (self.cur_mod.clone(), name.clone());
        if self.lookup(name).is_some() || self.tp.funcs.contains_key(&key) {
            return None;
        }
        let (q, idx) = self.tp.variants.get(&key)?;
        let tys = &self.tp.enums.get(q)?.variants.get(*idx)?.1;
        let params = tys.iter().map(|t| (None, t.clone())).collect();
        Some(CallShape { callee: TypedCallee::Variant { enum_key: q.clone(), index: *idx }, params, variadic: false, ret: Ty::Enum(q.clone()) })
    }

    /// Binds the arguments of a call to the parameters and types each one from its parameter's
    /// type. Not covered (`None`): spread arguments, missing or mistyped arguments.
    fn bind_args(&mut self, params: &[BoundParam], variadic: bool, args: &[Arg]) -> Option<Vec<TypedArg>> {
        let fixed = if variadic { params.len() - 1 } else { params.len() };
        let mut slots: Vec<Option<&Arg>> = vec![None; params.len()];
        let mut rest: Vec<&Arg> = Vec::new();
        let mut pos = 0;
        for a in args {
            if a.spread {
                return None;
            }
            let i = match &a.name {
                Some(n) => params.iter().position(|p| p.0.as_ref() == Some(n))?,
                None if pos < fixed => {
                    pos += 1;
                    pos - 1
                }
                // the values of a variadic parameter
                None if variadic => {
                    rest.push(a);
                    continue;
                }
                None => return None,
            };
            if i >= fixed || slots[i].is_some() {
                return None;
            }
            slots[i] = Some(a);
        }
        let mut typed = Vec::new();
        for (i, (slot, (_, pty))) in slots.into_iter().zip(params).enumerate() {
            if variadic && i == fixed {
                // the values collected into the list parameter: each typed from the element type
                let Ty::List(el) = pty else { return None };
                let mut items = Vec::new();
                for a in &rest {
                    let x = self.check_expr(&a.value, Some(el))?;
                    if x.ty != **el || a.by_ref {
                        return None;
                    }
                    items.push(x);
                }
                let line = items.first().map_or(0, |x| x.line);
                let list = TypedExpr { kind: TypedExprKind::List(items), ty: pty.clone(), line };
                typed.push(TypedArg { param: i, by_ref: false, value: list });
                continue;
            }
            let a = slot?;
            let x = self.check_expr(&a.value, Some(pty))?;
            if x.ty != *pty {
                return None;
            }
            typed.push(TypedArg { param: i, by_ref: a.by_ref, value: x });
        }
        Some(typed)
    }

    /// Calls of the built-ins whose typing needs no inference: `print`, `println`, `error`,
    /// `abort`, `char`, `float`/`f64`/`f32`, `sleep`, `live_objects`. Arguments are typed in the
    /// order of the call; `by_ref` and spread arguments are not covered.
    fn check_builtin_call(&mut self, callee: &Expr, args: &[Arg]) -> Option<(TypedExprKind, Ty)> {
        let ExprKind::Ident(name) = &callee.kind else { return None };
        if self.lookup(name).is_some() || args.iter().any(|a| a.by_ref || a.spread || a.name.is_some()) {
            return None;
        }
        let user = self.tp.funcs.contains_key(&(self.cur_mod.clone(), name.clone()));
        // `int(x)`, `u8(x)`, `bool(x)`...: a conversion to the named type
        if let Some(target) = int_type(name).or(if name == "bool" { Some(Ty::Bool) } else { None }) {
            let [a] = args else { return None };
            let x = self.check_expr(&a.value, None)?;
            let from_float = matches!(x.ty, Ty::Float { .. });
            let ok = from_float && target != Ty::Bool || x.ty.is_int() || matches!(x.ty, Ty::Ptr(_) | Ty::Bool | Ty::Func { ext: true, .. });
            if !ok {
                return None;
            }
            let arg = TypedArg { param: 0, by_ref: false, value: x };
            return Some((TypedExprKind::Call { callee: TypedCallee::Builtin(name.clone()), args: vec![arg] }, target));
        }
        let (want, ty): (Vec<Option<Ty>>, Ty) = match (name.as_str(), args.len()) {
            ("print" | "println", _) => (vec![None; args.len()], Ty::Void),
            ("error", 1) => (vec![Some(Ty::Text)], Ty::Err),
            ("error", 2) => (vec![Some(Ty::Kind), Some(Ty::Text)], Ty::Err),
            ("error", 3) => (vec![Some(Ty::Kind), Some(Ty::Text), Some(Ty::Err)], Ty::Err),
            ("abort", 1) if !user => (vec![Some(Ty::Text)], Ty::Void),
            ("char", 1) if !user => (vec![None], Ty::Char),
            ("float" | "f64", 1) => (vec![None], Ty::Float { bits: 64 }),
            ("f32", 1) => (vec![None], Ty::Float { bits: 32 }),
            ("sleep", 1) if !user => (vec![Some(Ty::i64())], Ty::Future(Box::new(Ty::Void))),
            ("live_objects", 0) => (vec![], Ty::int()),
            ("__from_bits", 1) => (vec![Some(Ty::i64())], Ty::Float { bits: 64 }),
            ("__bits", 1) => (vec![Some(Ty::Float { bits: 64 })], Ty::i64()),
            ("__sqrt" | "__floor" | "__ceil" | "__trunc", 1) => (vec![Some(Ty::Float { bits: 64 })], Ty::Float { bits: 64 }),
            ("__kind", 1) => (vec![Some(Ty::Text)], Ty::Kind),
            _ => return None,
        };
        let mut typed = Vec::new();
        for (i, (a, w)) in args.iter().zip(&want).enumerate() {
            let x = self.check_expr(&a.value, w.as_ref())?;
            if matches!(name.as_str(), "char" | "sleep") && !x.ty.is_int() {
                return None;
            }
            if let Some(w) = w {
                if x.ty != *w {
                    return None;
                }
            }
            typed.push(TypedArg { param: i, by_ref: false, value: x });
        }
        Some((TypedExprKind::Call { callee: TypedCallee::Builtin(name.clone()), args: typed }, ty))
    }

    /// A call of a generic function whose type arguments follow from its arguments: the type
    /// arguments are bound as emission binds them, the instance's signature is built (and
    /// forgotten again) and the arguments are typed against it. Not covered: explicit type
    /// arguments, closures or bare number literals where a type parameter is inferred, variadic
    /// parameters, named arguments.
    fn check_generic_call(&mut self, callee: &Expr, args: &[Arg]) -> Option<(TypedExprKind, Ty)> {
        if !self.checking_body || args.iter().any(|a| a.spread || a.by_ref || a.name.is_some()) {
            return None;
        }
        let (module, fd) = match &callee.kind {
            ExprKind::Ident(name) if self.lookup(name).is_none() => {
                let key = (self.cur_mod.clone(), name.clone());
                (key.0.clone(), self.tmpl_funcs.get(&key)?.0.clone())
            }
            ExprKind::Field { expr, name, safe: false } => {
                let ExprKind::Ident(m) = &expr.kind else { return None };
                if self.lookup(m).is_some() || !self.is_module(m) {
                    return None;
                }
                let module = self.modkey(&self.cur_mod, m);
                let fd = self.tmpl_funcs.get(&(module.clone(), name.clone()))?.0.clone();
                (module, fd)
            }
            _ => return None,
        };
        if fd.params.len() != args.len() || fd.params.iter().any(|p| p.variadic || p.ty.is_none() || p.default.is_some()) {
            return None;
        }
        let names: Vec<String> = fd.generics.iter().map(|g| g.name.clone()).collect();
        self.speculative(|c| {
            let mut b: HashMap<String, Ty> = HashMap::new();
            for (a, p) in args.iter().zip(&fd.params) {
                let pt = p.ty.as_ref()?;
                if !mentions_param(pt, &names) {
                    continue;
                }
                if matches!(a.value.kind, ExprKind::Lambda { .. }) || is_int_literal(&a.value) {
                    return None;
                }
                let x = c.check_expr(&a.value, None)?;
                c.unify(pt, &x.ty, &names, &mut b, &module).ok()?;
            }
            if names.iter().any(|n| !b.contains_key(n) || b[n] == Ty::Void) {
                return None;
            }
            let argkey = names.iter().map(|n| ty_full(&b[n])).collect::<Vec<_>>().join(",");
            let inst = format!("{}__{}", fd.name, sym_of(&argkey));
            let mut decl = fd.clone();
            decl.generics.clear();
            decl.name = inst.clone();
            let saved = std::mem::replace(&mut c.bindings, b);
            let saved_mod = std::mem::replace(&mut c.cur_mod, module.clone());
            let sig = c.func_sig(&module, &decl);
            c.bindings = saved;
            c.cur_mod = saved_mod;
            let sig = sig.ok()?;
            let params: Vec<BoundParam> = sig.params.iter().map(|p| (Some(p.name.clone()), p.ty.clone())).collect();
            let typed = c.bind_args(&params, false, args)?;
            Some((TypedExprKind::Call { callee: TypedCallee::GenericInstance { inst_name: inst }, args: typed }, sig.ret.clone()))
        })
    }

    /// Calls of the methods of built-in types that need no inference: `push`/`add`/`clear` on a
    /// list, `has` on a map, `slice`/`byte`/`bytes`/`char_count` on a text, `to_text`/`len_utf8`/
    /// `is_ascii` on a char. A method the user declared on the type wins.
    fn check_member_call(&mut self, callee: &Expr, args: &[Arg]) -> Option<(TypedExprKind, Ty)> {
        let ExprKind::Field { expr, name, safe: false } = &callee.kind else { return None };
        if args.iter().any(|a| a.by_ref || a.spread || a.name.is_some()) {
            return None;
        }
        let recv = self.check_expr(expr, None)?;
        let ot = rc_view(&recv.ty);
        if self.tp.methods.contains_key(&(method_key(&ot), name.clone())) {
            return None;
        }
        // a method of the bound of a type parameter
        if let Ty::Param { bound: Some(b), .. } = &ot {
            let (im, sg) = self.bound_sig(b, name)?;
            let self_ty = ot.clone();
            return self.speculative(|c| {
                let (ps, ret) = c.interface_method(&im, &sg, &self_ty).ok()?;
                if ps.len() != args.len() {
                    return None;
                }
                let mut typed = Vec::new();
                for (i, (a, pt)) in args.iter().zip(&ps).enumerate() {
                    let x = c.check_expr(&a.value, Some(pt))?;
                    if x.ty != *pt {
                        return None;
                    }
                    typed.push(TypedArg { param: i, by_ref: false, value: x });
                }
                Some((TypedExprKind::Call { callee: TypedCallee::Member { receiver: Box::new(recv), name: name.clone() }, args: typed }, ret))
            });
        }
        let (want, ty): (Vec<Ty>, Ty) = match (&ot, name.as_str(), args.len()) {
            (Ty::List(el), "push" | "add", 1) => (vec![(**el).clone()], Ty::Void),
            (Ty::List(_), "clear", 0) => (vec![], Ty::Void),
            (Ty::Map(k, _), "has", 1) => (vec![(**k).clone()], Ty::Bool),
            (Ty::Text, "slice" | "__raw_slice", 2) => (vec![Ty::i64(), Ty::i64()], Ty::Text),
            (Ty::Text, "byte", 1) => (vec![Ty::i64()], Ty::Int { bits: 8, signed: false }),
            (Ty::Text, "bytes", 0) => (vec![], Ty::List(Box::new(Ty::Int { bits: 8, signed: false }))),
            (Ty::Text, "char_count", 0) => (vec![], Ty::int()),
            (Ty::Char, "to_text", 0) => (vec![], Ty::Text),
            (Ty::Char, "len_utf8", 0) => (vec![], Ty::int()),
            (Ty::Char, "is_ascii", 0) => (vec![], Ty::Bool),
            _ => return None,
        };
        let mut typed = Vec::new();
        for (i, (a, w)) in args.iter().zip(&want).enumerate() {
            let x = self.check_expr(&a.value, Some(w))?;
            let fits = if ot == Ty::Text { x.ty.is_int() } else { x.ty == *w };
            if !fits {
                return None;
            }
            typed.push(TypedArg { param: i, by_ref: false, value: x });
        }
        Some((TypedExprKind::Call { callee: TypedCallee::Member { receiver: Box::new(recv), name: name.clone() }, args: typed }, ty))
    }

    /// A record literal of a record that is not generic: its typed fields (each typed from the
    /// declared field type) and the record's key. Unknown fields and mistyped values are not typed.
    fn check_record_literal(&mut self, e: &Expr) -> Option<(TypedExprKind, String)> {
        let ExprKind::Record { path, fields } = &e.kind else { return None };
        let q = self.qname(&self.cur_mod, path);
        if !self.tp.rec_decls.contains_key(&q) {
            return None;
        }
        let decl = self.tp.records.get(&q)?.fields.clone();
        let mut typed = Vec::new();
        for (name, value) in fields {
            let fty = &decl.iter().find(|d| &d.0 == name)?.1;
            let x = self.check_expr(value, Some(fty))?;
            if x.ty != *fty {
                return None;
            }
            typed.push((name.clone(), x));
        }
        Some((TypedExprKind::Record { rec_key: q.clone(), fields: typed }, q))
    }

    /// A local, or a global, constant or function of the current module used as a value.
    fn type_of_ident(&self, name: &str, want: Option<&Ty>) -> Option<Ty> {
        if let Some(l) = self.lookup(name) {
            return Some(l.ty);
        }
        let key = (self.cur_mod.clone(), name.to_string());
        if let Some(g) = self.tp.globals.get(&key) {
            return Some(match g.konst {
                Some(_) => match want {
                    Some(t) if t.is_int() => t.clone(),
                    _ => g.ty.clone(),
                },
                None => g.ty.clone(),
            });
        }
        if let Some(sig) = self.tp.funcs.get(&key) {
            if sig.is_extern {
                return None;
            }
            return Some(sig.func_ty(matches!(want, Some(Ty::Func { ext: true, .. }))));
        }
        None
    }

    /// The destination and the result type of a call of a function declared by name (`f(x)` or
    /// `module.f(x)`) or of a method of a record. Calls of built-ins, of generic functions, of
    /// closures and of other methods are not covered yet.
    fn resolve_call(&mut self, callee: &Expr) -> Option<(TypedCallee, Ty)> {
        const BUILTINS: &[&str] = &["abort", "bool", "char", "error", "print", "println", "weak", "live_objects", "channel", "timeout", "sleep", "heap_bytes", "float", "f64", "f32"];
        let func = |key: (String, String)| {
            let ret = self.tp.funcs.get(&key)?.ret.clone();
            Some((TypedCallee::Func { module: key.0, name: key.1 }, ret))
        };
        match &callee.kind {
            ExprKind::Ident(name) => {
                if self.lookup(name).is_some() || name.starts_with("__") || BUILTINS.contains(&name.as_str()) || int_type(name).is_some() {
                    return None;
                }
                func((self.cur_mod.clone(), name.clone()))
            }
            ExprKind::Field { expr, name, safe: false } => {
                if let ExprKind::Ident(m) = &expr.kind {
                    if self.is_module(m) {
                        if m == "c" || m == "format" {
                            return None;
                        }
                        return func((self.modkey(&self.cur_mod, m), name.clone()));
                    }
                }
                // a method of a record: found by the type of the receiver
                let key = method_key(&rc_view(&self.check_expr(expr, None)?.ty));
                let ret = self.tp.methods.get(&(key.clone(), name.clone()))?.ret.clone();
                Some((TypedCallee::Method { rec_key: key, name: name.clone() }, ret))
            }
            _ => None,
        }
    }
}
