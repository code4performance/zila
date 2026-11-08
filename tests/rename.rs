//! The rename of the language (change `rename-language`): the tool's own rules, and the check that
//! nothing of the old name is left once the language has been renamed. The old name is never written
//! in this file: it is read from `python tools/rename-language.py --print-rules`, so the file does not
//! change when the tool rewrites the repository.

use std::path::Path;
use std::process::Command;

fn python() -> Option<&'static str> {
    ["python", "python3"].into_iter().find(|p| Command::new(p).arg("--version").output().map(|o| o.status.success()).unwrap_or(false))
}

fn tool(args: &[&str]) -> Option<(bool, String)> {
    let py = python()?;
    let o = Command::new(py).arg("tools/rename-language.py").args(args).output().ok()?;
    Some((o.status.success(), String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr)))
}

struct Rules {
    old: Vec<String>,
    prefixes: Vec<String>,
    files: Vec<String>,
}

fn field(json: &str, key: &str) -> Vec<String> {
    // the tool prints a small, flat JSON; the strings of a field are read without a JSON library
    let start = json.find(&format!("\"{}\"", key)).unwrap_or_else(|| panic!("no {} in the rules", key));
    let rest = &json[start..];
    let open = rest.find(['[', '{']).unwrap();
    let close = rest.find([']', '}']).unwrap();
    let body = &rest[open + 1..close];
    let mut out = Vec::new();
    let mut chars = body.chars().peekable();
    let mut in_str = false;
    let mut cur = String::new();
    let mut is_key = true;
    while let Some(c) = chars.next() {
        if in_str {
            match c {
                '\\' => {
                    if let Some(n) = chars.next() {
                        cur.push(n);
                    }
                }
                '"' => {
                    in_str = false;
                    if !(rest[open..].starts_with('{') && is_key) {
                        out.push(std::mem::take(&mut cur));
                    } else {
                        cur.clear();
                    }
                    is_key = !is_key;
                }
                _ => cur.push(c),
            }
        } else if c == '"' {
            in_str = true;
        }
    }
    out
}

fn rules() -> Option<Rules> {
    let (ok, json) = tool(&["--print-rules"])?;
    assert!(ok, "{}", json);
    Some(Rules { old: field(&json, "old"), prefixes: field(&json, "exclude_prefixes"), files: field(&json, "exclude_files") })
}

#[test]
fn the_tool_passes_its_own_self_test() {
    let Some((ok, out)) = tool(&["--self-test"]) else {
        eprintln!("python is not installed: skipping");
        return;
    };
    assert!(ok && out.contains("self-test ok"), "{}", out);
}

#[test]
fn a_bad_name_is_refused_before_anything_changes() {
    let Some(_) = python() else { return };
    for bad in ["9lives", "two words"] {
        let (ok, out) = tool(&[bad]).unwrap();
        assert!(!ok && out.contains("must start with a letter"), "{}: {}", bad, out);
    }
}

#[test]
fn a_dry_run_derives_every_form_changes_nothing_and_skips_the_archive() {
    let Some(_) = python() else { return };
    let before = std::process::Command::new("git").args(["status", "--porcelain"]).output().unwrap().stdout;
    let (ok, out) = tool(&["Zorbo"]).unwrap();
    let after = std::process::Command::new("git").args(["status", "--porcelain"]).output().unwrap().stdout;
    assert!(ok, "{}", out);
    assert_eq!(before, after, "a dry run must not change the tree");
    for form in ["zorbo-parse", "ZORBO_", "ZorboWindow", "Dry run: nothing was changed."] {
        assert!(out.contains(form), "the plan lacks {}:\n{}", form, out);
    }
    assert!(!out.contains("openspec/changes/archive/"), "the archive must not be listed");
}

#[test]
fn the_checklist_the_tool_prints_is_in_contributing() {
    let Some((ok, list)) = tool(&["--print-checklist"]) else { return };
    assert!(ok);
    let contributing = std::fs::read_to_string("CONTRIBUTING.md").unwrap();
    for item in list.lines().map(|l| l.trim_start_matches("- ")) {
        assert!(contributing.contains(item), "CONTRIBUTING.md lacks the checklist item: {}", item);
    }
}

/// Whether the repository has been renamed: the crate no longer has the old name.
fn renamed(old: &str) -> bool {
    let manifest = std::fs::read_to_string("Cargo.toml").unwrap();
    !manifest.lines().any(|l| l.trim() == format!("name = \"{}\"", old))
}

fn tracked() -> Vec<String> {
    let out = Command::new("git").args(["ls-files", "-z"]).output().unwrap();
    String::from_utf8_lossy(&out.stdout).split('\0').filter(|p| !p.is_empty()).map(|p| p.to_string()).collect()
}

fn excluded(rules: &Rules, path: &str) -> bool {
    rules.prefixes.iter().any(|p| path.starts_with(p.as_str())) || rules.files.iter().any(|f| f == path)
}

/// `path:pattern` lines of `tools/rename-allow.txt`: a line of that file containing the pattern is accepted.
fn allowed() -> Vec<(String, String)> {
    std::fs::read_to_string("tools/rename-allow.txt")
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once(':').map(|(p, s)| (p.trim().to_string(), s.trim().to_string())))
        .collect()
}

#[test]
fn nothing_of_the_old_name_is_left_once_the_language_is_renamed() {
    let Some(rules) = rules() else {
        eprintln!("python is not installed: skipping");
        return;
    };
    let old_lower = rules.old.iter().find(|o| o.chars().all(|c| c.is_lowercase())).unwrap().clone();
    if !renamed(&old_lower) {
        eprintln!("the language has not been renamed yet: the leftover check is dormant");
        return;
    }
    let allow = allowed();
    let mut problems = Vec::new();
    for path in tracked() {
        if excluded(&rules, &path) || !Path::new(&path).is_file() {
            continue;
        }
        if path.to_lowercase().contains(&old_lower) && !allow.iter().any(|(p, s)| p == &path && s.is_empty()) {
            problems.push(format!("{}: the file name has the old name", path));
        }
        let Ok(bytes) = std::fs::read(&path) else { continue };
        if bytes.contains(&0) {
            continue;
        }
        let Ok(text) = String::from_utf8(bytes) else { continue };
        for (i, line) in text.lines().enumerate() {
            if line.to_lowercase().contains(&old_lower) && !allow.iter().any(|(p, s)| p == &path && !s.is_empty() && line.contains(s.as_str())) {
                problems.push(format!("{}:{}: {}", path, i + 1, line.trim()));
            }
        }
    }
    assert!(problems.is_empty(), "the old name is still used ({} places):\n{}", problems.len(), problems.join("\n"));
}
