//! The description of types a generator receives: built from the declarations, serialized in
//! the line-oriented wire format (used for the cache key, `--comptime-dump` and tests) and read
//! back. The generated driver program builds the same tables with constructor calls.

use crate::ast::*;
use std::collections::HashMap;

/// A module of the program: the program's own package or an imported one.
pub struct ModFile {
    pub name: String,
    pub file: File,
}

/// Every module a comptime run can see.
pub struct Universe {
    pub modules: Vec<ModFile>,
}

impl Universe {
    pub fn file(&self, name: &str) -> Option<&File> {
        self.modules.iter().find(|m| m.name == name).map(|m| &m.file)
    }

    /// The declaration of type `name` in `module` with its attributes and line.
    pub fn type_decl(&self, module: &str, name: &str) -> Option<(&TypeDecl, &Decl)> {
        let f = self.file(module)?;
        f.decls.iter().find_map(|d| match &d.kind {
            DeclKind::Type(td) if td.name == name => Some((td, d)),
            _ => None,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AttrData {
    pub name: String,
    pub args: Vec<String>,
    /// For each argument: was it written as a quoted text (`"full_name"`) rather than a bare word (`skip`)?
    pub quoted: Vec<bool>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FieldData {
    pub name: String,
    pub ty: usize,
    pub attrs: Vec<AttrData>,
    pub line: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VariantData {
    pub name: String,
    pub fields: Vec<FieldData>,
    pub attrs: Vec<AttrData>,
    pub line: u32,
}

/// A type as a generator sees it. Links to other types are indices into the table; `-1` means
/// none.
#[derive(Clone, Debug, PartialEq)]
pub struct TypeEntry {
    pub name: String,
    pub kind: String,
    pub line: u32,
    pub elem: i64,
    pub key: i64,
    pub value: i64,
    pub inner: i64,
    pub attrs: Vec<AttrData>,
    pub fields: Vec<FieldData>,
    pub variants: Vec<VariantData>,
}

impl TypeEntry {
    fn new(name: String, kind: &str, line: u32) -> Self {
        TypeEntry {
            name,
            kind: kind.to_string(),
            line,
            elem: -1,
            key: -1,
            value: -1,
            inner: -1,
            attrs: Vec::new(),
            fields: Vec::new(),
            variants: Vec::new(),
        }
    }
}

/// A type and every type reachable from it.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct TypeTable {
    pub types: Vec<TypeEntry>,
}

/// Type parameters bound to types (and the module those types are written in).
type Subst = HashMap<String, (Type, String)>;

const INT_NAMES: &[&str] = &["i8", "i16", "i32", "int", "i64", "isize", "long", "u8", "byte", "u16", "u32", "u64", "usize"];

/// Is `name` a built-in type name?
pub fn is_builtin_type(name: &str) -> bool {
    INT_NAMES.contains(&name) || matches!(name, "bool" | "text" | "float" | "f32" | "f64" | "error" | "ErrorKind")
}

fn attr_text(e: &Expr) -> String {
    match &e.kind {
        ExprKind::Text(parts) => {
            let mut s = String::new();
            for p in parts {
                match p {
                    TextPart::Lit(l) => s.push_str(l),
                    TextPart::Expr(_) => s.push('?'),
                }
            }
            s
        }
        ExprKind::Int(n) => n.to_string(),
        ExprKind::Float(f) => f.to_string(),
        ExprKind::Bool(b) => b.to_string(),
        ExprKind::Char(c) => c.to_string(),
        ExprKind::Ident(n) => n.clone(),
        ExprKind::Unary { op: "-", expr } => format!("-{}", attr_text(expr)),
        ExprKind::Field { expr, name, .. } => format!("{}.{}", attr_text(expr), name),
        _ => "?".to_string(),
    }
}

pub fn attr_data(attrs: &[Attr]) -> Vec<AttrData> {
    attrs
        .iter()
        .map(|a| AttrData {
            name: a.name.clone(),
            args: a.args.iter().map(attr_text).collect(),
            quoted: a.args.iter().map(|e| matches!(e.kind, ExprKind::Text(_))).collect(),
        })
        .collect()
}

/// `User`, `[]User`, `Map<text, int>`, `User?`: how a type is written.
pub fn show_type(t: &Type, subst: &Subst) -> String {
    match t {
        Type::Shared(i) => format!("shared {}", show_type(i, subst)),
        Type::Named { path, args } => {
            if path.len() == 1 && args.is_empty() {
                if let Some((s, _)) = subst.get(&path[0]) {
                    return show_type(s, &Subst::new());
                }
            }
            let mut s = path.join(".");
            if !args.is_empty() {
                s.push('<');
                s.push_str(&args.iter().map(|a| show_type(a, subst)).collect::<Vec<_>>().join(", "));
                s.push('>');
            }
            s
        }
        Type::Optional(i) => format!("{}?", show_type(i, subst)),
        Type::Pointer(i) => format!("*{}", show_type(i, subst)),
        Type::Slice(i) => format!("[]{}", show_type(i, subst)),
        Type::Array(n, i) => format!("[{}]{}", n, show_type(i, subst)),
        Type::Func { params, .. } => format!("func({})", params.iter().map(|p| show_type(p, subst)).collect::<Vec<_>>().join(", ")),
    }
}

/// A key that is the same for the same type wherever it is written.
fn qualified(t: &Type, module: &str, subst: &Subst) -> String {
    match t {
        Type::Shared(i) => qualified(i, module, subst),
        Type::Named { path, args } => {
            if path.len() == 1 && args.is_empty() {
                if let Some((s, m)) = subst.get(&path[0]) {
                    return qualified(s, m, &Subst::new());
                }
            }
            let mut s = if path.len() == 1 && !is_builtin_type(&path[0]) { format!("{}.{}", module, path[0]) } else { path.join(".") };
            if !args.is_empty() {
                s.push('<');
                s.push_str(&args.iter().map(|a| qualified(a, module, subst)).collect::<Vec<_>>().join(","));
                s.push('>');
            }
            s
        }
        Type::Optional(i) => format!("{}?", qualified(i, module, subst)),
        Type::Pointer(i) => format!("*{}", qualified(i, module, subst)),
        Type::Slice(i) => format!("[]{}", qualified(i, module, subst)),
        Type::Array(n, i) => format!("[{}]{}", n, qualified(i, module, subst)),
        Type::Func { params, .. } => format!("func({})", params.iter().map(|p| qualified(p, module, subst)).collect::<Vec<_>>().join(",")),
    }
}

/// A key that is the same for the same type wherever it is written in `module`.
pub fn canon_type(t: &Type, module: &str) -> String {
    qualified(t, module, &Subst::new())
}

/// Builds a `TypeTable` from the declarations of a program.
pub struct Reflector<'a> {
    uni: &'a Universe,
    pub table: TypeTable,
    canon: HashMap<String, usize>,
}

impl<'a> Reflector<'a> {
    pub fn new(uni: &'a Universe) -> Self {
        Reflector { uni, table: TypeTable::default(), canon: HashMap::new() }
    }

    /// The type declared as `name` in `module`.
    pub fn declared(&mut self, module: &str, name: &str) -> Option<usize> {
        self.uni.type_decl(module, name)?;
        let ty = Type::Named { path: vec![name.to_string()], args: vec![] };
        Some(self.reflect(&ty, module))
    }

    pub fn reflect(&mut self, ty: &Type, module: &str) -> usize {
        self.reflect_in(ty, module, &Subst::new())
    }

    fn reflect_in(&mut self, ty: &Type, module: &str, subst: &Subst) -> usize {
        // a type parameter stands for the type it is bound to
        if let Type::Named { path, args } = ty {
            if path.len() == 1 && args.is_empty() {
                if let Some((s, m)) = subst.get(&path[0]) {
                    let (s, m) = (s.clone(), m.clone());
                    return self.reflect_in(&s, &m, &Subst::new());
                }
            }
        }
        let key = qualified(ty, module, subst);
        if let Some(&i) = self.canon.get(&key) {
            return i;
        }
        let name = show_type(ty, subst);
        let idx = self.table.types.len();
        self.canon.insert(key, idx);
        self.table.types.push(TypeEntry::new(name.clone(), "record", 0));
        let mut entry = TypeEntry::new(name, "record", 0);
        match ty {
            Type::Named { path, args } => {
                let last = path.last().unwrap().as_str();
                let single = path.len() == 1;
                if single && INT_NAMES.contains(&last) {
                    entry.kind = "int".into();
                } else if single && matches!(last, "float" | "f32" | "f64") {
                    entry.kind = "float".into();
                } else if single && matches!(last, "bool" | "text") {
                    entry.kind = last.to_string();
                } else if single && matches!(last, "error" | "ErrorKind") {
                    entry.kind = "error".into();
                } else if last == "List" && args.len() == 1 {
                    entry.kind = "list".into();
                    entry.elem = self.reflect_in(&args[0], module, subst) as i64;
                } else if last == "Set" && args.len() == 1 {
                    entry.kind = "set".into();
                    entry.elem = self.reflect_in(&args[0], module, subst) as i64;
                } else if last == "Map" && args.len() == 2 {
                    entry.kind = "map".into();
                    entry.key = self.reflect_in(&args[0], module, subst) as i64;
                    entry.value = self.reflect_in(&args[1], module, subst) as i64;
                } else {
                    let (dm, dname) = if single { (module.to_string(), last.to_string()) } else { (path[0].clone(), last.to_string()) };
                    if let Some((td, d)) = self.uni.type_decl(&dm, &dname) {
                        let (td, d) = (td.clone(), d.clone());
                        entry.line = d.line;
                        entry.attrs = attr_data(&d.attrs);
                        // arguments of a generic record bind its parameters
                        let mut inner_subst = Subst::new();
                        for (g, a) in td.generics.iter().zip(args.iter()) {
                            let (at, am) = match a {
                                Type::Named { path, args: aa } if path.len() == 1 && aa.is_empty() => match subst.get(&path[0]) {
                                    Some((s, m)) => (s.clone(), m.clone()),
                                    None => (a.clone(), module.to_string()),
                                },
                                _ => (a.clone(), module.to_string()),
                            };
                            inner_subst.insert(g.name.clone(), (at, am));
                        }
                        match &td.body {
                            TypeBody::Record(fs) => {
                                entry.kind = "record".into();
                                for f in fs {
                                    let t = self.reflect_in(&f.ty, &dm, &inner_subst);
                                    entry.fields.push(FieldData { name: f.name.clone(), ty: t, attrs: attr_data(&f.attrs), line: f.line });
                                }
                            }
                            TypeBody::Enum(vs) => {
                                entry.kind = "enum".into();
                                for v in vs {
                                    let mut fields = Vec::new();
                                    for (i, t) in v.fields.iter().enumerate() {
                                        let id = self.reflect_in(t, &dm, &inner_subst);
                                        let fname = match v.field_names.get(i) {
                                            Some(n) if !n.is_empty() => n.clone(),
                                            _ => format!("_{}", i),
                                        };
                                        fields.push(FieldData { name: fname, ty: id, attrs: vec![], line: v.line });
                                    }
                                    entry.variants.push(VariantData { name: v.name.clone(), fields, attrs: attr_data(&v.attrs), line: v.line });
                                }
                            }
                            TypeBody::Alias(t) => {
                                entry.kind = "alias".into();
                                entry.inner = self.reflect_in(t, &dm, &inner_subst) as i64;
                            }
                            TypeBody::Interface(_) => entry.kind = "record".into(),
                        }
                    }
                }
            }
            Type::Shared(i) => {
                let k = self.reflect_in(i, module, subst);
                entry = self.table.types[k].clone();
            }
            Type::Optional(i) => {
                entry.kind = "optional".into();
                entry.inner = self.reflect_in(i, module, subst) as i64;
            }
            Type::Slice(i) | Type::Array(_, i) => {
                entry.kind = "list".into();
                entry.elem = self.reflect_in(i, module, subst) as i64;
            }
            Type::Pointer(i) => {
                entry.kind = "pointer".into();
                entry.inner = self.reflect_in(i, module, subst) as i64;
            }
            Type::Func { .. } => entry.kind = "func".into(),
        }
        self.table.types[idx] = entry;
        idx
    }
}

// ---------------------------------------------------------------------------
// The wire format
// ---------------------------------------------------------------------------

/// One generator call: which function, what it is given.
#[derive(Clone, Debug, PartialEq)]
pub struct WireCall {
    pub id: usize,
    pub generator: String,
    /// `derive`, `generated` or `block`
    pub arg_kind: String,
    pub table: TypeTable,
    pub root: usize,
    /// The raw text of a block.
    pub raw: Option<String>,
}

/// A token of the wire format: spaces, newlines, tabs and backslashes escaped.
fn esc(s: &str) -> String {
    if s.is_empty() {
        return "\\e".to_string();
    }
    let mut o = String::new();
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            ' ' => o.push_str("\\s"),
            '\n' => o.push_str("\\n"),
            '\t' => o.push_str("\\t"),
            '\r' => o.push_str("\\r"),
            c => o.push(c),
        }
    }
    o
}

fn unesc(s: &str) -> String {
    if s == "\\e" {
        return String::new();
    }
    let mut o = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            o.push(c);
            continue;
        }
        match it.next() {
            Some('s') => o.push(' '),
            Some('n') => o.push('\n'),
            Some('t') => o.push('\t'),
            Some('r') => o.push('\r'),
            Some('e') => {}
            Some(c) => o.push(c),
            None => o.push('\\'),
        }
    }
    o
}

fn attr_line(out: &mut String, tag: &str, owner: &[String], a: &AttrData) {
    out.push_str(tag);
    for o in owner {
        out.push(' ');
        out.push_str(o);
    }
    out.push(' ');
    out.push_str(&esc(&a.name));
    out.push_str(" #");
    for q in &a.quoted {
        out.push(if *q { '1' } else { '0' });
    }
    for x in &a.args {
        out.push(' ');
        out.push_str(&esc(x));
    }
    out.push('\n');
}

/// The text a generator call is serialized as (and hashed for the cache).
pub fn serialize_call(c: &WireCall) -> String {
    let mut o = String::new();
    o.push_str(&format!("call {} {} {}\n", c.id, esc(&c.generator), esc(&c.arg_kind)));
    o.push_str(&format!("root {}\n", c.root));
    for (i, t) in c.table.types.iter().enumerate() {
        o.push_str(&format!("type {} {} {} {} {} {} {} {}\n", i, esc(&t.name), esc(&t.kind), t.line, t.elem, t.key, t.value, t.inner));
        for a in &t.attrs {
            attr_line(&mut o, "tattr", &[i.to_string()], a);
        }
        for (fi, f) in t.fields.iter().enumerate() {
            o.push_str(&format!("field {} {} {} {} {}\n", i, fi, esc(&f.name), f.ty, f.line));
            for a in &f.attrs {
                attr_line(&mut o, "fattr", &[i.to_string(), fi.to_string()], a);
            }
        }
        for (vi, v) in t.variants.iter().enumerate() {
            o.push_str(&format!("variant {} {} {} {}\n", i, vi, esc(&v.name), v.line));
            for a in &v.attrs {
                attr_line(&mut o, "vattr", &[i.to_string(), vi.to_string()], a);
            }
            for (fi, f) in v.fields.iter().enumerate() {
                o.push_str(&format!("vfield {} {} {} {} {} {}\n", i, vi, fi, esc(&f.name), f.ty, f.line));
            }
        }
    }
    if let Some(r) = &c.raw {
        o.push_str(&format!("raw {}\n", esc(r)));
    }
    o.push_str("end\n");
    o
}

/// Reads what `serialize_call` wrote. Lines of an unknown kind are ignored, so a later version
/// can add queries.
pub fn parse_wire(text: &str) -> Result<Vec<WireCall>, String> {
    let mut calls = Vec::new();
    let mut cur: Option<WireCall> = None;
    let num = |s: Option<&&str>| -> Result<i64, String> {
        s.ok_or_else(|| "short line".to_string())?.parse::<i64>().map_err(|e| e.to_string())
    };
    for (ln, line) in text.lines().enumerate() {
        let parts: Vec<&str> = line.split(' ').collect();
        let bad = |m: String| format!("wire line {}: {}", ln + 1, m);
        match parts[0] {
            "call" => {
                if parts.len() < 4 {
                    return Err(bad("short call line".into()));
                }
                cur = Some(WireCall {
                    id: parts[1].parse().map_err(|_| bad("bad id".into()))?,
                    generator: unesc(parts[2]),
                    arg_kind: unesc(parts[3]),
                    table: TypeTable::default(),
                    root: 0,
                    raw: None,
                });
            }
            "end" => {
                if let Some(c) = cur.take() {
                    calls.push(c);
                }
            }
            tag => {
                let Some(c) = cur.as_mut() else { continue };
                let ty_of = |c: &mut WireCall, i: usize| -> Result<usize, String> {
                    if i < c.table.types.len() {
                        Ok(i)
                    } else {
                        Err(format!("wire line {}: unknown type {}", ln + 1, i))
                    }
                };
                match tag {
                    "root" => c.root = num(parts.get(1)).map_err(bad)? as usize,
                    "type" => {
                        if parts.len() < 9 {
                            return Err(bad("short type line".into()));
                        }
                        let mut t = TypeEntry::new(unesc(parts[2]), "record", 0);
                        t.kind = unesc(parts[3]);
                        t.line = num(parts.get(4)).map_err(bad)? as u32;
                        t.elem = num(parts.get(5)).map_err(bad)?;
                        t.key = num(parts.get(6)).map_err(bad)?;
                        t.value = num(parts.get(7)).map_err(bad)?;
                        t.inner = num(parts.get(8)).map_err(bad)?;
                        c.table.types.push(t);
                    }
                    "tattr" | "fattr" | "vattr" => {
                        let owners = if tag == "tattr" { 1 } else { 2 };
                        let ti = ty_of(c, num(parts.get(1)).map_err(bad)? as usize)?;
                        let flags = parts.get(owners + 2).copied().unwrap_or("#");
                        let a = AttrData {
                            name: unesc(parts.get(owners + 1).copied().unwrap_or("")),
                            args: parts.iter().skip(owners + 3).map(|s| unesc(s)).collect(),
                            quoted: flags.trim_start_matches('#').chars().map(|c| c == '1').collect(),
                        };
                        match tag {
                            "tattr" => c.table.types[ti].attrs.push(a),
                            "fattr" => {
                                let fi = num(parts.get(2)).map_err(bad)? as usize;
                                if let Some(f) = c.table.types[ti].fields.get_mut(fi) {
                                    f.attrs.push(a);
                                }
                            }
                            _ => {
                                let vi = num(parts.get(2)).map_err(bad)? as usize;
                                if let Some(v) = c.table.types[ti].variants.get_mut(vi) {
                                    v.attrs.push(a);
                                }
                            }
                        }
                    }
                    "field" => {
                        let ti = ty_of(c, num(parts.get(1)).map_err(bad)? as usize)?;
                        c.table.types[ti].fields.push(FieldData {
                            name: unesc(parts.get(3).copied().unwrap_or("")),
                            ty: num(parts.get(4)).map_err(bad)? as usize,
                            attrs: vec![],
                            line: num(parts.get(5)).map_err(bad)? as u32,
                        });
                    }
                    "variant" => {
                        let ti = ty_of(c, num(parts.get(1)).map_err(bad)? as usize)?;
                        c.table.types[ti].variants.push(VariantData {
                            name: unesc(parts.get(3).copied().unwrap_or("")),
                            fields: vec![],
                            attrs: vec![],
                            line: num(parts.get(4)).map_err(bad)? as u32,
                        });
                    }
                    "vfield" => {
                        let ti = ty_of(c, num(parts.get(1)).map_err(bad)? as usize)?;
                        let vi = num(parts.get(2)).map_err(bad)? as usize;
                        let f = FieldData {
                            name: unesc(parts.get(4).copied().unwrap_or("")),
                            ty: num(parts.get(5)).map_err(bad)? as usize,
                            attrs: vec![],
                            line: num(parts.get(6)).map_err(bad)? as u32,
                        };
                        if let Some(v) = c.table.types[ti].variants.get_mut(vi) {
                            v.fields.push(f);
                        }
                    }
                    "raw" => c.raw = Some(unesc(parts.get(1).copied().unwrap_or(""))),
                    _ => {}
                }
            }
        }
    }
    Ok(calls)
}

/// A Brasa text literal for `s`.
pub fn brasa_lit(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '"' => o.push_str("\\\""),
            '{' => o.push_str("\\{"),
            '}' => o.push_str("\\}"),
            '\n' => o.push_str("\\n"),
            '\t' => o.push_str("\\t"),
            '\r' => o.push_str("\\r"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\x{:02x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn attrs_lit(attrs: &[AttrData]) -> String {
    let items: Vec<String> = attrs
        .iter()
        .map(|a| {
            let args: Vec<String> = a.args.iter().map(|x| brasa_lit(x)).collect();
            let quoted: Vec<String> = a.quoted.iter().map(|q| q.to_string()).collect();
            format!("reflect.Attribute {{ name: {}, args: [{}], quoted: [{}] }}", brasa_lit(&a.name), args.join(", "), quoted.join(", "))
        })
        .collect();
    format!("[{}]", items.join(", "))
}

fn fields_lit(fs: &[FieldData]) -> String {
    let items: Vec<String> = fs
        .iter()
        .map(|f| {
            format!(
                "reflect.Field {{ name: {}, ty: reflect.type_at({}), attrs: {}, line: {} }}",
                brasa_lit(&f.name),
                f.ty,
                attrs_lit(&f.attrs),
                f.line
            )
        })
        .collect();
    format!("[{}]", items.join(", "))
}

/// The statements of the driver program that rebuild `table` for a generator.
pub fn table_statements(table: &TypeTable) -> Vec<String> {
    let mut v = Vec::new();
    for t in &table.types {
        let variants: Vec<String> = t
            .variants
            .iter()
            .map(|x| {
                format!(
                    "reflect.Variant {{ name: {}, fields: {}, attrs: {}, line: {} }}",
                    brasa_lit(&x.name),
                    fields_lit(&x.fields),
                    attrs_lit(&x.attrs),
                    x.line
                )
            })
            .collect();
        v.push(format!(
            "reflect.add_type({}, {}, {}, {}, {}, {}, {}, {}, {}, [{}])",
            brasa_lit(&t.name),
            brasa_lit(&t.kind),
            t.line,
            t.elem,
            t.key,
            t.value,
            t.inner,
            attrs_lit(&t.attrs),
            fields_lit(&t.fields),
            variants.join(", ")
        ));
    }
    v
}
