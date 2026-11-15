//! Part of the code generator (`codegen_x64`): see the section comment below.

use super::*;

// ---------------------------------------------------------------------------
// Constant folding
//
// An expression made only of literals is computed here instead of by the program. The rules are
// exactly the ones the generated code follows: integers wrap at the width of their type (each
// operation is normalized, as `binary` does), shifts use the count the processor masks, floats
// follow IEEE 754, and a division that could fail at run time is never folded.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Const {
    Int(i64),
    Float(f64),
    Bool(bool),
}

/// `v` as the register value an integer of type `t` holds after normalization.
pub(super) fn norm_val(v: i64, t: &Ty) -> i64 {
    match t {
        Ty::Int { bits: 8, signed: true } => v as i8 as i64,
        Ty::Int { bits: 8, signed: false } => v as u8 as i64,
        Ty::Int { bits: 16, signed: true } => v as i16 as i64,
        Ty::Int { bits: 16, signed: false } => v as u16 as i64,
        Ty::Int { bits: 32, signed: true } => v as i32 as i64,
        Ty::Int { bits: 32, signed: false } => v as u32 as i64,
        _ => v,
    }
}

/// An assembler spelling of a 64-bit constant that `mov` accepts for any value.
pub(super) fn imm64(v: i64) -> String {
    if v >= i32::MIN as i64 && v <= i32::MAX as i64 {
        v.to_string()
    } else {
        format!("0{:X}h", v as u64)
    }
}

impl Compiler {
    /// The value of `e` when it is made of literals only. `t` is the integer type its literals
    /// have, `fbits` the float width.
    pub(super) fn fold(&self, e: &Expr, t: &Ty, fbits: u8) -> Option<Const> {
        match &e.kind {
            ExprKind::Int(n) => lit_fits(*n, false, t).then_some(Const::Int(*n as i64)),
            ExprKind::Float(f) => Some(Const::Float(if fbits == 32 { *f as f32 as f64 } else { *f })),
            ExprKind::Bool(b) => Some(Const::Bool(*b)),
            ExprKind::Unary { op, expr } => {
                let v = self.fold(expr, t, fbits)?;
                match (*op, v) {
                    ("-", Const::Int(n)) => Some(Const::Int(norm_val(n.wrapping_neg(), t))),
                    ("-", Const::Float(f)) => Some(Const::Float(-f)),
                    ("not", Const::Bool(b)) => Some(Const::Bool(!b)),
                    ("bitnot", Const::Int(n)) => Some(Const::Int(norm_val(!n, t))),
                    _ => None,
                }
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let is_cmp = matches!(*op, "==" | "!=" | "<" | "<=" | ">" | ">=");
                if is_cmp {
                    // comparison operands are typed on their own: literals are plain `int`s
                    let i64t = Ty::int();
                    let (a, b) = (self.fold(lhs, &i64t, fbits)?, self.fold(rhs, &i64t, fbits)?);
                    let r = match (a, b) {
                        (Const::Int(a), Const::Int(b)) => cmp_op(op, a.cmp(&b)),
                        (Const::Float(a), Const::Float(b)) => match *op {
                            "==" => a == b,
                            "!=" => a != b,
                            "<" => a < b,
                            "<=" => a <= b,
                            ">" => a > b,
                            _ => a >= b,
                        },
                        (Const::Bool(a), Const::Bool(b)) => match *op {
                            "==" => a == b,
                            "!=" => a != b,
                            _ => return None,
                        },
                        _ => return None,
                    };
                    return Some(Const::Bool(r));
                }
                let (a, b) = (self.fold(lhs, t, fbits)?, self.fold(rhs, t, fbits)?);
                match (a, b) {
                    (Const::Bool(a), Const::Bool(b)) => match *op {
                        "and" => Some(Const::Bool(a && b)),
                        "or" => Some(Const::Bool(a || b)),
                        _ => None,
                    },
                    (Const::Int(a), Const::Int(b)) => {
                        let unsigned = t.is_unsigned();
                        let v = match *op {
                            "+" => a.wrapping_add(b),
                            "-" => a.wrapping_sub(b),
                            "*" => a.wrapping_mul(b),
                            "/" | "%" => {
                                // a zero divisor (and the one overflowing signed division) must still
                                // fail at run time
                                if b == 0 || (!unsigned && a == i64::MIN && b == -1) {
                                    return None;
                                }
                                if unsigned {
                                    let (x, y) = (a as u64, b as u64);
                                    (if *op == "/" { x / y } else { x % y }) as i64
                                } else if *op == "/" {
                                    a / b
                                } else {
                                    a % b
                                }
                            }
                            "bitand" => a & b,
                            "bitor" => a | b,
                            "bitxor" => a ^ b,
                            "<<" => a.wrapping_shl((b & 63) as u32),
                            ">>" => {
                                let n = (b & 63) as u32;
                                if unsigned {
                                    ((a as u64) >> n) as i64
                                } else {
                                    a >> n
                                }
                            }
                            _ => return None,
                        };
                        Some(Const::Int(norm_val(v, t)))
                    }
                    (Const::Float(a), Const::Float(b)) => {
                        let v = match *op {
                            "+" => a + b,
                            "-" => a - b,
                            "*" => a * b,
                            "/" => a / b,
                            _ => return None,
                        };
                        // an f32 operation rounds to f32
                        Some(Const::Float(if fbits == 32 { v as f32 as f64 } else { v }))
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Emits a folded constant and returns its type.
    pub(super) fn emit_const(&mut self, c: Const, t: &Ty, fbits: u8) -> Ty {
        match c {
            Const::Int(v) => {
                self.e(format!("mov rax, {}", imm64(v)));
                t.clone()
            }
            Const::Bool(b) => {
                self.e(format!("mov eax, {}", b as i32));
                Ty::Bool
            }
            Const::Float(f) => {
                if fbits == 32 {
                    let b = (f as f32).to_bits();
                    if b == 0 {
                        self.e("xor eax, eax");
                    } else {
                        self.e(format!("mov eax, {}", imm64(b as i64)));
                    }
                } else {
                    let b = f.to_bits();
                    if b == 0 {
                        self.e("xor eax, eax");
                    } else {
                        self.e(format!("mov rax, {}", imm64(b as i64)));
                    }
                }
                Ty::Float { bits: fbits }
            }
        }
    }
}

pub(super) fn cmp_op(op: &str, o: std::cmp::Ordering) -> bool {
    use std::cmp::Ordering::*;
    match op {
        "==" => o == Equal,
        "!=" => o != Equal,
        "<" => o == Less,
        "<=" => o != Greater,
        ">" => o == Greater,
        _ => o != Less,
    }
}

// ---------------------------------------------------------------------------
// Conditions that jump
//
// `if` and `while` do not need the 0 or 1 a comparison produces: they jump on the processor
// flags the comparison sets. `and`, `or` and `not` become jumps as well.
// ---------------------------------------------------------------------------

pub(super) fn inverse_cc(cc: &str) -> &'static str {
    match cc {
        "e" => "ne",
        "ne" => "e",
        "l" => "ge",
        "ge" => "l",
        "le" => "g",
        "g" => "le",
        "b" => "ae",
        "ae" => "b",
        "be" => "a",
        _ => "be",
    }
}

pub(super) fn is_cmp_op(op: &str) -> bool {
    matches!(op, "==" | "!=" | "<" | "<=" | ">" | ">=")
}

impl Compiler {
    /// A right operand `simple_rhs` can load without evaluating anything.
    pub(super) fn rhs_is_simple(&self, e: &Expr) -> bool {
        match &e.kind {
            ExprKind::Int(_) | ExprKind::Float(_) => true,
            ExprKind::Ident(n) => self.lookup(n).is_some_and(|l| !l.captured),
            _ => false,
        }
    }

    /// Jumps to `label` when `cond` is `jump_if`, and falls through otherwise.
    pub(super) fn cond_jump(&mut self, cond: &Expr, label: &str, jump_if: bool) -> R<()> {
        self.line = cond.line;
        let m = self.fs.stmt_temps.len();
        if self.opt {
            match &cond.kind {
                ExprKind::Unary { op: "not", expr } => return self.cond_jump(expr, label, !jump_if),
                ExprKind::Binary { op: "and", lhs, rhs } => {
                    if jump_if {
                        let skip = self.lbl("andskip");
                        self.cond_jump(lhs, &skip, false)?;
                        self.cond_jump(rhs, label, true)?;
                        self.label(&skip);
                    } else {
                        self.cond_jump(lhs, label, false)?;
                        self.cond_jump(rhs, label, false)?;
                    }
                    return Ok(());
                }
                ExprKind::Binary { op: "or", lhs, rhs } => {
                    if jump_if {
                        self.cond_jump(lhs, label, true)?;
                        self.cond_jump(rhs, label, true)?;
                    } else {
                        let ok = self.lbl("orok");
                        self.cond_jump(lhs, &ok, true)?;
                        self.cond_jump(rhs, label, false)?;
                        self.label(&ok);
                    }
                    return Ok(());
                }
                ExprKind::Binary { op, lhs, rhs } if is_cmp_op(op) && self.rhs_is_simple(rhs) => {
                    let lit = self.literal_side_type(op, lhs, rhs, None);
                    let (lt, lown) = self.operand(lhs, lit.as_ref())?;
                    if lown && self.is_rc(&lt) {
                        // an owned heap value on the left: the general path releases it
                        let t1 = self.temp();
                        self.e(format!("mov qword ptr [rbp - {}], rax", t1));
                        let rt = self.simple_rhs(rhs, &lt).unwrap();
                        self.binary_core(op, &lt, &rt, true)?;
                        self.release_slot(t1);
                        self.free(t1);
                        self.flush_temps(m);
                        self.e("test rax, rax");
                        self.e(format!("{} {}", if jump_if { "jnz" } else { "jz" }, label));
                        return Ok(());
                    }
                    let rt = self.simple_rhs(rhs, &lt).unwrap();
                    if self.fs.stmt_temps.len() == m && self.fuse_compare(op, &lt, &rt, label, jump_if) {
                        return Ok(());
                    }
                    self.binary_core(op, &lt, &rt, true)?;
                    self.flush_temps(m);
                    self.e("test rax, rax");
                    self.e(format!("{} {}", if jump_if { "jnz" } else { "jz" }, label));
                    return Ok(());
                }
                _ => {}
            }
        }
        self.expr(cond, Some(&Ty::Bool))?;
        self.flush_temps(m);
        self.e("test rax, rax");
        self.e(format!("{} {}", if jump_if { "jnz" } else { "jz" }, label));
        Ok(())
    }

    /// With the operands in rax and rcx, compares them and jumps. Returns false, having emitted
    /// nothing, when the types need the general code (text, enums, results, type errors).
    pub(super) fn fuse_compare(&mut self, op: &str, lt: &Ty, rt: &Ty, label: &str, jump_if: bool) -> bool {
        let intlike = |t: &Ty| t.is_int() || matches!(t, Ty::Char | Ty::Bool | Ty::Ptr(_));
        if intlike(lt) && intlike(rt) && (compatible(lt, rt) || compatible(rt, lt)) {
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
                _ => "ae",
            };
            self.e("cmp rax, rcx");
            let cc = if jump_if { cc } else { inverse_cc(cc) };
            self.e(format!("j{} {}", cc, label));
            return true;
        }
        if let (Ty::Float { bits: a }, Ty::Float { bits: b }) = (lt, rt) {
            if a == b {
                let d = *a == 64;
                let (mv, uc) = if d { ("movq", "ucomisd") } else { ("movd", "ucomiss") };
                let (x, y) = if d { ("rax", "rcx") } else { ("eax", "ecx") };
                self.e(format!("{} xmm0, {}", mv, x));
                self.e(format!("{} xmm1, {}", mv, y));
                // unordered (a NaN) sets ZF, PF and CF: only `ne` is true then
                match op {
                    ">" | ">=" | "<" | "<=" => {
                        let swap = op.starts_with('<');
                        if swap {
                            self.e(format!("{} xmm1, xmm0", uc));
                        } else {
                            self.e(format!("{} xmm0, xmm1", uc));
                        }
                        let strict = op == ">" || op == "<";
                        let cc = match (strict, jump_if) {
                            (true, true) => "a",
                            (true, false) => "be",
                            (false, true) => "ae",
                            (false, false) => "b",
                        };
                        self.e(format!("j{} {}", cc, label));
                    }
                    _ => {
                        self.e(format!("{} xmm0, xmm1", uc));
                        // `==` is true when equal and ordered; `!=` is its opposite
                        let jump_when_equal = (op == "==") == jump_if;
                        if jump_when_equal {
                            // jump when equal and ordered
                            let skip = self.lbl("nan");
                            self.e(format!("jp {}", skip));
                            self.e(format!("je {}", label));
                            self.label(&skip);
                        } else {
                            // jump when not equal or unordered
                            self.e(format!("jp {}", label));
                            self.e(format!("jne {}", label));
                        }
                    }
                }
                return true;
            }
        }
        false
    }
}

/// A note for the debugger (`;@...`): it stands between instructions without being one, so every
/// rule below looks past it, and a build with notes gives the same code as one without.
fn is_note(line: &str) -> bool {
    line.starts_with(";@")
}

/// The last line of `out` that is an instruction or label, not a note.
fn last_real(out: &[String]) -> Option<&String> {
    out.iter().rev().find(|l| !is_note(l))
}

/// Local cleanups of one function's code: a reload of a value just stored, a jump to the next
/// label, code that can never run, and a register moved to itself. Every rule looks at adjacent
/// lines only and never moves anything across a label.
pub(super) fn peephole(code: &[String], ret_label: &str) -> Vec<String> {
    let mut cur: Vec<String> = code.to_vec();
    loop {
        let before = cur.len();
        let mut out: Vec<String> = Vec::with_capacity(cur.len());
        let mut dead = false; // after an unconditional jump or ret, up to the next label
        for line in &cur {
            let t = line.trim();
            if is_note(line) {
                // a note in code that can never run points at nothing
                if !dead {
                    out.push(line.clone());
                }
                continue;
            }
            let is_label = t.ends_with(':') && !line.starts_with(' ');
            if is_label {
                dead = false;
                // a jump to the label that follows it is no jump at all
                let mut i = out.len();
                while i > 0 && ((out[i - 1].ends_with(':') && !out[i - 1].starts_with(' ')) || is_note(&out[i - 1])) {
                    i -= 1;
                }
                if i > 0 && out[i - 1].trim() == format!("jmp {}", &t[..t.len() - 1]) {
                    out.remove(i - 1);
                }
                out.push(line.clone());
                continue;
            }
            if dead {
                continue;
            }
            // sign-extending twice in a row is the same as once
            if (t == "movsxd rax, eax" || t == "movsx rax, al" || t == "movsx rax, ax") && last_real(&out).is_some_and(|p| p.trim() == t) {
                continue;
            }
            // `mov [X], r` then `mov r, [X]`: the register already holds the value
            if let Some(rest) = t.strip_prefix("mov ") {
                if let Some((dst, src)) = rest.split_once(", ") {
                    // only a 64-bit register: `mov eax, eax` clears the upper half
                    if dst == src && dst.starts_with('r') && !dst.contains('[') {
                        continue;
                    }
                    if let Some(prev) = last_real(&out) {
                        if let Some(pr) = prev.trim().strip_prefix("mov ") {
                            if let Some((pdst, psrc)) = pr.split_once(", ") {
                                if pdst.starts_with("qword ptr [") && src == pdst && !psrc.contains('[') && psrc.starts_with('r') {
                                    if dst == psrc {
                                        continue;
                                    }
                                    // the value is still in the register that was stored
                                    if dst.starts_with('r') && !dst.contains('[') {
                                        out.push(format!("    mov {}, {}", dst, psrc));
                                        continue;
                                    }
                                }
                                // `mov r12, rax` then `mov rax, r12`: both already hold the value
                                let is_r = |x: &str| x.starts_with('r') && !x.contains(' ') && !x.contains('[');
                                if is_r(pdst) && is_r(psrc) && dst == psrc && src == pdst {
                                    continue;
                                }
                            }
                        }
                    }
                }
            }
            let unconditional = t.starts_with("jmp ") || t == "ret";
            out.push(line.clone());
            if unconditional {
                dead = true;
            }
        }
        // a final jump to the epilogue that follows
        if let Some(i) = out.iter().rposition(|l| !is_note(l)) {
            if out[i].trim() == format!("jmp {}", ret_label) {
                out.remove(i);
            }
        }
        cur = out;
        if cur.len() == before {
            return cur;
        }
    }
}

pub(super) fn norm_insn(t: &Ty) -> Option<&'static str> {
    Some(match t {
        Ty::Bool => "movzx eax, al",
        Ty::Int { bits: 8, signed: true } => "movsx rax, al",
        Ty::Int { bits: 8, signed: false } => "movzx eax, al",
        Ty::Int { bits: 16, signed: true } => "movsx rax, ax",
        Ty::Int { bits: 16, signed: false } => "movzx eax, ax",
        Ty::Int { bits: 32, signed: true } => "movsxd rax, eax",
        Ty::Int { bits: 32, signed: false } | Ty::Char => "mov eax, eax",
        _ => return None,
    })
}

pub(super) fn binop_static(op: &str) -> &'static str {
    match op {
        "+" => "+",
        "-" => "-",
        "*" => "*",
        "/" => "/",
        _ => "%",
    }
}

pub(super) fn is_register(n: &str) -> bool {
    const R: [&str; 32] = [
        "rax", "rbx", "rcx", "rdx", "rsi", "rdi", "r8", "r9", "r10", "r11", "r12", "r13", "r14", "r15", "eax", "ebx",
        "ecx", "edx", "esi", "edi", "r8d", "r9d", "r10d", "r11d", "ax", "bx", "cx", "dx", "al", "bl", "cl", "dl",
    ];
    R.contains(&n)
}

pub(super) fn reg64(r: &str) -> String {
    match r {
        "eax" | "ax" | "al" => "rax".into(),
        "ebx" | "bx" | "bl" => "rbx".into(),
        "ecx" | "cx" | "cl" => "rcx".into(),
        "edx" | "dx" | "dl" => "rdx".into(),
        "esi" => "rsi".into(),
        "edi" => "rdi".into(),
        r if r.ends_with('d') && r.starts_with('r') => r[..r.len() - 1].to_string(),
        r => r.to_string(),
    }
}

