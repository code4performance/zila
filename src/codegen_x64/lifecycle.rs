//! Part of the code generator (`codegen_x64`), methods of `Compiler`: `defer`, async functions, futures and channels.

use super::*;

impl Compiler {
    /// Creates this function's list of deferred closures when its body has a `defer`.
    pub(super) fn init_defers(&mut self, body: &[Stmt]) -> R<()> {
        if !block_has_defer(body) {
            return Ok(());
        }
        let list_ty = Ty::List(Box::new(Ty::Func { params: vec![], ret: Box::new(Ty::Void), ext: false }));
        let list = self.hidden_local(list_ty);
        self.e("mov ecx, 1");
        self.e("call rt_list_new");
        self.e(format!("mov qword ptr [rbp - {}], rax", list));
        let save = self.slot();
        self.fs.defer_list = Some(list);
        self.fs.may_own = true;
        self.fs.defer_save = Some(save);
        Ok(())
    }

    /// `defer call(...)`: builds a closure for the call and adds it to the list.
    pub(super) fn defer_stmt(&mut self, e: &Expr, line: u32) -> R<()> {
        let Some(list) = self.fs.defer_list else {
            return self.err("`defer` is not allowed here");
        };
        if !matches!(e.kind, ExprKind::Call { .. }) {
            return self.err("`defer` needs a call, like `defer file.close()`");
        }
        let body = vec![Stmt { kind: StmtKind::Expr(e.clone()), line }];
        let want = Ty::Func { params: vec![], ret: Box::new(Ty::Void), ext: false };
        // records are shared (not copied) so the call sees them as they are at exit;
        // the result of a call that can fail is discarded
        self.next_lambda_share = true;
        self.next_lambda_ignore_result = true;
        self.lambda(&[], &body, Some(&want))?;
        self.e("mov rdx, rax");
        self.e(format!("mov rcx, qword ptr [rbp - {}]", list));
        self.e("call rt_list_push");
        Ok(())
    }

    /// Runs the deferred closures, last in first out, keeping the value in rax.
    pub(super) fn emit_run_defers(&mut self) {
        let (Some(list), Some(save)) = (self.fs.defer_list, self.fs.defer_save) else { return };
        let (top, done) = (self.lbl("defer"), self.lbl("deferdone"));
        self.e(format!("mov qword ptr [rbp - {}], rax", save));
        self.label(&top);
        self.e(format!("mov rcx, qword ptr [rbp - {}]", list));
        self.e("cmp qword ptr [rcx], 0");
        self.e(format!("je {}", done));
        self.e("call rt_list_take");
        let c = self.temp();
        self.e(format!("mov qword ptr [rbp - {}], rax", c));
        self.emit_call(&[], "", Some((c, false)));
        self.release_slot(c);
        self.free(c);
        self.e(format!("jmp {}", top));
        self.label(&done);
        self.e(format!("mov rax, qword ptr [rbp - {}]", save));
    }

    // ---------------- futures ----------------

    pub(super) fn future_fn(&self, name: &str) -> R<Sig> {
        match self.funcs.get(&("future".to_string(), name.to_string())) {
            Some(s) => Ok(s.clone()),
            None => self.err("async needs the future library"),
        }
    }

    /// Body of an `async func`: the original body becomes a task (a closure
    /// that returns the declared type) and the function returns the future
    /// that task completes.
    pub(super) fn gen_async_body(&mut self, sig: &Sig, body: &[Stmt]) -> R<()> {
        let Ty::Future(inner) = sig.ret.clone() else { unreachable!() };
        self.emit_async_launch(&inner, body)
    }

    /// Runs `body` (which returns `inner`) as a task; leaves its future in rax.
    pub(super) fn emit_async_launch(&mut self, inner: &Ty, body: &[Stmt]) -> R<()> {
        let inner = Box::new(inner.clone());
        let want = Ty::Func { params: vec![], ret: inner.clone(), ext: false };
        self.lambda(&[], body, Some(&want))?;
        let c = self.temp();
        self.e(format!("mov qword ptr [rbp - {}], rax", c));
        let new = self.future_fn("future_new")?;
        self.emit_call(&[], &format!("call {}", new.sym), None);
        let f = self.temp();
        self.e(format!("mov qword ptr [rbp - {}], rax", f));
        if self.is_rc(&inner) {
            // the result slot must own what it holds: use a list flagged as holding heap values
            let (_, off) = self.field_of("future.Future", "slot").expect("future library layout");
            self.e("mov ecx, 1");
            self.e("call rt_list_new");
            self.e(format!("mov rcx, qword ptr [rbp - {}]", f));
            self.e(format!("mov rdx, qword ptr [rcx + {}]", off));
            self.e(format!("mov qword ptr [rcx + {}], rax", off));
            self.e("mov rcx, rdx");
            self.e("call rt_release");
        }
        // `launch` takes the closure and one reference; the caller gets the other
        self.e(format!("mov rax, qword ptr [rbp - {}]", f));
        self.retain();
        let launch = self.future_fn("launch")?;
        self.emit_call(&[c, f], &format!("call {}", launch.sym), None);
        self.e(format!("mov rax, qword ptr [rbp - {}]", f));
        self.free(f);
        self.free(c);
        Ok(())
    }

    /// `timeout(ms, future)`: a future of `T or error` that fails when `ms` pass first.
    pub(super) fn timeout_future(&mut self, ms_e: &Expr, f_e: &Expr) -> R<Ty> {
        let t = self.expr(ms_e, Some(&Ty::i64()))?;
        if !t.is_int() {
            return self.err(format!("timeout() needs a number of milliseconds, found {}", t.show()));
        }
        let tag = self.lbl("tm");
        let ms_name = format!("__{}_ms", tag);
        let f_name = format!("__{}_f", tag);
        let off = self.declare_local(&ms_name, Ty::i64(), false);
        self.e(format!("mov qword ptr [rbp - {}], rax", off));
        let ft = self.expr(f_e, None)?;
        let Ty::Future(inner) = ft.clone() else {
            return self.err(format!("timeout() needs a Future, found {}", ft.show()));
        };
        let off = self.declare_local(&f_name, ft, false);
        self.e(format!("mov qword ptr [rbp - {}], rax", off));
        let wait = if *inner == Ty::Void {
            format!("        await {}\n        return\n", f_name)
        } else {
            format!("        return await {}\n", f_name)
        };
        let src = format!(
            "package t\nfunc f()\n    let i = future.timeout_wait({}, {})\n    if i == 1\n{}    end\n    return future.timed_out()\nend\n",
            ms_name, f_name, wait
        );
        let file = parse_source(&src).map_err(|d| format!("internal: timeout template: {}", d.msg))?;
        let DeclKind::Func(fd) = &file.decls[0].kind else { unreachable!() };
        // a future that can already fail keeps a single `or error`: the failure of
        // either the operation or the timeout
        let result = match *inner {
            Ty::Result(u) => Ty::Result(u),
            t => Ty::Result(Box::new(t)),
        };
        let line = self.line;
        self.emit_async_launch(&result, &fd.body)?;
        self.line = line;
        Ok(Ty::Future(Box::new(result)))
    }

    /// `await all(xs)` / `await race(xs)` on a list of futures.
    pub(super) fn await_combinator(&mut self, which: &str, arg: &Expr) -> R<Ty> {
        let lt = self.expr(arg, None)?;
        let Ty::List(el) = lt.clone() else {
            return self.err(format!("{}() needs a list of futures, found {}", which, lt.show()));
        };
        let Ty::Future(v) = *el.clone() else {
            return self.err(format!("{}() needs a list of futures, found {}", which, lt.show()));
        };
        let wait = self.future_fn("wait")?;
        self.push_scope();
        let l = self.hidden_local(lt.clone());
        self.e(format!("mov qword ptr [rbp - {}], rax", l));
        let res;
        if which == "all" {
            if *v == Ty::Void {
                return self.err("all() needs futures that return a value");
            }
            let out = self.slot();
            let idx = self.slot();
            self.new_list(&v.clone());
            self.e(format!("mov qword ptr [rbp - {}], rax", out));
            self.e(format!("mov qword ptr [rbp - {}], 0", idx));
            let (start, end) = (self.lbl("all"), self.lbl("allend"));
            self.label(&start);
            self.e(format!("mov rdx, qword ptr [rbp - {}]", idx));
            self.e(format!("mov rcx, qword ptr [rbp - {}]", l));
            self.e("cmp rdx, qword ptr [rcx]");
            self.e(format!("jge {}", end));
            self.e("call rt_list_get");
            self.retain();
            let tmp = self.temp();
            self.e(format!("mov qword ptr [rbp - {}], rax", tmp));
            self.emit_call(&[tmp], &format!("call {}", wait.sym), None);
            self.free(tmp);
            self.e("mov rdx, rax");
            self.e(format!("mov rcx, qword ptr [rbp - {}]", out));
            self.e("call rt_list_push");
            self.e(format!("inc qword ptr [rbp - {}]", idx));
            self.e(format!("jmp {}", start));
            self.label(&end);
            self.e(format!("mov rax, qword ptr [rbp - {}]", out));
            res = Ty::List(v);
        } else {
            let any = self.future_fn("wait_any")?;
            let arg_t = self.temp();
            self.e(format!("mov rax, qword ptr [rbp - {}]", l));
            self.retain();
            self.e(format!("mov qword ptr [rbp - {}], rax", arg_t));
            self.emit_call(&[arg_t], &format!("call {}", any.sym), None);
            self.free(arg_t);
            self.e("mov rdx, rax");
            self.e(format!("mov rcx, qword ptr [rbp - {}]", l));
            self.e("call rt_list_get");
            self.retain();
            let tmp = self.temp();
            self.e(format!("mov qword ptr [rbp - {}], rax", tmp));
            self.emit_call(&[tmp], &format!("call {}", wait.sym), None);
            self.free(tmp);
            self.norm(&v);
            res = *v;
        }
        self.pop_scope();
        Ok(res)
    }

    // ---------------- channels ----------------

    pub(super) fn task_fn(&self, name: &str) -> R<Sig> {
        match self.funcs.get(&("task".to_string(), name.to_string())) {
            Some(s) => Ok(s.clone()),
            None => self.err("channels need the task library"),
        }
    }

    /// `channel(T)`
    pub(super) fn channel_new(&mut self, args: &[Arg]) -> R<Ty> {
        if args.len() != 1 {
            return self.err("channel() takes the element type: `channel(int)`");
        }
        let ty = match &args[0].value.kind {
            ExprKind::Ident(n) => Type::Named { path: vec![n.clone()], args: vec![] },
            ExprKind::Field { expr, name, .. } => match &expr.kind {
                ExprKind::Ident(m) => Type::Named { path: vec![m.clone(), name.clone()], args: vec![] },
                _ => return self.err("channel() takes a type, like `channel(int)`"),
            },
            _ => return self.err("channel() takes a type, like `channel(int)`"),
        };
        let el = self.resolve(&ty, &self.cur_mod.clone())?;
        let sig = self.task_fn("chan_new")?;
        self.emit_call(&[], &format!("call {}", sig.sym), None);
        Ok(Ty::Chan(Box::new(el)))
    }

    /// `ch.send(v)`, `ch.receive()`, `ch.close()`; the channel is in rax (owned).
    pub(super) fn chan_method(&mut self, el: &Ty, name: &str, args: &[Arg]) -> R<Ty> {
        let c = self.temp();
        self.e(format!("mov qword ptr [rbp - {}], rax", c));
        let res = match name {
            "send" => {
                if args.len() != 1 {
                    return self.err("'send' takes one value");
                }
                let vt = self.value(&args[0].value, Some(el))?;
                self.check(el, &vt, "value sent on the channel")?;
                self.norm(el);
                let v = self.temp();
                self.e(format!("mov qword ptr [rbp - {}], rax", v));
                let sig = self.task_fn("chan_send")?;
                self.emit_call(&[c, v], &format!("call {}", sig.sym), None);
                self.free(v);
                Ty::Void
            }
            "receive" => {
                let sig = self.task_fn("chan_recv")?;
                self.emit_call(&[c], &format!("call {}", sig.sym), None);
                self.norm(el);
                el.clone()
            }
            "close" => {
                let sig = self.task_fn("chan_close")?;
                self.emit_call(&[c], &format!("call {}", sig.sym), None);
                Ty::Void
            }
            _ => return self.err(format!("a channel has no method '{}'; use send, receive or close", name)),
        };
        self.free(c);
        Ok(res)
    }
}
