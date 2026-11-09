//! `sql <dialect>` blocks (spec `sql-blocks`): the profile of a dialect and the scanner that reads
//! a block with it.
//!
//! The scanner knows the lexical rules of a SQL dialect (strings, quoted identifiers, comments,
//! dollar quoting, placeholders) and nothing about SQL grammar. It finds the holes `{expr}`,
//! replaces each with the dialect's placeholder, and reports the mistakes that can be found
//! without a server. A dialect is described by attributes on a declaration of a package; this
//! module reads them into a `Profile`.

use crate::ast::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Placeholder {
    /// `$1`, `$2`, ... (PostgreSQL)
    Dollar,
    /// `@p1`, `@p2`, ... (T-SQL)
    AtP,
    /// `:1`, `:2`, ... (Oracle)
    Colon,
    /// `?` (MySQL, SQLite)
    Question,
}

impl Placeholder {
    fn render(self, n: usize) -> String {
        match self {
            Placeholder::Dollar => format!("${}", n),
            Placeholder::AtP => format!("@p{}", n),
            Placeholder::Colon => format!(":{}", n),
            Placeholder::Question => "?".to_string(),
        }
    }

    /// How a hand-written placeholder of this style looks, for the error.
    fn example(self) -> &'static str {
        match self {
            Placeholder::Dollar => "$1",
            Placeholder::AtP => "@p1",
            Placeholder::Colon => ":1",
            Placeholder::Question => "?",
        }
    }
}

/// The lexical profile of a dialect.
#[derive(Clone, Debug)]
pub struct Profile {
    pub name: String,
    pub placeholder: Placeholder,
    /// String quotes as (open, close); a doubled close quote is a quote in the text.
    pub strings: Vec<(char, char)>,
    /// Identifier quotes as (open, close).
    pub idents: Vec<(char, char)>,
    /// A backslash escapes the next character inside strings.
    pub backslash: bool,
    /// `E'...'` strings, where a backslash escapes (PostgreSQL).
    pub estring: bool,
    pub line_comments: Vec<String>,
    /// (open, close, nests)
    pub block_comment: Option<(String, String, bool)>,
    pub dollar: bool,
    /// One block may hold several statements.
    pub multi: bool,
    /// Lines (compared whole, ignoring case and spaces) that are refused, with the reason.
    pub reject: Vec<(String, String)>,
}

fn text_of(e: &Expr) -> Option<String> {
    if let ExprKind::Text(parts) = &e.kind {
        if let [TextPart::Lit(l)] = parts.as_slice() {
            return Some(l.clone());
        }
        if parts.is_empty() {
            return Some(String::new());
        }
    }
    None
}

fn one_char(s: &str) -> Option<char> {
    let mut it = s.chars();
    match (it.next(), it.next()) {
        (Some(c), None) => Some(c),
        _ => None,
    }
}

/// The dialect a declaration's attributes declare, if any (`None` when there is no
/// `#[sql_dialect("name")]`); `Err` for a malformed profile.
pub fn profile_from_attrs(attrs: &[Attr]) -> Option<Result<Profile, String>> {
    let head = attrs.iter().find(|a| a.name == "sql_dialect")?;
    Some(build(head, attrs))
}

fn build(head: &Attr, attrs: &[Attr]) -> Result<Profile, String> {
    let name = head.args.first().and_then(text_of).ok_or("#[sql_dialect] needs the name of the dialect: #[sql_dialect(\"name\")]")?;
    let mut p = Profile { name: name.clone(), placeholder: Placeholder::Question, strings: Vec::new(), idents: Vec::new(), backslash: false, estring: false, line_comments: Vec::new(), block_comment: None, dollar: false, multi: false, reject: Vec::new() };
    let mut have_placeholder = false;
    let strs = |a: &Attr| -> Result<Vec<String>, String> {
        a.args.iter().map(|e| text_of(e).ok_or_else(|| format!("#[{}] takes text literals", a.name))).collect()
    };
    let pair = |a: &Attr| -> Result<(char, char), String> {
        let v = strs(a)?;
        match v.as_slice() {
            [o] => one_char(o).map(|c| (c, c)).ok_or_else(|| format!("#[{}]: '{}' is not one character", a.name, o)),
            [o, c] => match (one_char(o), one_char(c)) {
                (Some(o), Some(c)) => Ok((o, c)),
                _ => Err(format!("#[{}] takes an opening and a closing character", a.name)),
            },
            _ => Err(format!("#[{}] takes one quote, or an opening and a closing one", a.name)),
        }
    };
    for a in attrs {
        match a.name.as_str() {
            "sql_placeholder" => {
                let v = strs(a)?;
                p.placeholder = match v.first().map(|s| s.as_str()) {
                    Some("$n") => Placeholder::Dollar,
                    Some("@pn") => Placeholder::AtP,
                    Some(":n") => Placeholder::Colon,
                    Some("?") => Placeholder::Question,
                    _ => return Err(format!("dialect '{}': #[sql_placeholder] is one of \"$n\", \"@pn\", \":n\" or \"?\"", name)),
                };
                have_placeholder = true;
            }
            "sql_string" => p.strings.push(pair(a)?),
            "sql_ident" => p.idents.push(pair(a)?),
            "sql_backslash" => p.backslash = true,
            "sql_estring" => p.estring = true,
            "sql_line_comment" => p.line_comments.extend(strs(a)?),
            "sql_block_comment" => {
                let v = strs(a)?;
                if v.len() < 2 {
                    return Err(format!("dialect '{}': #[sql_block_comment] needs the opening and the closing text", name));
                }
                p.block_comment = Some((v[0].clone(), v[1].clone(), v.get(2).is_some_and(|s| s == "nested")));
            }
            "sql_dollar_quote" => p.dollar = true,
            "sql_multi" => p.multi = true,
            "sql_reject" => {
                let v = strs(a)?;
                if v.len() != 2 {
                    return Err(format!("dialect '{}': #[sql_reject] takes a line and the reason", name));
                }
                p.reject.push((v[0].clone(), v[1].clone()));
            }
            _ => {}
        }
    }
    if !have_placeholder {
        return Err(format!("dialect '{}' needs #[sql_placeholder(...)]", name));
    }
    Ok(p)
}

/// A value of the program inside a block: its source text and the line it is on.
#[derive(Clone, Debug, PartialEq)]
pub struct Hole {
    pub src: String,
    pub line: u32,
}

#[derive(Debug, PartialEq)]
pub struct Scanned {
    /// The statement as the server receives it: placeholders in place of the holes.
    pub text: String,
    pub holes: Vec<Hole>,
}

#[derive(Debug, PartialEq)]
pub struct SqlError {
    pub line: u32,
    pub msg: String,
}

fn err<T>(line: u32, msg: impl Into<String>) -> Result<T, SqlError> {
    Err(SqlError { line, msg: msg.into() })
}

fn check_code(p: &Profile, after_semi: bool, line: u32) -> Result<(), SqlError> {
    if after_semi && !p.multi {
        return err(line, format!("the dialect '{}' allows one statement per block: there is code after a ';'", p.name));
    }
    Ok(())
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Removes the indentation shared by every non-blank line.
fn dedent(raw: &str) -> String {
    let lines: Vec<&str> = raw.split('\n').collect();
    let indent = lines.iter().filter(|l| !l.trim().is_empty()).map(|l| l.chars().take_while(|c| *c == ' ' || *c == '\t').count()).min().unwrap_or(0);
    lines.iter().map(|l| l.trim_end_matches('\r').chars().skip(indent.min(l.chars().take_while(|c| *c == ' ' || *c == '\t').count())).collect::<String>()).collect::<Vec<_>>().join("\n")
}

/// Reads a block. `first_line` is the line of the first raw line (the line after `sql <dialect>`).
pub fn scan(p: &Profile, raw: &str, first_line: u32) -> Result<Scanned, SqlError> {
    for (i, l) in raw.split('\n').enumerate() {
        let t = l.trim();
        if let Some((_, why)) = p.reject.iter().find(|(w, _)| w.eq_ignore_ascii_case(t)) {
            return err(first_line + i as u32, why.clone());
        }
    }
    let text = dedent(raw);
    let c: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut line = first_line;
    let mut i = 0;
    let mut holes: Vec<Hole> = Vec::new();
    let mut parens: Vec<u32> = Vec::new();
    let mut any_code = false;
    let mut after_semi = false;

    let starts = |c: &[char], i: usize, s: &str| -> bool { s.chars().enumerate().all(|(k, ch)| c.get(i + k) == Some(&ch)) };

    while i < c.len() {
        let ch = c[i];
        if ch == '\n' {
            out.push('\n');
            line += 1;
            i += 1;
            continue;
        }
        if ch.is_whitespace() {
            out.push(ch);
            i += 1;
            continue;
        }
        // comments
        if p.line_comments.iter().any(|lc| starts(&c, i, lc)) {
            while i < c.len() && c[i] != '\n' {
                out.push(c[i]);
                i += 1;
            }
            continue;
        }
        if let Some((open, close, nests)) = &p.block_comment {
            if starts(&c, i, open) {
                let start_line = line;
                let mut depth = 0;
                loop {
                    if i >= c.len() {
                        return err(start_line, format!("the block comment that begins here is not closed (expected '{}')", close));
                    }
                    if *nests && starts(&c, i, open) {
                        depth += 1;
                        out.push_str(open);
                        i += open.chars().count();
                    } else if starts(&c, i, close) {
                        depth -= 1;
                        out.push_str(close);
                        i += close.chars().count();
                        if depth <= 0 {
                            break;
                        }
                    } else if !*nests && depth == 0 {
                        depth = 1;
                        out.push_str(open);
                        i += open.chars().count();
                    } else {
                        if c[i] == '\n' {
                            line += 1;
                        }
                        out.push(c[i]);
                        i += 1;
                    }
                }
                continue;
            }
        }
        // from here the character is code (or the start of a quoted part)
        // strings and quoted identifiers
        let quote = p.strings.iter().map(|q| (*q, false)).chain(p.idents.iter().map(|q| (*q, true))).find(|&((o, _), _)| o == ch);
        if let Some(((open, close), is_ident)) = quote {
            check_code(p, after_semi, line)?;
            any_code = true;
            let start_line = line;
            let prev_is_e = !is_ident && p.estring && matches!(out.chars().last(), Some('e' | 'E')) && !out.chars().rev().nth(1).is_some_and(is_word);
            let backslash = !is_ident && (p.backslash || prev_is_e);
            out.push(open);
            i += 1;
            loop {
                if i >= c.len() {
                    return err(start_line, format!("the {} that begins here is not closed (expected {})", if is_ident { "quoted identifier" } else { "string" }, close));
                }
                let d = c[i];
                if d == '\n' {
                    line += 1;
                }
                if backslash && d == '\\' {
                    out.push(d);
                    if let Some(n) = c.get(i + 1) {
                        if *n == '\n' {
                            line += 1;
                        }
                        out.push(*n);
                    }
                    i += 2;
                    continue;
                }
                out.push(d);
                i += 1;
                if d == close {
                    if c.get(i) == Some(&close) {
                        out.push(close);
                        i += 1;
                        continue;
                    }
                    break;
                }
            }
            continue;
        }
        // dollar quoting and hand-written `$n`
        if ch == '$' {
            let prev = out.chars().last();
            if p.dollar && !prev.is_some_and(|x| is_word(x) || x == '$') {
                let mut j = i + 1;
                while j < c.len() && is_word(c[j]) && !(j == i + 1 && c[j].is_ascii_digit()) {
                    j += 1;
                }
                if c.get(j) == Some(&'$') {
                    check_code(p, after_semi, line)?;
                    any_code = true;
                    let tag: String = c[i..=j].iter().collect();
                    let start_line = line;
                    out.push_str(&tag);
                    i = j + 1;
                    loop {
                        if i >= c.len() {
                            return err(start_line, format!("the dollar-quoted text that begins here is not closed (expected {})", tag));
                        }
                        if starts(&c, i, &tag) {
                            out.push_str(&tag);
                            i += tag.chars().count();
                            break;
                        }
                        if c[i] == '\n' {
                            line += 1;
                        }
                        out.push(c[i]);
                        i += 1;
                    }
                    continue;
                }
            }
            if p.placeholder == Placeholder::Dollar && c.get(i + 1).is_some_and(|d| d.is_ascii_digit()) && !prev.is_some_and(is_word) {
                return err(line, format!("'{}' is a placeholder, which the compiler writes: write the value as {{expr}}", p.placeholder.example()));
            }
        }
        match ch {
            '{' => {
                check_code(p, after_semi, line)?;
                any_code = true;
                if c.get(i + 1) == Some(&'{') {
                    out.push('{');
                    i += 2;
                    continue;
                }
                // a hole: up to the matching `}` on this line, skipping Zila Lang text and char literals
                let start_line = line;
                let mut depth = 1;
                let mut j = i + 1;
                let mut src = String::new();
                while depth > 0 {
                    let Some(&d) = c.get(j) else { return err(start_line, "the hole that begins here is not closed with '}'") };
                    if d == '\n' {
                        return err(start_line, "the hole that begins here is not closed with '}' on its line");
                    }
                    if d == '"' || d == '\'' {
                        src.push(d);
                        j += 1;
                        loop {
                            let Some(&e) = c.get(j) else { return err(start_line, "the hole that begins here is not closed with '}'") };
                            if e == '\n' {
                                return err(start_line, "the hole that begins here is not closed with '}' on its line");
                            }
                            src.push(e);
                            j += 1;
                            if e == '\\' {
                                if let Some(&n) = c.get(j) {
                                    src.push(n);
                                    j += 1;
                                }
                            } else if e == d {
                                break;
                            }
                        }
                        continue;
                    }
                    if d == '{' {
                        depth += 1;
                    } else if d == '}' {
                        depth -= 1;
                        if depth == 0 {
                            j += 1;
                            break;
                        }
                    }
                    src.push(d);
                    j += 1;
                }
                if src.trim().is_empty() {
                    return err(start_line, "empty hole: write a value between the braces");
                }
                holes.push(Hole { src: src.trim().to_string(), line: start_line });
                out.push_str(&p.placeholder.render(holes.len()));
                i = j;
                continue;
            }
            '}' => {
                check_code(p, after_semi, line)?;
                any_code = true;
                if c.get(i + 1) == Some(&'}') {
                    out.push('}');
                    i += 2;
                    continue;
                }
                return err(line, "'}' without a '{': write }} for a brace in the text");
            }
            '(' => {
                check_code(p, after_semi, line)?;
                any_code = true;
                parens.push(line);
            }
            ')' => {
                check_code(p, after_semi, line)?;
                any_code = true;
                if parens.pop().is_none() {
                    return err(line, "')' without a '('");
                }
            }
            ';' => {
                any_code = true;
                after_semi = true;
            }
            '@' if p.placeholder == Placeholder::AtP && matches!(c.get(i + 1), Some('p' | 'P')) && c.get(i + 2).is_some_and(|d| d.is_ascii_digit()) && !out.chars().last().is_some_and(is_word) => {
                return err(line, format!("'{}' is a placeholder, which the compiler writes: write the value as {{expr}}", p.placeholder.example()));
            }
            ':' if p.placeholder == Placeholder::Colon && c.get(i + 1).is_some_and(|d| d.is_ascii_digit()) && !out.ends_with(':') => {
                return err(line, format!("'{}' is a placeholder, which the compiler writes: write the value as {{expr}}", p.placeholder.example()));
            }
            '?' if p.placeholder == Placeholder::Question => {
                return err(line, "'?' is a placeholder, which the compiler writes: write the value as {expr}");
            }
            _ => {
                check_code(p, after_semi, line)?;
                any_code = true;
            }
        }
        out.push(ch);
        i += 1;
    }
    if let Some(l) = parens.first() {
        return err(*l, "this '(' is never closed");
    }
    if !any_code {
        return err(first_line, "the sql block is empty");
    }
    let mut text = out.trim().to_string();
    if text.ends_with(';') {
        text.pop();
        text = text.trim_end().to_string();
    }
    Ok(Scanned { text, holes })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn postgres() -> Profile {
        Profile {
            name: "postgres".into(),
            placeholder: Placeholder::Dollar,
            strings: vec![('\'', '\'')],
            idents: vec![('"', '"')],
            backslash: false,
            estring: true,
            line_comments: vec!["--".into()],
            block_comment: Some(("/*".into(), "*/".into(), true)),
            dollar: true,
            multi: false,
            reject: vec![],
        }
    }

    fn tsql() -> Profile {
        Profile {
            name: "tsql".into(),
            placeholder: Placeholder::AtP,
            strings: vec![('\'', '\'')],
            idents: vec![('[', ']'), ('"', '"')],
            backslash: false,
            estring: false,
            line_comments: vec!["--".into()],
            block_comment: Some(("/*".into(), "*/".into(), true)),
            dollar: false,
            multi: true,
            reject: vec![("GO".into(), "GO is a command of client tools, not T-SQL".into())],
        }
    }

    fn ok(p: &Profile, raw: &str) -> Scanned {
        scan(p, raw, 10).unwrap_or_else(|e| panic!("line {}: {}", e.line, e.msg))
    }

    fn bad(p: &Profile, raw: &str) -> SqlError {
        scan(p, raw, 10).unwrap_err()
    }

    #[test]
    fn holes_become_placeholders_in_order() {
        let s = ok(&postgres(), "    select id from t\n    where a < {limit} and b = {city}");
        assert_eq!(s.text, "select id from t\nwhere a < $1 and b = $2");
        assert_eq!(s.holes, vec![Hole { src: "limit".into(), line: 11 }, Hole { src: "city".into(), line: 11 }]);
        let s = ok(&tsql(), "select {a}, {b}");
        assert_eq!(s.text, "select @p1, @p2");
    }

    #[test]
    fn braces_inside_strings_and_comments_are_text_and_double_braces_escape() {
        let s = ok(&postgres(), "select 'a{b}c', \"x{y}\" -- {c}\n/* {d} */ , {{e}}");
        assert!(s.holes.is_empty());
        assert!(s.text.contains("'a{b}c'") && s.text.contains("{e}"));
    }

    #[test]
    fn a_hole_may_hold_text_and_braces() {
        let s = ok(&postgres(), "select {name.upper() + \"}\"}");
        assert_eq!(s.holes[0].src, "name.upper() + \"}\"");
    }

    #[test]
    fn postgres_dollar_quotes_and_casts() {
        let s = ok(&postgres(), "select $$it's {not a hole}$$, {n}::int, $tag$ x $tag$");
        assert_eq!(s.holes.len(), 1);
        assert!(s.text.contains("$$it's {not a hole}$$") && s.text.contains("$1::int"));
        // `$` inside an identifier is not a dollar quote
        assert!(scan(&postgres(), "select a$b$c from t", 1).is_ok());
    }

    #[test]
    fn brackets_are_identifiers_in_tsql_and_text_in_postgres() {
        assert!(scan(&tsql(), "select [order] from [dbo].[t] where x = {a}", 1).is_ok());
        let s = ok(&postgres(), "select a[1] from t where b = {x}");
        assert_eq!(s.text, "select a[1] from t where b = $1");
        // a `]` doubled stays inside a bracket identifier; an unclosed one is an error
        assert!(scan(&tsql(), "select [a]]b] from t", 1).is_ok());
        assert!(scan(&tsql(), "select [a from t", 1).is_err());
    }

    #[test]
    fn variables_and_casts_are_not_holes() {
        let s = ok(&tsql(), "declare @total int = 0; select @total = count(*) from t where id < {limit}");
        assert_eq!(s.holes.len(), 1);
        assert!(s.text.contains("@total") && s.text.contains("@p1"));
    }

    #[test]
    fn unterminated_constructs_name_the_line_where_they_begin() {
        let e = bad(&postgres(), "select 1\nwhere name = 'abc\nand x = 2");
        assert_eq!(e.line, 11);
        assert!(e.msg.contains("string"));
        assert_eq!(bad(&postgres(), "select 1 /* open\n more").line, 10);
        assert_eq!(bad(&postgres(), "select $a$ open\n more").line, 10);
        assert!(bad(&postgres(), "select \"id from t").msg.contains("quoted identifier"));
    }

    #[test]
    fn nested_block_comments_follow_the_profile() {
        assert!(scan(&postgres(), "select 1 /* a /* b */ c */", 1).is_ok());
        assert!(scan(&postgres(), "select 1 /* a /* b */", 1).is_err());
    }

    #[test]
    fn parentheses_must_balance() {
        assert_eq!(bad(&postgres(), "select (1\n+ 2").line, 10);
        assert!(bad(&postgres(), "select 1)").msg.contains("without"));
        assert!(scan(&postgres(), "select ')' , (2)", 1).is_ok());
    }

    #[test]
    fn hand_written_placeholders_are_refused() {
        assert!(bad(&postgres(), "where id = $1").msg.contains("{expr}"));
        assert!(bad(&tsql(), "where id = @p1").msg.contains("{expr}"));
        let colon = Profile { placeholder: Placeholder::Colon, dollar: false, ..postgres() };
        assert!(bad(&colon, "where id = :1").msg.contains("{expr}"));
        assert!(scan(&colon, "select a::int, {x}", 1).is_ok());
        let q = Profile { placeholder: Placeholder::Question, dollar: false, ..postgres() };
        assert!(bad(&q, "where id = ?").msg.contains("{expr}"));
        assert_eq!(ok(&q, "where id = {x} and b = {y}").text, "where id = ? and b = ?");
    }

    #[test]
    fn one_statement_per_block_unless_the_dialect_allows_more() {
        assert!(bad(&postgres(), "select 1; select 2").msg.contains("one statement"));
        assert!(scan(&tsql(), "select 1; select 2", 1).is_ok());
        assert_eq!(ok(&postgres(), "select 1;").text, "select 1");
        assert_eq!(ok(&postgres(), "select 1 ;  \n").text, "select 1");
        // a `;` in a string or a comment is not a separator
        assert!(scan(&postgres(), "select 'a; b' -- ; x\n, 2", 1).is_ok());
    }

    #[test]
    fn go_is_refused_in_tsql_with_the_reason() {
        let e = bad(&tsql(), "select 1\n  go\nselect 2");
        assert_eq!(e.line, 11);
        assert!(e.msg.contains("client tools"));
    }

    #[test]
    fn empty_blocks_and_stray_braces_are_errors() {
        assert!(bad(&postgres(), "-- nothing\n").msg.contains("empty"));
        assert!(bad(&postgres(), "select }").msg.contains("}}"));
        assert!(bad(&postgres(), "select {a").msg.contains("not closed"));
        assert!(bad(&postgres(), "select {}").msg.contains("empty hole"));
    }

    #[test]
    fn e_strings_escape_with_a_backslash_and_plain_strings_do_not() {
        assert!(scan(&postgres(), "select E'it\\'s {x}', {y}", 1).unwrap().holes.len() == 1);
        // a plain string ends at the second quote, so the brace after it is a hole
        assert_eq!(scan(&postgres(), "select 'a\\' , {y}", 1).unwrap().holes.len(), 1);
    }

    #[test]
    fn indentation_is_removed_and_ends_are_trimmed() {
        assert_eq!(ok(&postgres(), "\n        select 1\n          from t\n").text, "select 1\n  from t");
    }

    #[test]
    fn profiles_are_read_from_attributes() {
        let src = "package p\n#[sql_dialect(\"sqlite\")]\n#[sql_placeholder(\"?\")]\n#[sql_string(\"'\")]\n#[sql_ident(\"[\", \"]\")]\n#[sql_ident(\"`\")]\n#[sql_line_comment(\"--\")]\n#[sql_block_comment(\"/*\", \"*/\")]\n#[sql_multi]\npub type Dialect = record\n    x: int\nend\n";
        let f = crate::parse_source(src).unwrap();
        let p = profile_from_attrs(&f.decls[0].attrs).unwrap().unwrap();
        assert_eq!(p.name, "sqlite");
        assert_eq!(p.placeholder, Placeholder::Question);
        assert_eq!(p.idents, vec![('[', ']'), ('`', '`')]);
        assert!(p.multi && !p.dollar);
        assert_eq!(p.block_comment, Some(("/*".into(), "*/".into(), false)));
        let src = "package p\n#[sql_dialect(\"x\")]\npub type D = record\n    x: int\nend\n";
        let f = crate::parse_source(src).unwrap();
        assert!(profile_from_attrs(&f.decls[0].attrs).unwrap().unwrap_err().contains("sql_placeholder"));
    }
}
