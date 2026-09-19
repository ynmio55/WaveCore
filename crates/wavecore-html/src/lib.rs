use std::collections::BTreeMap;
use wavecore_dom::Node;

pub fn parse(input: &str) -> Node {
    let mut parser = Parser { input, pos: 0 };
    Node::document(parser.parse_nodes(None))
}
struct Parser<'a> { input: &'a str, pos: usize }
impl<'a> Parser<'a> {
    fn parse_nodes(&mut self, closing: Option<&str>) -> Vec<Node> {
        let mut nodes=Vec::new();
        while self.pos < self.input.len() {
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
                let is_void=matches!(tag.as_str(),"area"|"base"|"br"|"col"|"embed"|"hr"|"img"|"input"|"link"|"meta"|"source"|"track"|"wbr");
                let children=if self_closing||is_void {vec![]} else {self.parse_nodes(Some(&tag))};
                nodes.push(Node::element_with_attributes(tag,attrs,children));
            } else {
                let text=self.consume_until('<'); if !text.is_empty(){nodes.push(Node::text(text));}
            }
        } nodes
    }
    fn starts_with(&self,s:&str)->bool{self.input[self.pos..].starts_with(s)}
    fn consume_until(&mut self,ch:char)->String{let start=self.pos; while self.pos<self.input.len()&&self.input[self.pos..].chars().next()!=Some(ch){self.pos+=self.input[self.pos..].chars().next().unwrap().len_utf8();} self.input[start..self.pos].to_owned()}
    fn consume_char(&mut self,c:char){if self.input[self.pos..].chars().next()==Some(c){self.pos+=c.len_utf8();}}
    fn consume_through(&mut self,needle:&str){if let Some(i)=self.input[self.pos..].find(needle){self.pos+=i+needle.len()}else{self.pos=self.input.len()}}
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
#[cfg(test)]
mod tests {
 use super::*; use wavecore_dom::NodeType;
 #[test] fn parses_document_attributes_void_and_comments(){
  let dom=parse("<!doctype html><!--x--><main id=\"app\" class=\"page hero\"><img src=\"a.png\"><p>Hello</p></main>");
  assert_eq!(dom.children.len(),1); let NodeType::Element(main)=&dom.children[0].node_type else{panic!()};
  assert_eq!(main.tag_name,"main"); assert_eq!(main.id(),Some("app")); assert!(main.has_class("hero")); assert_eq!(dom.children[0].children.len(),2);
 }
}
