//! The VS Code extension (`editors/vscode`): its Node tests run against the freshly built
//! `brasa-parse --lsp`. Skipped when `node` is not installed.

use std::process::Command;

fn node_available() -> bool {
    Command::new("node").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

#[test]
fn the_extension_client_and_layer_pass_their_tests_against_the_real_server() {
    if !node_available() {
        eprintln!("node is not installed: skipping");
        return;
    }
    let out = Command::new("node")
        .args(["--test", "editors/vscode/test/client.test.js", "editors/vscode/test/extension.test.js"])
        .env("BRASA_PARSE", env!("CARGO_BIN_EXE_brasa-parse"))
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "node tests failed:\n{}", text);
    assert!(text.contains("pass 14"), "expected 14 passing tests:\n{}", text);
}

#[test]
fn the_package_manifest_declares_the_settings_command_and_entry_point() {
    let text = std::fs::read_to_string("editors/vscode/package.json").unwrap();
    for needle in ["\"main\": \"./extension.js\"", "brasa.server.path", "brasa.server.enabled", "brasa.trace", "brasa.restartServer", "restrictedConfigurations"] {
        assert!(text.contains(needle), "package.json lacks {}", needle);
    }
    for file in ["editors/vscode/extension.js", "editors/vscode/client.js", "editors/vscode/.vscodeignore", "editors/vscode/README.md"] {
        assert!(std::path::Path::new(file).exists(), "{} is missing", file);
    }
    // tests are not shipped in the .vsix
    assert!(std::fs::read_to_string("editors/vscode/.vscodeignore").unwrap().contains("test/**"));
}
