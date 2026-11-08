//! The commands that change a project's packages: `--init`, `--add`, `--fetch`, `--tidy` and
//! `--update`. This is the only module that runs `git`, and only `--add`, `--fetch` and
//! `--update` reach it; a build never imports this module.

use crate::package::*;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const GIT_MISSING: &str = "git was not found on PATH. It is used only to download dependencies (zila --fetch); building from an already populated cache does not need it.";

// ---------------------------------------------------------------------------
// git
// ---------------------------------------------------------------------------

fn git(args: &[&str], dir: Option<&Path>, url: &str) -> Result<String, String> {
    let mut c = Command::new("git");
    c.args(args).env("GIT_TERMINAL_PROMPT", "0");
    if let Some(d) = dir {
        c.current_dir(d);
    }
    match c.output() {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(GIT_MISSING.to_string()),
        Err(e) => Err(format!("git failed while fetching {}:\n{}", url, e)),
        Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout).to_string()),
        Ok(o) => Err(format!("git failed while fetching {}:\n{}", url, String::from_utf8_lossy(&o.stderr).trim())),
    }
}

/// The tags of a repository and the commit each points to (the peeled commit of annotated tags).
pub fn ls_remote_tags(url: &str) -> Result<BTreeMap<String, String>, String> {
    let out = git(&["ls-remote", "--tags", "--", url], None, url)?;
    let mut tags: BTreeMap<String, String> = BTreeMap::new();
    let mut peeled: BTreeMap<String, String> = BTreeMap::new();
    for line in out.lines() {
        let Some((sha, r)) = line.split_once('\t') else { continue };
        let Some(name) = r.strip_prefix("refs/tags/") else { continue };
        match name.strip_suffix("^{}") {
            Some(n) => {
                peeled.insert(n.to_string(), sha.to_string());
            }
            None => {
                tags.insert(name.to_string(), sha.to_string());
            }
        }
    }
    for (n, sha) in peeled {
        tags.insert(n, sha);
    }
    Ok(tags)
}

/// Clones `url` without a checkout, checks out `commit` detached and removes `.git`.
pub fn clone_commit(url: &str, commit: &str, tmp: &Path) -> Result<(), String> {
    let t = tmp.to_string_lossy().to_string();
    git(&["clone", "--quiet", "--no-checkout", "--", url, &t], None, url)?;
    git(&["-C", &t, "checkout", "--quiet", "--detach", commit], None, url)?;
    let _ = std::fs::remove_dir_all(tmp.join(".git"));
    if tmp.join(".git").exists() {
        // read-only pack files on Windows
        if let Ok(rd) = std::fs::read_dir(tmp.join(".git")) {
            for e in rd.flatten() {
                if let Ok(md) = e.metadata() {
                    let mut p = md.permissions();
                    p.set_readonly(false);
                    let _ = std::fs::set_permissions(e.path(), p);
                }
            }
        }
        let _ = std::fs::remove_dir_all(tmp.join(".git"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// fetch
// ---------------------------------------------------------------------------

struct Fetcher<'a> {
    home: &'a Path,
    offline: bool,
    old: Lock,
    tags: HashMap<String, BTreeMap<String, String>>,
    /// (module, version) to (commit, directory)
    got: BTreeMap<(String, Version), (String, PathBuf)>,
}

impl<'a> Fetcher<'a> {
    fn tags_of(&mut self, url: &str) -> Result<&BTreeMap<String, String>, String> {
        if !self.tags.contains_key(url) {
            let t = ls_remote_tags(url)?;
            self.tags.insert(url.to_string(), t);
        }
        Ok(&self.tags[url])
    }

    /// Makes sure `module@version` is in the cache and returns its manifest.
    fn ensure(&mut self, module: &str, version: &Version, url: &str) -> Result<Manifest, String> {
        let url = &repo_url(url);
        let id = format!("{}@{}", module, version);
        let locked = self.old.find(module, version).cloned();
        let commit = if self.offline {
            match &locked {
                Some(e) => e.commit.clone(),
                None => return Err(format!("package '{}' is not in {}; it cannot be fetched with --offline", id, LOCK)),
            }
        } else {
            let tag = format!("v{}", version);
            let now = self.tags_of(url)?.get(&tag).cloned().ok_or_else(|| format!("{} has no tag {} (needed for '{}')", url, tag, id))?;
            if let Some(e) = &locked {
                if e.commit != now {
                    return Err(format!("tag {} of {} moved (locked {}, now {}); the lock is kept. Run --update to accept it", tag, url, e.commit, now));
                }
            }
            now
        };
        let mut dir = cache_lookup(self.home, module, version, &commit);
        if let (Some(d), Some(e)) = (&dir, &locked) {
            // verify what is there; replace it when it was altered
            let found = tree_hash(d)?;
            if found != e.hash {
                if self.offline {
                    return Err(format!("package '{}' in the cache does not match zila.lock (expected {}, found {}); run zila --fetch to restore it", id, e.hash, found));
                }
                eprintln!("note: the cached copy of '{}' did not match zila.lock; downloading it again", id);
                let _ = std::fs::remove_dir_all(d);
                dir = None;
            }
        }
        let dir = match dir {
            Some(d) => d,
            None => {
                if self.offline {
                    return Err(format!("package '{}' is not in the cache; run zila --fetch (without --offline)", id));
                }
                let tmp = cache_tmp(self.home, module, version, &commit)?;
                if let Err(e) = clone_commit(url, &commit, &tmp) {
                    let _ = std::fs::remove_dir_all(&tmp);
                    return Err(e);
                }
                cache_store(self.home, module, version, &commit, &tmp)?
            }
        };
        let m = if dir.join(MANIFEST).is_file() { read_manifest(&dir)? } else { Manifest::default() };
        self.got.insert((module.to_string(), *version), (commit, dir));
        Ok(m)
    }
}

/// Resolves the graph of the project at `root`, downloads what the cache lacks and writes the lock.
pub fn fetch(root: &Path, home: &Path, offline: bool) -> Result<Lock, String> {
    fetch_with(root, home, offline, false)
}

/// `accept_moved` ignores the old lock (`--update` accepts the tags as they are now).
pub fn fetch_with(root: &Path, home: &Path, offline: bool, accept_moved: bool) -> Result<Lock, String> {
    let manifest = read_manifest(root)?;
    let old = if accept_moved { Lock::default() } else { read_lock(root)?.unwrap_or_default() };
    let reqs: Vec<Req> = manifest.deps.iter().map(Req::from).collect();
    let mut f = Fetcher { home, offline, old, tags: HashMap::new(), got: BTreeMap::new() };
    let mut deps_of: HashMap<(String, Version), Vec<Req>> = HashMap::new();
    let selection = select(&reqs, |m, v, url| {
        let man = f.ensure(m, v, url)?;
        let r: Vec<Req> = man.deps.iter().map(Req::from).collect();
        deps_of.insert((m.to_string(), *v), r.clone());
        Ok(r)
    })?;
    let mut lock = Lock::default();
    for (module, (version, _)) in &selection {
        let (commit, dir) = f.got.get(&(module.clone(), *version)).cloned().ok_or_else(|| format!("internal error: '{}' was not fetched", module))?;
        let needs: Vec<String> = deps_of
            .get(&(module.clone(), *version))
            .map(|rs| rs.iter().filter_map(|r| selection.get(&r.module).map(|(v, _)| format!("{}@{}", r.module, v))).collect())
            .unwrap_or_default();
        lock.entries.push(LockEntry { module: module.clone(), version: *version, commit, hash: tree_hash(&dir)?, needs });
    }
    std::fs::write(root.join(LOCK), lock.write()).map_err(|e| format!("cannot write {}: {}", LOCK, e))?;
    Ok(lock)
}

// ---------------------------------------------------------------------------
// the commands
// ---------------------------------------------------------------------------

fn project_root(cwd: &Path) -> Result<PathBuf, String> {
    find_manifest_dir(cwd).ok_or_else(|| format!("no {} found here or above; run zila --init first", MANIFEST))
}

pub fn init(dir: &Path, name: Option<&str>) -> Result<String, String> {
    let path = dir.join(MANIFEST);
    if path.exists() {
        return Err(format!("{} already exists", path.display()));
    }
    let default = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "app".into());
    let name = name.map(|s| s.to_string()).unwrap_or(default);
    let m = Manifest { name: Some(name), version: Version::parse("0.1.0"), edition: None, deps: Vec::new(), forbid: None };
    std::fs::write(&path, m.write()).map_err(|e| format!("cannot write {}: {}", path.display(), e))?;
    Ok(format!("created {}", path.display()))
}

/// `url[@version]`: the `@version` is the part after the last `@` when it is a version.
fn split_spec(spec: &str) -> (String, Option<Version>) {
    if let Some((u, v)) = spec.rsplit_once('@') {
        if let Some(ver) = Version::parse(v) {
            return (u.to_string(), Some(ver));
        }
    }
    (spec.to_string(), None)
}

/// What the packages that were downloaded or changed can do (spec `package-effects`): one line per
/// package whose version or commit is new, and a marked line when an update gave it an effect it
/// did not have.
pub fn effects_report(home: &Path, prev: &Lock, now: &Lock) -> String {
    let mut out = String::new();
    for e in &now.entries {
        let old = prev.for_module(&e.module);
        if old.map_or(false, |o| o.version == e.version && o.commit == e.commit) {
            continue;
        }
        let Some(dir) = cache_lookup(home, &e.module, &e.version, &e.commit) else { continue };
        let eff = crate::effects::of_dir(&dir);
        out.push_str(&format!("\n  {}@{}: effects: {}", e.module, e.version, eff.show()));
        if let Some(o) = old {
            if let Some(od) = cache_lookup(home, &o.module, &o.version, &o.commit) {
                let was = crate::effects::of_dir(&od);
                let gained = eff.gained_over(&was);
                if !gained.is_empty() {
                    out.push_str(&format!("  [CHANGED: was {}, gained {}]", was.show(), gained.join(", ")));
                }
            }
        }
    }
    out
}

pub fn add(cwd: &Path, home: &Path, spec: &str, as_name: Option<&str>, offline: bool) -> Result<String, String> {
    let root = project_root(cwd)?;
    let (url, version) = split_spec(spec);
    let module = module_path(&url);
    let name = as_name.map(|s| s.to_string()).unwrap_or_else(|| default_import_name(&module));
    if is_reserved(&name) {
        return Err(format!("the name '{}' belongs to the standard library and cannot name a dependency; choose another with --as", name));
    }
    let mut manifest = read_manifest(&root)?;
    if manifest.deps.iter().any(|d| d.name == name) {
        return Err(format!("'{}' is already a dependency; choose another name with --as", name));
    }
    let version = match version {
        Some(v) => v,
        None => {
            if offline {
                return Err("--add needs @version with --offline (the tags cannot be listed)".into());
            }
            let tags: Vec<String> = ls_remote_tags(&repo_url(&url))?.keys().cloned().collect();
            tag_versions(&module, &tags).last().copied().ok_or_else(|| format!("{} has no version tags (vMAJOR.MINOR.PATCH) that match '{}'", url, module))?
        }
    };
    if !major_matches(&module, version.major) {
        return Err(format!("version {} does not match the module path '{}' (a major version 2 or higher ends the path with /vN)", version, module));
    }
    manifest.deps.push(Dep { name: name.clone(), git: url, version });
    let before = std::fs::read_to_string(root.join(MANIFEST)).ok();
    std::fs::write(root.join(MANIFEST), manifest.write()).map_err(|e| format!("cannot write {}: {}", MANIFEST, e))?;
    let prev = read_lock(&root).ok().flatten().unwrap_or_default();
    match fetch(&root, home, offline) {
        Ok(lock) => Ok(format!("added {} ({}@{}){}", name, module, version, effects_report(home, &prev, &lock))),
        Err(e) => {
            // leave the manifest as it was
            if let Some(b) = before {
                let _ = std::fs::write(root.join(MANIFEST), b);
            }
            Err(e)
        }
    }
}

pub fn run_fetch(cwd: &Path, home: &Path, offline: bool) -> Result<String, String> {
    let root = project_root(cwd)?;
    let prev = read_lock(&root).ok().flatten().unwrap_or_default();
    let lock = fetch(&root, home, offline)?;
    Ok(format!("locked {} package(s){}", lock.entries.len(), effects_report(home, &prev, &lock)))
}

/// Removes the dependencies no source uses and rewrites the lock to the graph that is needed.
pub fn tidy(cwd: &Path, home: &Path) -> Result<String, String> {
    let root = project_root(cwd)?;
    let mut manifest = read_manifest(&root)?;
    let mut used: std::collections::HashSet<String> = std::collections::HashSet::new();
    for f in all_sources(&root) {
        if let Ok(text) = std::fs::read_to_string(&f) {
            used.extend(crate::comptime::use_names(&text));
        }
    }
    let before = manifest.deps.len();
    let removed: Vec<String> = manifest.deps.iter().filter(|d| !used.contains(&d.name)).map(|d| d.name.clone()).collect();
    manifest.deps.retain(|d| used.contains(&d.name));
    if !removed.is_empty() {
        std::fs::write(root.join(MANIFEST), manifest.write()).map_err(|e| format!("cannot write {}: {}", MANIFEST, e))?;
    }
    let lock = fetch(&root, home, true)?;
    Ok(format!("removed {} of {} dependencies{}; {} locked package(s)", removed.len(), before, if removed.is_empty() { String::new() } else { format!(" ({})", removed.join(", ")) }, lock.entries.len()))
}

/// Raises the minimum of `name` (or of every dependency) to the newest tag of its major.
pub fn update(cwd: &Path, home: &Path, name: Option<&str>, offline: bool) -> Result<String, String> {
    let root = project_root(cwd)?;
    let mut manifest = read_manifest(&root)?;
    if let Some(n) = name {
        if !manifest.deps.iter().any(|d| d.name == n) {
            return Err(format!("'{}' is not a dependency of this project", n));
        }
    }
    if offline {
        return Err("--update needs the network to list tags; it cannot run with --offline".into());
    }
    let mut changed = Vec::new();
    for d in manifest.deps.iter_mut() {
        if name.map_or(false, |n| n != d.name) {
            continue;
        }
        let tags: Vec<String> = ls_remote_tags(&repo_url(&d.git))?.keys().cloned().collect();
        let module = d.module();
        let newest = tag_versions(&module, &tags).into_iter().filter(|v| path_major(&module).map_or(true, |m| v.major == m) && v.major == d.version.major).last();
        if let Some(v) = newest {
            if v > d.version {
                changed.push(format!("{} {} -> {}", d.name, d.version, v));
                d.version = v;
            }
        }
    }
    std::fs::write(root.join(MANIFEST), manifest.write()).map_err(|e| format!("cannot write {}: {}", MANIFEST, e))?;
    let prev = read_lock(&root).ok().flatten().unwrap_or_default();
    let lock = fetch_with(&root, home, false, true)?;
    Ok(format!("{}; locked {} package(s){}", if changed.is_empty() { "already up to date".to_string() } else { changed.join(", ") }, lock.entries.len(), effects_report(home, &prev, &lock)))
}

/// Runs the package command of `opts`; returns the exit code.
pub fn run(opts: &crate::cli::Options) -> i32 {
    use crate::cli::Mode;
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let home = cache_root();
    let r = match opts.mode {
        Mode::Init => init(&cwd, opts.path.as_deref()),
        Mode::Add => add(&cwd, &home, opts.add.as_deref().unwrap_or(""), opts.as_name.as_deref(), opts.offline),
        Mode::Fetch => run_fetch(&cwd, &home, opts.offline),
        Mode::Tidy => tidy(&cwd, &home),
        Mode::Update => update(&cwd, &home, opts.path.as_deref(), opts.offline),
        _ => unreachable!(),
    };
    match r {
        Ok(msg) => {
            println!("{}", msg);
            0
        }
        Err(e) => {
            eprintln!("error: {}", e);
            1
        }
    }
}
