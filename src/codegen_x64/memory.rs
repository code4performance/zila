//! Part of the code generator (`codegen_x64`), methods of `Compiler`: reference counts, retain and release, allocation.

use super::*;

impl Compiler {
    pub(super) fn is_rc(&self, t: &Ty) -> bool {
        match t {
            Ty::Text | Ty::Kind | Ty::Rec(_) | Ty::List(_) | Ty::Map(..) | Ty::Err | Ty::Result(_) | Ty::Chan(_) | Ty::Future(_) | Ty::Rc(_) | Ty::Weak(_) => true,
            Ty::Func { ext, .. } => !*ext,
            Ty::Enum(q) => self.tp.enum_payload.contains(q),
            _ => false,
        }
    }

    /// rax keeps its value; the reference count of the object in rax grows.
    pub(super) fn retain(&mut self) {
        self.e("mov rcx, rax");
        self.e("call rt_retain");
    }

    pub(super) fn retain_if(&mut self, t: &Ty) {
        if self.is_rc(t) {
            self.retain();
        }
    }

    /// Releases the value in rax (rax itself is preserved).
    pub(super) fn release_rax(&mut self) {
        self.e("mov rcx, rax");
        self.e("call rt_release");
    }

    /// Releases the object stored in a slot (rax is preserved).
    pub(super) fn release_slot(&mut self, off: i32) {
        self.e(format!("mov rcx, qword ptr [rbp - {}]", off));
        self.e("call rt_release");
    }

    /// rax = new zeroed heap object. `drop` is a procedure name or "0".
    pub(super) fn alloc(&mut self, size: u32, drop: &str) {
        self.e(format!("mov ecx, {}", size));
        if drop == "0" {
            self.e("xor edx, edx");
        } else {
            self.e(format!("lea rdx, {}", drop));
        }
        self.e("call rt_alloc");
    }

    /// Checks the key type of a `Map<K, V>` and builds the type.
    pub(super) fn map_type(&mut self, k: Ty, v: Ty) -> R<Ty> {
        if self.target == Target::Bare {
            return self.err("maps need the Windows runtime and cannot be used in a bare-metal kernel");
        }
        if !matches!(k, Ty::Param { .. }) {
            if self.declared {
                self.check_key(&k)?;
            } else {
                self.deferred_keys.push((k.clone(), self.line, self.cur_mod.clone()));
            }
        }
        Ok(Ty::Map(Box::new(k), Box::new(v)))
    }

    /// A key type must be `Hash`: the error names the type and the reason.
    pub(super) fn check_key(&self, k: &Ty) -> R<()> {
        match self.lacks(k, Cap::Hash) {
            None => Ok(()),
            Some((leaf, why)) => {
                if leaf {
                    self.err(format!("{} cannot be a map key because it is not Hash ({})", k.show(), why))
                } else {
                    self.err(format!("{} cannot be a map key because {}", k.show(), why))
                }
            }
        }
    }

    /// Keys kept as plain numbers: integers, `bool` and enums without data.
    pub(super) fn plain_key(&self, k: &Ty) -> bool {
        k.is_int() || matches!(k, Ty::Char | Ty::Bool | Ty::Param { .. }) || matches!(k, Ty::Enum(q) if !self.tp.enum_payload.contains(q))
    }

    /// rax = new empty map of type `t`.
    pub(super) fn new_map(&mut self, t: &Ty) -> R<()> {
        let Ty::Map(k, v) = t else { unreachable!() };
        let mut flags = (self.is_rc(v) as i32) << 1;
        let mut thunks = None;
        if **k == Ty::Text {
            flags |= 1;
        } else if !self.plain_key(k) {
            flags |= 12;
            thunks = Some(self.map_thunks_for(k)?);
        }
        self.e(format!("mov ecx, {}", flags));
        if let Some((h, e)) = thunks {
            self.e(format!("lea rdx, {}", h));
            self.e(format!("lea r8, {}", e));
        }
        self.e("call rt_map_new");
        Ok(())
    }

    pub(super) fn new_list(&mut self, el: &Ty) {
        let flag = self.is_rc(el) as i32;
        self.e(format!("mov ecx, {}", flag));
        self.e("call rt_list_new");
    }

    pub(super) fn rec_has_rc(&self, q: &str) -> bool {
        self.tp.records[q].fields.iter().any(|f| self.is_rc(&f.1))
    }

    /// `func (ref x: T) drop()` runs when the last reference to a `T` goes away.
    pub(super) fn drop_hook(&self, q: &str) -> Option<String> {
        self.tp.methods.get(&(q.to_string(), "drop".to_string())).map(|s| s.sym.clone())
    }

    pub(super) fn rec_drop(&self, q: &str) -> String {
        if self.rec_has_rc(q) || self.drop_hook(q).is_some() {
            format!("B_drop_{}", sym_of(q))
        } else {
            "0".into()
        }
    }

    pub(super) fn enum_drop(&self, q: &str) -> String {
        let info = &self.tp.enums[q];
        if info.variants.iter().any(|v| v.1.iter().any(|t| self.is_rc(t))) {
            format!("B_edrop_{}", sym_of(q))
        } else {
            "0".into()
        }
    }

    pub(super) fn box_drop(&self, inner: &Ty) -> &'static str {
        if self.is_rc(inner) {
            "rt_drop_result_rc"
        } else {
            "rt_drop_result_plain"
        }
    }

}
