//! Part of the code generator (`codegen_x64`), methods of `Compiler`: drop procedures of records and enums.

use super::*;

impl Compiler {
    /// Per-record `B_drop_*` (releases heap fields) and `B_copy_*` (shallow copy: nested plain records are copied, lists, maps and text are retained, so they are copied by their first write).
    pub(super) fn gen_record_helpers(&mut self, q: &str) {
        let info = &self.records[q];
        let size = info.size;
        let rc_fields: Vec<(Ty, u32)> =
            info.fields.iter().filter(|f| self.is_rc(&f.1)).map(|f| (f.1.clone(), f.2)).collect();
        let s = sym_of(q);
        let drop = self.rec_drop(q);
        if drop != "0" {
            let mut d = format!("B_drop_{s} proc\n    push rbp\n    mov rbp, rsp\n    sub rsp, 48\n    mov qword ptr [rbp - 8], rcx\n");
            if let Some(hook) = self.drop_hook(q) {
                // the object is being destroyed: give the hook's receiver a reference of its own
                d.push_str(&format!("    mov qword ptr [rcx - 16], 2\n    call {}\n", hook));
            }
            for (_, off) in &rc_fields {
                d.push_str(&format!(
                    "    mov rcx, qword ptr [rbp - 8]\n    mov rcx, qword ptr [rcx + {off}]\n    call rt_release\n"
                ));
            }
            d.push_str(&format!("    mov rsp, rbp\n    pop rbp\n    ret\nB_drop_{s} endp\n"));
            self.procs.push(d);
        }
        let drop_arg = if drop == "0" { "xor edx, edx".to_string() } else { format!("lea rdx, {}", drop) };
        let mut c = format!(
            "B_copy_{s} proc\n    push rbp\n    mov rbp, rsp\n    sub rsp, 48\n    test rcx, rcx\n    jnz Lgo_{s}\n    xor eax, eax\n    jmp Lend_{s}\nLgo_{s}:\n    mov qword ptr [rbp - 8], rcx\n    mov ecx, {size}\n    {drop_arg}\n    call rt_alloc\n    mov qword ptr [rbp - 16], rax\n    mov rcx, rax\n    mov rdx, qword ptr [rbp - 8]\n    mov r8d, {size}\n    call rt_memcpy\n"
        );
        for (ty, off) in &rc_fields {
            if matches!(ty, Ty::Rec(f) if !self.shared_recs.contains(f)) {
                let Ty::Rec(f) = ty else { unreachable!() };
                c.push_str(&format!(
                    "    mov rcx, qword ptr [rbp - 16]\n    mov rcx, qword ptr [rcx + {off}]\n    call B_copy_{f}\n    mov rcx, qword ptr [rbp - 16]\n    mov qword ptr [rcx + {off}], rax\n",
                    f = sym_of(f)
                ));
            } else {
                c.push_str(&format!(
                    "    mov rcx, qword ptr [rbp - 16]\n    mov rcx, qword ptr [rcx + {off}]\n    call rt_retain\n"
                ));
            }
        }
        c.push_str(&format!(
            "    mov rax, qword ptr [rbp - 16]\nLend_{s}:\n    mov rsp, rbp\n    pop rbp\n    ret\nB_copy_{s} endp\n"
        ));
        self.procs.push(c);
    }

    /// `B_rcdrop_*`: the drop of an object made by `new`. The weak cell is cleared first (so `upgrade()`
    /// is `nil` from here on, also inside `drop`), then the record's own drop runs (user `drop`, heap fields).
    pub(super) fn gen_rc_drop(&mut self, q: &str) {
        let s = sym_of(q);
        let cell = self.cell_off(q);
        let inner = self.rec_drop(q);
        let mut d = format!(
            "B_rcdrop_{s} proc\n    push rbp\n    mov rbp, rsp\n    sub rsp, 48\n    mov qword ptr [rbp - 8], rcx\n    mov rcx, qword ptr [rcx + {cell}]\n    test rcx, rcx\n    jz Lrcd_{s}\n    mov qword ptr [rcx], 0\n    call rt_release\nLrcd_{s}:\n    mov rcx, qword ptr [rbp - 8]\n"
        );
        if inner != "0" {
            d.push_str(&format!("    call {}\n", inner));
        }
        d.push_str(&format!("    mov rsp, rbp\n    pop rbp\n    ret\nB_rcdrop_{s} endp\n"));
        self.procs.push(d);
    }

    /// `weak(x)`: rax = the weak cell of the `Rc<T>` `x` (made on first use, one reference for the caller).
    pub(super) fn weak_of(&mut self, args: &[Arg]) -> R<Ty> {
        if args.len() != 1 {
            return self.err("weak() takes one argument, an Rc<T>");
        }
        let (t, own) = self.operand(&args[0].value, None)?;
        let Ty::Rc(inner) = &t else {
            return self.err(format!("weak() takes an Rc<T> (made with `new`), found {}", t.show()));
        };
        let Ty::Rec(r) = &**inner else {
            return self.err(format!("weak() takes an Rc<T> of a record, found {}", t.show()));
        };
        let cell = self.cell_off(r);
        let (have, done) = (self.lbl("wkhave"), self.lbl("wkdone"));
        let slot = self.temp();
        self.e(format!("mov qword ptr [rbp - {}], rax", slot));
        self.e("test rax, rax");
        self.e(format!("jz {}", done));
        self.e(format!("mov rdx, qword ptr [rax + {}]", cell));
        self.e("test rdx, rdx");
        self.e(format!("jnz {}", have));
        self.e("mov ecx, 8");
        self.e("xor edx, edx");
        self.e("call rt_alloc");
        self.e(format!("mov rcx, qword ptr [rbp - {}]", slot));
        self.e("mov qword ptr [rax], rcx");
        self.e(format!("mov qword ptr [rcx + {}], rax", cell));
        self.e("mov rdx, rax");
        self.label(&have);
        self.e("mov rax, rdx");
        self.retain();
        self.label(&done);
        if own {
            let keep = self.temp();
            self.e(format!("mov qword ptr [rbp - {}], rax", keep));
            self.release_slot(slot);
            self.e(format!("mov rax, qword ptr [rbp - {}]", keep));
            self.free(keep);
        }
        self.free(slot);
        Ok(Ty::Weak(inner.clone()))
    }

    /// `w.upgrade()` with the weak cell in rax (owned): rax = a new strong reference or nil.
    pub(super) fn weak_upgrade(&mut self, inner: &Ty, args: &[Arg]) -> R<Ty> {
        if !args.is_empty() {
            return self.err("upgrade() takes no arguments");
        }
        let nil = self.lbl("upnil");
        let slot = self.temp();
        self.e(format!("mov qword ptr [rbp - {}], rax", slot));
        self.e("test rax, rax");
        self.e(format!("jz {}", nil));
        self.e("mov rax, qword ptr [rax]");
        self.e("test rax, rax");
        self.e(format!("jz {}", nil));
        self.retain();
        self.label(&nil);
        let keep = self.temp();
        self.e(format!("mov qword ptr [rbp - {}], rax", keep));
        self.release_slot(slot);
        self.e(format!("mov rax, qword ptr [rbp - {}]", keep));
        self.free(keep);
        self.free(slot);
        Ok(Ty::Rc(Box::new(inner.clone())))
    }

    /// Per-enum `B_edrop_*`: releases the heap fields of the active variant.
    pub(super) fn gen_enum_drop(&mut self, q: &str) {
        if self.enum_drop(q) == "0" {
            return;
        }
        let info = self.enums[q].clone();
        let s = sym_of(q);
        let mut d = format!(
            "B_edrop_{s} proc\n    push rbp\n    mov rbp, rsp\n    sub rsp, 48\n    mov qword ptr [rbp - 8], rcx\n    mov rax, qword ptr [rcx]\n    mov qword ptr [rbp - 16], rax\n"
        );
        for (idx, (_, tys)) in info.variants.iter().enumerate() {
            if !tys.iter().any(|t| self.is_rc(t)) {
                continue;
            }
            d.push_str(&format!("    cmp qword ptr [rbp - 16], {idx}\n    jne Lnx_{s}_{idx}\n"));
            for (i, t) in tys.iter().enumerate() {
                if self.is_rc(t) {
                    d.push_str(&format!(
                        "    mov rcx, qword ptr [rbp - 8]\n    mov rcx, qword ptr [rcx + {}]\n    call rt_release\n",
                        8 + 8 * i
                    ));
                }
            }
            d.push_str(&format!("Lnx_{s}_{idx}:\n"));
        }
        d.push_str(&format!("    mov rsp, rbp\n    pop rbp\n    ret\nB_edrop_{s} endp\n"));
        self.procs.push(d);
    }

    // ---------------- helpers ----------------

}
