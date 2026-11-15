//! Navigation: definition and references (symbols and rename are here too).

use super::diagnostics::index_of;
use super::documents::{char_col, path_to_uri, range_of, uri_to_path};
use super::json::{num, obj, Json};
use crate::ast::{DeclKind, File, Span, Type, TypeBody};
use crate::codegen_x64::{std_source, IndexEntry, PositionIndex, Referent};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A path compared without the spelling differences the platform ignores.
fn norm(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        s.to_lowercase()
    } else {
        s
    }
}

/// The text of a file: the open document when there is one, else the file on disk.
pub(super) fn text_at(path: &Path, docs: &HashMap<String, String>) -> Option<String> {
    let want = norm(path);
    for (u, t) in docs {
        if uri_to_path(u).is_some_and(|p| norm(&p) == want) {
            return Some(t.clone());
        }
    }
    std::fs::read_to_string(path).ok()
}

/// The entry under the cursor (`line` and `character` count from 0, the character in UTF-16 units).
fn entry_at(uri: &str, docs: &HashMap<String, String>, line: i64, character: i64) -> Option<(std::rc::Rc<PositionIndex>, IndexEntry)> {
    let text = docs.get(uri)?;
    let index = index_of(uri, docs)?;
    let col = char_col(text.lines().nth(line.max(0) as usize)?, character);
    let entry = index.at("main", line as u32 + 1, col as u32 + 1)?.clone();
    Some((index, entry))
}

/// The uri and range of a range of a module of the program: the requesting document for `main`,
/// else the file of the package it comes from.
pub(super) fn locate(index: &PositionIndex, module: &str, span: Span, uri: &str, docs: &HashMap<String, String>) -> Option<(String, Json)> {
    if module == "main" {
        return Some((uri.to_string(), range_of(docs.get(uri)?, span)));
    }
    let (path, line) = index.file_of(module, span.line)?;
    let end = line + (span.end_line - span.line);
    let text = text_at(path, docs)?;
    Some((path_to_uri(path), range_of(&text, Span::new(line, span.col, end, span.end_col))))
}

fn location_json(uri: &str, range: Json) -> Json {
    obj(vec![("uri", Json::Str(uri.into())), ("range", range)])
}

/// The module a target is declared in.
fn module_of(t: &Referent) -> Option<String> {
    match t {
        Referent::Function { module, .. } | Referent::Type { module, .. } | Referent::Global { module, .. } => Some(module.clone()),
        Referent::Method { type_key: k, .. } | Referent::Field { type_key: k, .. } | Referent::Variant { enum_key: k, .. } => k.split('.').next().map(|s| s.to_string()),
        _ => None,
    }
}

/// The range of the name that declares `target` in the parsed `file` of its module.
fn find_decl(file: &File, target: &Referent) -> Option<Span> {
    let type_decl = |key: &str| {
        let name = key.rsplit('.').next().unwrap_or(key).to_string();
        file.decls.iter().find_map(|d| match &d.kind {
            DeclKind::Type(t) if t.name == name => Some(t),
            _ => None,
        })
    };
    match target {
        Referent::Function { name, .. } => file.decls.iter().find_map(|d| match &d.kind {
            DeclKind::Func(f) if f.receiver.is_none() && &f.name == name => Some(f.name_span),
            DeclKind::Extern(x) if &x.name == name => Some(x.name_span),
            _ => None,
        }),
        Referent::Global { name, .. } => file.decls.iter().find_map(|d| match &d.kind {
            DeclKind::Var(v) if &v.name == name => Some(v.name_span),
            _ => None,
        }),
        Referent::Type { name, .. } => file.decls.iter().find_map(|d| match &d.kind {
            DeclKind::Type(t) if &t.name == name => Some(t.name_span),
            _ => None,
        }),
        Referent::Method { type_key, name } => {
            let ty = type_key.rsplit('.').next().unwrap_or(type_key);
            file.decls.iter().find_map(|d| match &d.kind {
                DeclKind::Func(f) if &f.name == name && matches!(f.receiver.as_ref().map(|r| &r.ty), Some(Type::Named { path, .. }) if path.last().map(|s| s.as_str()) == Some(ty)) => Some(f.name_span),
                _ => None,
            })
        }
        Referent::Field { type_key, name } => type_decl(type_key).and_then(|t| match &t.body {
            TypeBody::Record(fs) => fs.iter().find(|f| &f.name == name).map(|f| f.name_span),
            _ => None,
        }),
        Referent::Variant { enum_key, name } => type_decl(enum_key).and_then(|t| match &t.body {
            TypeBody::Enum(vs) => vs.iter().find(|v| &v.name == name).map(|v| v.name_span),
            _ => None,
        }),
        _ => None,
    }
}

/// The copy of a standard-library source that definitions point into: the sources are part of the
/// compiler, so they are written to a folder of the temporary directory when first needed.
fn std_file(module: &str) -> Option<PathBuf> {
    let src = std_source(module)?;
    let dir = std::env::temp_dir().join("zila-std");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("{}.zila", module));
    if std::fs::read_to_string(&path).ok().as_deref() != Some(src) {
        std::fs::write(&path, src).ok()?;
    }
    Some(path)
}

/// Where a standard-library declaration is: its file (a copy) and range.
fn std_location(target: &Referent) -> Option<(String, Json)> {
    let module = module_of(target)?;
    let src = std_source(&module)?;
    let file = crate::parser::parse_source(src).ok()?;
    let span = find_decl(&file, target)?;
    let path = std_file(&module)?;
    Some((path_to_uri(&path), range_of(src, span)))
}

/// `textDocument/definition`: the declaration of the name at the position, in this document, in
/// another file of a package of the project or in the standard library; null for anything else.
pub fn definition(uri: &str, docs: &HashMap<String, String>, line: i64, character: i64) -> Json {
    let Some((index, entry)) = entry_at(uri, docs, line, character) else { return Json::Null };
    if let Some((module, decl)) = index.declaration_of(&entry.target) {
        return match locate(&index, module, decl.span, uri, docs) {
            Some((u, r)) => location_json(&u, r),
            None => Json::Null,
        };
    }
    match std_location(&entry.target) {
        Some((u, r)) => location_json(&u, r),
        None => Json::Null,
    }
}

/// `textDocument/references`: every place the program names the thing at the position (its
/// declaration too when `include_declaration`), in every file of the project.
pub fn references(uri: &str, docs: &HashMap<String, String>, line: i64, character: i64, include_declaration: bool) -> Json {
    let Some((index, entry)) = entry_at(uri, docs, line, character) else { return Json::Null };
    if matches!(entry.target, Referent::Unknown | Referent::Builtin { .. } | Referent::TypeParameter { .. }) {
        return Json::Null;
    }
    let mut out = Vec::new();
    for (module, e) in index.references(&entry.target) {
        if e.is_declaration && !include_declaration {
            continue;
        }
        if let Some((u, r)) = locate(&index, module, e.span, uri, docs) {
            out.push(location_json(&u, r));
        }
    }
    // a declaration in the standard library is not among the uses of the program
    if include_declaration && index.declaration_of(&entry.target).is_none() {
        if let Some((u, r)) = std_location(&entry.target) {
            out.push(location_json(&u, r));
        }
    }
    Json::Arr(out)
}

// symbol kinds, as the protocol numbers them
const SK_CLASS: i64 = 5;
const SK_METHOD: i64 = 6;
const SK_ENUM: i64 = 10;
const SK_INTERFACE: i64 = 11;
const SK_FUNCTION: i64 = 12;
const SK_VARIABLE: i64 = 13;
const SK_FIELD: i64 = 8;
const SK_ENUM_MEMBER: i64 = 22;
const SK_STRUCT: i64 = 23;

/// The kind of symbol a declaration is, and the key of the type when it declares one.
fn symbol_kind(index: &PositionIndex, e: &IndexEntry) -> Option<i64> {
    Some(match &e.target {
        Referent::Function { .. } => SK_FUNCTION,
        Referent::Global { .. } => SK_VARIABLE,
        Referent::Method { .. } => SK_METHOD,
        Referent::Type { module, name } => {
            let key = format!("{}.{}", module, name);
            if index.variants.contains_key(&key) {
                SK_ENUM
            } else if index.members.get(&key).is_some_and(|m| m.iter().any(|x| x.kind == crate::codegen_x64::MemberKind::Field)) {
                SK_STRUCT
            } else if index.entries(module).iter().any(|x| x.is_declaration && matches!(&x.target, Referent::Method { type_key, .. } if type_key == &key) && x.span.line > e.span.line) && !index.members.contains_key(&key) {
                SK_INTERFACE
            } else {
                SK_CLASS
            }
        }
        _ => return None,
    })
}

/// `textDocument/documentSymbol`: the declarations of a document, a type with its fields or
/// variants inside. The range of a symbol is its name.
pub fn document_symbols(uri: &str, docs: &HashMap<String, String>) -> Json {
    let Some(text) = docs.get(uri) else { return Json::Null };
    let Some(index) = index_of(uri, docs) else { return Json::Null };
    let entries = index.entries("main");
    let symbol = |e: &IndexEntry, kind: i64, children: Vec<Json>, detail: Option<&str>| {
        let mut v = vec![("name", Json::Str(e.name.clone())), ("kind", num(kind)), ("range", range_of(text, e.span)), ("selectionRange", range_of(text, e.span))];
        if let Some(d) = detail {
            v.push(("detail", Json::Str(d.to_string())));
        }
        if !children.is_empty() {
            v.push(("children", Json::Arr(children)));
        }
        obj(v)
    };
    // the methods of an interface are listed inside it
    let interfaces: Vec<String> = entries.iter().filter(|e| e.is_declaration && symbol_kind(&index, e) == Some(SK_INTERFACE)).filter_map(|e| match &e.target {
        Referent::Type { module, name } => Some(format!("{}.{}", module, name)),
        _ => None,
    }).collect();
    let mut out = Vec::new();
    for e in entries.iter().filter(|e| e.is_declaration) {
        let Some(kind) = symbol_kind(&index, e) else { continue };
        if matches!(&e.target, Referent::Method { type_key, .. } if interfaces.contains(type_key)) {
            continue;
        }
        let mut children = Vec::new();
        if let Referent::Type { module, name } = &e.target {
            let key = format!("{}.{}", module, name);
            for c in entries.iter().filter(|c| c.is_declaration) {
                match &c.target {
                    Referent::Field { type_key, .. } if type_key == &key => children.push(symbol(c, SK_FIELD, Vec::new(), c.ty.as_deref())),
                    Referent::Variant { enum_key, .. } if enum_key == &key => children.push(symbol(c, SK_ENUM_MEMBER, Vec::new(), None)),
                    Referent::Method { type_key, .. } if type_key == &key && c.span.line > e.span.line && kind == SK_INTERFACE => children.push(symbol(c, SK_METHOD, Vec::new(), None)),
                    _ => {}
                }
            }
        }
        let detail = match &e.target {
            Referent::Method { type_key, .. } => type_key.rsplit('.').next(),
            _ => e.ty.as_deref(),
        };
        out.push(symbol(e, kind, children, detail));
    }
    Json::Arr(out)
}

/// `workspace/symbol`: the declarations of the program (every file of its packages) whose name
/// contains `query`, ignoring case. The program is that of the first open `main` document.
pub fn workspace_symbols(docs: &HashMap<String, String>, query: &str) -> Json {
    let mut uris: Vec<&String> = docs.keys().collect();
    uris.sort();
    let q = query.to_lowercase();
    for uri in uris {
        let Some(index) = index_of(uri, docs) else { continue };
        let mut out = Vec::new();
        for module in index.modules() {
            for e in index.entries(module).iter().filter(|e| e.is_declaration) {
                let Some(kind) = symbol_kind(&index, e) else { continue };
                if !e.name.to_lowercase().contains(&q) {
                    continue;
                }
                let Some((u, r)) = locate(&index, module, e.span, uri, docs) else { continue };
                out.push(obj(vec![("name", Json::Str(e.name.clone())), ("kind", num(kind)), ("location", location_json(&u, r)), ("containerName", Json::Str(module.to_string()))]));
            }
        }
        return Json::Arr(out);
    }
    Json::Arr(Vec::new())
}

/// Whether `name` can be written as a name: letters, digits and `_`, not starting with a digit,
/// and not a keyword.
fn valid_name(name: &str) -> bool {
    let mut cs = name.chars();
    let first = cs.next();
    first.is_some_and(|c| c.is_alphabetic() || c == '_') && cs.all(|c| c.is_alphanumeric() || c == '_') && !crate::lexer::KEYWORDS.contains(&name)
}

/// What stops `target` from being renamed to `new` where it is declared: another thing of that name
/// in the same scope, or a use in its extent that the new name would capture.
fn clash(index: &PositionIndex, module: &str, decl: &IndexEntry, new: &str) -> Option<String> {
    let entries = index.entries(module);
    let same = |a: &IndexEntry| a.name == new && a.target != decl.target;
    match &decl.target {
        Referent::Local { .. } => {
            let v = decl.visible;
            let inside = |e: &IndexEntry| (e.span.line, e.span.col) >= (v.line, v.col) && (e.span.line, e.span.col) <= (v.end_line, v.end_col);
            if let Some(o) = entries.iter().find(|e| e.is_declaration && same(e) && matches!(e.target, Referent::Local { .. }) && !e.visible.is_none() && {
                let ov = e.visible;
                // the two extents overlap
                (ov.line, ov.col) <= (v.end_line, v.end_col) && (v.line, v.col) <= (ov.end_line, ov.end_col)
            }) {
                return Some(format!("'{}' is already declared in this scope (line {})", new, o.span.line));
            }
            // a use inside this extent of something else called `new` would now mean this local
            if let Some(u) = entries.iter().find(|e| !e.is_declaration && same(e) && inside(e)) {
                return Some(format!("'{}' is used on line {} for something else; the renamed name would hide it", new, u.span.line));
            }
            None
        }
        Referent::Function { .. } | Referent::Type { .. } | Referent::Global { .. } | Referent::Variant { .. } => entries
            .iter()
            .find(|e| e.is_declaration && same(e) && matches!(e.target, Referent::Function { .. } | Referent::Type { .. } | Referent::Global { .. } | Referent::Variant { .. }))
            .map(|o| format!("'{}' is already declared in package '{}' (line {})", new, module, o.span.line)),
        Referent::Method { type_key, .. } | Referent::Field { type_key, .. } => entries
            .iter()
            .find(|e| e.is_declaration && same(e) && matches!(&e.target, Referent::Method { type_key: k, .. } | Referent::Field { type_key: k, .. } if k == type_key))
            .map(|o| format!("'{}' is already a member of '{}' (line {})", new, type_key.rsplit('.').next().unwrap_or(type_key), o.span.line)),
        _ => None,
    }
}

/// `textDocument/rename`: the edits (a `WorkspaceEdit`) that rename the symbol at the position
/// everywhere in the program, or why it cannot be done: the new name is not a name or clashes with
/// another one, or the symbol is declared outside the project.
pub fn rename(uri: &str, docs: &HashMap<String, String>, line: i64, character: i64, new: &str) -> Result<Json, String> {
    if !valid_name(new) {
        return Err(format!("'{}' is not a valid name", new));
    }
    let Some((index, entry)) = entry_at(uri, docs, line, character) else { return Err("there is no name here to rename".into()) };
    if matches!(entry.target, Referent::Unknown | Referent::Builtin { .. } | Referent::Package { .. } | Referent::TypeParameter { .. }) {
        return Err(format!("'{}' cannot be renamed: it is not declared by the program", entry.name));
    }
    let Some((decl_module, decl)) = index.declaration_of(&entry.target) else {
        return Err(format!("'{}' is declared outside the package (in the standard library) and cannot be renamed", entry.name));
    };
    // a package that comes from a dependency is outside the project
    if decl_module != "main" {
        let root = crate::package::find_manifest_dir(&uri_to_path(uri).unwrap_or_default());
        let outside = match (index.file_of(decl_module, decl.span.line), &root) {
            (Some((p, _)), Some(r)) => !norm(p).starts_with(&norm(r)),
            (None, _) => true,
            (_, None) => false,
        };
        if outside {
            return Err(format!("'{}' is declared outside the package (in a dependency) and cannot be renamed", entry.name));
        }
    }
    if let Some(why) = clash(&index, decl_module, decl, new) {
        return Err(why);
    }
    let mut changes: Vec<(String, Vec<Json>)> = Vec::new();
    for (module, e) in index.references(&entry.target) {
        let Some((u, range)) = locate(&index, module, e.span, uri, docs) else { continue };
        let edit = obj(vec![("range", range), ("newText", Json::Str(new.to_string()))]);
        match changes.iter_mut().find(|c| c.0 == u) {
            Some(c) => c.1.push(edit),
            None => changes.push((u, vec![edit])),
        }
    }
    Ok(obj(vec![("changes", Json::Obj(changes.into_iter().map(|(u, edits)| (u, Json::Arr(edits))).collect()))]))
}
