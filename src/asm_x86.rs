//! A small x86 assembler that produces a flat binary.
//!
//! It exists so Zila Lang can build bare-metal images (boot sector, kernel)
//! without NASM or any other external tool. It understands the MASM-flavoured
//! text the Zila Lang compiler emits plus the extra directives a boot sector needs:
//!
//! - `org N`, `bits 16|32|64`, `times N db V`, `name:` / `name proc` / `name endp`
//! - `db dw dd dq` (numbers, expressions, symbols, strings), optionally named
//! - `$` and `$$` in expressions, `+ - * / << >> | &`, parentheses
//! - 16/32/64-bit instructions: the integer subset the compiler and a loader
//!   need (arithmetic, moves, branches, string ops, port I/O, descriptor
//!   tables, control registers, MSRs, far jumps)
//!
//! Operand dialect: `[...]` or `<size> ptr [...]` or `<size> ptr symbol` is a
//! memory operand; a bare symbol is an immediate address (`mov si, msg`).
//! In 64-bit mode a memory operand that is only a symbol is RIP-relative.
//! All branch displacements use the full rel16/rel32 form, so the size of
//! every instruction is known before symbols are resolved (two passes).

use std::collections::HashMap;

type R<T> = Result<T, String>;

#[derive(Debug)]
pub struct Assembled {
    pub bytes: Vec<u8>,
    pub symbols: HashMap<String, u64>,
    pub org: u64,
}

pub fn assemble(src: &str, default_org: u64, default_bits: u8) -> R<Assembled> {
    let (lines, numbers) = parse_lines(src)?;
    let mut syms: HashMap<String, u64> = HashMap::new();
    let mut result = None;
    for pass in 1..=2 {
        let mut a = Asm {
            pass,
            bits: default_bits,
            org: default_org,
            cur: default_org,
            out: Vec::new(),
            syms: &mut syms,
            new_syms: HashMap::new(),
        };
        for (n, l) in lines.iter().enumerate() {
            a.line(l).map_err(|e| format!("asm line {}: {} (in `{}`)", numbers[n], e, l.trim()))?;
        }
        let Asm { out, new_syms, org, .. } = a;
        if pass == 1 {
            syms = new_syms;
        } else {
            result = Some(Assembled { bytes: out, symbols: syms.clone(), org });
        }
    }
    Ok(result.unwrap())
}

// ---------------------------------------------------------------------------
// Text handling
// ---------------------------------------------------------------------------

/// The code lines of `src` (comments and blank lines dropped) and the line number of each in `src`.
fn parse_lines(src: &str) -> R<(Vec<String>, Vec<usize>)> {
    let mut out = Vec::new();
    let mut numbers = Vec::new();
    for (at, raw) in src.lines().enumerate() {
        // strip comments, but not inside quotes
        let mut line = String::new();
        let mut q: Option<char> = None;
        for c in raw.chars() {
            match q {
                Some(qc) => {
                    line.push(c);
                    if c == qc {
                        q = None;
                    }
                }
                None => {
                    if c == ';' {
                        break;
                    }
                    if c == '"' || c == '\'' {
                        q = Some(c);
                    }
                    line.push(c);
                }
            }
        }
        let t = line.trim();
        if !t.is_empty() {
            out.push(t.to_string());
            numbers.push(at + 1);
        }
    }
    Ok((out, numbers))
}

/// Splits on `sep` outside brackets and quotes.
fn split_top(s: &str, sep: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let (mut depth, mut q) = (0i32, None::<char>);
    for c in s.chars() {
        if let Some(qc) = q {
            cur.push(c);
            if c == qc {
                q = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => {
                q = Some(c);
                cur.push(c);
            }
            '[' | '(' => {
                depth += 1;
                cur.push(c);
            }
            ']' | ')' => {
                depth -= 1;
                cur.push(c);
            }
            c if c == sep && depth == 0 => {
                out.push(cur.trim().to_string());
                cur.clear();
            }
            c => cur.push(c),
        }
    }
    if !cur.trim().is_empty() || !out.is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

// ---------------------------------------------------------------------------
// Expressions
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum Expr {
    Num(i64),
    Sym(String),
    Cur,
    Start,
    Neg(Box<Expr>),
    Bin(char, Box<Expr>, Box<Expr>), // + - * / < (shl) > (shr) | &
}

impl Expr {
    fn has_sym(&self) -> bool {
        match self {
            Expr::Sym(_) => true,
            Expr::Num(_) | Expr::Cur | Expr::Start => false,
            Expr::Neg(e) => e.has_sym(),
            Expr::Bin(_, a, b) => a.has_sym() || b.has_sym(),
        }
    }
}

struct ExprParser<'a> {
    s: &'a [u8],
    i: usize,
}

fn parse_expr(s: &str) -> R<Expr> {
    let mut p = ExprParser { s: s.as_bytes(), i: 0 };
    let e = p.or()?;
    p.ws();
    if p.i != p.s.len() {
        return Err(format!("unexpected '{}' in expression `{}`", &s[p.i..], s));
    }
    Ok(e)
}

impl<'a> ExprParser<'a> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i] == b' ' {
            self.i += 1;
        }
    }
    fn peek(&mut self) -> Option<u8> {
        self.ws();
        self.s.get(self.i).copied()
    }
    fn or(&mut self) -> R<Expr> {
        let mut l = self.and()?;
        while self.peek() == Some(b'|') {
            self.i += 1;
            let r = self.and()?;
            l = Expr::Bin('|', Box::new(l), Box::new(r));
        }
        Ok(l)
    }
    fn and(&mut self) -> R<Expr> {
        let mut l = self.shift()?;
        while self.peek() == Some(b'&') {
            self.i += 1;
            let r = self.shift()?;
            l = Expr::Bin('&', Box::new(l), Box::new(r));
        }
        Ok(l)
    }
    fn shift(&mut self) -> R<Expr> {
        let mut l = self.add()?;
        loop {
            self.ws();
            if self.s[self.i..].starts_with(b"<<") {
                self.i += 2;
                let r = self.add()?;
                l = Expr::Bin('<', Box::new(l), Box::new(r));
            } else if self.s[self.i..].starts_with(b">>") {
                self.i += 2;
                let r = self.add()?;
                l = Expr::Bin('>', Box::new(l), Box::new(r));
            } else {
                return Ok(l);
            }
        }
    }
    fn add(&mut self) -> R<Expr> {
        let mut l = self.mul()?;
        loop {
            match self.peek() {
                Some(c @ (b'+' | b'-')) => {
                    self.i += 1;
                    let r = self.mul()?;
                    l = Expr::Bin(c as char, Box::new(l), Box::new(r));
                }
                _ => return Ok(l),
            }
        }
    }
    fn mul(&mut self) -> R<Expr> {
        let mut l = self.unary()?;
        loop {
            match self.peek() {
                Some(c @ (b'*' | b'/')) => {
                    self.i += 1;
                    let r = self.unary()?;
                    l = Expr::Bin(c as char, Box::new(l), Box::new(r));
                }
                _ => return Ok(l),
            }
        }
    }
    fn unary(&mut self) -> R<Expr> {
        match self.peek() {
            Some(b'-') => {
                self.i += 1;
                Ok(Expr::Neg(Box::new(self.unary()?)))
            }
            Some(b'+') => {
                self.i += 1;
                self.unary()
            }
            Some(b'(') => {
                self.i += 1;
                let e = self.or()?;
                if self.peek() != Some(b')') {
                    return Err("missing ')'".into());
                }
                self.i += 1;
                Ok(e)
            }
            Some(b'$') => {
                self.i += 1;
                if self.s.get(self.i) == Some(&b'$') {
                    self.i += 1;
                    Ok(Expr::Start)
                } else {
                    Ok(Expr::Cur)
                }
            }
            Some(b'\'') => {
                let c = *self.s.get(self.i + 1).ok_or("bad char literal")?;
                if self.s.get(self.i + 2) != Some(&b'\'') {
                    return Err("bad char literal".into());
                }
                self.i += 3;
                Ok(Expr::Num(c as i64))
            }
            Some(c) if c.is_ascii_digit() => {
                let st = self.i;
                while self.i < self.s.len() && (self.s[self.i].is_ascii_alphanumeric() || self.s[self.i] == b'_') {
                    self.i += 1;
                }
                let t = std::str::from_utf8(&self.s[st..self.i]).unwrap().replace('_', "");
                let v = if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
                    u64::from_str_radix(h, 16).map(|v| v as i64)
                } else if let Some(h) = t.strip_suffix('h').or_else(|| t.strip_suffix('H')) {
                    u64::from_str_radix(h, 16).map(|v| v as i64)
                } else if let Some(b) = t.strip_prefix("0b") {
                    u64::from_str_radix(b, 2).map(|v| v as i64)
                } else {
                    t.parse::<u64>().map(|v| v as i64)
                };
                v.map(Expr::Num).map_err(|_| format!("bad number '{}'", t))
            }
            Some(c) if c.is_ascii_alphabetic() || c == b'_' || c == b'.' => {
                let st = self.i;
                while self.i < self.s.len() && (self.s[self.i].is_ascii_alphanumeric() || self.s[self.i] == b'_' || self.s[self.i] == b'.') {
                    self.i += 1;
                }
                Ok(Expr::Sym(std::str::from_utf8(&self.s[st..self.i]).unwrap().to_string()))
            }
            other => Err(format!("unexpected {:?} in expression", other.map(|c| c as char))),
        }
    }
}

// ---------------------------------------------------------------------------
// Operands
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
struct Reg {
    code: u8,
    size: u8,
    /// spl/bpl/sil/dil and r8b..: need a REX prefix
    rex: bool,
    /// ah/ch/dh/bh
    high: bool,
}

fn reg_by_name(n: &str) -> Option<Reg> {
    let r = |code: u8, size: u8| Some(Reg { code, size, rex: false, high: false });
    const R64: [&str; 8] = ["rax", "rcx", "rdx", "rbx", "rsp", "rbp", "rsi", "rdi"];
    const R32: [&str; 8] = ["eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi"];
    const R16: [&str; 8] = ["ax", "cx", "dx", "bx", "sp", "bp", "si", "di"];
    const R8: [&str; 4] = ["al", "cl", "dl", "bl"];
    const H8: [&str; 4] = ["ah", "ch", "dh", "bh"];
    if let Some(i) = R64.iter().position(|x| *x == n) {
        return r(i as u8, 8);
    }
    if let Some(i) = R32.iter().position(|x| *x == n) {
        return r(i as u8, 4);
    }
    if let Some(i) = R16.iter().position(|x| *x == n) {
        return r(i as u8, 2);
    }
    if let Some(i) = R8.iter().position(|x| *x == n) {
        return r(i as u8, 1);
    }
    if let Some(i) = H8.iter().position(|x| *x == n) {
        return Some(Reg { code: 4 + i as u8, size: 1, rex: false, high: true });
    }
    if let Some(k) = n.strip_prefix("xmm").and_then(|d| d.parse::<u8>().ok()) {
        if k < 16 {
            return Some(Reg { code: k, size: 16, rex: false, high: false });
        }
    }
    match n {
        "spl" => return Some(Reg { code: 4, size: 1, rex: true, high: false }),
        "bpl" => return Some(Reg { code: 5, size: 1, rex: true, high: false }),
        "sil" => return Some(Reg { code: 6, size: 1, rex: true, high: false }),
        "dil" => return Some(Reg { code: 7, size: 1, rex: true, high: false }),
        _ => {}
    }
    if let Some(rest) = n.strip_prefix('r') {
        let (num, size) = if let Some(d) = rest.strip_suffix('d') {
            (d, 4)
        } else if let Some(w) = rest.strip_suffix('w') {
            (w, 2)
        } else if let Some(b) = rest.strip_suffix('b') {
            (b, 1)
        } else {
            (rest, 8)
        };
        if let Ok(k) = num.parse::<u8>() {
            if (8..=15).contains(&k) {
                return Some(Reg { code: k, size, rex: size == 1, high: false });
            }
        }
    }
    None
}

#[derive(Clone, Debug)]
struct Mem {
    /// 0 = unspecified
    size: u8,
    base: Option<Reg>,
    index: Option<(Reg, u8)>,
    disp: Expr,
    /// only a symbol (no registers) in 64-bit mode: RIP-relative
    sym_only: bool,
}

#[derive(Clone, Debug)]
enum Op {
    Reg(Reg),
    Imm(Expr),
    Mem(Mem),
    Far(Expr, Expr),
    Seg(u8),
    Cr(u8),
}

fn parse_op(s: &str) -> R<Op> {
    let s = s.trim();
    let lower = s.to_ascii_lowercase();
    if let Some(r) = reg_by_name(&lower) {
        return Ok(Op::Reg(r));
    }
    if let Some(i) = ["es", "cs", "ss", "ds", "fs", "gs"].iter().position(|x| *x == lower) {
        return Ok(Op::Seg(i as u8));
    }
    if let Some(n) = lower.strip_prefix("cr") {
        if let Ok(k) = n.parse::<u8>() {
            return Ok(Op::Cr(k));
        }
    }
    // size prefix
    let mut size = 0u8;
    let mut rest = s;
    for (name, sz) in [("byte", 1), ("word", 2), ("dword", 4), ("qword", 8)] {
        if let Some(r) = lower.strip_prefix(name) {
            let r = r.trim_start();
            if let Some(r2) = r.strip_prefix("ptr") {
                size = sz;
                rest = s[s.len() - r2.len()..].trim();
                break;
            }
        }
    }
    if rest.starts_with('[') && rest.ends_with(']') {
        return parse_mem(&rest[1..rest.len() - 1], size);
    }
    if size != 0 {
        // `qword ptr symbol`
        return Ok(Op::Mem(Mem { size, base: None, index: None, disp: parse_expr(rest)?, sym_only: true }));
    }
    // far pointer `sel:off`
    let parts = split_top(s, ':');
    if parts.len() == 2 {
        return Ok(Op::Far(parse_expr(&parts[0])?, parse_expr(&parts[1])?));
    }
    Ok(Op::Imm(parse_expr(s)?))
}

fn parse_mem(inner: &str, size: u8) -> R<Op> {
    // split into signed terms
    let mut terms: Vec<(bool, String)> = Vec::new();
    let (mut neg, mut cur, mut depth) = (false, String::new(), 0i32);
    for c in inner.chars() {
        match c {
            '(' => {
                depth += 1;
                cur.push(c);
            }
            ')' => {
                depth -= 1;
                cur.push(c);
            }
            '+' | '-' if depth == 0 => {
                if !cur.trim().is_empty() {
                    terms.push((neg, cur.trim().to_string()));
                    cur.clear();
                }
                neg = c == '-';
            }
            c => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        terms.push((neg, cur.trim().to_string()));
    }
    let (mut base, mut index) = (None, None);
    let mut disp: Option<Expr> = None;
    let push_disp = |neg: bool, e: Expr, disp: &mut Option<Expr>| {
        let e = if neg { Expr::Neg(Box::new(e)) } else { e };
        *disp = Some(match disp.take() {
            None => e,
            Some(d) => Expr::Bin('+', Box::new(d), Box::new(e)),
        });
    };
    for (neg, t) in terms {
        let lower = t.to_ascii_lowercase();
        if let Some(r) = reg_by_name(&lower) {
            if neg {
                return Err("cannot subtract a register".into());
            }
            if base.is_none() {
                base = Some(r);
            } else if index.is_none() {
                index = Some((r, 1));
            } else {
                return Err("too many registers in address".into());
            }
            continue;
        }
        if t.contains('*') {
            let parts = split_top(&t, '*');
            if parts.len() == 2 {
                let (a, b) = (parts[0].to_ascii_lowercase(), parts[1].to_ascii_lowercase());
                let (r, sc) = if let Some(r) = reg_by_name(&a) { (r, b) } else if let Some(r) = reg_by_name(&b) { (r, a) } else { (Reg { code: 255, size: 0, rex: false, high: false }, String::new()) };
                if r.code != 255 {
                    let sc: u8 = sc.parse().map_err(|_| "bad scale")?;
                    if index.is_some() {
                        return Err("two index registers".into());
                    }
                    index = Some((r, sc));
                    continue;
                }
            }
        }
        push_disp(neg, parse_expr(&t)?, &mut disp);
    }
    // a lone register with a scale given first: `[ecx*8 + x]` leaves base empty (ok)
    if base.is_some() && index.is_some() && index.unwrap().1 != 1 {
        // fine: base + index*scale
    }
    // `[reg*2]` style where the only register was parsed as base with scale handled above
    let sym_only = base.is_none() && index.is_none() && disp.as_ref().is_some_and(|d| d.has_sym());
    Ok(Op::Mem(Mem { size, base, index, disp: disp.unwrap_or(Expr::Num(0)), sym_only }))
}

// ---------------------------------------------------------------------------
// Encoder
// ---------------------------------------------------------------------------

struct Asm<'a> {
    pass: u8,
    bits: u8,
    org: u64,
    cur: u64,
    out: Vec<u8>,
    syms: &'a mut HashMap<String, u64>,
    new_syms: HashMap<String, u64>,
}

#[derive(Default)]
struct Ins {
    pre: Vec<u8>,
    rex: u8,
    rex_force: bool,
    op: Vec<u8>,
    modrm: Vec<u8>,
    imm: Vec<u8>,
    /// (offset inside `modrm`, target) for RIP-relative displacement
    rip: Option<(usize, Expr)>,
}

#[derive(Clone)]
enum Rm {
    Reg(Reg),
    Mem(Mem),
}

fn fits_i8(v: i64) -> bool {
    (-128..=127).contains(&v)
}

fn cond_code(c: &str) -> Option<u8> {
    Some(match c {
        "o" => 0,
        "no" => 1,
        "b" | "c" | "nae" => 2,
        "ae" | "nb" | "nc" => 3,
        "e" | "z" => 4,
        "ne" | "nz" => 5,
        "be" | "na" => 6,
        "a" | "nbe" => 7,
        "s" => 8,
        "ns" => 9,
        "p" | "pe" => 10,
        "np" | "po" => 11,
        "l" | "nge" => 12,
        "ge" | "nl" => 13,
        "le" | "ng" => 14,
        "g" | "nle" => 15,
        _ => return None,
    })
}

impl<'a> Asm<'a> {
    fn eval(&self, e: &Expr) -> R<i64> {
        Ok(match e {
            Expr::Num(n) => *n,
            Expr::Cur => self.cur as i64,
            Expr::Start => self.org as i64,
            Expr::Sym(s) => match self.syms.get(s) {
                Some(v) => *v as i64,
                None if self.pass == 1 => 0,
                None => return Err(format!("undefined symbol '{}'", s)),
            },
            Expr::Neg(a) => self.eval(a)?.wrapping_neg(),
            Expr::Bin(op, a, b) => {
                let (x, y) = (self.eval(a)?, self.eval(b)?);
                match op {
                    '+' => x.wrapping_add(y),
                    '-' => x.wrapping_sub(y),
                    '*' => x.wrapping_mul(y),
                    '/' => {
                        if y == 0 {
                            return Err("division by zero".into());
                        }
                        x / y
                    }
                    '<' => x << y,
                    '>' => ((x as u64) >> y) as i64,
                    '|' => x | y,
                    '&' => x & y,
                    _ => unreachable!(),
                }
            }
        })
    }

    fn emit(&mut self, b: &[u8]) {
        self.out.extend_from_slice(b);
        self.cur += b.len() as u64;
    }

    fn define(&mut self, name: &str) {
        self.new_syms.insert(name.to_string(), self.cur);
        if self.pass == 2 {
            if let Some(old) = self.syms.get(name) {
                debug_assert_eq!(*old, self.cur, "label {} moved between passes", name);
            }
        }
    }

    fn line(&mut self, line: &str) -> R<()> {
        let mut text = line.trim().to_string();
        // labels: `name:` possibly followed by an instruction
        while let Some(colon) = text.find(':') {
            let head = &text[..colon];
            if !head.is_empty() && head.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.') && !head.contains(' ') {
                // not a far pointer like `jmp 0x08:label` (that has a space before the colon-less head)
                let name = head.to_string();
                self.define(&name);
                text = text[colon + 1..].trim().to_string();
                if text.is_empty() {
                    return Ok(());
                }
            } else {
                break;
            }
        }
        let mut words = text.split_whitespace();
        let first = words.next().unwrap().to_ascii_lowercase();
        let rest_of = |skip: &str| text[text.to_ascii_lowercase().find(skip).unwrap() + skip.len()..].trim().to_string();

        // directives without names
        match first.as_str() {
            "option" | ".code" | ".data" | "end" | "extern" | "public" | ".model" | ".386" | ".x64" | "align" if first != "align" => return Ok(()),
            "org" => {
                let v = self.eval(&parse_expr(&rest_of("org"))?)? as u64;
                if self.out.is_empty() {
                    self.org = v;
                    self.cur = v;
                } else if v >= self.cur {
                    let pad = (v - self.cur) as usize;
                    self.emit(&vec![0u8; pad]);
                } else {
                    return Err("org moves backwards".into());
                }
                return Ok(());
            }
            "bits" | "use16" | "use32" | "use64" => {
                self.bits = match first.as_str() {
                    "use16" => 16,
                    "use32" => 32,
                    "use64" => 64,
                    _ => rest_of("bits").parse().map_err(|_| "bits needs 16, 32 or 64")?,
                };
                return Ok(());
            }
            "align" => {
                let n = self.eval(&parse_expr(&rest_of("align"))?)? as u64;
                while !self.cur.is_multiple_of(n) {
                    self.emit(&[0]);
                }
                return Ok(());
            }
            "times" => {
                let body = rest_of("times");
                let dir = ["db", "dw", "dd", "dq"]
                    .into_iter()
                    .min_by_key(|d| body.find(&format!(" {} ", d)).unwrap_or(usize::MAX))
                    .unwrap();
                let (count, inner) = body.split_once(&format!(" {} ", dir)).ok_or("times needs db, dw, dd or dq")?;
                let n = self.eval(&parse_expr(count)?)?;
                if n < 0 {
                    return Err("times count is negative (code too large for the sector?)".into());
                }
                for _ in 0..n {
                    self.data(dir, inner)?;
                }
                return Ok(());
            }
            "db" | "dw" | "dd" | "dq" => {
                let r = rest_of(&first);
                return self.data(&first, &r);
            }
            _ => {}
        }
        // `name proc` / `name endp` / `name db ...`
        if let Some(second) = words.clone().next() {
            let sl = second.to_ascii_lowercase();
            match sl.as_str() {
                "proc" => {
                    self.define(text.split_whitespace().next().unwrap());
                    return Ok(());
                }
                "endp" => return Ok(()),
                "db" | "dw" | "dd" | "dq" => {
                    let name = text.split_whitespace().next().unwrap().to_string();
                    self.define(&name);
                    let r = rest_of(&format!(" {}", sl));
                    return self.data(&sl, &r);
                }
                _ => {}
            }
        }
        self.instruction(&text)
    }

    fn data(&mut self, dir: &str, args: &str) -> R<()> {
        let width = match dir {
            "db" => 1,
            "dw" => 2,
            "dd" => 4,
            _ => 8,
        };
        for item in split_top(args, ',') {
            if item.is_empty() {
                continue;
            }
            if item.starts_with('"') && item.ends_with('"') && item.len() >= 2 {
                let b = item.as_bytes()[1..item.len() - 1].to_vec();
                self.emit(&b);
                continue;
            }
            let v = self.eval(&parse_expr(&item)?)?;
            let bytes = (v as u64).to_le_bytes();
            self.emit(&bytes[..width]);
        }
        Ok(())
    }

    // ---- instruction encoding ----

    fn instruction(&mut self, text: &str) -> R<()> {
        let mut t = text.trim();
        let mut prefixes: Vec<u8> = Vec::new();
        loop {
            let (w, r) = match t.split_once(char::is_whitespace) {
                Some((w, r)) => (w, r.trim()),
                None => (t, ""),
            };
            match w.to_ascii_lowercase().as_str() {
                "rep" | "repe" | "repz" => prefixes.push(0xF3),
                "repne" | "repnz" => prefixes.push(0xF2),
                "lock" => prefixes.push(0xF0),
                _ => break,
            }
            if r.is_empty() {
                break;
            }
            t = r;
        }
        let (mn, rest) = match t.split_once(char::is_whitespace) {
            Some((m, r)) => (m.to_ascii_lowercase(), r.trim()),
            None => (t.to_ascii_lowercase(), ""),
        };
        let mut ops = Vec::new();
        if !rest.is_empty() {
            for o in split_top(rest, ',') {
                ops.push(parse_op(&o)?);
            }
        }
        let mut ins = Ins { pre: prefixes, ..Default::default() };
        self.encode(&mn, &mut ops, &mut ins)?;
        self.finish(ins)
    }

    fn finish(&mut self, ins: Ins) -> R<()> {
        let mut b = ins.pre.clone();
        if ins.rex != 0 || ins.rex_force {
            if self.bits != 64 {
                return Err("64-bit registers or REX are not available in this mode".into());
            }
            b.push(0x40 | ins.rex);
        }
        b.extend(&ins.op);
        let modrm_at = b.len();
        b.extend(&ins.modrm);
        b.extend(&ins.imm);
        if let Some((off, target)) = &ins.rip {
            let end = self.cur + b.len() as u64;
            let v = self.eval(target)?.wrapping_sub(end as i64);
            let at = modrm_at + off;
            b[at..at + 4].copy_from_slice(&(v as i32).to_le_bytes());
        }
        self.emit(&b);
        Ok(())
    }

    /// Operand-size prefix / REX.W for an operation of `size` bytes.
    fn osize(&self, ins: &mut Ins, size: u8) -> R<()> {
        match (self.bits, size) {
            (_, 1) => {}
            (64, 8) => ins.rex |= 8,
            (64, 4) => {}
            (64, 2) => ins.pre.push(0x66),
            (32, 4) => {}
            (32, 2) => ins.pre.push(0x66),
            (16, 2) => {}
            (16, 4) => ins.pre.push(0x66),
            (b, s) => return Err(format!("{}-bit operands are not available in {}-bit mode", s * 8, b)),
        }
        Ok(())
    }

    fn imm_bytes(&self, ins: &mut Ins, v: i64, width: u8) {
        ins.imm.extend_from_slice(&v.to_le_bytes()[..width as usize]);
    }

    /// Builds ModRM (+SIB, displacement) for `reg_field` and `rm`.
    fn modrm(&self, ins: &mut Ins, reg_field: u8, rm: &Rm) -> R<()> {
        let rexr = reg_field >> 3;
        if rexr != 0 {
            ins.rex |= 4;
        }
        let rf = (reg_field & 7) << 3;
        match rm {
            Rm::Reg(r) => {
                if r.code >= 8 {
                    ins.rex |= 1;
                }
                if r.rex {
                    ins.rex_force = true;
                }
                ins.modrm.push(0xC0 | rf | (r.code & 7));
            }
            Rm::Mem(m) => self.mem_bytes(ins, rf, m)?,
        }
        Ok(())
    }

    fn mem_bytes(&self, ins: &mut Ins, rf: u8, m: &Mem) -> R<()> {
        let disp = self.eval(&m.disp)?;
        let dsym = m.disp.has_sym();
        if self.bits == 16 {
            let regs: Vec<u8> = [m.base, m.index.map(|x| x.0)].iter().flatten().map(|r| r.code).collect();
            if regs.is_empty() {
                ins.modrm.push(rf | 0x06);
                ins.modrm.extend_from_slice(&(disp as u16).to_le_bytes());
                return Ok(());
            }
            let has = |c: u8| regs.contains(&c);
            let (bx, bp, si, di) = (has(3), has(5), has(6), has(7));
            let rm = match (bx, bp, si, di) {
                (true, false, true, false) => 0,
                (true, false, false, true) => 1,
                (false, true, true, false) => 2,
                (false, true, false, true) => 3,
                (false, false, true, false) => 4,
                (false, false, false, true) => 5,
                (false, true, false, false) => 6,
                (true, false, false, false) => 7,
                _ => return Err("unsupported 16-bit address".into()),
            };
            if disp == 0 && !dsym && rm != 6 {
                ins.modrm.push(rf | rm);
            } else if fits_i8(disp) && !dsym {
                ins.modrm.push(0x40 | rf | rm);
                ins.modrm.push(disp as u8);
            } else {
                ins.modrm.push(0x80 | rf | rm);
                ins.modrm.extend_from_slice(&(disp as u16).to_le_bytes());
            }
            return Ok(());
        }
        let want = if self.bits == 64 { 8 } else { 4 };
        for r in [m.base, m.index.map(|x| x.0)].iter().flatten() {
            if r.size != want {
                return Err(format!("address registers must be {}-bit in {}-bit mode", want * 8, self.bits));
            }
        }
        // RIP-relative (64-bit): symbol with no registers
        if self.bits == 64 && m.base.is_none() && m.index.is_none() && m.sym_only {
            ins.modrm.push(rf | 0x05);
            ins.rip = Some((ins.modrm.len(), m.disp.clone()));
            ins.modrm.extend_from_slice(&[0; 4]);
            return Ok(());
        }
        let Some(base) = m.base else {
            // no base: absolute or index*scale + disp32
            if let Some((idx, sc)) = m.index {
                if idx.code == 4 {
                    return Err("rsp cannot be an index".into());
                }
                if idx.code >= 8 {
                    ins.rex |= 2;
                }
                ins.modrm.push(rf | 0x04);
                ins.modrm.push(scale_bits(sc)? << 6 | (idx.code & 7) << 3 | 0x05);
            } else if self.bits == 64 {
                ins.modrm.push(rf | 0x04);
                ins.modrm.push(0x25);
            } else {
                ins.modrm.push(rf | 0x05);
            }
            ins.modrm.extend_from_slice(&(disp as i32).to_le_bytes());
            return Ok(());
        };
        if base.code >= 8 {
            ins.rex |= 1;
        }
        let need_sib = m.index.is_some() || base.code & 7 == 4;
        let (md, dsize) = if disp == 0 && !dsym && base.code & 7 != 5 {
            (0u8, 0)
        } else if fits_i8(disp) && !dsym {
            (0x40, 1)
        } else {
            (0x80, 4)
        };
        if need_sib {
            let (ibits, sbits) = match m.index {
                Some((idx, sc)) => {
                    if idx.code == 4 {
                        return Err("rsp cannot be an index".into());
                    }
                    if idx.code >= 8 {
                        ins.rex |= 2;
                    }
                    (idx.code & 7, scale_bits(sc)?)
                }
                None => (4, 0),
            };
            ins.modrm.push(md | rf | 0x04);
            ins.modrm.push(sbits << 6 | ibits << 3 | (base.code & 7));
        } else {
            ins.modrm.push(md | rf | (base.code & 7));
        }
        match dsize {
            1 => ins.modrm.push(disp as u8),
            4 => ins.modrm.extend_from_slice(&(disp as i32).to_le_bytes()),
            _ => {}
        }
        Ok(())
    }

    fn mem_size(&self, m: &Mem) -> u8 {
        m.size
    }

    fn rel(&self, ins: &mut Ins, target: &Expr, extra_after: usize) -> R<()> {
        // displacement relative to the end of the instruction
        let width = if self.bits == 16 { 2 } else { 4 };
        let len = ins.pre.len() + ins.op.len() + width as usize + extra_after;
        let end = self.cur as i64 + len as i64;
        let v = self.eval(target)? - end;
        self.imm_bytes(ins, v, width);
        Ok(())
    }

    fn encode(&mut self, mn: &str, ops: &mut [Op], ins: &mut Ins) -> R<()> {
        // ---- no operands ----
        let simple: Option<&[u8]> = match mn {
            "nop" => Some(&[0x90]),
            "hlt" => Some(&[0xF4]),
            "cli" => Some(&[0xFA]),
            "syscall" => Some(&[0x0F, 0x05]),
            "sti" => Some(&[0xFB]),
            "cld" => Some(&[0xFC]),
            "std" => Some(&[0xFD]),
            "ret" | "retn" => Some(&[0xC3]),
            "leave" => Some(&[0xC9]),
            "int3" => Some(&[0xCC]),
            "pause" => Some(&[0xF3, 0x90]),
            "rdmsr" => Some(&[0x0F, 0x32]),
            "wrmsr" => Some(&[0x0F, 0x30]),
            "cpuid" => Some(&[0x0F, 0xA2]),
            "rdtsc" => Some(&[0x0F, 0x31]),
            "ud2" => Some(&[0x0F, 0x0B]),
            "movsb" => Some(&[0xA4]),
            "stosb" => Some(&[0xAA]),
            "lodsb" => Some(&[0xAC]),
            "cmpsb" => Some(&[0xA6]),
            "scasb" => Some(&[0xAE]),
            _ => None,
        };
        if let Some(b) = simple {
            if !ops.is_empty() {
                return Err(format!("'{}' takes no operands", mn));
            }
            ins.op.extend_from_slice(b);
            return Ok(());
        }
        match mn {
            "cqo" => {
                ins.rex |= 8;
                ins.op.push(0x99);
                return Ok(());
            }
            "cdq" | "cwd" | "cwde" | "cbw" => {
                let sz = if mn == "cdq" || mn == "cwde" { 4 } else { 2 };
                self.osize(ins, sz)?;
                ins.op.push(if mn == "cdq" || mn == "cwd" { 0x99 } else { 0x98 });
                return Ok(());
            }
            "iretq" => {
                ins.rex |= 8;
                ins.op.push(0xCF);
                return Ok(());
            }
            "iret" | "iretd" => {
                if mn == "iretd" || self.bits == 32 {
                    if self.bits == 16 {
                        ins.pre.push(0x66);
                    }
                } else if self.bits == 64 {
                    return Err("use iretq in 64-bit mode".into());
                }
                ins.op.push(0xCF);
                return Ok(());
            }
            "movsw" | "movsd" | "movsq" | "stosw" | "stosd" | "stosq" | "lodsw" | "lodsd" | "lodsq"
                if !(mn == "movsd" && !ops.is_empty()) =>
            {
                let sz = match &mn[4..] {
                    "w" => 2,
                    "d" => 4,
                    _ => 8,
                };
                self.osize(ins, sz)?;
                ins.op.push(match &mn[..4] {
                    "movs" => 0xA5,
                    "stos" => 0xAB,
                    _ => 0xAD,
                });
                return Ok(());
            }
            _ => {}
        }

        // ---- ALU group ----
        let alu = ["add", "or", "adc", "sbb", "and", "sub", "xor", "cmp"].iter().position(|x| *x == mn);
        if let Some(n) = alu {
            return self.enc_alu(n as u8, ops, ins);
        }
        match mn {
            "mov" => self.enc_mov(ops, ins),
            "lea" => {
                let (Op::Reg(r), op2) = (&ops[0], &ops[1]) else { return Err("lea needs a register".into()) };
                let m = match op2 {
                    Op::Mem(m) => m.clone(),
                    Op::Imm(e) => Mem { size: 0, base: None, index: None, disp: e.clone(), sym_only: true },
                    _ => return Err("lea needs a memory operand".into()),
                };
                self.osize(ins, r.size)?;
                ins.op.push(0x8D);
                self.modrm(ins, r.code, &Rm::Mem(m))
            }
            "test" => self.enc_test(ops, ins),
            "inc" | "dec" | "neg" | "not" | "mul" | "div" | "idiv" => {
                let (rm, size) = self.one_rm(ops)?;
                self.osize(ins, size)?;
                let (op8, op, ext) = match mn {
                    "inc" => (0xFE, 0xFF, 0),
                    "dec" => (0xFE, 0xFF, 1),
                    "not" => (0xF6, 0xF7, 2),
                    "neg" => (0xF6, 0xF7, 3),
                    "mul" => (0xF6, 0xF7, 4),
                    "div" => (0xF6, 0xF7, 6),
                    _ => (0xF6, 0xF7, 7),
                };
                ins.op.push(if size == 1 { op8 } else { op });
                self.modrm(ins, ext, &rm)
            }
            "imul" => self.enc_imul(ops, ins),
            "shl" | "sal" | "shr" | "sar" | "rol" | "ror" => {
                let ext = match mn {
                    "rol" => 0,
                    "ror" => 1,
                    "shl" | "sal" => 4,
                    "shr" => 5,
                    _ => 7,
                };
                if ops.len() != 2 {
                    return Err("shift needs two operands".into());
                }
                let (rm, size) = rm_of(&ops[0])?;
                self.osize(ins, size)?;
                match &ops[1] {
                    Op::Reg(r) if r.code == 1 && r.size == 1 => {
                        ins.op.push(if size == 1 { 0xD2 } else { 0xD3 });
                        self.modrm(ins, ext, &rm)
                    }
                    Op::Imm(e) => {
                        let v = self.eval(e)?;
                        if v == 1 && !e.has_sym() {
                            ins.op.push(if size == 1 { 0xD0 } else { 0xD1 });
                            self.modrm(ins, ext, &rm)
                        } else {
                            ins.op.push(if size == 1 { 0xC0 } else { 0xC1 });
                            self.modrm(ins, ext, &rm)?;
                            self.imm_bytes(ins, v, 1);
                            Ok(())
                        }
                    }
                    _ => Err("shift count must be cl or an immediate".into()),
                }
            }
            "push" | "pop" => self.enc_pushpop(mn == "push", ops, ins),
            "call" | "jmp" => self.enc_calljmp(mn == "call", ops, ins),
            "movzx" | "movsx" | "movsxd" => {
                let (Op::Reg(d), src) = (&ops[0], &ops[1]) else { return Err("needs a register destination".into()) };
                let (rm, ssize) = rm_of(src)?;
                if mn == "movsxd" {
                    self.osize(ins, d.size)?;
                    ins.op.push(0x63);
                    return self.modrm(ins, d.code, &rm);
                }
                self.osize(ins, d.size)?;
                let b = match (mn, ssize) {
                    ("movzx", 1) => 0xB6,
                    ("movzx", 2) => 0xB7,
                    ("movsx", 1) => 0xBE,
                    ("movsx", 2) => 0xBF,
                    _ => return Err("movzx/movsx source must be 8 or 16 bits".into()),
                };
                ins.op.extend_from_slice(&[0x0F, b]);
                self.modrm(ins, d.code, &rm)
            }
            "int" => {
                let Op::Imm(e) = &ops[0] else { return Err("int needs a number".into()) };
                ins.op.push(0xCD);
                let v = self.eval(e)?;
                self.imm_bytes(ins, v, 1);
                Ok(())
            }
            "in" | "out" => {
                let (acc, port) = if mn == "in" { (&ops[0], &ops[1]) } else { (&ops[1], &ops[0]) };
                let Op::Reg(a) = acc else { return Err("in/out need al/ax/eax".into()) };
                self.osize(ins, a.size)?;
                match port {
                    Op::Reg(r) if r.code == 2 && r.size == 2 => {
                        let base = if mn == "in" { 0xEC } else { 0xEE };
                        ins.op.push(if a.size == 1 { base } else { base + 1 });
                    }
                    Op::Imm(e) => {
                        let base = if mn == "in" { 0xE4 } else { 0xE6 };
                        ins.op.push(if a.size == 1 { base } else { base + 1 });
                        let v = self.eval(e)?;
                        self.imm_bytes(ins, v, 1);
                    }
                    _ => return Err("port must be dx or a number".into()),
                }
                Ok(())
            }
            "lgdt" | "lidt" => {
                let Op::Mem(m) = &ops[0] else { return Err("lgdt/lidt need memory".into()) };
                ins.op.extend_from_slice(&[0x0F, 0x01]);
                self.modrm(ins, if mn == "lgdt" { 2 } else { 3 }, &Rm::Mem(m.clone()))
            }
            "xchg" => {
                let (rm, reg) = match (&ops[0], &ops[1]) {
                    (Op::Reg(a), Op::Reg(b)) => (Rm::Reg(*a), *b),
                    (Op::Mem(m), Op::Reg(r)) | (Op::Reg(r), Op::Mem(m)) => (Rm::Mem(m.clone()), *r),
                    _ => return Err("xchg needs a register and a register or memory".into()),
                };
                self.osize(ins, reg.size)?;
                ins.op.push(if reg.size == 1 { 0x86 } else { 0x87 });
                self.modrm(ins, reg.code, &rm)
            }
            "xadd" | "cmpxchg" => {
                let Op::Reg(r) = &ops[1] else { return Err(format!("{} needs a register as its second operand", mn)) };
                let (rm, _) = rm_of(&ops[0])?;
                self.osize(ins, r.size)?;
                let base = if mn == "xadd" { 0xC0 } else { 0xB0 };
                ins.op.extend_from_slice(&[0x0F, if r.size == 1 { base } else { base + 1 }]);
                self.modrm(ins, r.code, &rm)
            }
            "retf" => {
                ins.op.push(0xCB);
                Ok(())
            }
            _ if sse_op(mn).is_some() => self.enc_sse(mn, ops, ins),
            _ => {
                if let Some(c) = mn.strip_prefix('j').and_then(cond_code) {
                    let Op::Imm(t) = &ops[0] else { return Err("jcc needs a label".into()) };
                    ins.op.extend_from_slice(&[0x0F, 0x80 + c]);
                    return self.rel(ins, &t.clone(), 0);
                }
                if let Some(c) = mn.strip_prefix("set").and_then(cond_code) {
                    let (rm, _) = self.one_rm(ops)?;
                    ins.op.extend_from_slice(&[0x0F, 0x90 + c]);
                    return self.modrm(ins, 0, &rm);
                }
                if let Some(c) = mn.strip_prefix("cmov").and_then(cond_code) {
                    let (Op::Reg(d), src) = (&ops[0], &ops[1]) else { return Err("cmov needs a register".into()) };
                    let (rm, _) = rm_of(src)?;
                    self.osize(ins, d.size)?;
                    ins.op.extend_from_slice(&[0x0F, 0x40 + c]);
                    return self.modrm(ins, d.code, &rm);
                }
                Err(format!("unknown instruction '{}'", mn))
            }
        }
    }

    /// Scalar SSE/SSE2 instructions (xmm registers are `Reg` with size 16).
    fn enc_sse(&mut self, mn: &str, ops: &mut [Op], ins: &mut Ins) -> R<()> {
        let (pre, op): (Option<u8>, &[u8]) = sse_op(mn).unwrap();
        let bad = || format!("bad operands for '{}'", mn);
        let is_x = |o: &Op| matches!(o, Op::Reg(r) if r.size == 16);
        let push_pre = |ins: &mut Ins| {
            if let Some(p) = pre {
                ins.pre.push(p);
            }
        };
        match mn {
            "movq" | "movd" => {
                let w = mn == "movq";
                if ops.len() != 2 {
                    return Err(bad());
                }
                match (&ops[0], &ops[1]) {
                    (Op::Reg(x), Op::Reg(g)) if x.size == 16 && g.size != 16 => {
                        if g.size != if w { 8 } else { 4 } {
                            return Err(bad());
                        }
                        ins.pre.push(0x66);
                        if w {
                            ins.rex |= 8;
                        }
                        ins.op.extend_from_slice(&[0x0F, 0x6E]);
                        self.modrm(ins, x.code, &Rm::Reg(*g))
                    }
                    (Op::Reg(g), Op::Reg(x)) if x.size == 16 && g.size != 16 => {
                        if g.size != if w { 8 } else { 4 } {
                            return Err(bad());
                        }
                        ins.pre.push(0x66);
                        if w {
                            ins.rex |= 8;
                        }
                        ins.op.extend_from_slice(&[0x0F, 0x7E]);
                        self.modrm(ins, x.code, &Rm::Reg(*g))
                    }
                    (Op::Reg(x), src) if x.size == 16 && w => {
                        let (rm, _) = rm_of(src)?;
                        ins.pre.push(0xF3);
                        ins.op.extend_from_slice(&[0x0F, 0x7E]);
                        self.modrm(ins, x.code, &rm)
                    }
                    (Op::Mem(m), Op::Reg(x)) if x.size == 16 && w => {
                        ins.pre.push(0x66);
                        ins.op.extend_from_slice(&[0x0F, 0xD6]);
                        self.modrm(ins, x.code, &Rm::Mem(m.clone()))
                    }
                    _ => Err(bad()),
                }
            }
            "movsd" | "movss" => {
                if ops.len() != 2 {
                    return Err(bad());
                }
                push_pre(ins);
                match (&ops[0], &ops[1]) {
                    (Op::Reg(x), src) if x.size == 16 => {
                        let (rm, _) = rm_of(src)?;
                        ins.op.extend_from_slice(&[0x0F, 0x10]);
                        self.modrm(ins, x.code, &rm)
                    }
                    (Op::Mem(m), Op::Reg(x)) if x.size == 16 => {
                        ins.op.extend_from_slice(&[0x0F, 0x11]);
                        self.modrm(ins, x.code, &Rm::Mem(m.clone()))
                    }
                    _ => Err(bad()),
                }
            }
            "cvtsi2sd" | "cvtsi2ss" => {
                let (Op::Reg(x), src) = (&ops[0], &ops[1]) else { return Err(bad()) };
                let (rm, size) = rm_of(src)?;
                if x.size != 16 || !(size == 4 || size == 8) {
                    return Err(bad());
                }
                push_pre(ins);
                if size == 8 {
                    ins.rex |= 8;
                }
                ins.op.extend_from_slice(op);
                self.modrm(ins, x.code, &rm)
            }
            "cvttsd2si" | "cvttss2si" => {
                let (Op::Reg(g), src) = (&ops[0], &ops[1]) else { return Err(bad()) };
                let (rm, _) = rm_of(src)?;
                if g.size != 4 && g.size != 8 {
                    return Err(bad());
                }
                push_pre(ins);
                if g.size == 8 {
                    ins.rex |= 8;
                }
                ins.op.extend_from_slice(op);
                self.modrm(ins, g.code, &rm)
            }
            "roundsd" | "roundss" => {
                let (Op::Reg(x), src, Op::Imm(i)) = (&ops[0], &ops[1], &ops[2]) else { return Err(bad()) };
                let (rm, _) = rm_of(src)?;
                push_pre(ins);
                ins.op.extend_from_slice(op);
                self.modrm(ins, x.code, &rm)?;
                let v = self.eval(i)?;
                self.imm_bytes(ins, v, 1);
                Ok(())
            }
            _ => {
                if ops.len() != 2 || !is_x(&ops[0]) {
                    return Err(bad());
                }
                let Op::Reg(x) = &ops[0] else { unreachable!() };
                let (rm, _) = rm_of(&ops[1])?;
                push_pre(ins);
                ins.op.extend_from_slice(op);
                self.modrm(ins, x.code, &rm)
            }
        }
    }

    fn one_rm(&self, ops: &[Op]) -> R<(Rm, u8)> {
        if ops.len() != 1 {
            return Err("expected one operand".into());
        }
        rm_of(&ops[0])
    }

    fn enc_alu(&mut self, n: u8, ops: &mut [Op], ins: &mut Ins) -> R<()> {
        if ops.len() != 2 {
            return Err("expected two operands".into());
        }
        let base = n * 8;
        match (&ops[0], &ops[1]) {
            (dst, Op::Imm(e)) => {
                let (rm, size) = rm_of(dst)?;
                if size == 0 {
                    return Err("operand size is unknown; use `qword ptr` etc.".into());
                }
                self.osize(ins, size)?;
                let v = self.eval(e)?;
                if size == 1 {
                    ins.op.push(0x80);
                    self.modrm(ins, n, &rm)?;
                    self.imm_bytes(ins, v, 1);
                } else if fits_i8(v) && !e.has_sym() {
                    ins.op.push(0x83);
                    self.modrm(ins, n, &rm)?;
                    self.imm_bytes(ins, v, 1);
                } else {
                    ins.op.push(0x81);
                    self.modrm(ins, n, &rm)?;
                    let w = if size == 2 { 2 } else { 4 };
                    self.imm_bytes(ins, v, w);
                }
                Ok(())
            }
            (Op::Reg(d), Op::Mem(m)) => {
                self.osize(ins, d.size)?;
                ins.op.push(base + if d.size == 1 { 2 } else { 3 });
                self.modrm(ins, d.code, &Rm::Mem(m.clone()))
            }
            (dst, Op::Reg(s)) => {
                let (rm, _) = rm_of(dst)?;
                self.osize(ins, s.size)?;
                ins.op.push(base + if s.size == 1 { 0 } else { 1 });
                self.modrm(ins, s.code, &rm)?;
                if s.rex {
                    ins.rex_force = true;
                }
                Ok(())
            }
            _ => Err("unsupported operands".into()),
        }
    }

    fn enc_mov(&mut self, ops: &mut [Op], ins: &mut Ins) -> R<()> {
        if ops.len() != 2 {
            return Err("mov needs two operands".into());
        }
        match (&ops[0], &ops[1]) {
            (Op::Seg(s), src) => {
                let (rm, _) = rm_of(src)?;
                ins.op.push(0x8E);
                self.modrm(ins, *s, &rm)
            }
            (dst, Op::Seg(s)) => {
                let (rm, _) = rm_of(dst)?;
                ins.op.push(0x8C);
                self.modrm(ins, *s, &rm)
            }
            (Op::Cr(c), Op::Reg(r)) => {
                ins.op.extend_from_slice(&[0x0F, 0x22]);
                self.modrm(ins, *c, &Rm::Reg(*r))
            }
            (Op::Reg(r), Op::Cr(c)) => {
                ins.op.extend_from_slice(&[0x0F, 0x20]);
                self.modrm(ins, *c, &Rm::Reg(*r))
            }
            (Op::Reg(d), Op::Imm(e)) => {
                let v = self.eval(e)?;
                let sym = e.has_sym();
                self.osize(ins, d.size)?;
                if d.rex {
                    ins.rex_force = true;
                }
                if d.code >= 8 {
                    ins.rex |= 1;
                }
                match d.size {
                    1 => {
                        ins.op.push(0xB0 + (d.code & 7));
                        self.imm_bytes(ins, v, 1);
                    }
                    2 => {
                        ins.op.push(0xB8 + (d.code & 7));
                        self.imm_bytes(ins, v, 2);
                    }
                    4 => {
                        ins.op.push(0xB8 + (d.code & 7));
                        self.imm_bytes(ins, v, 4);
                    }
                    _ => {
                        if (i32::MIN as i64..=i32::MAX as i64).contains(&v) || sym {
                            // REX.W C7 /0 imm32 (sign-extended)
                            ins.rex &= !1;
                            ins.op.push(0xC7);
                            self.modrm(ins, 0, &Rm::Reg(*d))?;
                            self.imm_bytes(ins, v, 4);
                        } else {
                            ins.op.push(0xB8 + (d.code & 7));
                            self.imm_bytes(ins, v, 8);
                        }
                    }
                }
                Ok(())
            }
            (Op::Mem(m), Op::Imm(e)) => {
                let size = self.mem_size(m);
                if size == 0 {
                    return Err("operand size is unknown; use `qword ptr` etc.".into());
                }
                self.osize(ins, size)?;
                let v = self.eval(e)?;
                ins.op.push(if size == 1 { 0xC6 } else { 0xC7 });
                self.modrm(ins, 0, &Rm::Mem(m.clone()))?;
                let w = match size {
                    1 => 1,
                    2 => 2,
                    _ => 4,
                };
                self.imm_bytes(ins, v, w);
                Ok(())
            }
            (Op::Reg(d), Op::Mem(m)) => {
                self.osize(ins, d.size)?;
                ins.op.push(if d.size == 1 { 0x8A } else { 0x8B });
                self.modrm(ins, d.code, &Rm::Mem(m.clone()))?;
                if d.rex {
                    ins.rex_force = true;
                }
                Ok(())
            }
            (dst, Op::Reg(s)) => {
                let (rm, _) = rm_of(dst)?;
                self.osize(ins, s.size)?;
                ins.op.push(if s.size == 1 { 0x88 } else { 0x89 });
                self.modrm(ins, s.code, &rm)?;
                if s.rex {
                    ins.rex_force = true;
                }
                Ok(())
            }
            _ => Err("unsupported mov operands".into()),
        }
    }

    fn enc_test(&mut self, ops: &mut [Op], ins: &mut Ins) -> R<()> {
        match (&ops[0], &ops[1]) {
            (dst, Op::Imm(e)) => {
                let (rm, size) = rm_of(dst)?;
                self.osize(ins, size)?;
                let v = self.eval(e)?;
                ins.op.push(if size == 1 { 0xF6 } else { 0xF7 });
                self.modrm(ins, 0, &rm)?;
                let w = match size {
                    1 => 1,
                    2 => 2,
                    _ => 4,
                };
                self.imm_bytes(ins, v, w);
                Ok(())
            }
            (dst, Op::Reg(s)) => {
                let (rm, _) = rm_of(dst)?;
                self.osize(ins, s.size)?;
                ins.op.push(if s.size == 1 { 0x84 } else { 0x85 });
                self.modrm(ins, s.code, &rm)?;
                if s.rex {
                    ins.rex_force = true;
                }
                Ok(())
            }
            _ => Err("unsupported test operands".into()),
        }
    }

    fn enc_imul(&mut self, ops: &mut [Op], ins: &mut Ins) -> R<()> {
        match ops.len() {
            1 => {
                let (rm, size) = self.one_rm(ops)?;
                self.osize(ins, size)?;
                ins.op.push(if size == 1 { 0xF6 } else { 0xF7 });
                self.modrm(ins, 5, &rm)
            }
            2 => {
                let (Op::Reg(d), src) = (&ops[0], &ops[1]) else { return Err("imul needs a register".into()) };
                let (rm, _) = rm_of(src)?;
                self.osize(ins, d.size)?;
                ins.op.extend_from_slice(&[0x0F, 0xAF]);
                self.modrm(ins, d.code, &rm)
            }
            _ => {
                let (Op::Reg(d), src, Op::Imm(e)) = (&ops[0], &ops[1], &ops[2]) else { return Err("bad imul".into()) };
                let (rm, _) = rm_of(src)?;
                self.osize(ins, d.size)?;
                let v = self.eval(e)?;
                if fits_i8(v) && !e.has_sym() {
                    ins.op.push(0x6B);
                    self.modrm(ins, d.code, &rm)?;
                    self.imm_bytes(ins, v, 1);
                } else {
                    ins.op.push(0x69);
                    self.modrm(ins, d.code, &rm)?;
                    self.imm_bytes(ins, v, if d.size == 2 { 2 } else { 4 });
                }
                Ok(())
            }
        }
    }

    fn enc_pushpop(&mut self, push: bool, ops: &mut [Op], ins: &mut Ins) -> R<()> {
        match &ops[0] {
            Op::Reg(r) => {
                // default operand size: 64 in long mode, 32 in protected, 16 in real
                let default = match self.bits {
                    64 => 8,
                    32 => 4,
                    _ => 2,
                };
                if r.size != default {
                    if self.bits == 64 && r.size == 2 || self.bits != 64 && r.size != default {
                        if r.size == 2 || r.size == 4 {
                            ins.pre.push(0x66);
                        }
                    } else {
                        return Err("bad push/pop register size".into());
                    }
                }
                if r.code >= 8 {
                    ins.rex |= 1;
                }
                ins.op.push((if push { 0x50 } else { 0x58 }) + (r.code & 7));
                Ok(())
            }
            Op::Imm(e) if push => {
                let v = self.eval(e)?;
                if fits_i8(v) && !e.has_sym() {
                    ins.op.push(0x6A);
                    self.imm_bytes(ins, v, 1);
                } else {
                    ins.op.push(0x68);
                    self.imm_bytes(ins, v, if self.bits == 16 { 2 } else { 4 });
                }
                Ok(())
            }
            Op::Mem(m) => {
                ins.op.push(if push { 0xFF } else { 0x8F });
                self.modrm(ins, if push { 6 } else { 0 }, &Rm::Mem(m.clone()))
            }
            _ => Err("unsupported push/pop operand".into()),
        }
    }

    fn enc_calljmp(&mut self, call: bool, ops: &mut [Op], ins: &mut Ins) -> R<()> {
        match &ops[0] {
            Op::Imm(t) => {
                ins.op.push(if call { 0xE8 } else { 0xE9 });
                self.rel(ins, &t.clone(), 0)
            }
            Op::Far(sel, off) if !call => {
                let (s, o) = (self.eval(sel)?, self.eval(off)?);
                ins.op.push(0xEA);
                let w = if self.bits == 16 { 2 } else { 4 };
                self.imm_bytes(ins, o, w);
                self.imm_bytes(ins, s, 2);
                Ok(())
            }
            Op::Reg(r) => {
                ins.op.push(0xFF);
                self.modrm(ins, if call { 2 } else { 4 }, &Rm::Reg(*r))
            }
            Op::Mem(m) => {
                ins.op.push(0xFF);
                self.modrm(ins, if call { 2 } else { 4 }, &Rm::Mem(m.clone()))
            }
            _ => Err("unsupported call/jmp operand".into()),
        }
    }
}

/// (mandatory prefix, opcode) for the scalar SSE instructions handled by `enc_sse`.
fn sse_op(mn: &str) -> Option<(Option<u8>, &'static [u8])> {
    const SD: Option<u8> = Some(0xF2);
    const SS: Option<u8> = Some(0xF3);
    const PD: Option<u8> = Some(0x66);
    Some(match mn {
        "movq" | "movd" => (None, &[]),
        "movsd" => (SD, &[]),
        "movss" => (SS, &[]),
        "addsd" => (SD, &[0x0F, 0x58]),
        "addss" => (SS, &[0x0F, 0x58]),
        "subsd" => (SD, &[0x0F, 0x5C]),
        "subss" => (SS, &[0x0F, 0x5C]),
        "mulsd" => (SD, &[0x0F, 0x59]),
        "mulss" => (SS, &[0x0F, 0x59]),
        "divsd" => (SD, &[0x0F, 0x5E]),
        "divss" => (SS, &[0x0F, 0x5E]),
        "sqrtsd" => (SD, &[0x0F, 0x51]),
        "sqrtss" => (SS, &[0x0F, 0x51]),
        "xorpd" => (PD, &[0x0F, 0x57]),
        "andpd" => (PD, &[0x0F, 0x54]),
        "ucomisd" => (PD, &[0x0F, 0x2E]),
        "ucomiss" => (None, &[0x0F, 0x2E]),
        "cvtsi2sd" => (SD, &[0x0F, 0x2A]),
        "cvtsi2ss" => (SS, &[0x0F, 0x2A]),
        "cvttsd2si" => (SD, &[0x0F, 0x2C]),
        "cvttss2si" => (SS, &[0x0F, 0x2C]),
        "cvtsd2ss" => (SD, &[0x0F, 0x5A]),
        "cvtss2sd" => (SS, &[0x0F, 0x5A]),
        "roundsd" => (PD, &[0x0F, 0x3A, 0x0B]),
        "roundss" => (PD, &[0x0F, 0x3A, 0x0A]),
        _ => return None,
    })
}

fn scale_bits(s: u8) -> R<u8> {
    Ok(match s {
        1 => 0,
        2 => 1,
        4 => 2,
        8 => 3,
        _ => return Err("scale must be 1, 2, 4 or 8".into()),
    })
}

fn rm_of(op: &Op) -> R<(Rm, u8)> {
    match op {
        Op::Reg(r) => Ok((Rm::Reg(*r), r.size)),
        Op::Mem(m) => Ok((Rm::Mem(m.clone()), m.size)),
        _ => Err("expected a register or memory operand".into()),
    }
}

// ---------------------------------------------------------------------------
// Tests: encodings checked against well-known byte sequences
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn enc(src: &str, bits: u8) -> Vec<u8> {
        assemble(src, 0x1000, bits).unwrap().bytes
    }

    #[test]
    fn syscall_is_two_bytes() {
        assert_eq!(enc("syscall", 64), [0x0F, 0x05]);
    }

    #[test]
    fn atomic_instructions() {
        assert_eq!(enc("lock xadd qword ptr [rcx], rax", 64), [0xF0, 0x48, 0x0F, 0xC1, 0x01]);
        assert_eq!(enc("lock cmpxchg qword ptr [rcx], rdx", 64), [0xF0, 0x48, 0x0F, 0xB1, 0x11]);
        assert_eq!(enc("xchg qword ptr [rcx], rax", 64), [0x48, 0x87, 0x01]);
        assert_eq!(enc("xchg rax, rcx", 64), [0x48, 0x87, 0xC8]);
    }

    #[test]
    fn mov_and_stack_64() {
        assert_eq!(enc("push rbp", 64), [0x55]);
        assert_eq!(enc("mov rbp, rsp", 64), [0x48, 0x89, 0xE5]);
        assert_eq!(enc("sub rsp, 32", 64), [0x48, 0x83, 0xEC, 0x20]);
        assert_eq!(enc("mov qword ptr [rbp - 8], rcx", 64), [0x48, 0x89, 0x4D, 0xF8]);
        assert_eq!(enc("mov rax, qword ptr [rbp - 16]", 64), [0x48, 0x8B, 0x45, 0xF0]);
        assert_eq!(enc("xor eax, eax", 64), [0x31, 0xC0]);
        assert_eq!(enc("mov eax, 1", 64), [0xB8, 1, 0, 0, 0]);
        assert_eq!(enc("mov rax, 5", 64), [0x48, 0xC7, 0xC0, 5, 0, 0, 0]);
        assert_eq!(enc("movzx eax, al", 64), [0x0F, 0xB6, 0xC0]);
        assert_eq!(enc("cmp rax, rcx", 64), [0x48, 0x39, 0xC8]);
        assert_eq!(enc("mov r8, r9", 64), [0x4D, 0x89, 0xC8]);
        assert_eq!(enc("pop rbp", 64), [0x5D]);
        assert_eq!(enc("push r12", 64), [0x41, 0x54]);
    }

    #[test]
    fn sse_moves() {
        assert_eq!(enc("movq xmm0, rax", 64), [0x66, 0x48, 0x0F, 0x6E, 0xC0]);
        assert_eq!(enc("movq rax, xmm0", 64), [0x66, 0x48, 0x0F, 0x7E, 0xC0]);
        assert_eq!(enc("movq xmm1, rcx", 64), [0x66, 0x48, 0x0F, 0x6E, 0xC9]);
        assert_eq!(enc("movq xmm9, r8", 64), [0x66, 0x4D, 0x0F, 0x6E, 0xC8]);
        assert_eq!(enc("movd xmm0, eax", 64), [0x66, 0x0F, 0x6E, 0xC0]);
        assert_eq!(enc("movd eax, xmm0", 64), [0x66, 0x0F, 0x7E, 0xC0]);
        assert_eq!(enc("movsd xmm0, xmm1", 64), [0xF2, 0x0F, 0x10, 0xC1]);
        assert_eq!(enc("movsd xmm0, qword ptr [rbp - 8]", 64), [0xF2, 0x0F, 0x10, 0x45, 0xF8]);
        assert_eq!(enc("movsd qword ptr [rbp - 8], xmm0", 64), [0xF2, 0x0F, 0x11, 0x45, 0xF8]);
        assert_eq!(enc("movss xmm2, dword ptr [rcx]", 64), [0xF3, 0x0F, 0x10, 0x11]);
        assert_eq!(enc("movss dword ptr [rcx], xmm2", 64), [0xF3, 0x0F, 0x11, 0x11]);
        assert_eq!(enc("movq xmm0, qword ptr [rcx]", 64), [0xF3, 0x0F, 0x7E, 0x01]);
        assert_eq!(enc("movq qword ptr [rcx], xmm0", 64), [0x66, 0x0F, 0xD6, 0x01]);
        // string instruction form still works
        assert_eq!(enc("movsd", 64), [0xA5]);
    }

    #[test]
    fn sse_arithmetic() {
        assert_eq!(enc("addsd xmm0, xmm1", 64), [0xF2, 0x0F, 0x58, 0xC1]);
        assert_eq!(enc("subsd xmm0, xmm1", 64), [0xF2, 0x0F, 0x5C, 0xC1]);
        assert_eq!(enc("mulsd xmm0, xmm1", 64), [0xF2, 0x0F, 0x59, 0xC1]);
        assert_eq!(enc("divsd xmm0, xmm1", 64), [0xF2, 0x0F, 0x5E, 0xC1]);
        assert_eq!(enc("addss xmm0, xmm1", 64), [0xF3, 0x0F, 0x58, 0xC1]);
        assert_eq!(enc("divss xmm2, xmm3", 64), [0xF3, 0x0F, 0x5E, 0xD3]);
        assert_eq!(enc("sqrtsd xmm0, xmm0", 64), [0xF2, 0x0F, 0x51, 0xC0]);
        assert_eq!(enc("xorpd xmm0, xmm0", 64), [0x66, 0x0F, 0x57, 0xC0]);
        assert_eq!(enc("andpd xmm0, xmm1", 64), [0x66, 0x0F, 0x54, 0xC1]);
        assert_eq!(enc("addsd xmm8, xmm1", 64), [0xF2, 0x44, 0x0F, 0x58, 0xC1]);
    }

    #[test]
    fn sse_compare_convert_round() {
        assert_eq!(enc("ucomisd xmm0, xmm1", 64), [0x66, 0x0F, 0x2E, 0xC1]);
        assert_eq!(enc("ucomiss xmm0, xmm1", 64), [0x0F, 0x2E, 0xC1]);
        assert_eq!(enc("cvtsi2sd xmm0, rax", 64), [0xF2, 0x48, 0x0F, 0x2A, 0xC0]);
        assert_eq!(enc("cvtsi2sd xmm0, eax", 64), [0xF2, 0x0F, 0x2A, 0xC0]);
        assert_eq!(enc("cvtsi2ss xmm0, rax", 64), [0xF3, 0x48, 0x0F, 0x2A, 0xC0]);
        assert_eq!(enc("cvttsd2si rax, xmm0", 64), [0xF2, 0x48, 0x0F, 0x2C, 0xC0]);
        assert_eq!(enc("cvttss2si eax, xmm0", 64), [0xF3, 0x0F, 0x2C, 0xC0]);
        assert_eq!(enc("cvtsd2ss xmm0, xmm0", 64), [0xF2, 0x0F, 0x5A, 0xC0]);
        assert_eq!(enc("cvtss2sd xmm0, xmm0", 64), [0xF3, 0x0F, 0x5A, 0xC0]);
        assert_eq!(enc("roundsd xmm0, xmm0, 9", 64), [0x66, 0x0F, 0x3A, 0x0B, 0xC0, 0x09]);
        assert_eq!(enc("roundss xmm1, xmm2, 10", 64), [0x66, 0x0F, 0x3A, 0x0A, 0xCA, 0x0A]);
        assert!(assemble("addsd rax, xmm0", 0x1000, 64).is_err());
    }

    #[test]
    fn memory_forms_64() {
        assert_eq!(enc("mov rax, qword ptr [rsp + 32]", 64), [0x48, 0x8B, 0x44, 0x24, 0x20]);
        assert_eq!(enc("mov rax, qword ptr [rcx + rdx * 8]", 64), [0x48, 0x8B, 0x04, 0xD1]);
        assert_eq!(enc("lea rcx, [rax * 8]", 64), [0x48, 0x8D, 0x0C, 0xC5, 0, 0, 0, 0]);
        assert_eq!(enc("mov rax, qword ptr [r8 + r10 * 8]", 64), [0x4B, 0x8B, 0x04, 0xD0]);
        assert_eq!(enc("inc qword ptr [rcx - 16]", 64), [0x48, 0xFF, 0x41, 0xF0]);
        assert_eq!(enc("mov byte ptr [rcx + rdx + 8], al", 64), [0x88, 0x44, 0x11, 0x08]);
        assert_eq!(enc("mov rax, qword ptr [rbp]", 64), [0x48, 0x8B, 0x45, 0x00]);
    }

    #[test]
    fn rip_relative_and_branches() {
        // lea rax, [rip + label]; label right after the instruction
        let a = assemble("lea rax, here\nhere:\nnop", 0x1000, 64).unwrap();
        assert_eq!(a.bytes, [0x48, 0x8D, 0x05, 0, 0, 0, 0, 0x90]);
        let a = assemble("start:\n jmp start", 0x1000, 64).unwrap();
        assert_eq!(a.bytes, [0xE9, 0xFB, 0xFF, 0xFF, 0xFF]);
        let a = assemble("call f\nret\nf proc\nret\nf endp", 0x1000, 64).unwrap();
        assert_eq!(a.bytes, [0xE8, 1, 0, 0, 0, 0xC3, 0xC3]);
        assert_eq!(enc("je x\nx:", 64), [0x0F, 0x84, 0, 0, 0, 0]);
        assert_eq!(enc("call qword ptr [rax]", 64), [0xFF, 0x10]);
        assert_eq!(enc("call rdx", 64), [0xFF, 0xD2]);
    }

    #[test]
    fn arithmetic_and_strings() {
        assert_eq!(enc("imul rax, rcx", 64), [0x48, 0x0F, 0xAF, 0xC1]);
        assert_eq!(enc("idiv rcx", 64), [0x48, 0xF7, 0xF9]);
        assert_eq!(enc("cqo", 64), [0x48, 0x99]);
        assert_eq!(enc("shl rax, cl", 64), [0x48, 0xD3, 0xE0]);
        assert_eq!(enc("sar rax, cl", 64), [0x48, 0xD3, 0xF8]);
        assert_eq!(enc("rep movsb", 64), [0xF3, 0xA4]);
        assert_eq!(enc("repe cmpsb", 64), [0xF3, 0xA6]);
        assert_eq!(enc("sete al", 64), [0x0F, 0x94, 0xC0]);
        assert_eq!(enc("cmovnz rax, rcx", 64), [0x48, 0x0F, 0x45, 0xC1]);
        assert_eq!(enc("movsxd rax, eax", 64), [0x48, 0x63, 0xC0]);
        assert_eq!(enc("test qword ptr [rsp], rsp", 64), [0x48, 0x85, 0x24, 0x24]);
        assert_eq!(enc("and rax, -16", 64), [0x48, 0x83, 0xE0, 0xF0]);
        assert_eq!(enc("mov rax, 4611686018427387904", 64), [0x48, 0xB8, 0, 0, 0, 0, 0, 0, 0, 0x40]);
    }

    #[test]
    fn real_and_protected_mode() {
        assert_eq!(enc("mov ax, 0x1234", 16), [0xB8, 0x34, 0x12]);
        assert_eq!(enc("mov ds, ax", 16), [0x8E, 0xD8]);
        assert_eq!(enc("mov ah, 0x0E", 16), [0xB4, 0x0E]);
        assert_eq!(enc("int 0x10", 16), [0xCD, 0x10]);
        assert_eq!(enc("in al, 0x92", 16), [0xE4, 0x92]);
        assert_eq!(enc("out dx, al", 16), [0xEE]);
        assert_eq!(enc("mov eax, cr0", 16), [0x0F, 0x20, 0xC0]);
        assert_eq!(enc("mov cr0, eax", 16), [0x0F, 0x22, 0xC0]);
        assert_eq!(enc("or eax, 1", 16), [0x66, 0x83, 0xC8, 0x01]);
        assert_eq!(enc("rep stosd", 16), [0xF3, 0x66, 0xAB]);
        assert_eq!(enc("mov eax, 0x1000", 32), [0xB8, 0, 0x10, 0, 0]);
        assert_eq!(enc("mov dword ptr [0x1000], 0x2003", 32), [0xC7, 0x05, 0, 0x10, 0, 0, 3, 0x20, 0, 0]);
        assert_eq!(enc("mov dword ptr [0x3000 + ecx*8], eax", 32), [0x89, 0x04, 0xCD, 0, 0x30, 0, 0]);
        // far jumps
        assert_eq!(enc("jmp 0x08:0x1234", 16), [0xEA, 0x34, 0x12, 0x08, 0x00]);
        assert_eq!(enc("jmp 0x18:0x12345678", 32), [0xEA, 0x78, 0x56, 0x34, 0x12, 0x18, 0x00]);
        // absolute 16-bit memory
        assert_eq!(enc("mov [0x7c00], dl", 16), [0x88, 0x16, 0x00, 0x7C]);
        assert_eq!(enc("lgdt [0x7c10]", 16), [0x0F, 0x01, 0x16, 0x10, 0x7C]);
        assert_eq!(enc("lodsb", 16), [0xAC]);
        assert_eq!(enc("rdtsc", 64), [0x0F, 0x31]);
        assert_eq!(enc("rdmsr", 32), [0x0F, 0x32]);
    }

    #[test]
    fn data_and_directives() {
        let a = assemble("org 0x7C00\nbits 16\nstart:\n db \"Hi\", 0\n dw 0x55AA\n times 8-($-$$) db 0\nend_:\n dq start", 0, 64).unwrap();
        assert_eq!(&a.bytes[..3], b"Hi\0");
        assert_eq!(&a.bytes[3..5], [0xAA, 0x55]);
        assert_eq!(a.bytes.len(), 16);
        assert_eq!(&a.bytes[8..16], 0x7C00u64.to_le_bytes());
        assert_eq!(a.symbols["end_"], 0x7C08);
    }

    #[test]
    fn errors_are_reported() {
        assert!(assemble("frobnicate rax", 0, 64).is_err());
        assert!(assemble("jmp nowhere", 0, 64).unwrap_err().contains("undefined symbol"));
    }
}
