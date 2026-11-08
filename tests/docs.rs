//! The documentation site must follow the code. These tests fail when it does not:
//! example blocks differ from `examples/`, flags or keywords are undocumented (or
//! documented but gone), or the site's pages and links are inconsistent.
//!
//! After changing an example, run `zila-parse --docs` to refresh the blocks.

use zila::cli::FLAGS;
use zila::docs::{blocks, normalize, plain};
use zila::lexer::KEYWORDS;
use std::fs;
use std::path::Path;

struct Page {
    name: String,
    html: String,
}

fn pages() -> Vec<Page> {
    let mut v: Vec<Page> = fs::read_dir("docs")
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map_or(false, |x| x == "html"))
        .map(|p| Page {
            name: p.file_name().unwrap().to_string_lossy().to_string(),
            html: fs::read_to_string(&p).unwrap().replace("\r\n", "\n"),
        })
        .collect();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    assert!(!v.is_empty(), "docs/ has no pages");
    v
}

fn page(name: &str) -> Page {
    pages().into_iter().find(|p| p.name == name).unwrap_or_else(|| panic!("docs/{} is missing", name))
}

fn has_word(text: &str, word: &str) -> bool {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    text.match_indices(word).any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let after = text[i + word.len()..].chars().next();
        !before.map_or(false, is_word) && !after.map_or(false, is_word)
    })
}

/// Text between `<tag ...>` and `</tag>` for every occurrence.
fn elements<'a>(html: &'a str, open: &str, close: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = html[from..].find(open) {
        let start = from + i + open.len();
        let Some(j) = html[start..].find(close) else { break };
        out.push(&html[start..start + j]);
        from = start + j + close.len();
    }
    out
}

#[test]
fn page_helpers_read_the_site() {
    let all = pages();
    assert!(all.iter().any(|p| p.name == "index.html"));
    let hello = page("index.html");
    let b = blocks(&hello.html);
    assert!(b.iter().any(|b| b.example_path() == Some("examples/hello.zila")));
    assert_eq!(plain("<span class=\"kw\">let</span> a &lt; b"), "let a < b");
}

// ---------------------------------------------------------------- examples

#[test]
fn embedded_examples_match_the_example_files() {
    let mut seen = 0;
    let mut problems = Vec::new();
    for p in pages() {
        for b in blocks(&p.html) {
            let Some(example) = b.example_path() else { continue };
            seen += 1;
            let file = match fs::read_to_string(Path::new(example)) {
                Ok(f) => f,
                Err(e) => {
                    problems.push(format!("docs/{}: block labelled {} but the file cannot be read ({})", p.name, example, e));
                    continue;
                }
            };
            let shown = normalize(&plain(&p.html[b.start..b.end]));
            let source = normalize(&file);
            if b.is_excerpt() {
                if !source.contains(&shown) {
                    problems.push(format!("docs/{}: the excerpt of {} is not a contiguous part of the file", p.name, example));
                }
            } else if shown != source {
                let line = shown.lines().zip(source.lines()).position(|(a, b)| a != b).map_or(shown.lines().count().min(source.lines().count()) + 1, |i| i + 1);
                problems.push(format!("docs/{}: the block for {} differs from the file (first difference at line {}); run `zila-parse --docs`", p.name, example, line));
            }
        }
    }
    assert!(seen >= 4, "expected the labelled example blocks of the site, found {}", seen);
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
}

// ------------------------------------------------------------------- flags

#[test]
fn documented_flags_match_the_command_line() {
    let gs = page("getting-started.html");
    let text = plain(&gs.html);
    // every option the tool accepts is described
    let undocumented: Vec<&&str> = FLAGS.iter().filter(|f| !text.contains(**f)).collect();
    assert!(undocumented.is_empty(), "options missing from getting-started.html: {:?}", undocumented);
    // every option listed as a term of the options list exists
    let mut stale = Vec::new();
    for dt in elements(&gs.html, "<dt>", "</dt>") {
        let term = plain(dt);
        let first = term.split_whitespace().next().unwrap_or("");
        if first.starts_with('-') && !FLAGS.contains(&first) {
            stale.push(first.to_string());
        }
    }
    assert!(stale.is_empty(), "options documented but not accepted: {:?}", stale);
}

// ---------------------------------------------------------------- keywords

#[test]
fn documented_keywords_match_the_language() {
    let syntax = plain(&page("syntax.html").html);
    let missing: Vec<&&str> = KEYWORDS.iter().filter(|k| !has_word(&syntax, k)).collect();
    assert!(missing.is_empty(), "reserved words not mentioned in syntax.html: {:?}", missing);

    // a word highlighted as a keyword anywhere on the site must be one
    let mut stale = Vec::new();
    for p in pages() {
        for word in elements(&p.html, "<span class=\"kw\">", "</span>") {
            let w = plain(word);
            if w.chars().all(|c| c.is_alphanumeric() || c == '_') && !KEYWORDS.contains(&w.as_str()) {
                stale.push(format!("{} in {}", w, p.name));
            }
        }
    }
    stale.sort();
    stale.dedup();
    assert!(stale.is_empty(), "shown as keywords but not reserved: {:?}", stale);
}

// --------------------------------------------------------------- integrity

fn hrefs(html: &str) -> Vec<String> {
    elements(html, "href=\"", "\"").into_iter().map(|s| s.to_string()).collect()
}

#[test]
fn every_page_shares_the_navigation_and_is_linked() {
    let all = pages();
    let nav = |p: &Page| -> Vec<String> {
        let n = elements(&p.html, "<nav class=\"site-nav\">", "</nav>");
        assert_eq!(n.len(), 1, "docs/{} needs exactly one site navigation", p.name);
        hrefs(n[0])
    };
    let reference = nav(&all[0]);
    for p in &all {
        assert_eq!(nav(p), reference, "docs/{} has a different navigation from docs/{}", p.name, all[0].name);
    }
    for p in &all {
        assert!(reference.contains(&p.name), "docs/{} is not linked from the navigation", p.name);
    }
}

#[test]
fn internal_links_and_anchors_resolve() {
    let all = pages();
    let mut broken = Vec::new();
    for p in &all {
        for href in hrefs(&p.html) {
            if href.starts_with("http") || href.starts_with("mailto:") {
                continue;
            }
            let (target, anchor) = match href.split_once('#') {
                Some((t, a)) => (t.to_string(), Some(a.to_string())),
                None => (href.clone(), None),
            };
            let target = if target.is_empty() { p.name.clone() } else { target };
            if !Path::new("docs").join(&target).exists() {
                broken.push(format!("docs/{}: link to {} (no such file)", p.name, href));
                continue;
            }
            if let (Some(a), true) = (anchor, target.ends_with(".html")) {
                let html = &all.iter().find(|q| q.name == target).unwrap().html;
                if !html.contains(&format!("id=\"{}\"", a)) {
                    broken.push(format!("docs/{}: link to {} (no such anchor)", p.name, href));
                }
            }
        }
    }
    assert!(broken.is_empty(), "\n{}", broken.join("\n"));
}

// ---------------------------------------------------------------- the web library

#[test]
fn the_web_library_is_documented() {
    let stdlib = page("stdlib.html").html;
    assert!(stdlib.contains("id=\"web\""), "docs/stdlib.html has no web section");
    let text = plain(&stdlib);
    for needed in [
        "use web", "web.server", "app.get", "app.use", "app.run", "app.start", "web.logger", "web.read_json", "web.json", "web.created", "web.get", "web.post",
        "web.request", "max_connections", "read_timeout", "idle_timeout", "max_body", "html", "@for", "web.escape", "web.Safe", "raw(",
    ] {
        assert!(text.contains(needed), "docs/stdlib.html does not mention `{}`", needed);
    }
    // the examples page and the async and native pages point at it
    assert!(page("examples.html").html.contains("stdlib.html#web"));
    assert!(page("async.html").html.contains("stdlib.html#web"));
    assert!(plain(&page("native.html").html).contains("web: HTTP server"));
    assert!(std::fs::read_to_string("README.md").unwrap().contains("(`use web`)"));
}

// ---------------------------------------------------------------- the database libraries

#[test]
fn the_database_libraries_are_documented() {
    let stdlib = page("stdlib.html").html;
    for id in ["crypto", "db", "postgres", "sqlserver", "tls-extras"] {
        assert!(stdlib.contains(&format!("id=\"{}\"", id)), "docs/stdlib.html has no {} section", id);
    }
    for needed in ["db.query", "db.one", "conn.exec", "db.int", "db.redact", "postgres.open", "sqlserver.open", "sslrootcert", "encrypt=strict", "sha256", "tls.upgrade", "connect_with", "ca_file"] {
        assert!(stdlib.contains(needed), "docs/stdlib.html does not mention `{}`", needed);
    }
    let databases = page("databases.html").html;
    for needed in ["db.query", "postgres.open", "sqlserver.open", "encrypt=on", "tests/docker", "ZILA_TEST_POSTGRES", "examples/users_crud.zila", "examples/db.zila", "#[db("] {
        assert!(databases.contains(needed), "docs/databases.html does not mention `{}`", needed);
    }
    let native = page("native.html").html;
    for lib in ["crypto", "postgres", "sqlserver"] {
        assert!(native.contains(&format!("<code>{}</code>", lib)), "the Linux table of docs/native.html does not mention {}", lib);
    }
    assert!(std::fs::read_to_string("README.md").unwrap().contains("docs/databases.html"));
}

// ---------------------------------------------------------------- the ai library

#[test]
fn the_ai_library_is_documented() {
    let stdlib = page("stdlib.html").html;
    assert!(stdlib.contains("id=\"ai\""), "docs/stdlib.html has no ai section");
    for needed in ["ai.anthropic", "ai.openai_compatible", "client.chat", "client.stream", "client.run", "ai.ask", "ai.tool", "ai.schema", "sys.env", "web.stream"] {
        assert!(stdlib.contains(needed), "docs/stdlib.html does not mention `{}`", needed);
    }
    let ai = page("ai.html").html;
    for needed in ["key_from_env", "max_steps", "retry-after", "#[desc(", "untrusted", "embeddings"] {
        assert!(ai.contains(needed), "docs/ai.html does not mention `{}`", needed);
    }
}

#[test]
fn the_mcp_library_is_documented() {
    let stdlib = page("stdlib.html").html;
    assert!(stdlib.contains("id=\"mcp-lib\""), "docs/stdlib.html has no mcp section");
    for needed in ["mcp.server", "serve_stdio", "serve_http", "mcp.connect", "ai_tools", "bearer_token", "sys.stdin_read"] {
        assert!(stdlib.contains(needed), "docs/stdlib.html does not mention `{}`", needed);
    }
    let ai = page("ai.html").html;
    for needed in ["id=\"mcp\"", "Mcp-Session-Id", "isError", "-32602", "Origin", "examples/mcp_server.zila"] {
        assert!(ai.contains(needed), "docs/ai.html does not mention `{}`", needed);
    }
}

#[test]
fn the_rag_library_is_documented() {
    let stdlib = page("stdlib.html").html;
    assert!(stdlib.contains("id=\"rag-lib\""), "docs/stdlib.html has no rag section");
    for needed in ["rag.chunk", "rag.memory_store", "rag.pg_store", "rag.index", "rag.retrieve", "rag.answer", "rag.search_tool", "rag.load_store"] {
        assert!(stdlib.contains(needed), "docs/stdlib.html does not mention `{}`", needed);
    }
    let ai = page("ai.html").html;
    for needed in ["id=\"rag\"", "max_context_chars", "pgvector", "not instructions"] {
        assert!(ai.contains(needed), "docs/ai.html does not mention `{}`", needed);
    }
}

// ---------------------------------------------------------------- brand and layout

/// The fields of `docs/assets/description.txt`: `tagline`, `summary` and the `for` list.
fn description() -> (String, String, Vec<String>) {
    let text = fs::read_to_string("docs/assets/description.txt").unwrap().replace("\r\n", "\n");
    let (mut tagline, mut summary, mut items) = (String::new(), String::new(), Vec::new());
    for line in text.lines() {
        if let Some((k, v)) = line.split_once(':') {
            match k {
                "tagline" => tagline = v.trim().to_string(),
                "summary" => summary = v.trim().to_string(),
                "for" => items.push(v.trim().to_string()),
                _ => {}
            }
        }
    }
    (tagline, summary, items)
}

#[test]
fn the_description_is_one_text_of_the_right_size() {
    let (tagline, summary, items) = description();
    assert!(!tagline.is_empty() && tagline.chars().count() <= 90, "the tagline has {} characters (at most 90)", tagline.chars().count());
    assert!(!summary.is_empty() && summary.chars().count() <= 500, "the summary has {} characters (at most 500)", summary.chars().count());
    assert!(items.len() >= 3);
    for text in [&tagline, &summary] {
        assert!(!text.contains(['`', '<', '>', '&']), "the description is plain text, without markup: {}", text);
    }
}

#[test]
fn the_description_is_used_as_it_is_everywhere() {
    let (tagline, summary, _) = description();
    let mut wrong = Vec::new();
    let mut want = |file: &str, text: &str, what: &str, found: &str| {
        if !found.contains(text) {
            wrong.push(format!("{} does not contain the {} of docs/assets/description.txt", file, what));
        }
    };
    let index = page("index.html").html;
    want("docs/index.html", &tagline, "tagline", &plain(&index));
    want("docs/index.html", &summary, "summary", &plain(&index));
    want("docs/index.html (meta description)", &format!("<meta name=\"description\" content=\"{}\">", tagline), "tagline", &index);
    let readme = fs::read_to_string("README.md").unwrap().replace("\r\n", "\n");
    want("README.md", &tagline, "tagline", &readme);
    want("README.md", &summary, "summary", &readme);
    let cargo = fs::read_to_string("Cargo.toml").unwrap();
    want("Cargo.toml", &format!("description = \"{}\"", tagline), "tagline", &cargo);
    let package = fs::read_to_string("editors/vscode/package.json").unwrap();
    want("editors/vscode/package.json", &format!("\"description\": \"{}\"", tagline), "tagline", &package);
    let ext = fs::read_to_string("editors/vscode/README.md").unwrap();
    want("editors/vscode/README.md", &tagline, "tagline", &ext);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn every_page_has_the_icon_the_skip_link_and_the_content_anchor() {
    for p in pages() {
        assert!(p.html.contains("<link rel=\"icon\" href=\"assets/zila.svg\""), "docs/{} does not link the icon", p.name);
        assert!(p.html.contains("<html lang=\"en\">"), "docs/{} has no language", p.name);
        let body = p.html.split("<body>").nth(1).unwrap_or_else(|| panic!("docs/{} has no body", p.name));
        assert!(body.trim_start().starts_with("<a class=\"skip\" href=\"#content\">"), "docs/{}: the skip link must come first", p.name);
        assert!(p.html.contains("<main id=\"content\">"), "docs/{} has no main#content", p.name);
        assert!(p.html.contains("<label for=\"menu\" class=\"menu-button\">") && p.html.contains("id=\"menu\" class=\"menu-toggle\""), "docs/{} has no menu control", p.name);
        assert!(!p.html.contains("ember-dot"), "docs/{} still has the old brand markup", p.name);
    }
    assert!(Path::new("docs/assets/zila.svg").is_file());
}

#[test]
fn the_icons_are_plain_and_small() {
    for file in ["docs/assets/zila.svg", "docs/assets/zila-mono.svg"] {
        let svg = fs::read_to_string(file).unwrap();
        assert!(svg.len() < 2048, "{} is {} bytes", file, svg.len());
        for bad in ["<script", "<image", "<text", "<foreignObject", "href="] {
            assert!(!svg.contains(bad), "{} contains {}", file, bad);
        }
        assert!(svg.contains("viewBox"), "{} has no viewBox", file);
    }
    // the extension's icon is a real 128 x 128 PNG
    let png = fs::read("editors/vscode/icon.png").unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let (w, h) = (u32::from_be_bytes(png[16..20].try_into().unwrap()), u32::from_be_bytes(png[20..24].try_into().unwrap()));
    assert_eq!((w, h), (128, 128));
    let manifest = fs::read_to_string("editors/vscode/package.json").unwrap();
    assert!(manifest.contains("\"icon\": \"icon.png\""), "package.json does not name the icon");
    let ignore = fs::read_to_string("editors/vscode/.vscodeignore").unwrap();
    assert!(!ignore.lines().any(|l| l.trim() == "icon.png" || l.trim() == "*.png"), ".vscodeignore excludes the icon");
}

#[test]
fn the_site_needs_nothing_from_another_origin_and_no_script() {
    let css = fs::read_to_string("docs/styles.css").unwrap();
    for bad in ["@import", "url(http", "url(\"http", "@font-face"] {
        assert!(!css.contains(bad), "docs/styles.css contains {}", bad);
    }
    for p in pages() {
        assert!(!p.html.contains("<script"), "docs/{} has a script", p.name);
        for tag in ["<link ", "<img "] {
            for chunk in p.html.split(tag).skip(1) {
                let attrs = chunk.split('>').next().unwrap();
                assert!(!attrs.contains("http://") && !attrs.contains("https://"), "docs/{} loads a file from another origin: {}{}", p.name, tag, attrs);
            }
        }
    }
}

fn hex(v: &str) -> (f64, f64, f64) {
    let v = v.trim().trim_start_matches('#');
    let c = |i: usize| u8::from_str_radix(&v[i..i + 2], 16).unwrap() as f64 / 255.0;
    (c(0), c(2), c(4))
}

fn luminance(rgb: (f64, f64, f64)) -> f64 {
    let f = |c: f64| if c <= 0.03928 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
    0.2126 * f(rgb.0) + 0.7152 * f(rgb.1) + 0.0722 * f(rgb.2)
}

fn contrast(a: &str, b: &str) -> f64 {
    let (la, lb) = (luminance(hex(a)), luminance(hex(b)));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// The custom properties of one `:root { ... }` block of the stylesheet, by their order in the file.
fn tokens(css: &str, block: usize) -> std::collections::HashMap<String, String> {
    let mut starts = css.match_indices(":root {").map(|(i, _)| i);
    let start = starts.nth(block).unwrap_or_else(|| panic!("the stylesheet has no :root block {}", block));
    let body = &css[start..];
    let body = &body[..body.find('}').unwrap()];
    body.lines()
        .filter_map(|l| {
            let l = l.trim();
            let (k, v) = l.strip_prefix("--")?.split_once(':')?;
            Some((k.to_string(), v.trim().trim_end_matches(';').to_string()))
        })
        .collect()
}

#[test]
fn text_and_background_colours_have_a_contrast_of_at_least_4_5_in_both_themes() {
    let css = fs::read_to_string("docs/styles.css").unwrap();
    let light = tokens(&css, 0);
    let mut dark = light.clone();
    dark.extend(tokens(&css, 1));
    for (theme, t) in [("light", &light), ("dark", &dark)] {
        for fg in ["fg", "muted", "accent", "kw", "str", "cm"] {
            for bg in ["bg", "bg-soft"] {
                let ratio = contrast(&t[fg], &t[bg]);
                assert!(ratio >= 4.5, "{} theme: --{} on --{} has a contrast of {:.2} (at least 4.5)", theme, fg, bg, ratio);
            }
        }
    }
}

#[test]
fn the_layout_keeps_to_its_rules() {
    let css = fs::read_to_string("docs/styles.css").unwrap();
    assert!(css.contains("--measure: 68ch"), "the reading column is 68ch");
    assert!(css.contains("prefers-color-scheme: dark") && css.contains("@media print") && css.contains("min-width: 800px"));
    assert!(css.contains(":focus-visible"), "focus must be visible");
    assert!(!css.contains("ember"), "the old ember palette is gone");
    assert!(css.contains("system-ui"), "system fonts");
}

// ---------------------------------------------------------------- the file extension

/// The old extension of source files: written in two parts so that this file does not match itself.
fn old_extension() -> String {
    format!("{}{}", ".b", "r")
}

fn tracked_files() -> Vec<String> {
    let out = std::process::Command::new("git").args(["ls-files", "-z"]).output().unwrap();
    String::from_utf8_lossy(&out.stdout).split('\0').filter(|p| !p.is_empty()).map(|p| p.to_string()).collect()
}

/// The lines of `text` where the old extension ends a word (a file name, or the extension quoted), but not where it starts a longer name.
fn mentions(text: &str, ext: &str) -> Vec<usize> {
    let mut found = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let mut from = 0;
        while let Some(at) = line[from..].find(ext) {
            let end = from + at + ext.len();
            let next = line[end..].chars().next();
            if !next.map_or(false, |c| c.is_alphanumeric() || c == '_') {
                found.push(i + 1);
                break;
            }
            from = end;
        }
    }
    found
}

#[test]
fn the_extension_rule_reads_words_and_not_longer_names() {
    let ext = old_extension();
    assert_eq!(mentions(&format!("a{} b{}:3\nlonger{}ackets\nok", ext, ext, ext), &ext), vec![1]);
    assert!(mentions(&format!("x{}acket", ext), &ext).is_empty());
}

#[test]
fn no_tracked_file_uses_the_old_source_extension() {
    let ext = old_extension();
    let mut problems = Vec::new();
    for path in tracked_files() {
        // the archive records what was true when each change was archived, and a change in progress
        // plans work that may be about the old extension itself
        if path.starts_with("openspec/changes/") || !Path::new(&path).is_file() {
            continue;
        }
        if path.ends_with(&ext) {
            problems.push(format!("{}: a source file must end in .zila", path));
            continue;
        }
        let Ok(bytes) = fs::read(&path) else { continue };
        if bytes.contains(&0) {
            continue;
        }
        let Ok(text) = String::from_utf8(bytes) else { continue };
        for line in mentions(&text, &ext) {
            problems.push(format!("{}:{}: names the old extension", path, line));
        }
    }
    assert!(problems.is_empty(), "the old source file extension is still used ({} places):\n{}", problems.len(), problems.join("\n"));
}
