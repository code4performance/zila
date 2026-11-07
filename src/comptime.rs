//! Compile-time reflection (spec `comptime-reflection`): `comptime func`, derive attributes,
//! type arguments and block forms. See the module sections below.

mod check;
mod expand;
pub mod reflect;
pub mod run;
mod state;
pub mod walk;

pub use expand::{expand, uses_comptime, Expansion};
pub use state::{driver_mode, launch_log, map_error, set_cache_dir, set_cache_enabled, set_dump, set_timeout_secs, set_version_tag, settings, Settings};

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// Packages and the block pre-scan
// ---------------------------------------------------------------------------

static PACKAGES: Mutex<Option<HashMap<String, &'static str>>> = Mutex::new(None);

/// Makes the package `name` available to `use` next to the standard library. The compiler has no
/// package manager yet: the command line registers the files that sit next to the source file,
/// and tests register the packages they need.
pub fn register_package(name: &str, source: &str) {
    let leaked: &'static str = Box::leak(source.to_string().into_boxed_str());
    PACKAGES.lock().unwrap().get_or_insert_with(HashMap::new).insert(name.to_string(), leaked);
}

/// The source of a registered package.
pub fn registered_source(name: &str) -> Option<&'static str> {
    PACKAGES.lock().unwrap().as_ref().and_then(|m| m.get(name).copied())
}

/// The source of package `name`: the standard library first, then registered packages.
pub fn package_source(name: &str, target: crate::codegen_x64::Target) -> Option<&'static str> {
    crate::codegen_x64::std_source_for(name, target).or_else(|| registered_source(name))
}

/// Registers the packages a program asks for with `use` that are files next to `path`
/// (`<dir>/<name>.br`), and theirs in turn. Used by the command line, the formatter and the
/// language server so that block names of those packages are known.
pub fn register_siblings(path: &std::path::Path, src: &str) {
    let dir = match path.parent() {
        Some(d) => d.to_path_buf(),
        None => std::path::PathBuf::from("."),
    };
    let mut todo: Vec<String> = use_names(src);
    let mut done: HashSet<String> = HashSet::new();
    while let Some(name) = todo.pop() {
        if !done.insert(name.clone()) || crate::codegen_x64::std_source(&name).is_some() {
            continue;
        }
        let file = dir.join(format!("{}.br", name));
        if let Ok(text) = std::fs::read_to_string(&file) {
            register_package(&name, &text);
            todo.extend(use_names(&text));
        }
    }
}

/// The first segment of every `use` at the top of a file.
pub fn use_names(src: &str) -> Vec<String> {
    let mut v = Vec::new();
    for line in src.lines() {
        let Some(rest) = line.strip_prefix("use ") else { continue };
        let rest = rest.split("//").next().unwrap_or("");
        for part in rest.split(',') {
            let name: String = part.trim().chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
            if !name.is_empty() {
                v.push(name);
            }
        }
    }
    v
}

fn scan_blocks(src: &str, out: &mut HashSet<String>) {
    if !src.contains("#[block]") {
        return;
    }
    let lines: Vec<&str> = src.lines().collect();
    for i in 0..lines.len() {
        if !lines[i].trim_start().starts_with("#[block]") {
            continue;
        }
        for line in lines.iter().skip(i).take(4) {
            if let Some(p) = line.find("comptime func ") {
                let name: String =
                    line[p + "comptime func ".len()..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
                if !name.is_empty() {
                    out.insert(name);
                }
                break;
            }
        }
    }
}

/// The names that start a block form in `src`: the `#[block] comptime func`s of the packages it
/// uses (the pre-scan of the `use` lines) and of the file itself. A package that cannot be found
/// adds nothing: its names are then ordinary identifiers.
pub fn block_names(src: &str) -> HashSet<String> {
    let mut set = HashSet::new();
    scan_blocks(src, &mut set);
    for pkg in use_names(src) {
        // the target does not change which functions are blocks
        if let Some(s) = package_source(&pkg, crate::codegen_x64::Target::Windows) {
            scan_blocks(s, &mut set);
        }
    }
    set
}
