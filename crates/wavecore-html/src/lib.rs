use std::collections::BTreeMap;
use wavecore_dom::Node;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentMode {
    Standards,
    Quirks,
}

pub fn parse(input: &str) -> Node {
    let mut parser = Parser { input, pos: 0 };
    Node::document(parser.parse_nodes(None))
}

pub fn parse_with_mode(input: &str) -> (Node, DocumentMode) {
    let trimmed = input.trim_start();
    let mode = if trimmed.to_ascii_lowercase().starts_with("<!doctype html")
        || trimmed.to_ascii_lowercase().starts_with("<!doctype")
    {
        DocumentMode::Standards
    } else {
        DocumentMode::Quirks
    };
    (parse(input), mode)
}
struct Parser<'a> { input: &'a str, pos: usize }
impl<'a> Parser<'a> {
    fn parse_nodes(&mut self, closing: Option<&str>) -> Vec<Node> {
        let mut nodes=Vec::new();
        while self.pos < self.input.len() {
            if let Some(parent) = closing {
                if let Some(next) = self.peek_start_tag_name() {
                    if should_implicitly_close(parent, &next) {
                        break;
                    }
                }
            }
            if self.starts_with("<!--") { self.pos+=4; self.consume_through("-->"); continue; }
            if self.starts_with("<!") || self.starts_with("<?") { self.consume_through(">"); continue; }
            if self.starts_with("</") {
                self.pos+=2; let name=self.consume_until('>'); self.consume_char('>');
                if closing.is_some_and(|t| t.eq_ignore_ascii_case(name.trim())) { break; }
                continue;
            }
            if self.starts_with("<") {
                self.pos+=1; let raw=self.consume_until('>'); self.consume_char('>');
                let self_closing=raw.trim_end().ends_with('/');
                let (tag,attrs)=parse_start_tag(raw.trim_end_matches('/').trim());
                if tag.is_empty(){continue}
                let is_void = matches!(tag.as_str(), "area" | "base" | "br" | "col" | "embed" | "hr" | "img" | "input" | "link" | "meta" | "source" | "track" | "wbr");
                let children = if self_closing || is_void {
                    vec![]
                } else if tag == "script" || tag == "style" {
                    let close_pattern = format!("</{}", tag);
                    let start = self.pos;
                    let lower = self.input[self.pos..].to_ascii_lowercase();
                    let raw_content = if let Some(idx) = lower.find(&close_pattern) {
                        let content = self.input[start..start + idx].to_string();
                        self.pos = start + idx;
                        self.consume_through(">");
                        content
                    } else {
                        let content = self.input[start..].to_string();
                        self.pos = self.input.len();
                        content
                    };
                    vec![Node::text(raw_content)]
                } else {
                    self.parse_nodes(Some(&tag))
                };
                nodes.push(Node::element_with_attributes(tag, attrs, children));
            } else {
                let text=self.consume_until('<');
                if !text.is_empty(){
                    nodes.push(Node::text(decode_entities(&text)));
                }
            }
        } nodes
    }
    fn starts_with(&self,s:&str)->bool{self.input[self.pos..].starts_with(s)}
    fn peek_start_tag_name(&self) -> Option<String> {
        if !self.starts_with("<") || self.starts_with("</") || self.starts_with("<!") || self.starts_with("<?") {
            return None;
        }
        let rest = &self.input[self.pos + 1..];
        let mut name = String::new();
        for ch in rest.chars() {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == ':' {
                name.push(ch.to_ascii_lowercase());
            } else {
                break;
            }
        }
        (!name.is_empty()).then_some(name)
    }
    fn consume_until(&mut self,ch:char)->String{let start=self.pos; while self.pos<self.input.len()&&self.input[self.pos..].chars().next()!=Some(ch){self.pos+=self.input[self.pos..].chars().next().unwrap().len_utf8();} self.input[start..self.pos].to_owned()}
    fn consume_char(&mut self,c:char){if self.input[self.pos..].chars().next()==Some(c){self.pos+=c.len_utf8();}}
    fn consume_through(&mut self,needle:&str){if let Some(i)=self.input[self.pos..].find(needle){self.pos+=i+needle.len()}else{self.pos=self.input.len()}}
}

fn should_implicitly_close(parent: &str, next: &str) -> bool {
    match parent.to_ascii_lowercase().as_str() {
        "p" => matches!(
            next,
            "address" | "article" | "aside" | "blockquote" | "div" | "dl" | "fieldset"
                | "footer" | "form" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
                | "header" | "hr" | "main" | "nav" | "ol" | "p" | "pre" | "section"
                | "table" | "ul"
        ),
        "li" => next == "li",
        "dt" | "dd" => matches!(next, "dt" | "dd"),
        "thead" => matches!(next, "tbody" | "tfoot"),
        "tbody" => matches!(next, "tbody" | "tfoot"),
        "tr" => next == "tr",
        "th" | "td" => matches!(next, "th" | "td"),
        "option" => matches!(next, "option" | "optgroup"),
        _ => false,
    }
}

fn parse_start_tag(raw:&str)->(String,BTreeMap<String,String>){
    let mut chars=raw.char_indices().peekable(); let mut end=0;
    while let Some((i,c))=chars.peek().copied(){if c.is_whitespace(){break} end=i+c.len_utf8(); chars.next();}
    let tag=raw[..end].to_ascii_lowercase(); let mut attrs=BTreeMap::new(); let mut i=end; let b=raw.as_bytes();
    while i<raw.len(){while i<raw.len()&&b[i].is_ascii_whitespace(){i+=1} if i>=raw.len(){break}
        let start=i; while i<raw.len()&&!b[i].is_ascii_whitespace()&&b[i]!=b'='{i+=1}
        let name=raw[start..i].to_ascii_lowercase(); while i<raw.len()&&b[i].is_ascii_whitespace(){i+=1}
        let mut value=String::new();
        if i<raw.len()&&b[i]==b'='{i+=1; while i<raw.len()&&b[i].is_ascii_whitespace(){i+=1}
            if i<raw.len()&&(b[i]==b'"'||b[i]==b'\''){let q=b[i]; i+=1; let s=i; while i<raw.len()&&b[i]!=q{i+=1} value=raw[s..i].to_string(); if i<raw.len(){i+=1}}
            else {let s=i; while i<raw.len()&&!b[i].is_ascii_whitespace(){i+=1} value=raw[s..i].to_string();}
        }
        if !name.is_empty(){attrs.insert(name,value);}
    } (tag,attrs)
}
pub fn decode_entities(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '&' {
            let mut entity = String::new();
            let mut found_semi = false;
            while let Some(&next_c) = chars.peek() {
                if next_c == ';' {
                    chars.next();
                    found_semi = true;
                    break;
                } else if next_c.is_alphanumeric() || next_c == '#' {
                    entity.push(chars.next().unwrap());
                    if entity.len() > 10 { break; }
                } else {
                    break;
                }
            }
            if found_semi {
                match entity.as_str() {
                    "amp" => out.push('&'),
                    "lt" => out.push('<'),
                    "gt" => out.push('>'),
                    "quot" => out.push('"'),
                    "apos" => out.push('\''),
                    "nbsp" => out.push(' '),
                    "bull" => out.push('•'),
                    "copy" => out.push('©'),
                    "reg" => out.push('®'),
                    "trade" => out.push('™'),
                    "mdash" => out.push('—'),
                    "ndash" => out.push('–'),
                    "hellip" => out.push('…'),
                    "euro" => out.push('€'),
                    "pound" => out.push('£'),
                    "yen" => out.push('¥'),
                    "deg" => out.push('°'),
                    "plusmn" => out.push('±'),
                    "times" => out.push('×'),
                    "divide" => out.push('÷'),
                    s if s.starts_with("#x") || s.starts_with("#X") => {
                        if let Ok(val) = u32::from_str_radix(&s[2..], 16) {
                            if let Some(ch) = char::from_u32(val) {
                                out.push(ch);
                            } else {
                                out.push('&'); out.push_str(&entity); out.push(';');
                            }
                        } else {
                            out.push('&'); out.push_str(&entity); out.push(';');
                        }
                    }
                    s if s.starts_with('#') => {
                        if let Ok(val) = s[1..].parse::<u32>() {
                            if let Some(ch) = char::from_u32(val) {
                                out.push(ch);
                            } else {
                                out.push('&'); out.push_str(&entity); out.push(';');
                            }
                        } else {
                            out.push('&'); out.push_str(&entity); out.push(';');
                        }
                    }
                    _ => {
                        out.push('&');
                        out.push_str(&entity);
                        out.push(';');
                    }
                }
            } else {
                out.push('&');
                out.push_str(&entity);
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub fn extract_styles(node: &Node) -> String {
    let mut s = String::new();
    walk_styles(node, &mut s);
    s
}

fn walk_styles(node: &Node, out: &mut String) {
    if let wavecore_dom::NodeType::Element(e) = &node.node_type {
        if e.tag_name.eq_ignore_ascii_case("style") {
            for child in &node.children {
                if let wavecore_dom::NodeType::Text(text) = &child.node_type {
                    out.push_str(text);
                    out.push('\n');
                }
            }
        }
    }
    for child in &node.children {
        walk_styles(child, out);
    }
}

pub fn extract_scripts(node: &Node) -> Vec<String> {
    let mut v = Vec::new();
    walk_scripts(node, &mut v);
    v
}

fn walk_scripts(node: &Node, out: &mut Vec<String>) {
    if let wavecore_dom::NodeType::Element(e) = &node.node_type {
        if e.tag_name.eq_ignore_ascii_case("script") {
            let mut script_body = String::new();
            for child in &node.children {
                if let wavecore_dom::NodeType::Text(text) = &child.node_type {
                    script_body.push_str(text);
                    script_body.push('\n');
                }
            }
            let trimmed = script_body.trim();
            if !trimmed.is_empty() {
                out.push(trimmed.to_string());
            }
        }
    }
    for child in &node.children {
        walk_scripts(child, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wavecore_dom::NodeType;

    #[test]
    fn parses_document_attributes_void_and_comments() {
        let dom = parse("<!doctype html><!--x--><main id=\"app\" class=\"page hero\"><img src=\"a.png\"><p>Hello &bull; &copy; 2026</p></main>");
        assert_eq!(dom.children.len(), 1);
        let NodeType::Element(main) = &dom.children[0].node_type else { panic!() };
        assert_eq!(main.tag_name, "main");
        assert_eq!(main.id(), Some("app"));
        assert!(main.has_class("hero"));
        assert_eq!(dom.children[0].children.len(), 2);

        let NodeType::Element(p) = &dom.children[0].children[1].node_type else { panic!() };
        assert_eq!(p.tag_name, "p");
        let NodeType::Text(text) = &dom.children[0].children[1].children[0].node_type else { panic!() };
        assert_eq!(text, "Hello • © 2026");
    }

    #[test]
    fn extracts_embedded_style_tags() {
        let dom = parse("<html><head><style>body { color: red; }</style></head><body><h1>Hi</h1></body></html>");
        let css = extract_styles(&dom);
        assert!(css.contains("color: red"));
    }

    #[test]
    fn extracts_script_tags() {
        let dom = parse("<html><head><script>let x = 10;</script></head><body><script>console.log('hi');</script></body></html>");
        let scripts = extract_scripts(&dom);
        assert_eq!(scripts.len(), 2);
        assert_eq!(scripts[0], "let x = 10;");
        assert_eq!(scripts[1], "console.log('hi');");
    }

    #[test]
    fn doctype_and_entity_extensions() {
        let (dom_standards, mode_std) = parse_with_mode("<!DOCTYPE html><html><body>&euro; 100 &plusmn; 5 &hellip;</body></html>");
        assert_eq!(mode_std, DocumentMode::Standards);
        let html_node = &dom_standards.children[0];
        let body_node = &html_node.children[0];
        let NodeType::Text(text) = &body_node.children[0].node_type else { panic!() };
        assert_eq!(text, "€ 100 ± 5 …");

        let (_, mode_quirks) = parse_with_mode("<div>No doctype page</div>");
        assert_eq!(mode_quirks, DocumentMode::Quirks);
    }
    #[test]
    fn implied_end_tags_recover_common_malformed_html() {
        let dom = parse("<ul><li>one<li>two<li>three</ul>");
        let ul = &dom.children[0];
        assert_eq!(ul.children.len(), 3);
        assert_eq!(ul.children[0].inner_text(), "one");
        assert_eq!(ul.children[1].inner_text(), "two");
        assert_eq!(ul.children[2].inner_text(), "three");

        let dom = parse("<p>intro<div>block</div><p>after");
        assert_eq!(dom.children.len(), 3);
        assert_eq!(dom.children[0].tag_name(), Some("p"));
        assert_eq!(dom.children[1].tag_name(), Some("div"));
        assert_eq!(dom.children[2].tag_name(), Some("p"));
    }

}
