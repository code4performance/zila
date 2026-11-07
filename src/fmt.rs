//! `brasa-parse --fmt`: rewrites source in the standard style.
//!
//! The formatter works on the token stream and the original text of each token, and changes
//! only whitespace: indentation (from the block structure the parser records), the spaces
//! around commas and brackets, blank lines, trailing spaces. It never joins or splits lines.
//! A result whose tokens differ from the original's is rejected, so a bug here cannot change
//! a program.

use crate::lexer::{Comment, Lexer, StrPart, Tok, Token};
use crate::parser::{parse_with_layout, Layout};
use std::collections::{HashMap, HashSet};

const INDENT: usize = 4;

/// Operators and keywords after which an expression continues on the next line.
const CONTINUING: &[&str] = &[
    "+", "-", "*", "/", "%", "==", "!=", "<", "<=", ">=", "<<", "??", "=", "+=", "-=", "*=", "/=", "%=", "&", "|", "and", "or",
    "bitand", "bitor", "bitxor", "..",
];

#[derive(Clone, Copy)]
enum Kind {
    Tok(usize),
    Com { block: bool },
}

#[derive(Clone, Copy)]
struct Elem {
    start: usize,
    end: usize,
    kind: Kind,
}

struct Line {
    first: usize,
    last: usize,
    start_line: usize,
    end_line: usize,
    level: i32,
}

/// The formatted text, or an error message (`line:col: message` for a file that does not parse).
pub fn format_source(src: &str) -> Result<String, String> {
    let crlf = src.matches("\r\n").count() * 2 > src.matches('\n').count();
    let text = src.replace("\r\n", "\n");
    let (_, layout, toks, comments) = parse_with_layout(&text).map_err(|d| format!("{}:{}: {}", d.line, d.col, d.msg))?;
    let out = render(&text, &toks, &comments, &layout);
    // the program must not change
    let (toks2, comments2) = Lexer::new(&out).tokenize_all().map_err(|d| format!("formatting would change the program ({}:{}: {})", d.line, d.col, d.msg))?;
    let same_toks = toks.len() == toks2.len() && toks.iter().zip(&toks2).all(|(a, b)| tok_eq(&a.tok, &b.tok));
    let chars: Vec<char> = text.chars().collect();
    let out_chars: Vec<char> = out.chars().collect();
    let comment_text = |cs: &[char], c: &Comment| -> String { cs[c.start..c.end].iter().collect::<String>().trim_end().to_string() };
    let same_comments = comments.len() == comments2.len()
        && comments.iter().zip(&comments2).all(|(a, b)| comment_text(&chars, a) == comment_text(&out_chars, b));
    // only whitespace may differ
    let squeeze = |s: &str| -> String { s.chars().filter(|c| !c.is_whitespace()).collect() };
    if !same_toks || !same_comments || squeeze(&out) != squeeze(&text) {
        return Err("formatting would change the program".to_string());
    }
    Ok(if crlf { out.replace('\n', "\r\n") } else { out })
}

/// Equal tokens, ignoring where the interpolations inside text were found.
pub fn tok_eq(a: &Tok, b: &Tok) -> bool {
    match (a, b) {
        (Tok::Text(x), Tok::Text(y)) => {
            x.len() == y.len()
                && x.iter().zip(y).all(|(p, q)| match (p, q) {
                    (StrPart::Lit(a), StrPart::Lit(b)) => a == b,
                    (StrPart::Interp { src: a, .. }, StrPart::Interp { src: b, .. }) => a == b,
                    _ => false,
                })
        }
        _ => a == b,
    }
}

fn render(text: &str, toks: &[Token], comments: &[Comment], layout: &Layout) -> String {
    let cs: Vec<char> = text.chars().collect();
    let slice = |a: usize, b: usize| -> String { cs[a..b].iter().collect::<String>().trim_end().to_string() };
    let mut line_starts = vec![0usize];
    for (i, c) in cs.iter().enumerate() {
        if *c == '\n' {
            line_starts.push(i + 1);
        }
    }
    let line_of = |off: usize| -> usize {
        match line_starts.binary_search(&off) {
            Ok(i) => i,
            Err(i) => i - 1,
        }
    };

    // every token and comment, in source order
    let mut elems: Vec<Elem> = Vec::new();
    for (i, t) in toks.iter().enumerate() {
        if matches!(t.tok, Tok::Newline | Tok::Eof) {
            continue;
        }
        elems.push(Elem { start: t.span.0, end: t.span.1, kind: Kind::Tok(i) });
    }
    for c in comments {
        elems.push(Elem { start: c.start, end: c.end, kind: Kind::Com { block: c.block } });
    }
    elems.sort_by_key(|e| e.start);
    if elems.is_empty() {
        return String::new();
    }
    let end_line_of = |e: &Elem| line_of(e.end.saturating_sub(1).max(e.start));

    // the regions whose tokens are one level deeper
    let n = toks.len();
    let mut regions: Vec<(usize, usize)> = layout.blocks.iter().map(|(a, b)| (*a, *b)).collect();
    regions.extend(layout.arms.iter().cloned());
    let mut diff = vec![0i32; n + 2];
    for &(a, b) in &regions {
        if b > a + 1 {
            diff[a + 1] += 1;
            diff[b] -= 1;
        }
    }
    let mut depth = vec![0i32; n];
    let mut acc = 0;
    for i in 0..n {
        acc += diff[i];
        depth[i] = acc;
    }
    let ends: HashSet<usize> = layout.blocks.values().cloned().collect();
    let tok_line = |i: usize| line_of(toks[i].span.0);
    let multi_openers: HashSet<usize> = regions.iter().filter(|(a, b)| *b < n && tok_line(*b) > tok_line(*a)).map(|(a, _)| *a).collect();

    // logical lines: elements that share a source line (a multi-line token joins its last line)
    let mut lines: Vec<Line> = Vec::new();
    for (k, e) in elems.iter().enumerate() {
        let sl = line_of(e.start);
        match lines.last_mut() {
            Some(l) if sl <= l.end_line => {
                l.last = k;
                l.end_line = l.end_line.max(end_line_of(e));
            }
            _ => lines.push(Line { first: k, last: k, start_line: sl, end_line: end_line_of(e), level: 0 }),
        }
    }

    // pass 1: the indentation level of lines that start with a token
    let mut stack: Vec<bool> = Vec::new(); // an open bracket; true when a block opened after it on its line
    let mut prev_last_tok: Option<usize> = None;
    let mut levels: Vec<Option<i32>> = vec![None; lines.len()];
    for (li, line) in lines.iter().enumerate() {
        let first = elems[line.first];
        let stack_at_start = stack.len();
        let mut open_count = stack.iter().filter(|s| !**s).count() as i32;
        if let Kind::Tok(ti) = first.kind {
            if matches!(toks[ti].tok, Tok::Op(")") | Tok::Op("]") | Tok::Op("}")) && open_count > 0 {
                open_count -= 1;
            }
            let mut cont = open_count;
            let chain = matches!(toks[ti].tok, Tok::Op(".") | Tok::Op("?."));
            let after_op = prev_last_tok.map_or(false, |p| match &toks[p].tok {
                Tok::Op(o) => CONTINUING.contains(o),
                Tok::Kw(k) => CONTINUING.contains(k),
                _ => false,
            });
            if (chain || after_op) && cont == 0 {
                cont = 1;
            }
            let mut lvl = depth[ti];
            if layout.elses.contains(&ti) {
                lvl -= 1;
            }
            levels[li] = Some((lvl + cont).max(0));
        }
        for k in line.first..=line.last {
            if let Kind::Tok(ti) = elems[k].kind {
                match &toks[ti].tok {
                    Tok::Op("(") | Tok::Op("[") | Tok::Op("{") | Tok::Op("#[") => stack.push(false),
                    Tok::Op(")") | Tok::Op("]") | Tok::Op("}") => {
                        stack.pop();
                    }
                    _ => {}
                }
                if multi_openers.contains(&ti) {
                    for s in stack.iter_mut().skip(stack_at_start) {
                        *s = true;
                    }
                }
                prev_last_tok = Some(ti);
            }
        }
    }
    // comment-only lines take the indentation of the next line that has code
    // (a comment right before an `end` stays inside the block)
    let mut next_level = 0;
    for li in (0..lines.len()).rev() {
        match levels[li] {
            Some(l) => {
                let is_end = matches!(elems[lines[li].first].kind, Kind::Tok(ti) if ends.contains(&ti));
                next_level = if is_end { l + 1 } else { l };
            }
            None => levels[li] = Some(next_level),
        }
        lines[li].level = levels[li].unwrap();
    }

    // assembly blocks: one level inside `asm`, relative indentation kept
    let tok_elem: HashMap<usize, usize> = elems.iter().enumerate().filter_map(|(k, e)| if let Kind::Tok(ti) = e.kind { Some((ti, k)) } else { None }).collect();
    let line_of_elem: Vec<usize> = {
        let mut v = vec![0usize; elems.len()];
        for (li, l) in lines.iter().enumerate() {
            for k in l.first..=l.last {
                v[k] = li;
            }
        }
        v
    };
    let mut cols: Vec<usize> = lines.iter().map(|l| l.level as usize * INDENT).collect();
    let leading = |e: &Elem| -> usize {
        let ls = line_starts[line_of(e.start)];
        cs[ls..e.start].iter().map(|c| if *c == '\t' { INDENT } else { 1 }).sum()
    };
    for (&a, &b) in layout.blocks.iter() {
        if toks[a].tok != Tok::Kw("asm") || b <= a + 1 {
            continue;
        }
        let (from, to) = (toks[a].span.1, toks[b].span.0);
        let inside: Vec<usize> = (0..lines.len()).filter(|&li| elems[lines[li].first].start >= from && elems[lines[li].first].start < to).collect();
        let Some(min) = inside.iter().map(|&li| leading(&elems[lines[li].first])).min() else { continue };
        let base = lines[line_of_elem[tok_elem[&a]]].level as usize * INDENT + INDENT;
        for &li in &inside {
            cols[li] = base + (leading(&elems[lines[li].first]) - min);
        }
    }

    // which lines open a block that is still open on the next line
    let mut opens_block = vec![false; lines.len()];
    for &(a, b) in &regions {
        let Some(&ea) = tok_elem.get(&a) else { continue };
        let la = line_of_elem[ea];
        let lb = tok_elem.get(&b).map(|&eb| line_of_elem[eb]);
        if lb.map_or(true, |lb| lb != la) {
            opens_block[la] = true;
        }
    }

    // output
    let mut out: Vec<String> = Vec::new();
    for (li, line) in lines.iter().enumerate() {
        if li > 0 {
            let blanks = line.start_line.saturating_sub(lines[li - 1].end_line + 1);
            let first = elems[line.first];
            let closes = matches!(first.kind, Kind::Tok(ti) if ends.contains(&ti) || layout.elses.contains(&ti));
            if blanks > 0 && !opens_block[li - 1] && !closes {
                out.push(String::new());
            }
        }
        let mut s = " ".repeat(cols[li]);
        for k in line.first..=line.last {
            let e = elems[k];
            if k > line.first {
                let prev = elems[k - 1];
                let had_space = e.start > prev.end;
                let written: String = cs[prev.end..e.start].iter().map(|c| if *c == '\t' { ' ' } else { *c }).collect();
                let gap = match (prev.kind, e.kind) {
                    (Kind::Tok(_), Kind::Com { block: false }) => {
                        if written.len() >= 2 {
                            written.as_str()
                        } else {
                            "  "
                        }
                    }
                    (_, Kind::Com { .. }) | (Kind::Com { .. }, _) => written.as_str(),
                    (_, Kind::Tok(ti)) if matches!(toks[ti].tok, Tok::Op(",") | Tok::Op(")") | Tok::Op("]")) => "",
                    (Kind::Tok(pi), _) if matches!(toks[pi].tok, Tok::Op("(") | Tok::Op("[") | Tok::Op("#[")) => "",
                    (Kind::Tok(pi), _) if matches!(toks[pi].tok, Tok::Op(",")) => " ",
                    _ => written.as_str(),
                };
                let _ = had_space;
                s.push_str(gap);
            }
            s.push_str(&slice(e.start, e.end));
        }
        out.push(s);
    }
    let mut result = out.join("\n");
    result.push('\n');
    result
}
