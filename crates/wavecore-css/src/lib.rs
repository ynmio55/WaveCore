use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Stylesheet {
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub selector: String,
    pub declarations: BTreeMap<String, String>,
}

pub fn parse(input: &str) -> Stylesheet {
    let source = strip_comments(input);
    let mut rules = Vec::new();
    parse_rule_list(&source, &mut rules);
    Stylesheet { rules }
}

fn strip_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

fn parse_rule_list(input: &str, out: &mut Vec<Rule>) {
    let bytes = input.as_bytes();
    let mut cursor = 0usize;

    while cursor < bytes.len() {
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor >= bytes.len() {
            break;
        }

        let Some(open_rel) = input[cursor..].find('{') else {
            break;
        };
        let open = cursor + open_rel;
        let header = input[cursor..open].trim();
        let Some(close) = matching_brace(input, open) else {
            break;
        };
        let body = &input[open + 1..close];

        if header.starts_with('@') {
            let at_name = header
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_ascii_lowercase();
            // Flatten conditional rule groups for now. The viewport/media evaluator
            // will eventually decide applicability; parsing them keeps rules usable.
            if matches!(
                at_name.as_str(),
                "@media" | "@supports" | "@layer" | "@container"
            ) {
                parse_rule_list(body, out);
            }
        } else if !header.is_empty() {
            out.push(Rule {
                selector: header.to_string(),
                declarations: parse_declarations(body),
            });
        }

        cursor = close + 1;
    }
}

fn matching_brace(input: &str, open: usize) -> Option<usize> {
    let bytes = input.as_bytes();
    let mut depth = 0usize;
    let mut quote: Option<u8> = None;
    let mut i = open;

    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == q {
                quote = None;
            }
        } else {
            match b {
                b'\'' | b'"' => quote = Some(b),
                b'{' => depth += 1,
                b'}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    None
}

pub fn parse_declarations(body: &str) -> BTreeMap<String, String> {
    let mut declarations = BTreeMap::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let chars: Vec<(usize, char)> = body.char_indices().collect();

    let mut flush = |end: usize| {
        let decl = body[start..end].trim();
        if let Some((name, value)) = split_declaration(decl) {
            declarations.insert(name.to_ascii_lowercase(), value.to_string());
        }
    };

    for (idx, ch) in chars {
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            ';' if depth == 0 => {
                flush(idx);
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }
    flush(body.len());
    declarations
}

fn split_declaration(decl: &str) -> Option<(&str, &str)> {
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    for (idx, ch) in decl.char_indices() {
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            ':' if depth == 0 => {
                let name = decl[..idx].trim();
                let value = decl[idx + 1..].trim();
                if !name.is_empty() && !value.is_empty() {
                    return Some((name, value));
                }
                return None;
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rule() {
        let css = parse("h1 { color: red; margin: 8px; }");
        assert_eq!(css.rules[0].declarations["color"], "red");
    }

    #[test]
    fn comments_and_nested_at_rules_do_not_break_parser() {
        let css = parse(
            r#"
            /* theme */
            @media screen and (min-width: 600px) {
                .card { color: red; width: calc(100% - 20px); }
            }
            @supports (display: grid) {
                .grid { display: grid; }
            }
            "#,
        );
        assert_eq!(css.rules.len(), 2);
        assert_eq!(css.rules[0].selector, ".card");
        assert_eq!(css.rules[0].declarations["width"], "calc(100% - 20px)");
        assert_eq!(css.rules[1].declarations["display"], "grid");
    }

    #[test]
    fn declaration_parser_preserves_colons_and_semicolons_inside_functions() {
        let decls = parse_declarations(
            r#"background: url("data:image/svg+xml;a:b"); color: blue;"#,
        );
        assert_eq!(
            decls["background"],
            r#"url("data:image/svg+xml;a:b")"#
        );
        assert_eq!(decls["color"], "blue");
    }
}
