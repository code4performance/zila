//! The documentation site must follow the code. These tests fail when it does not:
//! example blocks differ from `examples/`, flags or keywords are undocumented (or
//! documented but gone), or the site's pages and links are inconsistent.
//!
//! After changing an example, run `brasa-parse --docs` to refresh the blocks.

use brasa::cli::FLAGS;
use brasa::docs::{blocks, normalize, plain};
use brasa::lexer::KEYWORDS;
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
    assert!(b.iter().any(|b| b.example_path() == Some("examples/hello.br")));
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
                problems.push(format!("docs/{}: the block for {} differs from the file (first difference at line {}); run `brasa-parse --docs`", p.name, example, line));
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
