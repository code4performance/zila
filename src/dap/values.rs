//! Values while the program is stopped: scopes, variables and evaluation.
//!
//! A value is read from the stopped process with the type descriptor of the debug-information
//! file (`codegen_x64/debuginfo.rs`) and the layouts the code generator uses: every slot, list
//! element, map entry and enum payload is one 64-bit word; a record's fields have their natural
//! sizes at the offsets of the descriptor; a text is `[length][bytes]`, a list `[length][capacity]
//! [data][flags]`, a map a header with an array of 32-byte entries `[state][hash][key][value]`, an
//! enum a bare tag (no payload anywhere) or `[tag][payload...]`, a `T or error` a box
//! `[error][value]`. What cannot be read is shown as unavailable, never as a guess.

use super::Session;
use crate::lsp::Json;

/// What a reference handed to the client stands for.
#[derive(Clone)]
enum Node {
    /// The locals of a frame.
    Frame(usize),
    /// A value: its type descriptor and its word (`None` when it could not be read).
    Value { ty: Json, raw: Option<u64> },
}

/// The references the client was given since the program last stopped.
#[derive(Default)]
pub(super) struct Handles {
    nodes: Vec<Node>,
}

impl Handles {
    pub(super) fn clear(&mut self) {
        self.nodes.clear();
    }

    fn add(&mut self, n: Node) -> i64 {
        self.nodes.push(n);
        self.nodes.len() as i64
    }

    /// The reference of the locals of a frame.
    pub(super) fn locals_scope(&mut self, frame: usize) -> i64 {
        self.add(Node::Frame(frame))
    }
}

fn obj(v: Vec<(&str, Json)>) -> Json {
    Json::Obj(v.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn num_of(j: &Json) -> f64 {
    match j {
        Json::Num(n) => *n,
        _ => 0.0,
    }
}

const UNAVAILABLE: &str = "unavailable";
/// The most children listed for one value.
const LIMIT: usize = 100;

fn read(s: &Session, addr: u64, len: usize) -> Option<Vec<u8>> {
    let d = s.dbg.as_ref()?;
    let b = d.read_memory(addr, len).ok()?;
    (b.len() == len).then_some(b)
}

fn read_u64(s: &Session, addr: u64) -> Option<u64> {
    Some(u64::from_le_bytes(read(s, addr, 8)?.try_into().ok()?))
}

/// The descriptor a reference points to, when it is one.
fn resolve(s: &Session, ty: &Json) -> Json {
    if let Some(r) = ty.get("ref").str() {
        if let Some(i) = &s.info {
            let d = i.types.get(r);
            if d != &Json::Null {
                return d.clone();
            }
        }
    }
    ty.clone()
}

/// How many bytes a value of this type takes in a record field.
fn field_size(ty: &Json) -> usize {
    match ty.get("kind").str() {
        Some("int") | Some("float") => (num_of(ty.get("bits")) as usize / 8).clamp(1, 8),
        Some("bool") => 1,
        Some("char") => 4,
        _ => 8,
    }
}

/// The name of a type as a person writes it.
fn type_name(s: &Session, ty: &Json) -> String {
    let d = resolve(s, ty);
    match d.get("kind").str().unwrap_or("") {
        "int" => {
            let (bits, signed) = (num_of(d.get("bits")) as u32, d.get("signed") == &Json::Bool(true));
            match (bits, signed) {
                (32, true) => "int".into(),
                (64, true) => "long".into(),
                (b, true) => format!("i{}", b),
                (b, false) => format!("u{}", b),
            }
        }
        "float" => if num_of(d.get("bits")) as u32 == 32 { "f32" } else { "float" }.into(),
        "record" | "enum" => d.get("name").str().unwrap_or("?").to_string(),
        "list" => format!("[]{}", type_name(s, d.get("element"))),
        "map" => format!("Map<{}, {}>", type_name(s, d.get("key")), type_name(s, d.get("value"))),
        "pointer" => format!("*{}", type_name(s, d.get("target"))),
        "rc" => format!("Rc<{}>", type_name(s, d.get("target"))),
        "weak" => format!("Weak<{}>", type_name(s, d.get("target"))),
        "result" => {
            let v = d.get("value");
            if v.get("kind").str() == Some("void") {
                "error".into()
            } else {
                format!("{} or error", type_name(s, v))
            }
        }
        "channel" => format!("Channel<{}>", type_name(s, d.get("element"))),
        "future" => format!("Future<{}>", type_name(s, d.get("value"))),
        "function" => "func".into(),
        "errorkind" => "ErrorKind".into(),
        other => other.to_string(),
    }
}

fn escape(t: &str) -> String {
    let mut out = String::from("\"");
    for c in t.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A text object: `[length][bytes]`.
fn read_text(s: &Session, ptr: u64) -> Option<String> {
    if ptr == 0 {
        return None;
    }
    let len = read_u64(s, ptr)? as usize;
    if len > 1 << 26 {
        return None;
    }
    let shown = len.min(2000);
    let bytes = if shown == 0 { Vec::new() } else { read(s, ptr + 8, shown)? };
    let mut t = String::from_utf8_lossy(&bytes).to_string();
    if len > shown {
        t.push('…');
    }
    Some(t)
}

/// An integer of `bits` from the low bits of a word.
fn int_value(raw: u64, bits: u32, signed: bool) -> String {
    let bits = bits.clamp(8, 64);
    let mask = if bits == 64 { u64::MAX } else { (1u64 << bits) - 1 };
    let v = raw & mask;
    if signed {
        let shift = 64 - bits;
        (((v << shift) as i64) >> shift).to_string()
    } else {
        v.to_string()
    }
}

/// How a value shows and whether it has parts to expand.
struct Shown {
    value: String,
    expandable: bool,
}

fn shown(value: impl Into<String>, expandable: bool) -> Shown {
    Shown { value: value.into(), expandable }
}

fn unavailable() -> Shown {
    shown(UNAVAILABLE, false)
}

/// The scalar a descriptor stands for, as text, when it is one.
fn scalar(s: &Session, d: &Json, raw: u64) -> Option<String> {
    Some(match d.get("kind").str()? {
        "int" => int_value(raw, num_of(d.get("bits")) as u32, d.get("signed") == &Json::Bool(true)),
        "float" => {
            if num_of(d.get("bits")) as u32 == 32 {
                crate::float_text::float_text_f32(f32::from_bits(raw as u32))
            } else {
                crate::float_text::float_text(f64::from_bits(raw))
            }
        }
        "bool" => (raw & 0xff != 0).to_string(),
        "char" => match char::from_u32(raw as u32) {
            Some('\'') => "'\\''".to_string(),
            Some('\n') => "'\\n'".to_string(),
            Some(c) => format!("'{}'", c),
            None => format!("{:#x}", raw as u32),
        },
        "text" => match read_text(s, raw) {
            Some(t) => escape(&t),
            None if raw == 0 => "nil".to_string(),
            None => return None,
        },
        _ => return None,
    })
}

fn render(s: &Session, ty: &Json, raw: Option<u64>, depth: usize) -> Shown {
    let Some(raw) = raw else { return unavailable() };
    let d = resolve(s, ty);
    if let Some(t) = scalar(s, &d, raw) {
        return shown(t, false);
    }
    let kind = d.get("kind").str().unwrap_or("");
    match kind {
        "text" => unavailable(),
        "record" | "rc" => {
            let rec = if kind == "rc" { resolve(s, d.get("target")) } else { d.clone() };
            if raw == 0 {
                return shown("nil", false);
            }
            let name = rec.get("name").str().unwrap_or("?").to_string();
            if depth >= 2 {
                return shown(format!("{}{{…}}", name), true);
            }
            let Json::Arr(fields) = rec.get("fields") else { return shown(format!("{}{{}}", name), false) };
            let parts: Vec<String> = fields.iter().take(6).map(|f| format!("{}: {}", f.get("name").str().unwrap_or("?"), render(s, f.get("type"), field_word(s, raw, f), depth + 1).value)).collect();
            let more = if fields.len() > 6 { ", …" } else { "" };
            shown(format!("{}{{{}{}}}", name, parts.join(", "), more), !fields.is_empty())
        }
        "enum" => {
            let payload = d.get("payload") == &Json::Bool(true);
            let Json::Arr(variants) = d.get("variants") else { return unavailable() };
            let (tag, base) = if payload {
                match read_u64(s, raw) {
                    Some(t) => (t, Some(raw)),
                    None => return unavailable(),
                }
            } else {
                (raw, None)
            };
            let Some(v) = variants.get(tag as usize) else { return unavailable() };
            let name = v.get("name").str().unwrap_or("?").to_string();
            let Json::Arr(tys) = v.get("fields") else { return shown(name, false) };
            if tys.is_empty() || base.is_none() {
                return shown(name, false);
            }
            let parts: Vec<String> = tys.iter().enumerate().take(4).map(|(i, t)| render(s, t, read_u64(s, base.unwrap() + 8 + 8 * i as u64), depth + 1).value).collect();
            shown(format!("{}({})", name, parts.join(", ")), true)
        }
        "list" => {
            if raw == 0 {
                return shown("nil", false);
            }
            let (Some(len), Some(data)) = (read_u64(s, raw), read_u64(s, raw + 16)) else { return unavailable() };
            if len > 1 << 26 {
                return unavailable();
            }
            let el = d.get("element");
            if len <= 6 && depth < 2 {
                let parts: Vec<String> = (0..len).map(|i| render(s, el, read_u64(s, data + 8 * i), depth + 1).value).collect();
                return shown(format!("[{}]", parts.join(", ")), len > 0);
            }
            shown(format!("[{} items]", len), len > 0)
        }
        "map" => {
            if raw == 0 {
                return shown("nil", false);
            }
            let Some(live) = read_u64(s, raw + 48) else { return unavailable() };
            shown(format!("{{{} entries}}", live), live > 0)
        }
        "pointer" => {
            let target = resolve(s, d.get("target"));
            let at = format!("{:#x}", raw);
            if raw == 0 {
                return shown("nil", false);
            }
            let inner = target.get("kind").str().is_some_and(|k| matches!(k, "int" | "float" | "bool" | "char"));
            shown(at, inner)
        }
        "weak" => shown(format!("weak {:#x}", raw), false),
        "result" => {
            if raw == 0 {
                return unavailable();
            }
            match read_u64(s, raw) {
                Some(0) => {
                    let v = d.get("value");
                    if v.get("kind").str() == Some("void") {
                        shown("ok", false)
                    } else {
                        let inner = render(s, v, read_u64(s, raw + 8), depth + 1);
                        shown(format!("ok({})", inner.value), true)
                    }
                }
                Some(e) => shown(format!("error({})", error_text(s, e)), true),
                None => unavailable(),
            }
        }
        "error" => {
            if raw == 0 {
                return shown("nil", false);
            }
            shown(format!("error({})", error_text(s, raw)), true)
        }
        "errorkind" => match read_u64(s, raw + 8).and_then(|t| read_text(s, t)) {
            Some(n) => shown(n, false),
            None => unavailable(),
        },
        "function" => shown(format!("func {:#x}", raw), false),
        "channel" => shown(format!("channel {:#x}", raw), false),
        "future" => shown(format!("future {:#x}", raw), false),
        "nil" => shown("nil", false),
        _ => unavailable(),
    }
}

/// The message of an error object (`[message][kind][cause]`).
fn error_text(s: &Session, err: u64) -> String {
    match read_u64(s, err).and_then(|m| read_text(s, m)) {
        Some(m) => escape(&m),
        None => UNAVAILABLE.to_string(),
    }
}

/// The word of a record's field: its bytes at the offset of the descriptor.
fn field_word(s: &Session, record: u64, field: &Json) -> Option<u64> {
    let at = record + num_of(field.get("offset")) as u64;
    let size = field_size(&resolve_ref(s, field.get("type")));
    let b = read(s, at, size)?;
    let mut w = [0u8; 8];
    w[..size].copy_from_slice(&b);
    Some(u64::from_le_bytes(w))
}

fn resolve_ref(s: &Session, ty: &Json) -> Json {
    resolve(s, ty)
}

/// The parts of a value: (name, type descriptor, word).
fn children(s: &Session, ty: &Json, raw: Option<u64>) -> Vec<(String, Json, Option<u64>)> {
    let Some(raw) = raw else { return Vec::new() };
    let d = resolve(s, ty);
    let kind = d.get("kind").str().unwrap_or("");
    let mut out: Vec<(String, Json, Option<u64>)> = Vec::new();
    match kind {
        "record" | "rc" => {
            let rec = if kind == "rc" { resolve(s, d.get("target")) } else { d.clone() };
            if raw == 0 {
                return out;
            }
            if let Json::Arr(fields) = rec.get("fields") {
                for f in fields {
                    out.push((f.get("name").str().unwrap_or("?").to_string(), f.get("type").clone(), field_word(s, raw, f)));
                }
            }
        }
        "enum" => {
            if d.get("payload") != &Json::Bool(true) {
                return out;
            }
            let (Some(tag), Json::Arr(variants)) = (read_u64(s, raw), d.get("variants")) else { return out };
            if let Some(Json::Arr(tys)) = variants.get(tag as usize).map(|v| v.get("fields").clone()) {
                for (i, t) in tys.iter().enumerate() {
                    out.push((format!("value {}", i + 1), t.clone(), read_u64(s, raw + 8 + 8 * i as u64)));
                }
            }
        }
        "list" => {
            if let (Some(len), Some(data)) = (read_u64(s, raw), read_u64(s, raw + 16)) {
                for i in 0..len.min(LIMIT as u64) {
                    out.push((i.to_string(), d.get("element").clone(), read_u64(s, data + 8 * i)));
                }
                if len > LIMIT as u64 {
                    out.push((format!("… {} more", len - LIMIT as u64), obj(vec![("kind", Json::Str("unknown".into()))]), None));
                }
            }
        }
        "map" => {
            if let (Some(used), Some(entries)) = (read_u64(s, raw), read_u64(s, raw + 16)) {
                for i in 0..used.min(4096) {
                    let e = entries + 32 * i;
                    if read_u64(s, e) != Some(1) {
                        continue;
                    }
                    let key = render(s, d.get("key"), read_u64(s, e + 16), 2).value;
                    out.push((key, d.get("value").clone(), read_u64(s, e + 24)));
                    if out.len() >= LIMIT {
                        break;
                    }
                }
            }
        }
        "pointer" => {
            let target = resolve(s, d.get("target"));
            let size = field_size(&target);
            if raw != 0 {
                if let Some(b) = read(s, raw, size) {
                    let mut w = [0u8; 8];
                    w[..size].copy_from_slice(&b);
                    out.push(("^".to_string(), d.get("target").clone(), Some(u64::from_le_bytes(w))));
                }
            }
        }
        "result" => match read_u64(s, raw) {
            Some(0) => {
                if d.get("value").get("kind").str() != Some("void") {
                    out.push(("value".to_string(), d.get("value").clone(), read_u64(s, raw + 8)));
                }
            }
            Some(e) => {
                out.push(("error".to_string(), obj(vec![("kind", Json::Str("error".into()))]), Some(e)));
            }
            None => {}
        },
        "error" if raw != 0 => {
            out.push(("message".to_string(), obj(vec![("kind", Json::Str("text".into()))]), read_u64(s, raw)));
            out.push(("kind".to_string(), obj(vec![("kind", Json::Str("errorkind".into()))]), read_u64(s, raw + 8)));
            out.push(("cause".to_string(), obj(vec![("kind", Json::Str("error".into()))]), read_u64(s, raw + 16)));
        }
        _ => {}
    }
    out
}

/// The register a local lives in, for the innermost frame.
fn register_value(s: &Session, name: &str) -> Option<u64> {
    let r = s.dbg.as_ref()?.regs().ok()?;
    Some(match name {
        "rax" => r.rax,
        "rbx" => r.rbx,
        "rcx" => r.rcx,
        "rdx" => r.rdx,
        "rsi" => r.rsi,
        "rdi" => r.rdi,
        "r8" => r.r8,
        "r9" => r.r9,
        "r10" => r.r10,
        "r11" => r.r11,
        "r12" => r.r12,
        "r13" => r.r13,
        "r14" => r.r14,
        "r15" => r.r15,
        _ => return None,
    })
}

/// A local of a frame: (name, type descriptor, word).
type Local = (String, Json, Option<u64>);

/// The parameters and locals of a frame that are in scope where it stopped, with their words.
/// A local declared on the line the frame is at is not yet set, so it is left out.
fn frame_locals(s: &Session, frame: usize) -> Vec<Local> {
    let (Some(info), Some(f)) = (&s.info, s.frames.get(frame)) else { return Vec::new() };
    let Json::Arr(locals) = &info.functions[f.func].locals else { return Vec::new() };
    let mut out = Vec::new();
    for l in locals {
        let (first, last) = (num_of(l.get("firstLine")) as u32, num_of(l.get("lastLine")) as u32);
        let param = l.get("parameter") == &Json::Bool(true);
        if !param && !(first < f.line && (last == 0 || f.line <= last)) {
            continue;
        }
        let loc = l.get("location");
        let word = match loc.get("kind").str() {
            Some("frame") => {
                let addr = f.fp.wrapping_sub(num_of(loc.get("offset")) as u64);
                let mut w = read_u64(s, addr);
                if l.get("indirect") == &Json::Bool(true) {
                    w = w.and_then(|p| read_u64(s, p));
                }
                w
            }
            // a register holds it for as long as the function runs, but only the innermost frame can
            // be asked: the registers of the frames above are saved where this does not look
            Some("register") if frame == 0 => loc.get("register").str().and_then(|r| register_value(s, r)),
            _ => None,
        };
        out.push((l.get("name").str().unwrap_or("?").to_string(), l.get("type").clone(), word));
    }
    out
}

fn variable(s: &mut Session, name: &str, ty: &Json, raw: Option<u64>) -> Json {
    let r = render(s, ty, raw, 0);
    let reference = if r.expandable && raw.is_some() { s.values.add(Node::Value { ty: ty.clone(), raw }) } else { 0 };
    obj(vec![("name", Json::Str(name.to_string())), ("value", Json::Str(r.value)), ("type", Json::Str(type_name(s, ty))), ("variablesReference", Json::Num(reference as f64))])
}

pub(super) fn variables(s: &mut Session, reference: i64) -> Result<Vec<Json>, String> {
    let node = s.values.nodes.get((reference - 1).max(0) as usize).cloned().ok_or("that reference is no longer valid: the program ran since")?;
    let list: Vec<(String, Json, Option<u64>)> = match node {
        Node::Frame(i) => frame_locals(s, i),
        Node::Value { ty, raw } => children(s, &ty, raw),
    };
    Ok(list.iter().map(|(n, t, w)| variable(s, n, t, *w)).collect())
}

/// Evaluates a name with `.field`, `.len` and `[index]` steps in a frame.
pub(super) fn evaluate(s: &mut Session, expression: &str, frame: usize) -> Result<Json, String> {
    let steps = parse_path(expression.trim())?;
    let (first, rest) = steps.split_first().ok_or("write a name")?;
    let Step::Name(name) = first else { return Err("an expression starts with a name".into()) };
    if s.frames.is_empty() {
        return Err("the program is not stopped".into());
    }
    let locals = frame_locals(s, frame);
    // the nearest declaration of the name (the last one that is in scope)
    let Some((_, mut ty, mut raw)) = locals.into_iter().rev().find(|l| &l.0 == name) else { return Err(format!("'{}' is not a variable here", name)) };
    for step in rest {
        let d = resolve(s, &ty);
        let kind = d.get("kind").str().unwrap_or("").to_string();
        let w = raw.ok_or(UNAVAILABLE)?;
        match step {
            Step::Name(f) => {
                if (kind == "list" || kind == "text" || kind == "map") && f == "len" {
                    raw = match kind.as_str() {
                        "map" => read_u64(s, w + 48),
                        _ => read_u64(s, w),
                    };
                    ty = obj(vec![("kind", Json::Str("int".into())), ("bits", Json::Num(32.0)), ("signed", Json::Bool(true))]);
                    continue;
                }
                let part = children(s, &ty, raw).into_iter().find(|c| &c.0 == f).ok_or_else(|| format!("{} has no field '{}'", type_name(s, &ty), f))?;
                ty = part.1;
                raw = part.2;
            }
            Step::Index(i) => {
                let part = match (kind.as_str(), i) {
                    ("list", Index::Number(n)) => {
                        let (len, data) = (read_u64(s, w).ok_or(UNAVAILABLE)?, read_u64(s, w + 16).ok_or(UNAVAILABLE)?);
                        if *n >= len {
                            return Err(format!("index {} is out of range: the list has {} items", n, len));
                        }
                        (d.get("element").clone(), read_u64(s, data + 8 * n))
                    }
                    ("map", key) => {
                        let want = match key {
                            Index::Number(n) => n.to_string(),
                            Index::Text(t) => escape(t),
                        };
                        let found = children(s, &ty, raw).into_iter().find(|c| c.0 == want).ok_or_else(|| format!("the map has no key {}", want))?;
                        (found.1, found.2)
                    }
                    _ => return Err(format!("cannot index a {}", type_name(s, &ty))),
                };
                ty = part.0;
                raw = part.1;
            }
        }
    }
    let r = render(s, &ty, raw, 0);
    let reference = if r.expandable && raw.is_some() { s.values.add(Node::Value { ty: ty.clone(), raw }) } else { 0 };
    Ok(obj(vec![("result", Json::Str(r.value)), ("type", Json::Str(type_name(s, &ty))), ("variablesReference", Json::Num(reference as f64))]))
}

enum Index {
    Number(u64),
    Text(String),
}

enum Step {
    Name(String),
    Index(Index),
}

/// `a`, `a.b.c`, `xs[0]`, `m["key"]`, `p.items[2].name`.
fn parse_path(src: &str) -> Result<Vec<Step>, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut steps = Vec::new();
    let name = |i: &mut usize| -> Result<String, String> {
        let start = *i;
        while *i < chars.len() && (chars[*i].is_alphanumeric() || chars[*i] == '_') {
            *i += 1;
        }
        if start == *i {
            return Err(format!("expected a name at character {} of `{}`", start + 1, src));
        }
        Ok(chars[start..*i].iter().collect())
    };
    steps.push(Step::Name(name(&mut i)?));
    while i < chars.len() {
        match chars[i] {
            '.' => {
                i += 1;
                steps.push(Step::Name(name(&mut i)?));
            }
            '[' => {
                i += 1;
                let step = if chars.get(i) == Some(&'"') {
                    i += 1;
                    let start = i;
                    while i < chars.len() && chars[i] != '"' {
                        i += 1;
                    }
                    let t: String = chars[start..i].iter().collect();
                    i += 1;
                    Index::Text(t)
                } else {
                    let start = i;
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                    Index::Number(chars[start..i].iter().collect::<String>().parse().map_err(|_| format!("expected an index in `{}`", src))?)
                };
                if chars.get(i) != Some(&']') {
                    return Err(format!("expected ']' in `{}`", src));
                }
                i += 1;
                steps.push(Step::Index(step));
            }
            c => return Err(format!("only names, `.field` and `[index]` can be evaluated, not `{}`", c)),
        }
    }
    Ok(steps)
}

/// The text the program aborted with, read at the runtime's `abort` (the message in `rcx`, the
/// place in `rdx` or 0).
pub(super) fn abort_message(s: &Session) -> String {
    let Some(r) = s.dbg.as_ref().and_then(|d| d.regs().ok()) else { return "the program aborted".into() };
    let msg = read_text(s, r.rcx).unwrap_or_default();
    match read_text(s, r.rdx) {
        Some(place) if r.rdx != 0 => format!("abort: {}: {}", place, msg),
        _ => format!("abort: {}", msg),
    }
}
