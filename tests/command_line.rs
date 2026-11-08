//! The name of the executable (spec `command-line`): it is `zila`, the old name is gone, and no
//! tracked text file outside `openspec/changes/` still uses it.

use std::path::Path;
use std::process::Command;

/// The old command, built from two parts so this file does not match itself.
fn old_name() -> String {
    format!("{}-{}", "zila", "parse")
}

#[test]
fn cargo_builds_an_executable_called_zila_and_no_other() {
    let exe = Path::new(env!("CARGO_BIN_EXE_zila"));
    assert_eq!(exe.file_stem().unwrap(), "zila", "{}", exe.display());

    let manifest = std::fs::read_to_string("Cargo.toml").unwrap();
    assert!(!manifest.contains(&old_name()), "Cargo.toml still names the old executable");
    assert_eq!(manifest.matches("[[bin]]").count(), 1, "exactly one executable is declared");
    assert!(manifest.contains("[[bin]]\nname = \"zila\"") || manifest.contains("[[bin]]\r\nname = \"zila\""));
    assert!(manifest.contains("[lib]\nname = \"zila\"") || manifest.contains("[lib]\r\nname = \"zila\""), "the library keeps its name");

    let old = exe.with_file_name(format!("{}{}", old_name(), std::env::consts::EXE_SUFFIX));
    // A stale file from earlier builds may exist in target/, but this build must not have made it
    // newer than the executable under test.
    if let (Ok(o), Ok(n)) = (std::fs::metadata(&old).and_then(|m| m.modified()), std::fs::metadata(exe).and_then(|m| m.modified())) {
        assert!(o < n, "{} was produced by this build", old.display());
    }
}

#[test]
fn the_options_of_the_toolchain_are_still_accepted() {
    let cases: &[&[&str]] = &[
        &["hello.zila"],
        &["hello.zila", "-o", "hello.exe"],
        &["hello.zila", "--run"],
        &["hello.zila", "--target", "linux", "-o", "hello"],
        &["--fmt", "a.zila"],
        &["--fmt-check", "a.zila"],
        &["--test", "a.zila"],
        &["--lsp"],
        &["--docs"],
        &["--init"],
        &["--add", "https://example.com/x.git@v1.0.0", "--as", "x"],
        &["--fetch", "--offline"],
        &["--tidy"],
        &["--update"],
        &["hello.zila", "--explain-link"],
        &["--os", "boot.zila", "kernel.zila", "-o", "disk.img"],
    ];
    for args in cases {
        let v: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        assert!(zila::cli::parse(&v).is_ok(), "{:?} is no longer accepted", args);
    }
    for flag in ["--run", "--fmt", "--fmt-check", "--test", "--lsp", "--docs", "--init", "--add", "--fetch", "--tidy", "--update", "--explain-link", "--target", "-o"] {
        assert!(zila::cli::FLAGS.contains(&flag), "{} left the option list", flag);
    }
}

#[test]
fn the_usage_text_names_the_command() {
    let out = Command::new(env!("CARGO_BIN_EXE_zila")).output().unwrap();
    let text = String::from_utf8_lossy(&out.stderr).to_string() + &String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("usage: zila "), "{}", text);
    assert!(!text.contains(&old_name()), "{}", text);
}

#[test]
fn no_tracked_text_file_outside_the_changes_names_the_old_command() {
    let listed = match Command::new("git").args(["ls-files", "-z"]).output() {
        Ok(o) if o.status.success() => o.stdout,
        _ => {
            eprintln!("git cannot list the files here: skipping");
            return;
        }
    };
    let old = old_name();
    let mut found = Vec::new();
    for rel in listed.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        let rel = String::from_utf8_lossy(rel).to_string();
        if rel.starts_with("openspec/changes/") {
            continue;
        }
        let Ok(bytes) = std::fs::read(&rel) else { continue };
        if bytes.contains(&0) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        for (i, line) in text.lines().enumerate() {
            if line.contains(&old) {
                found.push(format!("{}:{}: {}", rel, i + 1, line.trim()));
            }
        }
    }
    assert!(found.is_empty(), "the old command is still named:\n{}", found.join("\n"));
}
