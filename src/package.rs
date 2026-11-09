//! The package manager (spec `package-manager`): the manifest `zila.toml`, the lock file
//! `zila.lock`, semantic versions with minimal version selection, the tree hash, the cache, and
//! the resolver that tells the compiler where the packages named by `use` are.
//!
//! Nothing in this file runs a program or touches the network. Fetching (the one place that runs
//! `git`) is in `fetch.rs`; a build only reads the lock and the cache.

use crate::ast::*;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub const MANIFEST: &str = "zila.toml";
pub const LOCK: &str = "zila.lock";

// ---------------------------------------------------------------------------
// The TOML subset
// ---------------------------------------------------------------------------

/// A `[table]` of the subset: its path (`["dependencies", "codec"]`) and its `key = "value"` lines.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub path: Vec<String>,
    pub line: usize,
    pub entries: Vec<(String, String, usize)>,
}

fn unquote(s: &str, line: usize) -> Result<String, String> {
    // `s` starts with a quote; the string must end the text (a comment may follow)
    let mut out = String::new();
    let mut chars = s.chars().skip(1);
    loop {
        match chars.next() {
            None => return Err(format!("line {}: the string is not closed", line)),
            Some('"') => break,
            Some('\\') => match chars.next() {
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(c) => return Err(format!("line {}: the escape '\\{}' is not supported (only \\\" and \\\\)", line, c)),
                None => return Err(format!("line {}: the string is not closed", line)),
            },
            Some(c) => out.push(c),
        }
    }
    let rest: String = chars.collect();
    let rest = rest.trim();
    if !rest.is_empty() && !rest.starts_with('#') {
        return Err(format!("line {}: unexpected text after the string", line));
    }
    Ok(out)
}

fn parse_header(inner: &str, line: usize) -> Result<Vec<String>, String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut chars = inner.trim().chars().peekable();
    loop {
        match chars.next() {
            None => break,
            Some('"') => {
                loop {
                    match chars.next() {
                        None => return Err(format!("line {}: the quoted table name is not closed", line)),
                        Some('"') => break,
                        Some(c) => cur.push(c),
                    }
                }
            }
            Some('.') => {
                parts.push(std::mem::take(&mut cur));
            }
            Some(c) if c.is_ascii_alphanumeric() || c == '_' || c == '-' => cur.push(c),
            Some(c) if c.is_whitespace() => {}
            Some(c) => return Err(format!("line {}: unexpected '{}' in a table name", line, c)),
        }
    }
    parts.push(cur);
    if parts.iter().any(|p| p.is_empty()) {
        return Err(format!("line {}: empty table name", line));
    }
    Ok(parts)
}

/// Parses the subset: `[a]`, `[a.b]`, `[a."quoted.name"]`, `key = "string"`, `#` comments.
pub fn parse_toml(text: &str) -> Result<Vec<Table>, String> {
    let mut tables: Vec<Table> = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = i + 1;
        let t = raw.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if t.starts_with("[[") {
            return Err(format!("line {}: arrays of tables are not supported", line));
        }
        if let Some(rest) = t.strip_prefix('[') {
            let end = rest.rfind(']').ok_or_else(|| format!("line {}: the table header is not closed", line))?;
            let after = rest[end + 1..].trim();
            if !after.is_empty() && !after.starts_with('#') {
                return Err(format!("line {}: unexpected text after the table header", line));
            }
            tables.push(Table { path: parse_header(&rest[..end], line)?, line, entries: Vec::new() });
            continue;
        }
        let Some(eq) = t.find('=') else {
            return Err(format!("line {}: expected `key = \"value\"` or a [table]", line));
        };
        let key = t[..eq].trim();
        if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            return Err(format!("line {}: '{}' is not a valid key (dotted and quoted keys are not supported)", line, key));
        }
        let val = t[eq + 1..].trim();
        let value = if val.starts_with("\"\"\"") || val.starts_with("'''") {
            return Err(format!("line {}: multi-line strings are not supported", line));
        } else if val.starts_with('"') {
            unquote(val, line)?
        } else if val.starts_with('[') {
            return Err(format!("line {}: arrays are not supported (values are strings)", line));
        } else if val.starts_with('{') {
            return Err(format!("line {}: inline tables are not supported (use a [table])", line));
        } else if val.starts_with('\'') {
            return Err(format!("line {}: single-quoted strings are not supported (use double quotes)", line));
        } else {
            return Err(format!("line {}: only string values are supported (write \"{}\")", line, val.split('#').next().unwrap_or("").trim()));
        };
        match tables.last_mut() {
            Some(tb) => {
                if tb.entries.iter().any(|(k, _, _)| k == key) {
                    return Err(format!("line {}: the key '{}' appears twice", line, key));
                }
                tb.entries.push((key.to_string(), value, line));
            }
            None => return Err(format!("line {}: a key must be inside a [table]", line)),
        }
    }
    Ok(tables)
}

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

// ---------------------------------------------------------------------------
// Versions and module paths
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    pub fn parse(s: &str) -> Option<Version> {
        let mut it = s.split('.');
        let mut n = || {
            let p = it.next()?;
            if p.is_empty() || !p.chars().all(|c| c.is_ascii_digit()) || (p.len() > 1 && p.starts_with('0')) {
                return None;
            }
            p.parse::<u32>().ok()
        };
        let v = Version { major: n()?, minor: n()?, patch: n()? };
        if it.next().is_some() {
            return None;
        }
        Some(v)
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// The version of a tag `vMAJOR.MINOR.PATCH` (other tags are not versions).
pub fn parse_tag(tag: &str) -> Option<Version> {
    Version::parse(tag.strip_prefix('v')?)
}

/// The module path of a repository URL: without scheme, user and `.git` suffix.
pub fn module_path(url: &str) -> String {
    let mut s = url.trim().to_string();
    let mut had_scheme = false;
    if let Some(p) = s.find("://") {
        s = s[p + 3..].to_string();
        had_scheme = true;
    }
    if had_scheme {
        // user@host
        if let (Some(at), slash) = (s.find('@'), s.find('/').unwrap_or(s.len())) {
            if at < slash {
                s = s[at + 1..].to_string();
            }
        }
    } else if let Some(at) = s.find('@') {
        // git@host:path
        s = s[at + 1..].to_string();
        if let Some(c) = s.find(':') {
            s.replace_range(c..c + 1, "/");
        }
    }
    let s = s.trim_matches('/').to_string();
    s.strip_suffix(".git").map(|x| x.to_string()).unwrap_or(s)
}

/// The URL to clone for a dependency URL: a `/vN` suffix names a major version of the same
/// repository, it is not part of the repository's address.
pub fn repo_url(url: &str) -> String {
    let t = url.trim_end_matches('/');
    match t.rsplit_once('/') {
        Some((head, last)) if path_major(last).is_some() => head.to_string(),
        _ => url.to_string(),
    }
}

/// The major version a module path stands for: `Some(n)` for a `/vN` suffix (N at least 2),
/// `None` for the base path (versions 0 and 1).
pub fn path_major(module: &str) -> Option<u32> {
    let last = module.rsplit('/').next()?;
    let n = last.strip_prefix('v')?;
    if !n.chars().all(|c| c.is_ascii_digit()) || n.is_empty() {
        return None;
    }
    n.parse::<u32>().ok().filter(|n| *n >= 2)
}

/// Whether tags of major `major` belong to the module path.
pub fn major_matches(module: &str, major: u32) -> bool {
    match path_major(module) {
        Some(n) => n == major,
        None => major <= 1,
    }
}

/// The versions among `tags` that belong to the module path, ascending.
pub fn tag_versions(module: &str, tags: &[String]) -> Vec<Version> {
    let mut v: Vec<Version> = tags.iter().filter_map(|t| parse_tag(t)).filter(|v| major_matches(module, v.major)).collect();
    v.sort();
    v.dedup();
    v
}

/// The default import name of a module path: its last segment, `_vN` for a `/vN` suffix.
pub fn default_import_name(module: &str) -> String {
    let clean = |s: &str| -> String { s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect() };
    let mut segs: Vec<&str> = module.split('/').collect();
    if let Some(n) = path_major(module) {
        segs.pop();
        return format!("{}_v{}", clean(segs.last().copied().unwrap_or("pkg")), n);
    }
    clean(segs.last().copied().unwrap_or("pkg"))
}

// ---------------------------------------------------------------------------
// Names
// ---------------------------------------------------------------------------

/// Names a package cannot take: the core standard library and the built-in `c` and `std`. The
/// other standard libraries may be shadowed by a package (with a warning).
pub fn is_reserved(name: &str) -> bool {
    crate::codegen_x64::is_core_std(name) || matches!(name, "c" | "std")
}

fn is_ident(s: &str) -> bool {
    let mut c = s.chars();
    matches!(c.next(), Some(x) if x.is_ascii_alphabetic() || x == '_') && c.all(|x| x.is_ascii_alphanumeric() || x == '_')
}

// ---------------------------------------------------------------------------
// The manifest
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Dep {
    /// The import name: `use <name>`.
    pub name: String,
    pub git: String,
    /// The minimum version.
    pub version: Version,
}

impl Dep {
    pub fn module(&self) -> String {
        module_path(&self.git)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Manifest {
    pub name: Option<String>,
    pub version: Option<Version>,
    /// Reserved: stored, not interpreted before 1.0.
    pub edition: Option<String>,
    pub deps: Vec<Dep>,
    /// `[policy] forbid`: `unsafe`, `extern` or `asm`; a dependency with that effect is refused.
    pub forbid: Option<String>,
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Manifest, String> {
        let tables = parse_toml(text)?;
        let mut m = Manifest::default();
        let mut seen_package = false;
        for t in &tables {
            let p: Vec<&str> = t.path.iter().map(|s| s.as_str()).collect();
            match p.as_slice() {
                ["package"] => {
                    if seen_package {
                        return Err(format!("line {}: [package] appears twice", t.line));
                    }
                    seen_package = true;
                    for (k, v, line) in &t.entries {
                        match k.as_str() {
                            "name" => m.name = Some(v.clone()),
                            "version" => m.version = Some(Version::parse(v).ok_or_else(|| format!("line {}: '{}' is not a version (MAJOR.MINOR.PATCH)", line, v))?),
                            "edition" => m.edition = Some(v.clone()),
                            other => return Err(format!("line {}: unknown key '{}' in [package] (known: name, version, edition)", line, other)),
                        }
                    }
                }
                ["dependencies", name] => {
                    if !is_ident(name) {
                        return Err(format!("line {}: '{}' is not a valid import name", t.line, name));
                    }
                    if is_reserved(name) {
                        return Err(format!("line {}: the name '{}' belongs to the standard library and cannot name a dependency", t.line, name));
                    }
                    if m.deps.iter().any(|d| d.name == *name) {
                        return Err(format!("line {}: dependency '{}' appears twice", t.line, name));
                    }
                    let (mut git, mut version) = (None, None);
                    for (k, v, line) in &t.entries {
                        match k.as_str() {
                            "git" => git = Some(v.clone()),
                            "version" => version = Some(Version::parse(v).ok_or_else(|| format!("line {}: '{}' is not a version (MAJOR.MINOR.PATCH)", line, v))?),
                            other => return Err(format!("line {}: unknown key '{}' in [dependencies.{}] (known: git, version)", line, other, name)),
                        }
                    }
                    let git = git.ok_or_else(|| format!("line {}: [dependencies.{}] needs `git`", t.line, name))?;
                    let version = version.ok_or_else(|| format!("line {}: [dependencies.{}] needs `version`", t.line, name))?;
                    let dep = Dep { name: name.to_string(), git, version };
                    if !major_matches(&dep.module(), dep.version.major) {
                        return Err(format!("line {}: version {} does not match the module path '{}' (a major version 2 or higher ends the path with /vN)", t.line, dep.version, dep.module()));
                    }
                    m.deps.push(dep);
                }
                ["policy"] => {
                    for (k, v, line) in &t.entries {
                        match k.as_str() {
                            "forbid" => {
                                if !matches!(v.as_str(), "unsafe" | "extern" | "asm") {
                                    return Err(format!("line {}: forbid must be \"unsafe\", \"extern\" or \"asm\" (one value; lists are not supported)", line));
                                }
                                m.forbid = Some(v.clone());
                            }
                            other => return Err(format!("line {}: unknown key '{}' in [policy] (known: forbid)", line, other)),
                        }
                    }
                }
                _ => return Err(format!("line {}: unknown table [{}] (known: package, dependencies.<name>, policy)", t.line, t.path.join("."))),
            }
        }
        Ok(m)
    }

    pub fn write(&self) -> String {
        let mut s = String::from("[package]\n");
        if let Some(n) = &self.name {
            s.push_str(&format!("name = {}\n", quote(n)));
        }
        if let Some(v) = &self.version {
            s.push_str(&format!("version = {}\n", quote(&v.to_string())));
        }
        if let Some(e) = &self.edition {
            s.push_str(&format!("edition = {}\n", quote(e)));
        }
        if let Some(f) = &self.forbid {
            s.push_str(&format!("\n[policy]\nforbid = {}\n", quote(f)));
        }
        for d in &self.deps {
            s.push_str(&format!("\n[dependencies.{}]\ngit = {}\nversion = {}\n", d.name, quote(&d.git), quote(&d.version.to_string())));
        }
        s
    }
}

// ---------------------------------------------------------------------------
// The lock file
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct LockEntry {
    pub module: String,
    pub version: Version,
    pub commit: String,
    /// `sha256:<hex>` of the tree.
    pub hash: String,
    /// `<module>@<version>` of the packages this one needs (informational).
    pub needs: Vec<String>,
}

impl LockEntry {
    pub fn id(&self) -> String {
        format!("{}@{}", self.module, self.version)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Lock {
    pub entries: Vec<LockEntry>,
}

impl Lock {
    pub fn parse(text: &str) -> Result<Lock, String> {
        let tables = parse_toml(text)?;
        let mut lock = Lock::default();
        for t in &tables {
            if t.path.len() != 2 || t.path[0] != "package" {
                return Err(format!("line {}: unknown table [{}] in the lock file", t.line, t.path.join(".")));
            }
            let id = &t.path[1];
            let (module, ver) = id.rsplit_once('@').ok_or_else(|| format!("line {}: '{}' is not <module path>@<version>", t.line, id))?;
            let version = Version::parse(ver).ok_or_else(|| format!("line {}: '{}' is not a version", t.line, ver))?;
            let (mut commit, mut hash, mut needs) = (None, None, Vec::new());
            for (k, v, line) in &t.entries {
                match k.as_str() {
                    "commit" => commit = Some(v.clone()),
                    "hash" => hash = Some(v.clone()),
                    "needs" => needs = v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
                    other => return Err(format!("line {}: unknown key '{}' in the lock file", line, other)),
                }
            }
            let commit = commit.ok_or_else(|| format!("line {}: the lock entry '{}' has no commit", t.line, id))?;
            if commit.len() != 40 || !commit.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(format!("line {}: '{}' is not a full commit", t.line, commit));
            }
            let hash = hash.ok_or_else(|| format!("line {}: the lock entry '{}' has no hash", t.line, id))?;
            lock.entries.push(LockEntry { module: module.to_string(), version, commit, hash, needs });
        }
        Ok(lock)
    }

    pub fn write(&self) -> String {
        let mut entries = self.entries.clone();
        entries.sort_by(|a, b| (a.module.as_str(), a.version).cmp(&(b.module.as_str(), b.version)));
        let mut s = String::from("# zila.lock: written by zila; commit it, do not edit it\n");
        for e in &entries {
            let mut needs = e.needs.clone();
            needs.sort();
            s.push_str(&format!("\n[package.{}]\ncommit = {}\nhash = {}\n", quote(&e.id()), quote(&e.commit), quote(&e.hash)));
            if !needs.is_empty() {
                s.push_str(&format!("needs = {}\n", quote(&needs.join(", "))));
            }
        }
        s
    }

    pub fn find(&self, module: &str, version: &Version) -> Option<&LockEntry> {
        self.entries.iter().find(|e| e.module == module && e.version == *version)
    }

    /// The entry for `module` (there is one version of a module in a lock).
    pub fn for_module(&self, module: &str) -> Option<&LockEntry> {
        self.entries.iter().find(|e| e.module == module)
    }
}

// ---------------------------------------------------------------------------
// Minimal version selection
// ---------------------------------------------------------------------------

/// One requirement: the import name used by the requiring manifest, the repository and the minimum.
#[derive(Debug, Clone, PartialEq)]
pub struct Req {
    pub name: String,
    pub module: String,
    pub url: String,
    pub version: Version,
}

impl From<&Dep> for Req {
    fn from(d: &Dep) -> Req {
        Req { name: d.name.clone(), module: d.module(), url: d.git.clone(), version: d.version }
    }
}

/// The selected packages: module path to version and URL.
pub type Selection = BTreeMap<String, (Version, String)>;

fn raise(sel: &mut Selection, r: &Req) {
    match sel.get(&r.module) {
        Some((v, _)) if *v >= r.version => {}
        _ => {
            sel.insert(r.module.clone(), (r.version, r.url.clone()));
        }
    }
}

/// Minimal version selection: for each module the highest of the minimums required by the root
/// and by the selected packages' own requirements (given by `reqs_of`). The result depends only on
/// the manifests, not on the order of the requirements.
pub fn select<F>(root: &[Req], mut reqs_of: F) -> Result<Selection, String>
where
    F: FnMut(&str, &Version, &str) -> Result<Vec<Req>, String>,
{
    let mut memo: HashMap<(String, Version), Vec<Req>> = HashMap::new();
    let mut cur = Selection::new();
    for r in root {
        raise(&mut cur, r);
    }
    for _ in 0..1000 {
        let mut next = Selection::new();
        for r in root {
            raise(&mut next, r);
        }
        for (m, (v, url)) in cur.clone() {
            let reqs = match memo.get(&(m.clone(), v)) {
                Some(r) => r.clone(),
                None => {
                    let r = reqs_of(&m, &v, &url)?;
                    memo.insert((m.clone(), v), r.clone());
                    r
                }
            };
            for r in &reqs {
                raise(&mut next, r);
            }
        }
        // keep what was selected through packages that are still selected
        let mut merged = next.clone();
        for (m, (v, url)) in cur.iter() {
            if let Some((nv, _)) = next.get(m) {
                if nv < v {
                    merged.insert(m.clone(), (*v, url.clone()));
                }
            }
        }
        if merged == cur {
            return Ok(cur);
        }
        cur = merged;
    }
    Err("version selection does not settle (the requirements form a cycle of increases)".into())
}

// ---------------------------------------------------------------------------
// SHA-256 and the tree hash
// ---------------------------------------------------------------------------

pub fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
        0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
        0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([chunk[4 * i], chunk[4 * i + 1], chunk[4 * i + 2], chunk[4 * i + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [t1.wrapping_add(t2), v[0], v[1], v[2], v[3].wrapping_add(t1), v[4], v[5], v[6]];
        }
        for i in 0..8 {
            h[i] = h[i].wrapping_add(v[i]);
        }
    }
    let mut out = [0u8; 32];
    for i in 0..8 {
        out[4 * i..4 * i + 4].copy_from_slice(&h[i].to_be_bytes());
    }
    out
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn collect_files(base: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) -> Result<(), String> {
    let rd = std::fs::read_dir(dir).map_err(|e| format!("cannot read {}: {}", dir.display(), e))?;
    for entry in rd {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let ft = entry.file_type().map_err(|e| e.to_string())?;
        if ft.is_dir() {
            if name != ".git" {
                collect_files(base, &path, out)?;
            }
        } else if ft.is_file() {
            let rel = path.strip_prefix(base).unwrap_or(&path).to_string_lossy().replace('\\', "/");
            out.push((rel, path));
        }
    }
    Ok(())
}

/// `sha256:<hex>` over the files of `dir` sorted by path: the path, a zero byte, the decimal
/// length, a zero byte and the contents of each (`.git` excluded).
pub fn tree_hash(dir: &Path) -> Result<String, String> {
    let mut files = Vec::new();
    collect_files(dir, dir, &mut files)?;
    files.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    let mut buf: Vec<u8> = Vec::new();
    for (rel, path) in files {
        let data = std::fs::read(&path).map_err(|e| format!("cannot read {}: {}", path.display(), e))?;
        buf.extend_from_slice(rel.as_bytes());
        buf.push(0);
        buf.extend_from_slice(data.len().to_string().as_bytes());
        buf.push(0);
        buf.extend_from_slice(&data);
    }
    Ok(format!("sha256:{}", hex(&sha256(&buf))))
}

// ---------------------------------------------------------------------------
// The cache
// ---------------------------------------------------------------------------

/// `$ZILA_HOME`, else `%USERPROFILE%\.zila` on Windows and `$HOME/.zila` elsewhere.
pub fn cache_root() -> PathBuf {
    if let Some(h) = std::env::var_os("ZILA_HOME") {
        if !h.is_empty() {
            return PathBuf::from(h);
        }
    }
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    match std::env::var_os(var) {
        Some(h) => PathBuf::from(h).join(".zila"),
        None => PathBuf::from(".zila"),
    }
}

fn dir_name(module: &str) -> String {
    module.trim_start_matches('/').replace(':', "_")
}

/// The directory of one package version and commit in the cache.
pub fn cache_dir(home: &Path, module: &str, version: &Version, commit: &str) -> PathBuf {
    let short: String = commit.chars().take(12).collect();
    home.join("src").join(format!("{}@{}-{}", dir_name(module), version, short))
}

/// Moves a finished temporary directory into place. Writes go to a sibling that is renamed, so an
/// interrupted fetch never leaves a half package.
pub fn cache_store(home: &Path, module: &str, version: &Version, commit: &str, tmp: &Path) -> Result<PathBuf, String> {
    let dest = cache_dir(home, module, version, commit);
    if dest.exists() {
        let _ = std::fs::remove_dir_all(tmp);
        return Ok(dest);
    }
    if let Some(p) = dest.parent() {
        std::fs::create_dir_all(p).map_err(|e| format!("cannot create {}: {}", p.display(), e))?;
    }
    std::fs::rename(tmp, &dest).map_err(|e| format!("cannot move the package into {}: {}", dest.display(), e))?;
    Ok(dest)
}

/// A fresh temporary directory next to where the package will live (never found by lookups).
pub fn cache_tmp(home: &Path, module: &str, version: &Version, commit: &str) -> Result<PathBuf, String> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static N: AtomicUsize = AtomicUsize::new(0);
    let dest = cache_dir(home, module, version, commit);
    let parent = dest.parent().ok_or("bad cache path")?.to_path_buf();
    std::fs::create_dir_all(&parent).map_err(|e| format!("cannot create {}: {}", parent.display(), e))?;
    let name = dest.file_name().unwrap().to_string_lossy().to_string();
    let tmp = parent.join(format!(".tmp-{}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst), name));
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(tmp)
}

/// The cache directory of a package version if it exists.
pub fn cache_lookup(home: &Path, module: &str, version: &Version, commit: &str) -> Option<PathBuf> {
    let d = cache_dir(home, module, version, commit);
    d.is_dir().then_some(d)
}

// ---------------------------------------------------------------------------
// Projects
// ---------------------------------------------------------------------------

fn clean_path(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy().to_string();
    match s.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => p,
    }
}

/// The directory that holds `zila.toml`, searching upward from `start`.
pub fn find_manifest_dir(start: &Path) -> Option<PathBuf> {
    let abs = if start.is_absolute() { start.to_path_buf() } else { std::env::current_dir().ok()?.join(start) };
    let mut dir = clean_path(abs.canonicalize().unwrap_or(abs));
    loop {
        if dir.join(MANIFEST).is_file() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

pub fn read_manifest(dir: &Path) -> Result<Manifest, String> {
    let p = dir.join(MANIFEST);
    let text = std::fs::read_to_string(&p).map_err(|e| format!("cannot read {}: {}", p.display(), e))?;
    Manifest::parse(&text).map_err(|e| format!("{}: {}", p.display(), e))
}

pub fn read_lock(dir: &Path) -> Result<Option<Lock>, String> {
    let p = dir.join(LOCK);
    if !p.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&p).map_err(|e| format!("cannot read {}: {}", p.display(), e))?;
    Lock::parse(&text).map(Some).map_err(|e| format!("{}: {}", p.display(), e))
}

/// A project loaded for a build: its manifest and lock, and where each locked package is.
#[derive(Debug, Clone)]
pub struct Project {
    pub root: PathBuf,
    pub manifest: Manifest,
    pub lock: Lock,
    /// Cache directory and manifest of every locked package, by module path.
    pub packages: BTreeMap<String, (PathBuf, Manifest)>,
}

const OUT_OF_DATE: &str = "zila.lock is out of date; run zila --fetch";

fn covers(lock: &Lock, dep_module: &str, min: &Version) -> bool {
    lock.for_module(dep_module).is_some_and(|e| e.version >= *min)
}

/// The resolver entry point: finds the manifest above `source`, reads the lock, checks that it
/// covers the manifests and that every cached package matches its hash. Never runs git and never
/// touches the network. `Ok(None)` when no manifest is above the source.
pub fn resolve_project(source: &Path) -> Result<Option<Project>, String> {
    resolve_project_in(source, &cache_root())
}

pub fn resolve_project_in(source: &Path, home: &Path) -> Result<Option<Project>, String> {
    let start = source.parent().map(|p| if p.as_os_str().is_empty() { Path::new(".") } else { p }).unwrap_or(Path::new("."));
    let Some(root) = find_manifest_dir(start) else { return Ok(None) };
    load_project(&root, home).map(Some)
}

pub fn load_project(root: &Path, home: &Path) -> Result<Project, String> {
    let manifest = read_manifest(root)?;
    let lock = if manifest.deps.is_empty() {
        read_lock(root)?.unwrap_or_default()
    } else {
        read_lock(root)?.ok_or_else(|| OUT_OF_DATE.to_string())?
    };
    for d in &manifest.deps {
        if !covers(&lock, &d.module(), &d.version) {
            return Err(OUT_OF_DATE.to_string());
        }
    }
    let mut packages = BTreeMap::new();
    for e in &lock.entries {
        let id = e.id();
        let dir = cache_lookup(home, &e.module, &e.version, &e.commit).ok_or_else(|| format!("package '{}' is not in the cache; run zila --fetch", id))?;
        let found = tree_hash(&dir)?;
        if found != e.hash {
            return Err(format!("package '{}' in the cache does not match zila.lock (expected {}, found {}); run zila --fetch to restore it", id, e.hash, found));
        }
        if let Some(kind) = &manifest.forbid {
            if let Some(file) = crate::effects::first_with(&dir, kind) {
                return Err(format!("package '{}' uses `{}` (in {}), which zila.toml forbids with [policy] forbid = \"{}\"", id, kind, file, kind));
            }
        }
        let m = if dir.join(MANIFEST).is_file() { read_manifest(&dir)? } else { Manifest::default() };
        packages.insert(e.module.clone(), (dir, m));
    }
    for (_, m) in packages.values() {
        for d in &m.deps {
            if !covers(&lock, &d.module(), &d.version) {
                return Err(OUT_OF_DATE.to_string());
            }
        }
    }
    Ok(Project { root: root.to_path_buf(), manifest, lock, packages })
}

// ---------------------------------------------------------------------------
// Resolution of `use`
// ---------------------------------------------------------------------------

/// The `.zila` files directly in `dir`, sorted, with the `_test.zila` files apart.
fn br_files(dir: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let (mut files, mut tests) = (Vec::new(), Vec::new());
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_file() || p.extension().and_then(|x| x.to_str()) != Some("zila") {
                continue;
            }
            if p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with("_test.zila")) {
                tests.push(p);
            } else {
                files.push(p);
            }
        }
    }
    files.sort();
    tests.sort();
    (files, tests)
}

fn has_br(dir: &Path) -> bool {
    let (f, t) = br_files(dir);
    !f.is_empty() || !t.is_empty()
}

fn file_label(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
}

/// One file split into its `package` name, `use` lines and body.
struct Split {
    package: Option<String>,
    uses: Vec<String>,
    body: Vec<String>,
}

fn split_file(text: &str) -> Split {
    let mut s = Split { package: None, uses: Vec::new(), body: Vec::new() };
    let mut header = true;
    for line in text.lines() {
        let t = line.trim_end();
        if header {
            if t.trim().is_empty() || t.trim_start().starts_with("//") {
                s.body.push(line.to_string());
                continue;
            }
            if let Some(p) = t.strip_prefix("package ") {
                s.package = Some(p.trim().to_string());
                s.body.push(String::new());
                continue;
            }
            if t.starts_with("use ") {
                s.uses.push(t.to_string());
                s.body.push(String::new());
                continue;
            }
            header = false;
        }
        s.body.push(line.to_string());
    }
    s
}

/// The merged text of a package directory and its files (for the duplicate check). `extra` lists
/// files (tests) added to the module.
fn merge_text(dir: &Path, expected: &str, extra: &[PathBuf]) -> Result<(String, Vec<(String, String)>), String> {
    let (mut files, _) = br_files(dir);
    files.extend(extra.iter().cloned());
    if files.is_empty() {
        return Err(format!("package '{}' has no .zila files in {}", expected, dir.display()));
    }
    let mut uses: Vec<String> = Vec::new();
    let mut bodies = String::new();
    let mut parts = Vec::new();
    for f in &files {
        let text = std::fs::read_to_string(f).map_err(|e| format!("cannot read {}: {}", f.display(), e))?;
        let sp = split_file(&text);
        match &sp.package {
            Some(p) if p == expected => {}
            Some(p) => return Err(format!("{}: the file says `package {}` but the package is '{}'", f.display(), p, expected)),
            None => return Err(format!("{}: the file has no `package {}` line", f.display(), expected)),
        }
        for u in &sp.uses {
            if !uses.contains(u) {
                uses.push(u.clone());
            }
        }
        let body = sp.body.join("\n");
        parts.push((file_label(f), body.clone()));
        bodies.push_str(&body);
        bodies.push('\n');
    }
    let mut out = format!("package {}\n", expected);
    for u in &uses {
        out.push_str(u);
        out.push('\n');
    }
    out.push_str(&bodies);
    Ok((out, parts))
}

/// A name declared at the top level of a file, for the duplicate check.
fn declared_names(file: &File) -> Vec<String> {
    let mut v = Vec::new();
    for d in &file.decls {
        match &d.kind {
            DeclKind::Func(f) => match &f.receiver {
                Some(r) => v.push(format!("{:?}.{}", r.ty, f.name)),
                None => v.push(f.name.clone()),
            },
            DeclKind::Type(t) => v.push(t.name.clone()),
            DeclKind::Var(x) => v.push(x.name.clone()),
            DeclKind::Extern(x) => v.push(x.name.clone()),
            DeclKind::Embed(_) => {}
        }
    }
    v
}

fn check_duplicates(parts: &[(String, String)], expected: &str) -> Result<(), String> {
    let mut seen: HashMap<String, String> = HashMap::new();
    for (label, body) in parts {
        let text = format!("package {}\n{}", expected, body);
        let file = match crate::parser::parse_source(&text) {
            Ok(f) => f,
            Err(_) => continue, // the compiler reports the syntax error with its own position
        };
        for n in declared_names(&file) {
            if let Some(first) = seen.get(&n) {
                return Err(format!("'{}' is declared twice in package '{}': in {} and in {}", n.rsplit('.').next().unwrap_or(&n), expected, first, label));
            }
            seen.insert(n, label.clone());
        }
    }
    Ok(())
}

#[derive(Clone)]
struct PkgMeta {
    /// `None` for the project itself, else the module path of the dependency it belongs to.
    scope: Option<String>,
    dir: PathBuf,
    expected: String,
    /// Parts for the duplicate check, once validated.
    validated: bool,
}

#[derive(Default)]
struct State {
    /// Directory (identity) to module key.
    keys: HashMap<String, String>,
    used: HashSet<String>,
    metas: HashMap<String, PkgMeta>,
    sources: HashMap<String, &'static str>,
    /// Qualifier a module writes, to the key of the package it means: `(importer, qualifier)`.
    aliases: HashMap<(String, String), String>,
}

struct Registry {
    project: Project,
    state: Mutex<State>,
}

// one per thread: a command line run, a test or a language-server request installs its project
// and compiles on the same thread, and concurrent compilations (tests) do not see each other's
thread_local! {
    static REGISTRY: std::cell::RefCell<Option<Arc<Registry>>> = const { std::cell::RefCell::new(None) };
}

/// Makes `project` the one `use` resolves against.
pub fn install(project: Project) -> Result<(), String> {
    check_local_names(&project)?;
    REGISTRY.with(|r| *r.borrow_mut() = Some(Arc::new(Registry { project, state: Mutex::new(State::default()) })));
    Ok(())
}

/// Forgets the installed project (a build without a manifest sees the standard library only).
pub fn clear() {
    REGISTRY.with(|r| *r.borrow_mut() = None);
}

/// Finds the project of `source`, checks it and installs it (or clears the previous one).
pub fn install_for(source: &Path) -> Result<(), String> {
    match resolve_project(source)? {
        Some(p) => install(p),
        None => {
            clear();
            Ok(())
        }
    }
}

fn check_local_names(p: &Project) -> Result<(), String> {
    if let Ok(rd) = std::fs::read_dir(&p.root) {
        for e in rd.flatten() {
            let path = e.path();
            if !path.is_dir() {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || !has_br(&path) {
                continue;
            }
            if is_reserved(&name) {
                return Err(format!("the local package '{}' has the name of a core standard library package; rename the directory", name));
            }
            if p.manifest.deps.iter().any(|d| d.name == name) {
                return Err(format!("'{}' is both a dependency in {} and a directory with .zila files; a name can exist in one place only", name, MANIFEST));
            }
        }
    }
    Ok(())
}

fn registry() -> Option<Arc<Registry>> {
    REGISTRY.with(|r| r.borrow().clone())
}

/// A package the compiler asked for.
pub struct Resolved {
    pub key: String,
    pub source: &'static str,
}

fn unknown(name: &str) -> String {
    format!("unknown package '{}' (not in the standard library, {} or the project directory); to add one: zila --add <git-url>", name, MANIFEST)
}

impl Registry {
    /// The directory of `segs` as seen from a package of `scope`, with its scope and import name.
    fn locate(&self, scope: &Option<String>, scope_dir: &Path, segs: &[String]) -> Result<Option<(PathBuf, Option<String>, bool)>, String> {
        let first = &segs[0];
        let (mut dir, new_scope, is_root) = match scope {
            None => match self.project.manifest.deps.iter().find(|d| d.name == *first) {
                Some(d) => {
                    let m = d.module();
                    let (dir, _) = self.project.packages.get(&m).ok_or_else(|| format!("package '{}' is not in the cache; run zila --fetch", m))?;
                    (dir.clone(), Some(m), true)
                }
                None => {
                    let d = self.project.root.join(first);
                    if d.is_dir() && has_br(&d) {
                        (d, None, false)
                    } else {
                        return Ok(None);
                    }
                }
            },
            Some(module) => {
                let (_, m) = self.project.packages.get(module).ok_or_else(|| unknown(first))?;
                match m.deps.iter().find(|d| d.name == *first) {
                    Some(d) => {
                        let m2 = d.module();
                        let lock_ver = self.project.lock.for_module(&m2).ok_or_else(|| OUT_OF_DATE.to_string())?;
                        let _ = lock_ver;
                        let (dir, _) = self.project.packages.get(&m2).ok_or_else(|| format!("package '{}' is not in the cache; run zila --fetch", m2))?;
                        (dir.clone(), Some(m2), true)
                    }
                    None => {
                        let d = scope_dir.join(first);
                        if d.is_dir() && has_br(&d) {
                            (d, scope.clone(), false)
                        } else {
                            return Ok(None);
                        }
                    }
                }
            }
        };
        for s in &segs[1..] {
            dir = dir.join(s);
            if !dir.is_dir() || !has_br(&dir) {
                return Ok(None);
            }
        }
        Ok(Some((dir, new_scope, is_root && segs.len() == 1)))
    }

    /// Finds the package of `segs` as seen from `importer` and gives it its key.
    fn intern(&self, importer: &str, segs: &[String]) -> Result<Option<(String, String)>, String> {
        let (scope, scope_dir) = {
            let st = self.state.lock().unwrap();
            match st.metas.get(importer) {
                Some(m) => (
                    m.scope.clone(),
                    match &m.scope {
                        None => self.project.root.clone(),
                        Some(module) => self.project.packages.get(module).map(|p| p.0.clone()).unwrap_or_else(|| m.dir.clone()),
                    },
                ),
                None => (None, self.project.root.clone()),
            }
        };
        let Some((dir, new_scope, is_dep_root)) = self.locate(&scope, &scope_dir, segs)? else { return Ok(None) };
        let last = segs.last().unwrap().clone();
        let expected = if is_dep_root {
            let m = new_scope.clone().unwrap();
            let base = if path_major(&m).is_some() { m.rsplit_once('/').map_or(m.clone(), |x| x.0.to_string()) } else { m.clone() };
            let named = self.project.packages.get(&m).and_then(|p| p.1.name.clone()).unwrap_or(base);
            named.rsplit('/').next().unwrap_or("pkg").to_string()
        } else {
            dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| last.clone())
        };
        let identity = dir.to_string_lossy().to_string();
        let mut st = self.state.lock().unwrap();
        let key = match st.keys.get(&identity) {
            Some(k) => k.clone(),
            None => {
                let mut k = last.clone();
                let mut n = 1;
                while st.used.contains(&k) || k == "main" || crate::codegen_x64::std_source(&k).is_some() {
                    n += 1;
                    k = format!("{}__{}", last, n);
                }
                st.used.insert(k.clone());
                st.keys.insert(identity.clone(), k.clone());
                st.metas.insert(k.clone(), PkgMeta { scope: new_scope.clone(), dir: dir.clone(), expected: expected.clone(), validated: false });
                k
            }
        };
        if key != last {
            st.aliases.insert((importer.to_string(), last), key.clone());
        }
        Ok(Some((key, expected)))
    }

    fn resolve(&self, importer: &str, segs: &[String]) -> Result<Option<Resolved>, String> {
        let Some((key, expected)) = self.intern(importer, segs)? else { return Ok(None) };
        let source = self.source_of(&key)?;
        self.validate(&key, &expected)?;
        Ok(Some(Resolved { key, source }))
    }

    /// The merged source of a package key (not checked for duplicates).
    fn source_of(&self, key: &str) -> Result<&'static str, String> {
        let meta = {
            let st = self.state.lock().unwrap();
            if let Some(s) = st.sources.get(key) {
                return Ok(s);
            }
            st.metas.get(key).cloned()
        };
        let meta = meta.ok_or_else(|| unknown(key))?;
        let (text, _) = merge_text(&meta.dir, &meta.expected, &test_files_of(&meta.dir))?;
        let leaked: &'static str = Box::leak(text.into_boxed_str());
        self.state.lock().unwrap().sources.insert(key.to_string(), leaked);
        Ok(leaked)
    }

    /// Once per package: no name declared twice across its files.
    fn validate(&self, key: &str, expected: &str) -> Result<(), String> {
        let meta = {
            let st = self.state.lock().unwrap();
            st.metas.get(key).cloned()
        };
        let Some(meta) = meta else { return Ok(()) };
        if meta.validated {
            return Ok(());
        }
        let (_, parts) = merge_text(&meta.dir, &meta.expected, &test_files_of(&meta.dir))?;
        check_duplicates(&parts, expected)?;
        if let Some(m) = self.state.lock().unwrap().metas.get_mut(key) {
            m.validated = true;
        }
        Ok(())
    }
}

fn test_files_of(dir: &Path) -> Vec<PathBuf> {
    TEST_FILES.lock().unwrap().as_ref().and_then(|m| m.get(&dir.to_string_lossy().to_string())).cloned().unwrap_or_default()
}

/// `_test.zila` files to include in a package (set by `--test` for the package under test).
static TEST_FILES: Mutex<Option<HashMap<String, Vec<PathBuf>>>> = Mutex::new(None);

/// Resolves `use <segs...>` from the module `importer` against the installed project.
/// `Ok(None)` when there is no project or the name is not one of its packages.
pub fn resolve_use(importer: &str, segs: &[String]) -> Result<Option<Resolved>, String> {
    match registry() {
        Some(r) => r.resolve(importer, segs),
        None => Ok(None),
    }
}

/// The key of the package a module's qualifier means, when it differs from the qualifier.
pub fn alias_of(importer: &str, qualifier: &str) -> Option<String> {
    let r = registry()?;
    let st = r.state.lock().unwrap();
    st.aliases.get(&(importer.to_string(), qualifier.to_string())).cloned()
}

/// The source of a package by the name the project gives it, without validating it (used to find
/// the block forms and generators of imported packages).
pub fn visible_source(name: &str) -> Option<&'static str> {
    let r = registry()?;
    let segs = vec![name.to_string()];
    let (key, _) = r.intern("", &segs).ok().flatten()?;
    r.source_of(&key).ok()
}

/// Makes the `_test.zila` files of the package in `dir` part of it (for `--test`).
pub fn include_tests(dir: &Path, files: Vec<PathBuf>) {
    TEST_FILES.lock().unwrap().get_or_insert_with(HashMap::new).insert(dir.to_string_lossy().to_string(), files);
}

/// The source of a directory merged as a package called `package` (`--test` uses it to build a
/// package together with its test file).
pub fn merged_dir_source(dir: &Path, package: &str, extra: &[PathBuf]) -> Result<String, String> {
    merge_text(dir, package, extra).map(|x| x.0)
}

/// The `.zila` files under `root` (for `--tidy`), skipping hidden directories.
pub fn all_sources(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                let name = e.file_name().to_string_lossy().to_string();
                if p.is_dir() {
                    if !name.starts_with('.') && name != "target" {
                        walk(&p, out);
                    }
                } else if p.extension().and_then(|x| x.to_str()) == Some("zila") {
                    out.push(p);
                }
            }
        }
    }
    let mut v = Vec::new();
    walk(root, &mut v);
    v.sort();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml_subset_round_trip() {
        let m = Manifest::parse("# c\n[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2027\"\n\n[dependencies.codec]\ngit = \"https://github.com/x/codec\"\nversion = \"1.2.0\"\n").unwrap();
        assert_eq!(m.edition.as_deref(), Some("2027"));
        assert_eq!(m.deps.len(), 1);
        assert_eq!(Manifest::parse(&m.write()).unwrap(), m);
    }

    #[test]
    fn toml_rejects_unsupported_constructs() {
        let e = Manifest::parse("[package]\nname = \"a\"\nnames = [\"a\", \"b\"]\n").unwrap_err();
        assert!(e.contains("line 3") && e.contains("arrays are not supported"), "{}", e);
        assert!(Manifest::parse("[package]\nx = { a = \"b\" }\n").unwrap_err().contains("inline tables"));
        assert!(Manifest::parse("[package]\nx = \"\"\"a\"\"\"\n").unwrap_err().contains("multi-line"));
        assert!(Manifest::parse("[package]\nfoo = \"a\"\n").unwrap_err().contains("unknown key 'foo'"));
        assert!(Manifest::parse("[package]\nversion = \"1.x\"\n").unwrap_err().contains("not a version"));
        assert!(Manifest::parse("[package]\nversion = 3\n").unwrap_err().contains("only string values"));
        assert!(Manifest::parse("[dependencies.text]\ngit = \"u\"\nversion = \"1.0.0\"\n").unwrap_err().contains("standard library"));
        assert!(Manifest::parse("[[a]]\n").unwrap_err().contains("arrays of tables"));
    }

    #[test]
    fn lock_round_trip() {
        let l = Lock { entries: vec![LockEntry { module: "github.com/x/codec".into(), version: Version::parse("1.2.0").unwrap(), commit: "a".repeat(40), hash: "sha256:00".into(), needs: vec!["b@1.0.0".into()] }] };
        assert_eq!(Lock::parse(&l.write()).unwrap(), l);
    }

    #[test]
    fn versions_and_tags() {
        assert!(Version::parse("1.2.3").unwrap() < Version::parse("1.10.0").unwrap());
        assert!(Version::parse("1.2").is_none() && Version::parse("01.2.3").is_none() && Version::parse("1.2.3.4").is_none());
        assert_eq!(parse_tag("v1.2.3"), Version::parse("1.2.3"));
        assert!(parse_tag("1.2.3").is_none() && parse_tag("v1.2.3-rc1").is_none());
    }

    #[test]
    fn module_paths() {
        assert_eq!(module_path("https://github.com/someone/codec"), "github.com/someone/codec");
        assert_eq!(module_path("https://user@github.com/someone/codec.git"), "github.com/someone/codec");
        assert_eq!(module_path("ssh://git@github.com/someone/codec.git"), "github.com/someone/codec");
        assert_eq!(module_path("git@github.com:someone/codec.git"), "github.com/someone/codec");
        assert_eq!(module_path("file:///tmp/repos/codec"), "tmp/repos/codec");
        assert_eq!(path_major("github.com/x/codec/v2"), Some(2));
        assert_eq!(path_major("github.com/x/codec"), None);
        assert_eq!(default_import_name("github.com/x/codec/v2"), "codec_v2");
        assert_eq!(default_import_name("github.com/x/codec"), "codec");
    }

    #[test]
    fn tags_of_the_wrong_major_are_ignored() {
        let tags: Vec<String> = ["v1.0.0", "v1.4.0", "v2.0.0", "v2.1.0", "v0.9.0", "nightly", "v1.5.0-rc1"].iter().map(|s| s.to_string()).collect();
        let base = tag_versions("github.com/x/codec", &tags);
        assert_eq!(base.iter().map(|v| v.to_string()).collect::<Vec<_>>(), ["0.9.0", "1.0.0", "1.4.0"]);
        let v2 = tag_versions("github.com/x/codec/v2", &tags);
        assert_eq!(v2.iter().map(|v| v.to_string()).collect::<Vec<_>>(), ["2.0.0", "2.1.0"]);
    }

    fn req(name: &str, module: &str, v: &str) -> Req {
        Req { name: name.into(), module: module.into(), url: format!("u/{}", module), version: Version::parse(v).unwrap() }
    }

    fn graph(entries: &[(&str, &str, Vec<Req>)]) -> HashMap<(String, Version), Vec<Req>> {
        entries.iter().map(|(m, v, r)| ((m.to_string(), Version::parse(v).unwrap()), r.clone())).collect()
    }

    fn run(root: &[Req], g: &HashMap<(String, Version), Vec<Req>>) -> Vec<String> {
        select(root, |m, v, _| Ok(g.get(&(m.to_string(), *v)).cloned().unwrap_or_default()))
            .unwrap()
            .iter()
            .map(|(m, (v, _))| format!("{}@{}", m, v))
            .collect()
    }

    #[test]
    fn two_requirements_pick_the_highest_minimum() {
        let g = graph(&[("a", "1.0.0", vec![req("lib", "lib", "1.4.1")])]);
        assert_eq!(run(&[req("a", "a", "1.0.0"), req("lib", "lib", "1.2.0")], &g), ["a@1.0.0", "lib@1.4.1"]);
    }

    #[test]
    fn diamond_and_majors() {
        let g = graph(&[
            ("a", "1.0.0", vec![req("c", "c", "1.1.0")]),
            ("b", "1.0.0", vec![req("c", "c", "1.3.0")]),
            ("c", "1.3.0", vec![req("d", "d/v2", "2.0.0")]),
        ]);
        let out = run(&[req("a", "a", "1.0.0"), req("b", "b", "1.0.0"), req("d", "d", "1.0.0")], &g);
        assert_eq!(out, ["a@1.0.0", "b@1.0.0", "c@1.3.0", "d@1.0.0", "d/v2@2.0.0"]);
    }

    #[test]
    fn selection_does_not_depend_on_order() {
        let g = graph(&[("a", "1.0.0", vec![req("l", "l", "1.4.0")]), ("b", "1.0.0", vec![req("l", "l", "1.2.0")])]);
        let r1 = run(&[req("a", "a", "1.0.0"), req("b", "b", "1.0.0")], &g);
        let r2 = run(&[req("b", "b", "1.0.0"), req("a", "a", "1.0.0")], &g);
        assert_eq!(r1, r2);
        assert!(r1.contains(&"l@1.4.0".to_string()));
    }

    #[test]
    fn sha256_vectors() {
        assert_eq!(hex(&sha256(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(hex(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(hex(&sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")), "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1");
        assert_eq!(hex(&sha256(&vec![b'a'; 1000])), "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3");
    }

    fn scratch(name: &str) -> PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let d = std::env::temp_dir().join(format!("zila_pkg_unit_{}_{}_{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst), name));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn tree_hash_ignores_listing_order_and_sees_edits() {
        let (a, b) = (scratch("ha"), scratch("hb"));
        for (d, order) in [(&a, ["x.zila", "sub/y.zila"]), (&b, ["sub/y.zila", "x.zila"])] {
            for f in order {
                let p = d.join(f);
                std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                std::fs::write(p, format!("content of {}", f)).unwrap();
            }
        }
        let h = tree_hash(&a).unwrap();
        assert_eq!(h, tree_hash(&b).unwrap());
        std::fs::create_dir_all(a.join(".git")).unwrap();
        std::fs::write(a.join(".git/ignored"), "x").unwrap();
        assert_eq!(h, tree_hash(&a).unwrap());
        std::fs::write(a.join("x.zila"), "content of x.bR").unwrap();
        assert_ne!(h, tree_hash(&a).unwrap());
        std::fs::write(a.join("x.zila"), "content of x.zila").unwrap();
        std::fs::rename(a.join("x.zila"), a.join("z.zila")).unwrap();
        assert_ne!(h, tree_hash(&a).unwrap());
    }

    #[test]
    fn cache_writes_are_atomic() {
        let home = scratch("home");
        let v = Version::parse("1.0.0").unwrap();
        let commit = "b".repeat(40);
        let tmp = cache_tmp(&home, "github.com/x/codec", &v, &commit).unwrap();
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("a.zila"), "x").unwrap();
        // interrupted: the temporary directory exists, the entry does not
        assert!(cache_lookup(&home, "github.com/x/codec", &v, &commit).is_none());
        let dest = cache_store(&home, "github.com/x/codec", &v, &commit, &tmp).unwrap();
        assert!(dest.join("a.zila").is_file());
        assert_eq!(cache_lookup(&home, "github.com/x/codec", &v, &commit), Some(dest));
    }
}
