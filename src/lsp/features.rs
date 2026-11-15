//! The requests that answer from a document: formatting and hover.

use super::diagnostics::index_of;
use super::documents::{char_col, position, range_of, utf16_col};
use super::json::{obj, Json};
use crate::codegen_x64::{IndexEntry, Member, MemberKind, PositionIndex, Referent, BUILTIN_NAMES};
use crate::fmt::format_source;
use std::collections::HashMap;

/// The edits that format a document: one replacement of the whole text, or none.
pub fn formatting(text: &str) -> Json {
    match format_source(text) {
        Ok(new) if new != text => {
            let lines = text.lines().count() as i64;
            let end = if text.ends_with('\n') || text.is_empty() { position(lines, 0) } else { position(lines - 1, utf16_col(text.lines().last().unwrap_or(""), usize::MAX)) };
            Json::Arr(vec![obj(vec![("range", obj(vec![("start", position(0, 0)), ("end", end)])), ("newText", Json::Str(new))])])
        }
        _ => Json::Arr(Vec::new()),
    }
}

/// The comment lines (`//`) right above `line` (from 1), skipping attribute lines, as one text.
fn doc_comment(text: &str, line: u32) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut i = (line as usize).checked_sub(1)?;
    let mut doc: Vec<&str> = Vec::new();
    while i > 0 {
        let l = lines.get(i - 1)?.trim();
        if let Some(c) = l.strip_prefix("//") {
            doc.push(c.strip_prefix(' ').unwrap_or(c));
        } else if !(doc.is_empty() && l.starts_with("#[")) {
            break;
        }
        i -= 1;
    }
    doc.reverse();
    (!doc.is_empty()).then(|| doc.join("
"))
}

/// What to show for a name: its type or signature as code.
fn describe(e: &IndexEntry) -> Option<String> {
    let typed = |e: &IndexEntry| match &e.ty {
        Some(t) => format!("{}: {}", e.name, t),
        None => e.name.clone(),
    };
    Some(match &e.target {
        Referent::Local { .. } | Referent::Field { .. } | Referent::Global { .. } => typed(e),
        Referent::Function { .. } | Referent::Method { .. } => e.ty.clone().unwrap_or_else(|| format!("func {}", e.name)),
        Referent::Type { .. } => format!("type {}", e.name),
        Referent::Variant { enum_key, .. } => format!("{}.{}", enum_key.rsplit('.').next().unwrap_or(enum_key), e.name),
        Referent::Package { name } => format!("package {}", name),
        Referent::TypeParameter { name } => format!("type parameter {}", name),
        Referent::Builtin { name } => format!("{} (built in)", name),
        Referent::Unknown => return None,
    })
}

/// `textDocument/hover`: the type or signature of the name at the position and the doc comment of
/// what it names; null where there is nothing to show. `line` and `character` count from 0, the
/// character in UTF-16 units.
pub fn hover(uri: &str, docs: &HashMap<String, String>, line: i64, character: i64) -> Json {
    let Some(text) = docs.get(uri) else { return Json::Null };
    let Some(index) = index_of(uri, docs) else { return Json::Null };
    let col = char_col(text.lines().nth(line.max(0) as usize).unwrap_or(""), character);
    let Some(entry) = index.at("main", line as u32 + 1, col as u32 + 1) else { return Json::Null };
    let Some(shown) = describe(entry) else { return Json::Null };
    let mut value = format!("```zila
{}
```", shown);
    // the comment of the declaration, when it is in this document
    if let Some(("main", decl)) = index.declaration_of(&entry.target) {
        if let Some(doc) = doc_comment(text, decl.span.line) {
            value.push_str("

");
            value.push_str(&doc);
        }
    }
    obj(vec![("contents", obj(vec![("kind", Json::Str("markdown".into())), ("value", Json::Str(value))])), ("range", range_of(text, entry.span))])
}

/// The attributes the compiler knows, for completion after `#[`.
const ATTRIBUTES: &[&str] = &["allocates", "allocator", "block", "derive", "export", "generated", "link", "link_src", "naked", "no_alloc", "shared", "sql_dialect", "test"];

// the kinds of an item, as the protocol numbers them
const METHOD: i64 = 2;
const FUNCTION: i64 = 3;
const FIELD: i64 = 5;
const VARIABLE: i64 = 6;
const CLASS: i64 = 7;
const MODULE: i64 = 9;
const KEYWORD: i64 = 14;
const ENUM_MEMBER: i64 = 20;

fn item(label: &str, kind: i64, detail: Option<&str>) -> Json {
    let mut v = vec![("label", Json::Str(label.into())), ("kind", super::json::num(kind))];
    if let Some(d) = detail {
        v.push(("detail", Json::Str(d.into())));
    }
    obj(v)
}

fn member_item(m: &Member) -> Json {
    let kind = match m.kind {
        MemberKind::Field => FIELD,
        MemberKind::Method => METHOD,
        MemberKind::Function => FUNCTION,
        MemberKind::Global => VARIABLE,
        MemberKind::Type => CLASS,
        MemberKind::Variant => ENUM_MEMBER,
    };
    item(&m.name, kind, Some(&m.ty))
}

/// The members a value or package that `chain` (`a`, `a.b`) names has, from the scope at the
/// cursor (line and column from 1).
fn members_of(index: &PositionIndex, chain: &str, at: (u32, u32)) -> Vec<Json> {
    let mut parts = chain.split('.');
    let first = parts.next().unwrap_or("");
    let rest: Vec<&str> = parts.collect();
    let entries = index.entries("main");
    let is_local = visible_locals(entries, at).iter().any(|e| e.name == first);
    let is_global = entries.iter().any(|e| e.is_declaration && e.name == first && matches!(e.target, Referent::Global { .. }));
    if !is_local && !is_global && rest.is_empty() {
        // a package named in a `use`
        if index.imports.get("main").is_some_and(|i| i.iter().any(|p| p == first)) {
            return index.package_members.get(first).map(|v| v.iter().map(member_item).collect()).unwrap_or_default();
        }
        // the variants of an enum, written `Enum.Variant`
        if entries.iter().any(|e| e.is_declaration && e.name == first && matches!(e.target, Referent::Type { .. })) {
            return index.variants.get(&format!("main.{}", first)).map(|v| v.iter().map(member_item).collect()).unwrap_or_default();
        }
    }
    value_key(index, chain, at).and_then(|k| index.members.get(&k)).map(|ms| ms.iter().map(member_item).collect()).unwrap_or_default()
}

/// The key of the type of the value `chain` (`a`, `a.b`) names: a local or global, then the field
/// of each step.
fn value_key(index: &PositionIndex, chain: &str, at: (u32, u32)) -> Option<String> {
    let mut parts = chain.split('.');
    let first = parts.next()?;
    let entries = index.entries("main");
    let locals = visible_locals(entries, at);
    let mut key = match locals.iter().find(|e| e.name == first) {
        Some(e) => e.type_key.clone(),
        None => entries.iter().find(|e| e.is_declaration && e.name == first && matches!(e.target, Referent::Global { .. })).and_then(|e| e.type_key.clone()),
    };
    for step in parts {
        let k = key.take()?;
        key = index.members.get(&k).and_then(|ms| ms.iter().find(|m| m.name == step)).and_then(|m| m.type_key.clone());
    }
    key
}

/// The locals and parameters usable at `at`, the nearest declaration of each name.
fn visible_locals(entries: &[IndexEntry], at: (u32, u32)) -> Vec<&IndexEntry> {
    let mut v: Vec<&IndexEntry> = Vec::new();
    for e in entries {
        if !e.is_declaration || !matches!(e.target, Referent::Local { .. }) || e.visible.is_none() {
            continue;
        }
        let starts = (e.span.line, e.span.col) <= at;
        let ends = at <= (e.visible.end_line, e.visible.end_col);
        if starts && ends {
            v.retain(|o| o.name != e.name);
            v.push(e);
        }
    }
    v
}

/// The names usable at the cursor: locals, parameters, the declarations of the package, the
/// packages it imports and what the language provides.
fn scope_items(index: &PositionIndex, at: (u32, u32)) -> Vec<Json> {
    let entries = index.entries("main");
    let mut out = Vec::new();
    for e in visible_locals(entries, at) {
        out.push(item(&e.name, VARIABLE, e.ty.as_deref()));
    }
    let mut seen = std::collections::HashSet::new();
    for e in entries.iter().filter(|e| e.is_declaration) {
        let kind = match e.target {
            Referent::Function { .. } => FUNCTION,
            Referent::Type { .. } => CLASS,
            Referent::Global { .. } => VARIABLE,
            Referent::Variant { .. } => ENUM_MEMBER,
            _ => continue,
        };
        if seen.insert((e.name.clone(), kind)) {
            out.push(item(&e.name, kind, e.ty.as_deref()));
        }
    }
    for p in index.imports.get("main").map(|v| v.as_slice()).unwrap_or(&[]) {
        out.push(item(p, MODULE, None));
    }
    for b in BUILTIN_NAMES {
        out.push(item(b, FUNCTION, Some("built in")));
    }
    for k in ["func", "let", "var", "if", "else", "for", "while", "match", "return", "type", "use", "end"] {
        out.push(item(k, KEYWORD, None));
    }
    out
}

/// `textDocument/completion`: after `#[` the attributes; after `name.` the members of what the
/// name is; elsewhere the names in scope. It works on a text with syntax errors elsewhere: the
/// scope comes from what parses.
pub fn completion(uri: &str, docs: &HashMap<String, String>, line: i64, character: i64) -> Json {
    let Some(text) = docs.get(uri) else { return Json::Null };
    let line_text = text.lines().nth(line.max(0) as usize).unwrap_or("");
    let col = char_col(line_text, character);
    let before: String = line_text.chars().take(col).collect();
    let list = |items: Vec<Json>| obj(vec![("isIncomplete", Json::Bool(false)), ("items", Json::Arr(items))]);
    // inside an attribute: `#[exp`
    let t = before.trim_start();
    if let Some(rest) = t.strip_prefix("#[") {
        if rest.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return list(ATTRIBUTES.iter().map(|a| item(a, KEYWORD, Some("attribute"))).collect());
        }
    }
    let Some(index) = index_of(uri, docs) else { return Json::Null };
    let at = (line as u32 + 1, col as u32 + 1);
    // after `name.` or `name.part`
    let word = before.trim_end_matches(|c: char| c.is_alphanumeric() || c == '_');
    if let Some(recv) = word.strip_suffix('.') {
        let chain: String = recv.chars().rev().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.').collect::<String>().chars().rev().collect();
        if chain.is_empty() {
            return Json::Null;
        }
        return list(members_of(&index, &chain, at));
    }
    list(scope_items(&index, at))
}

/// The call the cursor is inside: the written callee (`f`, `p.m`, `pkg.f`) and how many commas of
/// the argument list come before the cursor. `None` outside a call.
fn enclosing_call(before: &str) -> Option<(String, usize)> {
    let chars: Vec<char> = before.chars().collect();
    let (mut depth, mut commas) = (0usize, 0usize);
    let mut i = chars.len();
    let mut scanned = 0;
    while i > 0 {
        i -= 1;
        scanned += 1;
        if scanned > 4000 {
            return None;
        }
        match chars[i] {
            ')' | ']' | '}' => depth += 1,
            '(' if depth == 0 => {
                let callee: String = chars[..i].iter().rev().take_while(|c| c.is_alphanumeric() || **c == '_' || **c == '.').collect::<String>().chars().rev().collect();
                return (!callee.is_empty()).then_some((callee, commas));
            }
            '(' | '[' | '{' if depth > 0 => depth -= 1,
            '[' | '{' => return None,
            ',' if depth == 0 => commas += 1,
            _ => {}
        }
    }
    None
}

/// The text of the parameters in a signature `func f(a: int, b: [](int, int)) -> int`, split at the
/// commas that are not inside brackets.
fn signature_parameters(sig: &str) -> Vec<String> {
    let Some(open) = sig.find('(') else { return Vec::new() };
    let (mut depth, mut start) = (0usize, open + 1);
    let mut out = Vec::new();
    for (i, c) in sig.char_indices().skip_while(|(i, _)| *i <= open) {
        match c {
            '(' | '[' | '<' => depth += 1,
            ')' | ']' | '>' if depth > 0 => depth -= 1,
            ')' => {
                let p = sig[start..i].trim();
                if !p.is_empty() {
                    out.push(p.to_string());
                }
                return out;
            }
            ',' if depth == 0 => {
                out.push(sig[start..i].trim().to_string());
                start = i + 1;
            }
            _ => {}
        }
    }
    out
}

/// The signature of the function or method a written callee names, from the scope at `at`.
fn callee_signature(index: &PositionIndex, callee: &str, at: (u32, u32)) -> Option<(String, Option<u32>)> {
    let (recv, name) = match callee.rsplit_once('.') {
        Some((r, n)) => (Some(r), n),
        None => (None, callee),
    };
    let entries = index.entries("main");
    let Some(recv) = recv else {
        let e = entries.iter().find(|e| e.is_declaration && e.name == name && matches!(e.target, Referent::Function { .. }))?;
        return Some((e.ty.clone()?, Some(e.span.line)));
    };
    // `package.function`
    if !recv.contains('.') && index.imports.get("main").is_some_and(|i| i.iter().any(|p| p == recv)) && visible_locals(entries, at).iter().all(|e| e.name != recv) {
        let m = index.package_members.get(recv)?.iter().find(|m| m.name == name && m.kind == MemberKind::Function)?;
        return Some((m.ty.clone(), None));
    }
    // `value.method`
    let key = value_key(index, recv, at)?;
    let m = index.members.get(&key)?.iter().find(|m| m.name == name && m.kind == MemberKind::Method)?;
    Some((m.ty.clone(), None))
}

/// `textDocument/signatureHelp`: inside the parentheses of a call, the signature of the callee with
/// the parameter the cursor is at marked; null outside a call and for a callee that is not known.
pub fn signature_help(uri: &str, docs: &HashMap<String, String>, line: i64, character: i64) -> Json {
    let Some(text) = docs.get(uri) else { return Json::Null };
    let lines: Vec<&str> = text.lines().collect();
    let line_text = lines.get(line.max(0) as usize).copied().unwrap_or("");
    let col = char_col(line_text, character);
    let mut before: String = lines.iter().take(line.max(0) as usize).map(|l| format!("{}\n", l)).collect();
    before.extend(line_text.chars().take(col));
    let Some((callee, commas)) = enclosing_call(&before) else { return Json::Null };
    let Some(index) = index_of(uri, docs) else { return Json::Null };
    let Some((sig, decl_line)) = callee_signature(&index, &callee, (line as u32 + 1, col as u32 + 1)) else { return Json::Null };
    let params = signature_parameters(&sig);
    let active = if params.is_empty() { 0 } else { commas.min(params.len() - 1) };
    let mut s = vec![("label", Json::Str(sig.clone())), ("parameters", Json::Arr(params.iter().map(|p| obj(vec![("label", Json::Str(p.clone()))])).collect()))];
    if let Some(doc) = decl_line.and_then(|l| doc_comment(text, l)) {
        s.push(("documentation", Json::Str(doc)));
    }
    obj(vec![("signatures", Json::Arr(vec![obj(s)])), ("activeSignature", super::json::num(0)), ("activeParameter", super::json::num(active as i64))])
}
