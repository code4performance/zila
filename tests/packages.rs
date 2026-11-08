//! The standard libraries that are not core (spec `package-manager`, change `language-design-review`):
//! they exist as packages in `packages/`, a project's package may shadow one of them, the core names
//! stay reserved, and using one as part of the standard library warns.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const NON_CORE: [&str; 11] = ["json", "crypto", "tls", "ui", "web", "ai", "mcp", "rag", "db", "postgres", "sqlserver"];

fn brasa(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_brasa-parse")).args(args).current_dir(dir).output().unwrap()
}

fn text(o: &Output) -> (String, String) {
    (String::from_utf8_lossy(&o.stdout).replace('\r', ""), String::from_utf8_lossy(&o.stderr).replace('\r', ""))
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("brasa-pkgs-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let p = e.path();
        let t = to.join(e.file_name());
        if p.is_dir() {
            copy_dir(&p, &t);
        } else {
            std::fs::copy(&p, &t).unwrap();
        }
    }
}

fn norm(s: String) -> String {
    s.replace("\r\n", "\n")
}

#[test]
fn the_packages_mirror_the_standard_library() {
    for n in NON_CORE {
        let std = norm(std::fs::read_to_string(format!("std/{}.br", n)).unwrap());
        let pkg = norm(std::fs::read_to_string(format!("packages/{}/{}.br", n, n)).unwrap());
        assert!(std == pkg, "packages/{n}/{n}.br differs from std/{n}.br: copy it again (see packages/README.md)");
        let manifest = std::fs::read_to_string(format!("packages/{}/brasa.toml", n)).unwrap();
        assert!(manifest.contains(&format!("name = \"{}\"", n)) && manifest.contains("version = "), "packages/{}/brasa.toml", n);
    }
    assert!(Path::new("packages/brasa.toml").is_file());
}

#[test]
fn every_package_builds_as_a_local_package_and_says_it_shadows_the_standard_one() {
    let dir = scratch("each");
    copy_dir(Path::new("packages"), &dir);
    for n in NON_CORE {
        std::fs::write(dir.join("main.br"), format!("package main\n\nuse {}\n\nfunc main()\n    print(\"hi\")\nend\n", n)).unwrap();
        let o = brasa(&dir, &["main.br", "--asm"]);
        let (_, err) = text(&o);
        assert!(o.status.success(), "package {} does not build: {}", n, err);
        assert!(err.contains(&format!("shadows the standard library's '{}'", n)), "{}: {}", n, err);
        assert!(!err.contains("is moving out of the standard library"), "{}: {}", n, err);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_package_shadows_a_non_core_standard_library_and_the_standard_libraries_keep_their_own() {
    let dir = scratch("shadow");
    std::fs::write(dir.join("brasa.toml"), "[package]\nname = \"app\"\nversion = \"0.1.0\"\n").unwrap();
    std::fs::create_dir_all(dir.join("json")).unwrap();
    std::fs::write(dir.join("json").join("json.br"), "package json\n\npub func hello() -> text\n    return \"my json\"\nend\n").unwrap();
    std::fs::write(dir.join("main.br"), "package main\n\nuse json\n\nfunc main()\n    print(json.hello())\nend\n").unwrap();
    let o = brasa(&dir, &["main.br", "--run"]);
    let (out, err) = text(&o);
    assert_eq!(out, "my json\n", "{}", err);
    assert!(err.contains("the package 'json' shadows the standard library's 'json'"), "{}", err);
    // `rag` uses the standard `json` inside: the local package must not replace it there
    std::fs::write(dir.join("main.br"), "package main\n\nuse rag\n\nfunc main()\n    print(\"ok\")\nend\n").unwrap();
    let o = brasa(&dir, &["main.br", "--run"]);
    let (out, err) = text(&o);
    assert_eq!(out, "ok\n", "{}", err);
    assert!(!err.contains("shadows"), "{}", err);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn core_names_stay_reserved_and_other_names_may_be_dependencies() {
    use brasa::package::Manifest;
    let m = |name: &str| format!("[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies.{}]\ngit = \"https://example.org/x/{}\"\nversion = \"1.0.0\"\n", name, name);
    for core in ["text", "list", "fs", "net", "errors", "thread"] {
        let e = Manifest::parse(&m(core)).unwrap_err();
        assert!(e.contains("belongs to the standard library"), "{}: {}", core, e);
    }
    for open in NON_CORE {
        assert!(Manifest::parse(&m(open)).is_ok(), "{} should be allowed", open);
    }
    let dir = scratch("localcore");
    std::fs::write(dir.join("brasa.toml"), "[package]\nname = \"app\"\nversion = \"0.1.0\"\n").unwrap();
    std::fs::create_dir_all(dir.join("text")).unwrap();
    std::fs::write(dir.join("text").join("text.br"), "package text\n\npub func x() -> int\n    return 1\nend\n").unwrap();
    std::fs::write(dir.join("main.br"), "package main\nfunc main()\nend\n").unwrap();
    let o = brasa(&dir, &["main.br", "--asm"]);
    let (_, err) = text(&o);
    assert!(!o.status.success() && err.contains("core standard library"), "{}", err);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn using_a_non_core_library_as_standard_warns_and_core_ones_do_not() {
    let dir = scratch("warn");
    std::fs::write(dir.join("main.br"), "package main\n\nuse json\nuse text\n\nfunc main()\n    print(\"hi\")\nend\n").unwrap();
    let o = brasa(&dir, &["main.br", "--run"]);
    let (out, err) = text(&o);
    assert_eq!(out, "hi\n", "{}", err);
    assert!(err.contains("'json' is moving out of the standard library") && err.contains("packages/json"), "{}", err);
    assert!(!err.contains("'text'"), "{}", err);
    // the warning comes once per library, and building a library that uses another does not
    // warn about the inner one
    std::fs::write(dir.join("main.br"), "package main\n\nuse db\nuse postgres\n\nfunc main()\n    print(\"hi\")\nend\n").unwrap();
    let (_, err) = text(&brasa(&dir, &["main.br", "--asm"]));
    assert_eq!(err.matches("is moving out").count(), 2, "{}", err);
    let _ = std::fs::remove_dir_all(&dir);
}
