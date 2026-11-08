//! The package manager: manifest, lock, cache, commands, resolution of `use`, integrity and offline
//! builds. Repositories are local `file://` git repositories created here; nothing reaches a
//! network. Tests that need `git` skip with a message when it cannot be run.

use brasa::package::*;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("brasa_pkg_{}_{}_{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write(dir: &Path, rel: &str, text: &str) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn have_git() -> bool {
    Command::new("git").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

macro_rules! need_git {
    () => {
        if !have_git() {
            eprintln!("git is not usable here: skipping");
            return;
        }
    };
}

fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.org")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.org")
        .output()
        .unwrap();
    assert!(o.status.success(), "git {:?}: {}", args, String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// A repository at `dir` with one commit holding `files`.
fn repo(dir: &Path, files: &[(&str, &str)]) {
    std::fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "--quiet"]);
    git(dir, &["config", "core.autocrlf", "false"]);
    commit(dir, files, "first");
}

fn commit(dir: &Path, files: &[(&str, &str)], msg: &str) -> String {
    for (p, t) in files {
        write(dir, p, t);
    }
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "--quiet", "-m", msg]);
    git(dir, &["rev-parse", "HEAD"])
}

fn url_of(dir: &Path) -> String {
    format!("file:///{}", dir.to_string_lossy().replace('\\', "/").trim_start_matches('/'))
}

struct Out {
    code: i32,
    out: String,
    err: String,
}

fn brasa(cwd: &Path, home: &Path, args: &[&str]) -> Out {
    brasa_env(cwd, home, args, None)
}

fn brasa_env(cwd: &Path, home: &Path, args: &[&str], path: Option<&str>) -> Out {
    let mut c = Command::new(env!("CARGO_BIN_EXE_brasa-parse"));
    c.args(args).current_dir(cwd).env("BRASA_HOME", home);
    if let Some(p) = path {
        c.env("PATH", p);
    }
    let o = c.output().unwrap();
    Out { code: o.status.code().unwrap_or(-1), out: String::from_utf8_lossy(&o.stdout).replace('\r', ""), err: String::from_utf8_lossy(&o.stderr).replace('\r', "") }
}

fn exe_name(n: &str) -> String {
    if cfg!(windows) {
        format!("{}.exe", n)
    } else {
        n.to_string()
    }
}

/// Builds `main.br` of a project and runs it (None when the machine cannot run the target).
fn build_and_run(proj: &Path, home: &Path) -> Result<String, String> {
    let exe = proj.join(exe_name("app"));
    let target: &[&str] = if cfg!(windows) { &[] } else { &["--target", "linux"] };
    let mut args = vec!["main.br", "-o", exe.to_str().unwrap()];
    args.extend_from_slice(target);
    let b = brasa(proj, home, &args);
    if b.code != 0 {
        return Err(format!("{}{}", b.out, b.err));
    }
    let o = Command::new(&exe).output().map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&o.stdout).replace('\r', ""))
}

const JSON_V1: &str = "package codec\n\npub func version() -> text\n    return \"codec 1\"\nend\n\nfunc hidden() -> text\n    return \"secret\"\nend\n";

fn project(name: &str, main: &str) -> PathBuf {
    let p = scratch(name);
    write(&p, "main.br", main);
    p
}

fn init(p: &Path, home: &Path) {
    let r = brasa(p, home, &["--init", "app"]);
    assert_eq!(r.code, 0, "{}{}", r.out, r.err);
}

// ---------------------------------------------------------------- commands

#[test]
fn init_writes_a_manifest_once() {
    let home = scratch("home");
    let p = scratch("init");
    let r = brasa(&p, &home, &["--init", "demo"]);
    assert_eq!(r.code, 0, "{}", r.err);
    let m = Manifest::parse(&std::fs::read_to_string(p.join("brasa.toml")).unwrap()).unwrap();
    assert_eq!(m.name.as_deref(), Some("demo"));
    assert!(m.edition.is_none(), "edition is omitted before 1.0");
    let again = brasa(&p, &home, &["--init"]);
    assert_eq!(again.code, 1);
    assert!(again.err.contains("already exists"), "{}", again.err);
}

#[test]
fn add_fetches_locks_and_the_program_builds() {
    need_git!();
    let (home, repos) = (scratch("home"), scratch("repos"));
    let codec = repos.join("codec");
    repo(&codec, &[("codec.br", JSON_V1)]);
    git(&codec, &["tag", "-a", "v1.0.0", "-m", "one"]);
    let commit_v1 = git(&codec, &["rev-parse", "v1.0.0^{commit}"]);
    commit(&codec, &[("codec.br", &JSON_V1.replace("codec 1", "codec 1.2"))], "second");
    git(&codec, &["tag", "v1.2.0"]);
    let commit_v12 = git(&codec, &["rev-parse", "HEAD"]);
    let p = project("add", "package main\nuse codec\n\nfunc main()\n    print(codec.version())\nend\n");
    init(&p, &home);
    let url = url_of(&codec);
    // the newest tag when none is given
    let r = brasa(&p, &home, &["--add", &url]);
    assert_eq!(r.code, 0, "{}{}", r.out, r.err);
    let m = Manifest::parse(&std::fs::read_to_string(p.join("brasa.toml")).unwrap()).unwrap();
    assert_eq!(m.deps.len(), 1);
    assert_eq!((m.deps[0].name.as_str(), m.deps[0].version.to_string(), m.deps[0].git.as_str()), ("codec", "1.2.0".to_string(), url.as_str()));
    let lock = Lock::parse(&std::fs::read_to_string(p.join("brasa.lock")).unwrap()).unwrap();
    assert_eq!(lock.entries.len(), 1);
    let e = &lock.entries[0];
    assert_eq!(e.commit, commit_v12);
    assert_ne!(e.commit, commit_v1);
    assert!(e.hash.starts_with("sha256:") && e.hash.len() == 71);
    assert!(e.module.ends_with("/codec") && e.version.to_string() == "1.2.0");
    // the cache holds the files without .git, with the hash the lock records
    let dir = cache_dir(&home, &e.module, &e.version, &e.commit);
    assert!(dir.join("codec.br").is_file() && !dir.join(".git").exists());
    assert_eq!(tree_hash(&dir).unwrap(), e.hash);
    assert_eq!(build_and_run(&p, &home).unwrap(), "codec 1.2\n");
}

#[test]
fn an_annotated_tag_locks_the_commit_not_the_tag_object() {
    need_git!();
    let (home, repos) = (scratch("home"), scratch("repos"));
    let lib = repos.join("lib");
    repo(&lib, &[("lib.br", "package lib\npub func n() -> int\n    return 7\nend\n")]);
    git(&lib, &["tag", "-a", "v1.0.0", "-m", "annotated"]);
    let tag_object = git(&lib, &["rev-parse", "v1.0.0"]);
    let commit_id = git(&lib, &["rev-parse", "v1.0.0^{commit}"]);
    assert_ne!(tag_object, commit_id);
    let p = project("annotated", "package main\nuse lib\n\nfunc main()\n    print(\"{lib.n()}\")\nend\n");
    init(&p, &home);
    let r = brasa(&p, &home, &["--add", &format!("{}@1.0.0", url_of(&lib))]);
    assert_eq!(r.code, 0, "{}{}", r.out, r.err);
    let lock = Lock::parse(&std::fs::read_to_string(p.join("brasa.lock")).unwrap()).unwrap();
    assert_eq!(lock.entries[0].commit, commit_id);
}

#[test]
fn a_moved_tag_is_reported_and_the_lock_is_kept() {
    need_git!();
    let (home, repos) = (scratch("home"), scratch("repos"));
    let lib = repos.join("lib");
    repo(&lib, &[("lib.br", "package lib\npub func n() -> int\n    return 1\nend\n")]);
    git(&lib, &["tag", "v1.0.0"]);
    let old = git(&lib, &["rev-parse", "HEAD"]);
    let p = project("moved", "package main\nuse lib\n\nfunc main()\n    print(\"{lib.n()}\")\nend\n");
    init(&p, &home);
    assert_eq!(brasa(&p, &home, &["--add", &format!("{}@1.0.0", url_of(&lib))]).code, 0);
    let lock_before = std::fs::read_to_string(p.join("brasa.lock")).unwrap();
    let new = commit(&lib, &[("lib.br", "package lib\npub func n() -> int\n    return 2\nend\n")], "moved");
    git(&lib, &["tag", "-f", "v1.0.0"]);
    let r = brasa(&p, &home, &["--fetch"]);
    assert_eq!(r.code, 1);
    assert!(r.err.contains("moved") && r.err.contains(&old) && r.err.contains(&new), "{}", r.err);
    assert_eq!(std::fs::read_to_string(p.join("brasa.lock")).unwrap(), lock_before);
    // --update accepts it
    let u = brasa(&p, &home, &["--update"]);
    assert_eq!(u.code, 0, "{}{}", u.out, u.err);
    let lock = Lock::parse(&std::fs::read_to_string(p.join("brasa.lock")).unwrap()).unwrap();
    assert_eq!(lock.entries[0].commit, new);
}

#[test]
fn update_raises_the_minimum_to_the_newest_tag_of_the_major() {
    need_git!();
    let (home, repos) = (scratch("home"), scratch("repos"));
    let lib = repos.join("lib");
    repo(&lib, &[("lib.br", "package lib\npub func n() -> int\n    return 1\nend\n")]);
    git(&lib, &["tag", "v1.0.0"]);
    let p = project("update", "package main\nuse lib\n\nfunc main()\n    print(\"{lib.n()}\")\nend\n");
    init(&p, &home);
    assert_eq!(brasa(&p, &home, &["--add", &format!("{}@1.0.0", url_of(&lib))]).code, 0);
    commit(&lib, &[("lib.br", "package lib\npub func n() -> int\n    return 4\nend\n")], "second");
    git(&lib, &["tag", "v1.3.0"]);
    commit(&lib, &[("lib.br", "package lib\npub func n() -> int\n    return 9\nend\n")], "third");
    git(&lib, &["tag", "v2.0.0"]);
    // without --update the minimum stays: nothing newer is chosen merely because it exists
    assert_eq!(brasa(&p, &home, &["--fetch"]).code, 0);
    let lock = Lock::parse(&std::fs::read_to_string(p.join("brasa.lock")).unwrap()).unwrap();
    assert_eq!(lock.entries[0].version.to_string(), "1.0.0");
    let u = brasa(&p, &home, &["--update", "lib"]);
    assert_eq!(u.code, 0, "{}{}", u.out, u.err);
    let m = Manifest::parse(&std::fs::read_to_string(p.join("brasa.toml")).unwrap()).unwrap();
    assert_eq!(m.deps[0].version.to_string(), "1.3.0", "version 2 is another package");
    assert_eq!(build_and_run(&p, &home).unwrap(), "4\n");
    assert_eq!(brasa(&p, &home, &["--update", "nothing"]).code, 1);
}

#[test]
fn tidy_removes_unused_dependencies_offline() {
    need_git!();
    let (home, repos) = (scratch("home"), scratch("repos"));
    for n in ["used", "spare"] {
        let r = repos.join(n);
        repo(&r, &[(&format!("{}.br", n), &format!("package {}\npub func name() -> text\n    return \"{}\"\nend\n", n, n))]);
        git(&r, &["tag", "v1.0.0"]);
    }
    let p = project("tidy", "package main\nuse used\n\nfunc main()\n    print(used.name())\nend\n");
    init(&p, &home);
    for n in ["used", "spare"] {
        assert_eq!(brasa(&p, &home, &["--add", &format!("{}@1.0.0", url_of(&repos.join(n)))]).code, 0);
    }
    assert_eq!(Lock::parse(&std::fs::read_to_string(p.join("brasa.lock")).unwrap()).unwrap().entries.len(), 2);
    // tidy needs neither the network nor git
    let r = brasa_env(&p, &home, &["--tidy"], Some(""));
    assert_eq!(r.code, 0, "{}{}", r.out, r.err);
    let m = Manifest::parse(&std::fs::read_to_string(p.join("brasa.toml")).unwrap()).unwrap();
    assert_eq!(m.deps.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(), ["used"]);
    let lock = Lock::parse(&std::fs::read_to_string(p.join("brasa.lock")).unwrap()).unwrap();
    assert_eq!(lock.entries.len(), 1);
    assert!(lock.entries[0].module.ends_with("/used"));
}

#[test]
fn reserved_names_are_refused_and_nothing_changes() {
    let home = scratch("home");
    let p = scratch("reserved");
    init(&p, &home);
    let before = std::fs::read_to_string(p.join("brasa.toml")).unwrap();
    let r = brasa(&p, &home, &["--add", "https://example.org/x/text@1.0.0"]);
    assert_eq!(r.code, 1);
    assert!(r.err.contains("standard library"), "{}", r.err);
    let r = brasa(&p, &home, &["--add", "https://example.org/x/lib@1.0.0", "--as", "format"]);
    assert_eq!(r.code, 1);
    assert!(r.err.contains("standard library"), "{}", r.err);
    assert_eq!(std::fs::read_to_string(p.join("brasa.toml")).unwrap(), before);
    assert!(!p.join("brasa.lock").exists());
    // a hand-written manifest is refused too
    write(&p, "brasa.toml", "[package]\nname = \"a\"\n\n[dependencies.list]\ngit = \"https://example.org/x/list\"\nversion = \"1.0.0\"\n");
    write(&p, "main.br", "package main\nfunc main()\nend\n");
    let b = brasa(&p, &home, &["main.br", "--ast"]);
    assert_eq!(b.code, 1);
    assert!(b.err.contains("standard library"), "{}", b.err);
}

#[test]
fn git_missing_is_explained() {
    let home = scratch("home");
    let p = scratch("nogit");
    write(&p, "brasa.toml", "[package]\nname = \"a\"\n\n[dependencies.lib]\ngit = \"https://example.org/x/lib\"\nversion = \"1.0.0\"\n");
    let r = brasa_env(&p, &home, &["--fetch"], Some(""));
    assert_eq!(r.code, 1);
    assert!(r.err.contains("error: git was not found on PATH. It is used only to download dependencies (brasa-parse --fetch); building from an already populated cache does not need it."), "{}", r.err);
}

#[test]
fn building_from_a_populated_cache_needs_neither_git_nor_network() {
    need_git!();
    let (home, repos) = (scratch("home"), scratch("repos"));
    let codec = repos.join("codec");
    repo(&codec, &[("codec.br", JSON_V1)]);
    git(&codec, &["tag", "v1.0.0"]);
    let p = project("offline", "package main\nuse codec\n\nfunc main()\n    print(codec.version())\nend\n");
    init(&p, &home);
    assert_eq!(brasa(&p, &home, &["--add", &format!("{}@1.0.0", url_of(&codec))]).code, 0);
    // the repository disappears and git is not on PATH
    std::fs::remove_dir_all(&codec).unwrap();
    let exe = p.join(exe_name("app"));
    let mut args = vec!["main.br", "-o", exe.to_str().unwrap()];
    if !cfg!(windows) {
        args.extend_from_slice(&["--target", "linux"]);
    }
    let b = brasa_env(&p, &home, &args, Some(""));
    assert_eq!(b.code, 0, "{}{}", b.out, b.err);
    // --offline is accepted for builds and for --fetch from the cache
    let f = brasa_env(&p, &home, &["--fetch", "--offline"], Some(""));
    assert_eq!(f.code, 0, "{}{}", f.out, f.err);
    let b2 = brasa(&p, &home, &["main.br", "--offline", "--ast"]);
    assert_eq!(b2.code, 0, "{}{}", b2.out, b2.err);
}

#[test]
fn a_missing_cache_entry_and_offline_fetch_name_the_package() {
    need_git!();
    let (home, repos) = (scratch("home"), scratch("repos"));
    let codec = repos.join("codec");
    repo(&codec, &[("codec.br", JSON_V1)]);
    git(&codec, &["tag", "v1.0.0"]);
    let p = project("missing", "package main\nuse codec\n\nfunc main()\n    print(codec.version())\nend\n");
    init(&p, &home);
    assert_eq!(brasa(&p, &home, &["--add", &format!("{}@1.0.0", url_of(&codec))]).code, 0);
    std::fs::remove_dir_all(home.join("src")).unwrap();
    let b = brasa(&p, &home, &["main.br", "--ast"]);
    assert_eq!(b.code, 1);
    assert!(b.err.contains("package '") && b.err.contains("/codec@1.0.0' is not in the cache; run brasa-parse --fetch"), "{}", b.err);
    let f = brasa(&p, &home, &["--fetch", "--offline"]);
    assert_eq!(f.code, 1);
    assert!(f.err.contains("/codec@1.0.0") && f.err.contains("not in the cache"), "{}", f.err);
    // a normal fetch restores it
    assert_eq!(brasa(&p, &home, &["--fetch"]).code, 0);
    assert_eq!(brasa(&p, &home, &["main.br", "--ast"]).code, 0);
}

#[test]
fn a_tampered_cache_and_a_stale_lock_are_refused() {
    need_git!();
    let (home, repos) = (scratch("home"), scratch("repos"));
    let codec = repos.join("codec");
    repo(&codec, &[("codec.br", JSON_V1)]);
    git(&codec, &["tag", "v1.0.0"]);
    let p = project("tamper", "package main\nuse codec\n\nfunc main()\n    print(codec.version())\nend\n");
    init(&p, &home);
    assert_eq!(brasa(&p, &home, &["--add", &format!("{}@1.0.0", url_of(&codec))]).code, 0);
    let lock = Lock::parse(&std::fs::read_to_string(p.join("brasa.lock")).unwrap()).unwrap();
    let e = &lock.entries[0];
    let dir = cache_dir(&home, &e.module, &e.version, &e.commit);
    let file = dir.join("codec.br");
    let original = std::fs::read_to_string(&file).unwrap();
    std::fs::write(&file, original.replace("codec 1", "codec X")).unwrap();
    let b = brasa(&p, &home, &["main.br", "--ast"]);
    assert_eq!(b.code, 1);
    assert!(b.err.contains("does not match brasa.lock") && b.err.contains(&format!("expected {}", e.hash)) && b.err.contains("found sha256:") && b.err.contains("run brasa-parse --fetch to restore it"), "{}", b.err);
    // --fetch puts it back
    assert_eq!(brasa(&p, &home, &["--fetch"]).code, 0);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), original);
    assert_eq!(build_and_run(&p, &home).unwrap(), "codec 1\n");
    // a dependency added by hand without --fetch
    let other = repos.join("other");
    repo(&other, &[("other.br", "package other\npub func x() -> int\n    return 1\nend\n")]);
    git(&other, &["tag", "v1.0.0"]);
    let mut m = Manifest::parse(&std::fs::read_to_string(p.join("brasa.toml")).unwrap()).unwrap();
    m.deps.push(Dep { name: "other".into(), git: url_of(&other), version: Version::parse("1.0.0").unwrap() });
    std::fs::write(p.join("brasa.toml"), m.write()).unwrap();
    let b = brasa(&p, &home, &["main.br", "--ast"]);
    assert_eq!(b.code, 1);
    assert!(b.err.contains("brasa.lock is out of date; run brasa-parse --fetch"), "{}", b.err);
}

#[test]
fn the_requirement_with_the_highest_minimum_wins_across_packages() {
    need_git!();
    let (home, repos) = (scratch("home"), scratch("repos"));
    let lib = repos.join("lib");
    repo(&lib, &[("lib.br", "package lib\npub func v() -> text\n    return \"1.2.0\"\nend\n")]);
    git(&lib, &["tag", "v1.2.0"]);
    commit(&lib, &[("lib.br", "package lib\npub func v() -> text\n    return \"1.4.1\"\nend\n")], "b");
    git(&lib, &["tag", "v1.4.1"]);
    commit(&lib, &[("lib.br", "package lib\npub func v() -> text\n    return \"1.9.0\"\nend\n")], "c");
    git(&lib, &["tag", "v1.9.0"]);
    let dep = repos.join("dep");
    repo(
        &dep,
        &[
            ("dep.br", "package dep\nuse lib\n\npub func v() -> text\n    return \"dep sees lib {lib.v()}\"\nend\n"),
            ("brasa.toml", &format!("[package]\nname = \"dep\"\n\n[dependencies.lib]\ngit = \"{}\"\nversion = \"1.4.1\"\n", url_of(&lib))),
        ],
    );
    git(&dep, &["tag", "v1.0.0"]);
    let p = project("mvs", "package main\nuse dep\nuse lib\n\nfunc main()\n    print(dep.v())\n    print(lib.v())\nend\n");
    write(
        &p,
        "brasa.toml",
        &format!("[package]\nname = \"app\"\n\n[dependencies.dep]\ngit = \"{}\"\nversion = \"1.0.0\"\n\n[dependencies.lib]\ngit = \"{}\"\nversion = \"1.2.0\"\n", url_of(&dep), url_of(&lib)),
    );
    let r = brasa(&p, &home, &["--fetch"]);
    assert_eq!(r.code, 0, "{}{}", r.out, r.err);
    let lock = Lock::parse(&std::fs::read_to_string(p.join("brasa.lock")).unwrap()).unwrap();
    let lib_entry = lock.entries.iter().find(|e| e.module.ends_with("/lib")).unwrap();
    assert_eq!(lib_entry.version.to_string(), "1.4.1", "the highest minimum, never 1.9.0");
    let dep_entry = lock.entries.iter().find(|e| e.module.ends_with("/dep")).unwrap();
    assert_eq!(dep_entry.needs, vec![lib_entry.id()]);
    assert_eq!(build_and_run(&p, &home).unwrap(), "dep sees lib 1.4.1\n1.4.1\n");
}

// ---------------------------------------------------------------- majors

#[test]
fn majors_coexist_with_distinct_types_even_below_the_project() {
    need_git!();
    let (home, repos) = (scratch("home"), scratch("repos"));
    let codec = repos.join("codec");
    let v1 = "package codec\n\npub type Box = record\n    pub n: int\nend\n\npub func make() -> Box\n    return Box{ n: 1 }\nend\n\npub func version() -> text\n    return \"one\"\nend\n";
    let v2 = v1.replace("n: 1", "n: 2").replace("\"one\"", "\"two\"");
    repo(&codec, &[("codec.br", v1)]);
    git(&codec, &["tag", "v1.0.0"]);
    commit(&codec, &[("codec.br", &v2)], "v2");
    git(&codec, &["tag", "v2.0.0"]);
    let (a, b) = (repos.join("a"), repos.join("b"));
    let (u1, u2) = (url_of(&codec), format!("{}/v2", url_of(&codec)));
    repo(
        &a,
        &[
            ("a.br", "package a\nuse codec\n\npub func v() -> text\n    return \"a uses {codec.version()} {codec.make().n}\"\nend\n"),
            ("brasa.toml", &format!("[package]\nname = \"a\"\n\n[dependencies.codec]\ngit = \"{}\"\nversion = \"1.0.0\"\n", u1)),
        ],
    );
    git(&a, &["tag", "v1.0.0"]);
    repo(
        &b,
        &[
            ("b.br", "package b\nuse codec\n\npub func v() -> text\n    return \"b uses {codec.version()} {codec.make().n}\"\nend\n"),
            ("brasa.toml", &format!("[package]\nname = \"b\"\n\n[dependencies.codec]\ngit = \"{}\"\nversion = \"2.0.0\"\n", u2)),
        ],
    );
    git(&b, &["tag", "v1.0.0"]);
    let main = "package main\nuse a, b\n\nfunc main()\n    print(a.v())\n    print(b.v())\nend\n";
    let p = project("majors", main);
    write(&p, "brasa.toml", &format!("[package]\nname = \"app\"\n\n[dependencies.a]\ngit = \"{}\"\nversion = \"1.0.0\"\n\n[dependencies.b]\ngit = \"{}\"\nversion = \"1.0.0\"\n", url_of(&a), url_of(&b)));
    let r = brasa(&p, &home, &["--fetch"]);
    assert_eq!(r.code, 0, "{}{}", r.out, r.err);
    let lock = Lock::parse(&std::fs::read_to_string(p.join("brasa.lock")).unwrap()).unwrap();
    assert_eq!(lock.entries.len(), 4, "a, b and both majors of codec");
    assert_eq!(build_and_run(&p, &home).unwrap(), "a uses one 1\nb uses two 2\n");

    // codec and codec_v2 in the project's own manifest: both usable, their types are different types
    let q = project("majors2", "package main\nuse codec\nuse codec_v2\n\nfunc main()\n    print(\"{codec.make().n} {codec_v2.make().n}\")\nend\n");
    init(&q, &home);
    assert_eq!(brasa(&q, &home, &["--add", &format!("{}@1.0.0", u1)]).code, 0);
    let add2 = brasa(&q, &home, &["--add", &format!("{}@2.0.0", u2)]);
    assert_eq!(add2.code, 0, "{}{}", add2.out, add2.err);
    let m = Manifest::parse(&std::fs::read_to_string(q.join("brasa.toml")).unwrap()).unwrap();
    assert_eq!(m.deps.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(), ["codec", "codec_v2"]);
    assert_eq!(build_and_run(&q, &home).unwrap(), "1 2\n");
    write(&q, "main.br", "package main\nuse codec\nuse codec_v2\n\nfunc take(b: codec_v2.Box) -> int\n    return b.n\nend\n\nfunc main()\n    let b = codec.make()\n    print(\"{take(b)}\")\nend\n");
    let bad = brasa(&q, &home, &["main.br", "-o", q.join(exe_name("bad")).to_str().unwrap()]);
    assert_eq!(bad.code, 1, "{}{}", bad.out, bad.err);
}

// ---------------------------------------------------------------- local packages and merging

fn local_project(name: &str, main: &str, files: &[(&str, &str)]) -> (PathBuf, PathBuf) {
    let home = scratch("home");
    let p = project(name, main);
    write(&p, "brasa.toml", "[package]\nname = \"app\"\n");
    for (f, t) in files {
        write(&p, f, t);
    }
    (p, home)
}

#[test]
fn a_local_package_is_a_directory_of_files_merged_into_one_module() {
    let (p, home) = local_project(
        "local",
        "package main\nuse util\n\nfunc main()\n    print(util.greet(\"Ana\"))\n    print(\"{util.twice(4)}\")\nend\n",
        &[
            ("util/a.br", "package util\n\npub func greet(n: text) -> text\n    return \"{prefix()}{n}\"\nend\n"),
            ("util/b.br", "package util\n\nfunc prefix() -> text\n    return \"Hello, \"\nend\n\npub func twice(n: int) -> int\n    return n * 2\nend\n"),
            // a test file does not belong to builds
            ("util/util_test.br", "package util\n\n#[test]\nfunc t_twice()\n    assert_eq(twice(2), 4)\nend\nfunc twice(n: int) -> int\n    return 0\nend\n"),
        ],
    );
    assert_eq!(build_and_run(&p, &home).unwrap(), "Hello, Ana\n8\n");
}

#[test]
fn a_duplicate_declaration_names_both_files() {
    let (p, home) = local_project(
        "dup",
        "package main\nuse util\n\nfunc main()\n    print(util.one())\nend\n",
        &[("util/a.br", "package util\n\npub func one() -> text\n    return \"a\"\nend\n"), ("util/b.br", "package util\n\npub func one() -> text\n    return \"b\"\nend\n")],
    );
    let b = brasa(&p, &home, &["main.br", "-o", p.join(exe_name("x")).to_str().unwrap()]);
    assert_eq!(b.code, 1);
    assert!(b.err.contains("'one' is declared twice") && b.err.contains("a.br") && b.err.contains("b.br"), "{}", b.err);
}

#[test]
fn the_package_line_must_match() {
    let (p, home) = local_project(
        "wrongpkg",
        "package main\nuse util\n\nfunc main()\n    print(util.one())\nend\n",
        &[("util/a.br", "package util\n\npub func one() -> text\n    return \"a\"\nend\n"), ("util/b.br", "package other\n\npub func two() -> text\n    return \"b\"\nend\n")],
    );
    let b = brasa(&p, &home, &["main.br", "-o", p.join(exe_name("x")).to_str().unwrap()]);
    assert_eq!(b.code, 1);
    assert!(b.err.contains("b.br") && b.err.contains("package other") && b.err.contains("'util'"), "{}", b.err);
}

#[test]
fn sub_packages_are_directories_and_the_last_segment_is_the_name() {
    let (p, home) = local_project(
        "sub",
        "package main\nuse util.stream\n\nfunc main()\n    print(stream.next())\nend\n",
        &[("util/util.br", "package util\n\npub func top() -> int\n    return 1\nend\n"), ("util/stream/stream.br", "package stream\n\npub func next() -> text\n    return \"next\"\nend\n")],
    );
    assert_eq!(build_and_run(&p, &home).unwrap(), "next\n");
}

#[test]
fn a_private_item_of_another_package_is_refused() {
    let (p, home) = local_project(
        "private",
        "package main\nuse util\n\nfunc main()\n    print(util.secret())\nend\n",
        &[("util/util.br", "package util\n\nfunc secret() -> text\n    return \"s\"\nend\n\npub func open() -> text\n    return secret()\nend\n")],
    );
    let b = brasa(&p, &home, &["main.br", "-o", p.join(exe_name("x")).to_str().unwrap()]);
    assert_eq!(b.code, 1);
    assert!(b.err.contains("secret") && b.err.contains("util") && b.err.contains("private"), "{}", b.err);
}

#[test]
fn a_local_directory_cannot_take_a_standard_name() {
    let (p, home) = local_project("stdname", "package main\n\nfunc main()\nend\n", &[("text/text.br", "package text\n\npub func x() -> int\n    return 1\nend\n")]);
    let b = brasa(&p, &home, &["main.br", "--ast"]);
    assert_eq!(b.code, 1);
    assert!(b.err.contains("'text'") && b.err.contains("standard library"), "{}", b.err);
}

#[test]
fn an_unknown_name_is_an_error_with_the_hint() {
    let home = scratch("home");
    let p = project("unknown", "package main\nuse nothing\n\nfunc main()\nend\n");
    let b = brasa(&p, &home, &["main.br", "-o", p.join(exe_name("x")).to_str().unwrap()]);
    assert_eq!(b.code, 1);
    assert!(b.err.contains("unknown package 'nothing'") && b.err.contains("brasa-parse --add <git-url>"), "{}{}", b.out, b.err);
}

#[test]
fn programs_without_a_manifest_are_unchanged() {
    let home = scratch("home");
    // the whole standard library and the sibling files of the tests before this change
    let p = project("plain", "package main\nuse text\nuse list\n\nfunc main()\n    print(\"plain\")\nend\n");
    assert_eq!(build_and_run(&p, &home).unwrap(), "plain\n");
    let hello = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/hello.br");
    let out = p.join(exe_name("hello"));
    let b = brasa(&p, &home, &[hello.to_str().unwrap(), "-o", out.to_str().unwrap()]);
    assert_eq!(b.code, 0, "{}{}", b.out, b.err);
}

#[test]
fn the_tests_of_a_package_run_with_its_files() {
    let (p, home) = local_project(
        "pkgtest",
        "package main\nuse util\n\nfunc main()\n    print(\"{util.twice(2)}\")\nend\n",
        &[
            ("util/a.br", "package util\n\nfunc half(n: int) -> int\n    return n / 2\nend\n"),
            ("util/b.br", "package util\n\npub func twice(n: int) -> int\n    return n * 2\nend\n"),
            ("util/util_test.br", "package util\n\n#[test]\nfunc t_twice()\n    assert_eq(twice(2), 4)\nend\n\n#[test]\nfunc t_half()\n    assert_eq(half(8), 4)\nend\n"),
        ],
    );
    let r = brasa(&p, &home, &["--test", "util/util_test.br"]);
    assert_eq!(r.code, 0, "{}{}", r.out, r.err);
    assert!(r.out.contains("ok t_twice") && r.out.contains("ok t_half") && r.out.contains("2 passed, 0 failed"), "{}", r.out);
}

#[test]
fn tests_of_a_project_with_a_dependency_run() {
    need_git!();
    let (home, repos) = (scratch("home"), scratch("repos"));
    let codec = repos.join("codec");
    repo(&codec, &[("codec.br", JSON_V1)]);
    git(&codec, &["tag", "v1.0.0"]);
    let p = project("deptest", "package main\nuse codec\n\n#[test]\nfunc t_version()\n    assert_eq(codec.version(), \"codec 1\")\nend\n\nfunc main()\nend\n");
    init(&p, &home);
    assert_eq!(brasa(&p, &home, &["--add", &format!("{}@1.0.0", url_of(&codec))]).code, 0);
    let r = brasa(&p, &home, &["--test", "main.br"]);
    assert_eq!(r.code, 0, "{}{}", r.out, r.err);
    assert!(r.out.contains("ok t_version"), "{}", r.out);
}

// ---------------------------------------------------------------- the sample project

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if src.is_dir() {
            copy_dir(&src, &dst);
        } else {
            std::fs::copy(&src, &dst).unwrap();
        }
    }
}

#[test]
fn the_sample_project_builds_offline_from_a_prepared_cache() {
    let proj = scratch("sample");
    copy_dir(Path::new("examples/packages"), &proj);
    let manifest = Manifest::parse(&std::fs::read_to_string(proj.join("brasa.toml")).unwrap()).unwrap();
    let lock = Lock::parse(&std::fs::read_to_string(proj.join("brasa.lock")).unwrap()).unwrap();
    // the committed lock covers the committed manifest
    for d in &manifest.deps {
        assert!(lock.for_module(&d.module()).map_or(false, |e| e.version >= d.version), "{} is not locked", d.name);
    }
    // the test fills the cache from the repository fixture and records its hash, as --fetch would
    let home = scratch("home");
    let mut lock = lock;
    for e in lock.entries.iter_mut() {
        let name = e.module.rsplit('/').next().unwrap().to_string();
        let fixture = proj.join("fixture").join(&name);
        assert!(fixture.is_dir(), "no fixture for {}", e.module);
        let dir = cache_dir(&home, &e.module, &e.version, &e.commit);
        copy_dir(&fixture, &dir);
        e.hash = tree_hash(&dir).unwrap();
    }
    std::fs::write(proj.join("brasa.lock"), lock.write()).unwrap();
    // no git, no network
    let out = proj.join(exe_name("sample"));
    let mut args = vec!["main.br", "-o", out.to_str().unwrap()];
    if !cfg!(windows) {
        args.extend_from_slice(&["--target", "linux"]);
    }
    let b = brasa_env(&proj, &home, &args, Some(""));
    assert_eq!(b.code, 0, "{}{}", b.out, b.err);
    let run = Command::new(&out).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&run.stdout).replace('\r', ""), "Hello, Brasa!\nHello again, Brasa!\n3 times 4 is 12\n");
}

// ---------------------------------------------------------------- structure

#[test]
fn the_build_path_never_references_git() {
    // the git runner lives in src/fetch.rs; only the package commands import it
    let pkg = std::fs::read_to_string("src/package.rs").unwrap();
    assert!(!pkg.contains("Command::new") && !pkg.contains("fetch::"), "src/package.rs must not run programs or use the fetch module");
    for f in ["src/codegen_x64.rs", "src/lsp.rs", "src/testrun.rs", "src/run.rs", "src/comptime.rs", "src/fmt.rs"] {
        let text = std::fs::read_to_string(f).unwrap();
        assert!(!text.contains("fetch::"), "{} imports the fetch module", f);
        assert!(!text.contains("ls_remote") && !text.contains("clone_commit"), "{} references the git runner", f);
    }
    let main = std::fs::read_to_string("src/main.rs").unwrap();
    assert_eq!(main.matches("fetch::run").count(), 1, "main.rs reaches the fetch module only to dispatch the package commands");
    let fetch = std::fs::read_to_string("src/fetch.rs").unwrap();
    assert!(fetch.contains("GIT_TERMINAL_PROMPT") && fetch.contains("Command::new(\"git\")"));
    // no new crate
    let toml = std::fs::read_to_string("Cargo.toml").unwrap();
    let deps = toml.split("[dependencies]").nth(1).unwrap_or("");
    assert!(deps.trim().is_empty(), "Cargo.toml has dependencies: {}", deps);
}

#[test]
fn edition_is_stored_and_changes_nothing() {
    let home = scratch("home");
    let p = project("edition", "package main\n\nfunc main()\n    print(\"e\")\nend\n");
    write(&p, "brasa.toml", "[package]\nname = \"a\"\nedition = \"2027\"\n");
    assert_eq!(build_and_run(&p, &home).unwrap(), "e\n");
}

#[test]
fn unsupported_toml_stops_the_tool() {
    let home = scratch("home");
    let p = project("badtoml", "package main\n\nfunc main()\nend\n");
    write(&p, "brasa.toml", "[package]\nname = \"a\"\nnames = [\"a\", \"b\"]\n");
    for args in [&["main.br", "--ast"][..], &["--fetch"][..]] {
        let r = brasa(&p, &home, args);
        assert_eq!(r.code, 1);
        assert!(r.err.contains("line 3") && r.err.contains("arrays are not supported"), "{}", r.err);
    }
    assert!(!p.join("brasa.lock").exists());
}

// ---------------------------------------------------------------- repository content

/// The names a file asks for with `use` (the first segment of each).
fn used_names(text: &str) -> Vec<String> {
    brasa::comptime::use_names(text)
}

#[test]
fn no_example_uses_a_package_that_exists_nowhere() {
    let mut checked = 0;
    for e in std::fs::read_dir("examples").unwrap().flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("br") {
            continue;
        }
        let src = std::fs::read_to_string(&p).unwrap();
        let file = brasa::parse_source(&src).unwrap_or_else(|d| panic!("{}: {}", p.display(), d.msg));
        if let Err(e) = brasa::codegen_x64::generate_asm_for(&file, &brasa::codegen_x64::CompileOptions::default(), brasa::codegen_x64::Target::Windows) {
            assert!(!e.contains("unknown package"), "{}: {}", p.display(), e);
        }
        checked += 1;
    }
    assert!(checked >= 20, "only {} examples checked", checked);
}

#[test]
fn std_and_os_use_only_standard_names() {
    let mut problems = Vec::new();
    for dir in ["std", "os"] {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("br") {
                continue;
            }
            for n in used_names(&std::fs::read_to_string(&p).unwrap()) {
                if brasa::codegen_x64::std_source(&n).is_none() && n != "c" {
                    problems.push(format!("{}: use {}", p.display(), n));
                }
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
