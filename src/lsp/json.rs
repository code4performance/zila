//! The JSON values of the protocol: a small reader and writer, no dependencies.

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn get(&self, key: &str) -> &Json {
        if let Json::Obj(v) = self {
            for (k, x) in v {
                if k == key {
                    return x;
                }
            }
        }
        &Json::Null
    }

    pub fn str(&self) -> Option<&str> {
        if let Json::Str(s) = self {
            Some(s)
        } else {
            None
        }
    }

    pub fn write(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Num(n) => out.push_str(&format!("{}", n)),
            Json::Str(s) => {
                out.push('"');
                for c in s.chars() {
                    match c {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\r' => out.push_str("\\r"),
                        '\t' => out.push_str("\\t"),
                        c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                        c => out.push(c),
                    }
                }
                out.push('"');
            }
            Json::Arr(v) => {
                out.push('[');
                for (i, x) in v.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    x.write(out);
                }
                out.push(']');
            }
            Json::Obj(v) => {
                out.push('{');
                for (i, (k, x)) in v.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    Json::Str(k.clone()).write(out);
                    out.push(':');
                    x.write(out);
                }
                out.push('}');
            }
        }
    }
}

pub(super) fn obj(fields: Vec<(&str, Json)>) -> Json {
    Json::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

pub(super) fn num(n: i64) -> Json {
    Json::Num(n as f64)
}

pub fn parse_json(src: &str) -> Result<Json, String> {
    let c: Vec<char> = src.chars().collect();
    let mut p = 0;
    let v = value(&c, &mut p)?;
    skip_ws(&c, &mut p);
    if p != c.len() {
        return Err("trailing characters".into());
    }
    Ok(v)
}

fn skip_ws(c: &[char], p: &mut usize) {
    while *p < c.len() && c[*p].is_whitespace() {
        *p += 1;
    }
}

fn value(c: &[char], p: &mut usize) -> Result<Json, String> {
    skip_ws(c, p);
    let Some(&ch) = c.get(*p) else { return Err("unexpected end".into()) };
    match ch {
        '{' => {
            *p += 1;
            let mut v = Vec::new();
            skip_ws(c, p);
            if c.get(*p) == Some(&'}') {
                *p += 1;
                return Ok(Json::Obj(v));
            }
            loop {
                skip_ws(c, p);
                let Json::Str(k) = value(c, p)? else { return Err("object key must be a string".into()) };
                skip_ws(c, p);
                if c.get(*p) != Some(&':') {
                    return Err("expected ':'".into());
                }
                *p += 1;
                v.push((k, value(c, p)?));
                skip_ws(c, p);
                match c.get(*p) {
                    Some(',') => *p += 1,
                    Some('}') => {
                        *p += 1;
                        return Ok(Json::Obj(v));
                    }
                    _ => return Err("expected ',' or '}'".into()),
                }
            }
        }
        '[' => {
            *p += 1;
            let mut v = Vec::new();
            skip_ws(c, p);
            if c.get(*p) == Some(&']') {
                *p += 1;
                return Ok(Json::Arr(v));
            }
            loop {
                v.push(value(c, p)?);
                skip_ws(c, p);
                match c.get(*p) {
                    Some(',') => *p += 1,
                    Some(']') => {
                        *p += 1;
                        return Ok(Json::Arr(v));
                    }
                    _ => return Err("expected ',' or ']'".into()),
                }
            }
        }
        '"' => {
            *p += 1;
            let mut s = String::new();
            loop {
                let Some(&ch) = c.get(*p) else { return Err("unterminated string".into()) };
                *p += 1;
                match ch {
                    '"' => return Ok(Json::Str(s)),
                    '\\' => {
                        let Some(&e) = c.get(*p) else { return Err("bad escape".into()) };
                        *p += 1;
                        match e {
                            'n' => s.push('\n'),
                            'r' => s.push('\r'),
                            't' => s.push('\t'),
                            'b' => s.push('\u{8}'),
                            'f' => s.push('\u{c}'),
                            'u' => {
                                let mut code = hex4(c, p)?;
                                if (0xD800..0xDC00).contains(&code) && c.get(*p) == Some(&'\\') && c.get(*p + 1) == Some(&'u') {
                                    *p += 2;
                                    let low = hex4(c, p)?;
                                    code = 0x10000 + ((code - 0xD800) << 10) + (low.wrapping_sub(0xDC00) & 0x3FF);
                                }
                                s.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                            }
                            other => s.push(other),
                        }
                    }
                    ch => s.push(ch),
                }
            }
        }
        't' | 'f' | 'n' => {
            for (word, v) in [("true", Json::Bool(true)), ("false", Json::Bool(false)), ("null", Json::Null)] {
                if c[*p..].iter().take(word.len()).collect::<String>() == word {
                    *p += word.len();
                    return Ok(v);
                }
            }
            Err("bad literal".into())
        }
        _ => {
            let start = *p;
            while *p < c.len() && matches!(c[*p], '-' | '+' | '.' | 'e' | 'E' | '0'..='9') {
                *p += 1;
            }
            c[start..*p].iter().collect::<String>().parse::<f64>().map(Json::Num).map_err(|_| "bad number".to_string())
        }
    }
}

fn hex4(c: &[char], p: &mut usize) -> Result<u32, String> {
    let s: String = c.iter().skip(*p).take(4).collect();
    *p += 4;
    u32::from_str_radix(&s, 16).map_err(|_| "bad \\u escape".to_string())
}
