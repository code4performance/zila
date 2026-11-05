//! Keeps the documentation site (`docs/*.html`) in step with `examples/*.br`.
//!
//! A code block preceded by `<div class="file-label">examples/NAME.br</div>` shows that file in
//! full; one labelled `examples/NAME.br (excerpt)` shows a contiguous part of it. `sync` (the
//! `brasa-parse --docs` command) rewrites the full blocks from the files, and `tests/docs.rs`
//! fails when a block no longer matches.

use crate::lexer::KEYWORDS;
use std::path::Path;

/// A `<pre><code>` block of a page.
#[derive(Debug, Clone)]
pub struct Block {
    /// Text of the `file-label` div right before the block, if any.
    pub label: Option<String>,
    /// Byte range of the block's inner HTML.
    pub start: usize,
    pub end: usize,
}

impl Block {
    /// `examples/NAME.br` for blocks that show an example file.
    pub fn example_path(&self) -> Option<&str> {
        let l = self.label.as_deref()?;
        let p = l.split_whitespace().next()?;
        p.starts_with("examples/").then_some(p)
    }

    pub fn is_excerpt(&self) -> bool {
        self.label.as_deref().map_or(false, |l| l.contains("(excerpt)"))
    }
}

const OPEN: &str = "<pre><code>";
const CLOSE: &str = "</code></pre>";
const LABEL: &str = "<div class=\"file-label\">";

pub fn blocks(html: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = html[from..].find(OPEN) {
        let pre = from + i;
        let start = pre + OPEN.len();
        let Some(j) = html[start..].find(CLOSE) else { break };
        let end = start + j;
        let before = html[..pre].trim_end();
        let label = before.strip_suffix("</div>").and_then(|b| {
            let k = b.rfind(LABEL)?;
            Some(plain(&b[k + LABEL.len()..]).trim().to_string())
        });
        out.push(Block { label, start, end });
        from = end + CLOSE.len();
    }
    out
}

/// Text of an HTML fragment: tags removed, entities decoded.
pub fn plain(fragment: &str) -> String {
    let mut text = String::new();
    let mut in_tag = false;
    for c in fragment.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            c if !in_tag => text.push(c),
            _ => {}
        }
    }
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
}

/// Line endings, trailing spaces and blank lines at both ends do not count as differences.
pub fn normalize(text: &str) -> String {
    let owned = text.replace("\r\n", "\n");
    let lines: Vec<&str> = owned.lines().map(|l| l.trim_end()).collect();
    let first = lines.iter().position(|l| !l.is_empty()).unwrap_or(lines.len());
    let last = lines.iter().rposition(|l| !l.is_empty()).map_or(first, |i| i + 1);
    lines[first..last].join("\n")
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Brasa source as HTML with the site's classes: `kw` keywords, `str` text and
/// character literals, `cm` comments.
pub fn highlight(src: &str) -> String {
    let chars: Vec<char> = src.replace("\r\n", "\n").trim_end().chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let span = |class: &str, text: &str| format!("<span class=\"{}\">{}</span>", class, escape(text));
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if c == '/' && next == Some('/') {
            let mut j = i;
            while j < chars.len() && chars[j] != '\n' {
                j += 1;
            }
            out.push_str(&span("cm", &chars[i..j].iter().collect::<String>()));
            i = j;
        } else if c == '/' && next == Some('*') {
            let mut j = i + 2;
            while j + 1 < chars.len() && !(chars[j] == '*' && chars[j + 1] == '/') {
                j += 1;
            }
            let j = (j + 2).min(chars.len());
            out.push_str(&span("cm", &chars[i..j].iter().collect::<String>()));
            i = j;
        } else if c == '"' {
            // a text literal; `{ ... }` may contain quotes of its own
            let mut j = i + 1;
            let mut depth = 0;
            while j < chars.len() {
                match chars[j] {
                    '\\' => j += 1,
                    '{' => depth += 1,
                    '}' if depth > 0 => depth -= 1,
                    '"' if depth == 0 => break,
                    _ => {}
                }
                j += 1;
            }
            let j = (j + 1).min(chars.len());
            out.push_str(&span("str", &chars[i..j].iter().collect::<String>()));
            i = j;
        } else if c == '\'' {
            let mut j = i + 1;
            while j < chars.len() && chars[j] != '\'' && chars[j] != '\n' {
                if chars[j] == '\\' {
                    j += 1;
                }
                j += 1;
            }
            let j = (j + 1).min(chars.len());
            out.push_str(&span("str", &chars[i..j].iter().collect::<String>()));
            i = j;
        } else if c.is_alphabetic() || c == '_' {
            let mut j = i;
            while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
                j += 1;
            }
            let word: String = chars[i..j].iter().collect();
            // a name after `.` is a field or method even when it spells a keyword (app.use)
            let after_dot = i > 0 && chars[i - 1] == '.';
            if KEYWORDS.contains(&word.as_str()) && !after_dot {
                out.push_str(&span("kw", &word));
            } else {
                out.push_str(&escape(&word));
            }
            i = j;
        } else {
            out.push_str(&escape(&c.to_string()));
            i += 1;
        }
    }
    out
}

pub struct Report {
    /// Full example blocks examined.
    pub checked: usize,
    /// One line per block that was rewritten.
    pub changes: Vec<String>,
}

/// Rewrites every full example block in `<root>/docs/*.html` from `<root>/examples/`.
pub fn sync(root: &Path) -> Result<Report, String> {
    let docs = root.join("docs");
    let mut pages: Vec<_> = std::fs::read_dir(&docs)
        .map_err(|e| format!("cannot read {}: {}", docs.display(), e))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map_or(false, |x| x == "html"))
        .collect();
    pages.sort();
    let mut report = Report { checked: 0, changes: Vec::new() };
    for page in pages {
        let html = std::fs::read_to_string(&page).map_err(|e| format!("cannot read {}: {}", page.display(), e))?.replace("\r\n", "\n");
        let mut out = String::new();
        let mut last = 0;
        let mut changed = false;
        for b in blocks(&html) {
            let Some(example) = b.example_path() else { continue };
            if b.is_excerpt() {
                continue;
            }
            let file = root.join(example);
            let src = std::fs::read_to_string(&file).map_err(|e| format!("{}: cannot read {}: {}", page.display(), file.display(), e))?;
            let fresh = highlight(&src);
            report.checked += 1;
            if html[b.start..b.end] != fresh {
                out.push_str(&html[last..b.start]);
                out.push_str(&fresh);
                last = b.end;
                changed = true;
                report.changes.push(format!("updated {} in {}", example, page.file_name().unwrap().to_string_lossy()));
            }
        }
        if changed {
            out.push_str(&html[last..]);
            std::fs::write(&page, out).map_err(|e| format!("cannot write {}: {}", page.display(), e))?;
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_strips_markup_and_decodes_entities() {
        let h = "<span class=\"kw\">let</span> x = <span class=\"str\">\"a&lt;b&gt;\"</span> &amp;&amp; y";
        assert_eq!(plain(h), "let x = \"a<b>\" && y");
    }

    #[test]
    fn highlight_round_trips_to_the_same_text() {
        let src = "package main\n\n// comment with <angle> & stuff\nfunc f(x: int) -> text\n    let s = \"hi {x} \\\" {fmt(\"a\")}\"\n    let c = 'a'\n    app.use(x)\nend\n";
        let html = highlight(src);
        assert_eq!(plain(&html), src.trim_end());
        assert!(html.contains("<span class=\"kw\">package</span>"));
        assert!(html.contains("<span class=\"cm\">// comment with &lt;angle&gt; &amp; stuff</span>"));
        assert!(html.contains("<span class=\"str\">\"hi {x} \\\" {fmt(\"a\")}\"</span>"));
        // a method named like a keyword is not a keyword there
        assert!(html.contains("app.use(x)"));
    }

    #[test]
    fn blocks_find_labels_and_ranges() {
        let h = "<div class=\"file-label\">examples/a.br</div>\n<pre><code>x</code></pre>\n<pre><code>y</code></pre>";
        let b = blocks(h);
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].example_path(), Some("examples/a.br"));
        assert!(!b[0].is_excerpt());
        assert_eq!(&h[b[0].start..b[0].end], "x");
        assert_eq!(b[1].label, None);
    }

    #[test]
    fn excerpts_are_recognised() {
        let h = "<div class=\"file-label\">examples/a.br (excerpt)</div><pre><code>z</code></pre>";
        assert!(blocks(h)[0].is_excerpt());
    }

    #[test]
    fn normalize_ignores_trailing_space_and_blank_edges() {
        assert_eq!(normalize("\n\nab  \r\ncd\n\n"), "ab\ncd");
    }
}
