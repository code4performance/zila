//! The VS Code extension (`editors/vscode`): its Node tests run against the freshly built
//! `zila --lsp`. Skipped when `node` is not installed.

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
        .env("ZILA_BIN", env!("CARGO_BIN_EXE_zila"))
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "node tests failed:\n{}", text);
    assert!(text.contains("pass 14"), "expected 14 passing tests:\n{}", text);
}

#[test]
fn the_package_manifest_declares_the_settings_command_and_entry_point() {
    let text = std::fs::read_to_string("editors/vscode/package.json").unwrap();
    for needle in ["\"main\": \"./extension.js\"", "zila.server.path", "zila.server.enabled", "zila.trace", "zila.restartServer", "restrictedConfigurations"] {
        assert!(text.contains(needle), "package.json lacks {}", needle);
    }
    for file in ["editors/vscode/extension.js", "editors/vscode/client.js", "editors/vscode/.vscodeignore", "editors/vscode/README.md"] {
        assert!(std::path::Path::new(file).exists(), "{} is missing", file);
    }
    // tests are not shipped in the .vsix
    assert!(std::fs::read_to_string("editors/vscode/.vscodeignore").unwrap().contains("test/**"));
}

#[test]
fn the_grammar_highlights_every_lexer_keyword_including_pub() {
    let g = std::fs::read_to_string("editors/vscode/syntaxes/zila.tmLanguage.json").unwrap();
    assert!(g.contains("let|var|embed|extern|pub"), "pub is not a declaration keyword in the grammar");
    for kw in zila::lexer::KEYWORDS {
        assert!(g.split(|c: char| !c.is_alphanumeric() && c != '_').any(|w| w == *kw), "the grammar does not mention the keyword {}", kw);
    }
}

#[test]
fn the_grammar_opens_a_sql_region_only_for_a_sql_block() {
    let g = std::fs::read_to_string("editors/vscode/syntaxes/zila.tmLanguage.json").unwrap();
    assert!(g.contains("\"sql-blocks\"") && g.contains("\"include\": \"source.sql\"") && g.contains("\"include\": \"source.zila\""), "the SQL region and its embedded languages are missing");
    if !node_available() {
        eprintln!("node is not installed: skipping the pattern check");
        return;
    }
    // the begin and end patterns are plain regular expressions: try them on lines
    let script = r#"
const g = JSON.parse(require('fs').readFileSync('editors/vscode/syntaxes/zila.tmLanguage.json', 'utf8'));
const r = g.repository['sql-blocks'].patterns[0];
const begin = new RegExp(r.begin), end = new RegExp(r.end);
const check = (ok, what) => { if (!ok) { console.error('FAIL ' + what); process.exit(1); } };
check(begin.test('    let q = sql postgres'), 'sql postgres opens');
check(begin.test('return sql tsql // note'), 'a comment may follow');
check(!begin.test('func run(sql: text)'), 'a parameter named sql');
check(!begin.test('    print(sql)'), 'sql in a call');
check(!begin.test('    let sql = "x"'), 'a variable named sql');
check(end.test('    end'), 'end closes');
check(end.test('end // done'), 'end with a comment closes');
check(!end.test('    end_date is null'), 'end_date does not close');
check(!end.test('    -- end'), 'a SQL comment does not close');
console.log('ok');
"#;
    let out = Command::new("node").args(["-e", script]).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success() && text.contains("ok"), "{}", text);
}
