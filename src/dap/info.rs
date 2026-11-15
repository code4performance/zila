//! The debug-information file (`.zdbg`, see `codegen_x64/debuginfo.rs`) as the adapter reads it:
//! functions, the line table, and the lookups between addresses, lines and files.

use crate::lsp::{parse_json, Json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A function of the program.
#[derive(Debug, Clone)]
pub struct Func {
    pub name: String,
    pub module: String,
    pub start: u64,
    pub end: u64,
    pub line: u32,
    /// The locals and parameters as the file has them (read when variables are shown).
    pub locals: Json,
}

/// One row of the line table: the code of the statement on `line` of `module` starts at `addr`.
#[derive(Debug, Clone)]
pub struct Row {
    pub addr: u64,
    pub module: String,
    pub line: u32,
}

/// The files of a package of the project: each file starts at a line of the merged text.
#[derive(Debug, Clone)]
pub struct Segment {
    pub path: PathBuf,
    pub start: u32,
    pub lines: u32,
}

#[derive(Debug, Clone)]
pub struct Info {
    /// The image base the addresses were built for.
    pub base: u64,
    pub functions: Vec<Func>,
    /// In address order.
    pub rows: Vec<Row>,
    pub types: Json,
    pub files: HashMap<String, Vec<Segment>>,
    /// The address of the runtime's `abort`, when the file has it.
    pub abort: Option<u64>,
}

fn num(j: &Json) -> f64 {
    match j {
        Json::Num(n) => *n,
        _ => 0.0,
    }
}

impl Info {
    pub fn load(path: &Path) -> Result<Info, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {}", path.display(), e))?;
        let j = parse_json(&text).map_err(|e| format!("{}: {}", path.display(), e))?;
        let mut functions = Vec::new();
        if let Json::Arr(fs) = j.get("functions") {
            for f in fs {
                functions.push(Func {
                    name: f.get("name").str().unwrap_or("").to_string(),
                    module: f.get("module").str().unwrap_or("").to_string(),
                    start: num(f.get("start")) as u64,
                    end: num(f.get("end")) as u64,
                    line: num(f.get("line")) as u32,
                    locals: f.get("locals").clone(),
                });
            }
        }
        functions.sort_by_key(|f| f.start);
        let mut rows = Vec::new();
        if let Json::Arr(rs) = j.get("lines") {
            for r in rs {
                if let Json::Arr(v) = r {
                    rows.push(Row { addr: num(&v[0]) as u64, module: v[1].str().unwrap_or("").to_string(), line: num(&v[2]) as u32 });
                }
            }
        }
        rows.sort_by_key(|r| r.addr);
        let mut files = HashMap::new();
        if let Json::Obj(m) = j.get("files") {
            for (module, segs) in m {
                if let Json::Arr(list) = segs {
                    let v = list.iter().map(|s| Segment { path: PathBuf::from(s.get("path").str().unwrap_or("")), start: num(s.get("start")) as u32, lines: num(s.get("lines")) as u32 }).collect();
                    files.insert(module.clone(), v);
                }
            }
        }
        let abort = match j.get("abort") {
            Json::Num(n) => Some(*n as u64),
            _ => None,
        };
        Ok(Info { base: num(j.get("base")) as u64, functions, rows, types: j.get("types").clone(), files, abort })
    }

    /// The function whose code holds `addr` (built addresses).
    pub fn function_at(&self, addr: u64) -> Option<&Func> {
        let i = self.functions.partition_point(|f| f.start <= addr);
        self.functions[..i].last().filter(|f| addr < f.end)
    }

    /// The module and line of the statement `addr` is in: the last row at or before it within the
    /// function. A statement that made no code shares an address with the next one, whose row is
    /// the later one.
    pub fn line_at(&self, addr: u64) -> Option<(&str, u32)> {
        let f = self.function_at(addr)?;
        let i = self.rows.partition_point(|r| r.addr <= addr);
        self.rows[..i].iter().rev().find(|r| r.addr >= f.start).map(|r| (r.module.as_str(), r.line))
    }

    /// Whether a statement's code starts exactly at `addr`, and its line.
    pub fn row_at(&self, addr: u64) -> Option<&Row> {
        let i = self.rows.partition_point(|r| r.addr <= addr);
        self.rows[..i].last().filter(|r| r.addr == addr)
    }

    /// The first line at or after `line` of `module` that has code, and the address of its first
    /// row (the lowest, for a line that appears more than once).
    pub fn resolve_line(&self, module: &str, line: u32) -> Option<(u32, u64)> {
        let actual = self.rows.iter().filter(|r| r.module == module && r.line >= line).map(|r| r.line).min()?;
        let addr = self.rows.iter().filter(|r| r.module == module && r.line == actual).map(|r| r.addr).min()?;
        Some((actual, addr))
    }

    /// The address where the first statement of `f` starts (after its entry code).
    pub fn first_statement(&self, f: &Func) -> Option<u64> {
        self.rows.iter().find(|r| r.addr >= f.start && r.addr < f.end).map(|r| r.addr)
    }

    /// The file and line a line of a module is in. `program` is the file of the `main` package.
    pub fn source_of(&self, module: &str, line: u32, program: &Path) -> Option<(PathBuf, u32)> {
        if module == "main" {
            return Some((program.to_path_buf(), line));
        }
        let segs = self.files.get(module)?;
        segs.iter().find(|s| line >= s.start && line < s.start + s.lines).map(|s| (s.path.clone(), line - s.start + 1))
    }

    /// The module and line of the merged text a line of a file is (the inverse of `source_of`).
    pub fn module_line_of(&self, path: &Path, line: u32, program: &Path) -> Option<(String, u32)> {
        let norm = |p: &Path| p.to_string_lossy().replace('\\', "/").to_lowercase();
        if norm(path) == norm(program) {
            return Some(("main".to_string(), line));
        }
        for (module, segs) in &self.files {
            if let Some(s) = segs.iter().find(|s| norm(&s.path) == norm(path)) {
                return Some((module.clone(), s.start + line - 1));
            }
        }
        None
    }
}
