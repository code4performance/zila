//! `zila eval` / `zila -e` / `zila run -`: turns a snippet into an ordinary program.
//!
//! The compiler never sees a "script": the snippet's statements become the body of
//! `func main() -> error`, its `use` lines and declarations stay at the top level, and the
//! result goes through the same path as `--run`. A line map lets diagnostics name the
//! snippet's own lines (spec `eval-command`).

/// Spaces the statements are indented by inside the generated `main`.
const INDENT: usize = 4;

/// A snippet turned into a program, with the positions to translate errors back.
#[derive(Debug, PartialEq)]
pub struct Wrapped {
    pub source: String,
    /// For each line of `source` (index 0 is line 1): the snippet line it came from
    /// (1-based) and how many columns of indentation were added, or `None` for generated lines.
    pub map: Vec<Option<(usize, usize)>>,
}

#[derive(PartialEq)]
enum Decl {
    /// Ends on its own line (or when its parentheses balance).
    Single,
    /// Runs until a line that is exactly `end`.
    Block,
    /// An attribute line; the declaration it belongs to follows.
    Attr,
}

fn strip_pub(line: &str) -> &str {
    line.strip_prefix("pub ").unwrap_or(line)
}

fn decl_kind(line: &str) -> Option<Decl> {
    let rest = strip_pub(line);
    if line.starts_with("#[") {
        return Some(Decl::Attr);
    }
    for p in ["func ", "async func ", "comptime func "] {
        if rest.starts_with(p) {
            return Some(Decl::Block);
        }
    }
    if rest.starts_with("type ") {
        let t = rest.trim_end();
        return Some(if t.ends_with("record") || t.ends_with("enum") || t.ends_with("interface") { Decl::Block } else { Decl::Single });
    }
    if rest.starts_with("extern ") || rest.starts_with("embed ") {
        return Some(Decl::Single);
    }
    None
}

fn parens(line: &str) -> i32 {
    line.chars().map(|c| if c == '(' { 1 } else if c == ')' { -1 } else { 0 }).sum()
}

/// True when the snippet defines its own `main` at the top level.
fn has_main(snippet: &str) -> bool {
    snippet.lines().any(|l| {
        let r = strip_pub(l);
        let r = r.strip_prefix("async ").unwrap_or(r);
        r.strip_prefix("func main").is_some_and(|t| t.starts_with('(') || t.starts_with(' '))
    })
}

fn has_package(snippet: &str) -> bool {
    snippet.lines().any(|l| l.starts_with("package "))
}

/// Wraps `snippet` as a program.
pub fn wrap(snippet: &str) -> Wrapped {
    let lines: Vec<&str> = snippet.lines().collect();
    let mut source = String::new();
    let mut map: Vec<Option<(usize, usize)>> = Vec::new();
    let mut push = |text: &str, from: Option<(usize, usize)>| {
        source.push_str(text);
        source.push('\n');
        map.push(from);
    };

    if has_main(snippet) {
        if !has_package(snippet) {
            push("package main", None);
        }
        for (i, l) in lines.iter().enumerate() {
            push(l, Some((i + 1, 0)));
        }
        return Wrapped { source, map };
    }

    let mut package: Option<(usize, &str)> = None;
    let mut uses: Vec<(usize, &str)> = Vec::new();
    let mut decls: Vec<(usize, &str)> = Vec::new();
    let mut stmts: Vec<(usize, &str)> = Vec::new();
    let mut in_block = false;
    let mut open_parens = 0;
    for (i, l) in lines.iter().enumerate() {
        let n = i + 1;
        if in_block {
            decls.push((n, l));
            if l.trim_end() == "end" {
                in_block = false;
            }
            continue;
        }
        if open_parens > 0 {
            decls.push((n, l));
            open_parens += parens(l);
            continue;
        }
        if l.starts_with("package ") {
            package = Some((n, l));
        } else if l.starts_with("use ") {
            uses.push((n, l));
        } else if let Some(kind) = decl_kind(l) {
            decls.push((n, l));
            match kind {
                Decl::Block => in_block = true,
                Decl::Single => open_parens = parens(l).max(0),
                Decl::Attr => {}
            }
        } else {
            stmts.push((n, l));
        }
    }

    match package {
        Some((n, l)) => push(l, Some((n, 0))),
        None => push("package main", None),
    }
    for (n, l) in &uses {
        push(l, Some((*n, 0)));
    }
    for (n, l) in &decls {
        push(l, Some((*n, 0)));
    }
    push("", None);
    push("func main() -> error", None);
    let pad = " ".repeat(INDENT);
    for (n, l) in &stmts {
        if l.trim().is_empty() {
            push("", Some((*n, 0)));
        } else {
            push(&format!("{}{}", pad, l), Some((*n, INDENT)));
        }
    }
    push(&format!("{}return", pad), None);
    push("end", None);
    Wrapped { source, map }
}

impl Wrapped {
    /// The snippet position of a position in the generated program, if it came from the snippet.
    pub fn locate(&self, line: usize, col: usize) -> Option<(usize, usize)> {
        let (n, indent) = (*self.map.get(line.checked_sub(1)?)?)?;
        Some((n, col.saturating_sub(indent).max(1)))
    }

    /// Rewrites `main.zila:<line>` (the form compile errors use) to `<label>:<snippet line>`.
    /// A line that is generated text is reported as the snippet's last line.
    pub fn map_message(&self, msg: &str, label: &str) -> String {
        let key = "main.zila:";
        let mut out = String::new();
        let mut rest = msg;
        while let Some(i) = rest.find(key) {
            out.push_str(&rest[..i]);
            let after = &rest[i + key.len()..];
            let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
            match digits.parse::<usize>() {
                Ok(line) if !digits.is_empty() => {
                    let n = self.locate(line, 1).map(|p| p.0).unwrap_or_else(|| self.map.iter().flatten().map(|p| p.0).max().unwrap_or(1));
                    out.push_str(&format!("{}:{}", label, n));
                    rest = &after[digits.len()..];
                }
                _ => {
                    out.push_str(key);
                    rest = after;
                }
            }
        }
        out.push_str(rest);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_one_liner_becomes_the_body_of_main() {
        let w = wrap("print(1 + 2)");
        assert_eq!(w.source, "package main\n\nfunc main() -> error\n    print(1 + 2)\n    return\nend\n");
    }

    #[test]
    fn declarations_and_uses_stay_at_the_top_level() {
        let w = wrap("use text\nfunc double(n: int) -> int\n    return n * 2\nend\nprint(double(21))");
        assert_eq!(
            w.source,
            "package main\nuse text\nfunc double(n: int) -> int\n    return n * 2\nend\n\nfunc main() -> error\n    print(double(21))\n    return\nend\n"
        );
    }

    #[test]
    fn a_block_type_runs_to_its_end_and_a_nested_block_stays_inside() {
        let w = wrap("type P = record\n    x: int\nend\nlet p = P{ x: 1 }\nif p.x == 1\n    print(\"one\")\nend");
        assert!(w.source.contains("type P = record\n    x: int\nend\n\nfunc main() -> error\n    let p = P{ x: 1 }\n    if p.x == 1\n        print(\"one\")\n    end\n    return\nend\n"), "{}", w.source);
    }

    #[test]
    fn attributes_and_multi_line_externs_belong_to_the_top_level() {
        let w = wrap("#[link(\"m\")]\nextern \"C\" func qsort(a: int,\n    b: int)\nprint(1)");
        assert!(w.source.starts_with("package main\n#[link(\"m\")]\nextern \"C\" func qsort(a: int,\n    b: int)\n\nfunc main() -> error\n    print(1)\n"), "{}", w.source);
    }

    #[test]
    fn a_snippet_with_its_own_main_is_not_wrapped() {
        let w = wrap("func main()\n    print(\"hi\")\nend");
        assert_eq!(w.source, "package main\nfunc main()\n    print(\"hi\")\nend\n");
        let w = wrap("package demo\nfunc main()\n    print(\"hi\")\nend");
        assert_eq!(w.source, "package demo\nfunc main()\n    print(\"hi\")\nend\n");
        assert_eq!(w.locate(3, 5), Some((3, 5)));
    }

    #[test]
    fn an_empty_snippet_is_an_empty_main() {
        assert_eq!(wrap("").source, "package main\n\nfunc main() -> error\n    return\nend\n");
    }

    #[test]
    fn positions_go_back_to_the_snippet() {
        let w = wrap("let a = 1\nprint(1 +)");
        // generated: 1 package, 2 blank, 3 func main, 4 `let a`, 5 `print(1 +)`
        assert_eq!(w.locate(5, 14), Some((2, 10)));
        assert_eq!(w.locate(4, 5), Some((1, 1)));
        assert_eq!(w.locate(3, 1), None);
        assert_eq!(w.map_message("main.zila:5: 'x': expected int, found text", "<eval>"), "<eval>:2: 'x': expected int, found text");
        assert_eq!(w.map_message("main.zila:3: boom", "<eval>"), "<eval>:2: boom");
    }
}
