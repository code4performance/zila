//! The standard libraries that are not core (spec `package-manager`, change `language-design-review`):
//! they exist as packages in `packages/`, a project's package may shadow one of them, the core names
//! stay reserved, and a program with no manifest still uses them as part of the standard library.

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
fn the_non_core_libraries_live_in_packages_and_the_compiler_embeds_them_from_there() {
    for n in NON_CORE {
        assert!(!Path::new(&format!("std/{}.br", n)).exists(), "std/{}.br should not exist: the source is packages/{}/{}.br", n, n, n);
        let manifest = std::fs::read_to_string(format!("packages/{}/brasa.toml", n)).unwrap();
        assert!(manifest.contains(&format!("name = \"{}\"", n)) && manifest.contains("version = "), "packages/{}/brasa.toml", n);
        // the compiler's copy is the package's file
        let embedded = norm(brasa::codegen_x64::std_source(n).unwrap().to_string());
        assert!(embedded == norm(std::fs::read_to_string(format!("packages/{}/{}.br", n, n)).unwrap()), "{}", n);
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
fn using_a_non_core_library_as_standard_needs_no_manifest_and_prints_no_warning() {
    let dir = scratch("nowarn");
    std::fs::write(dir.join("main.br"), "package main

use json
use db
use postgres

func main()
    print(\"hi\")
end
").unwrap();
    let o = brasa(&dir, &["main.br", "--run"]);
    let (out, err) = text(&o);
    assert_eq!(out, "hi
", "{}", err);
    assert_eq!(err, "", "{}", err);
    let _ = std::fs::remove_dir_all(&dir);
}
