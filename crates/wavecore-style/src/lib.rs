use std::collections::BTreeMap;
use wavecore_css::Stylesheet;
use wavecore_dom::{ElementData,Node,NodeType};

#[derive(Debug,Clone)]
pub struct StyledNode { pub node:Node, pub properties:BTreeMap<String,String>, pub children:Vec<StyledNode> }

pub fn style_tree(node:&Node,sheet:&Stylesheet)->StyledNode{
 let mut properties=BTreeMap::new();
 if let NodeType::Element(element)=&node.node_type {
  let mut matches:Vec<_>=sheet.rules.iter().enumerate().filter_map(|(order,r)| selector_specificity(&r.selector,element).map(|s|(s,order,r))).collect();
  matches.sort_by_key(|(s,o,_)|(*s,*o));
  for (_,_,rule) in matches { properties.extend(rule.declarations.clone()); }
 }
 StyledNode{node:node.clone(),properties,children:node.children.iter().map(|c|style_tree(c,sheet)).collect()}
}
fn selector_specificity(selector:&str,e:&ElementData)->Option<(u16,u16,u16)>{
 let s=selector.trim(); if s=="*"{return Some((0,0,0))}
 if let Some(id)=s.strip_prefix('#'){return (e.id()==Some(id)).then_some((1,0,0))}
 if let Some(class)=s.strip_prefix('.'){return e.has_class(class).then_some((0,1,0))}
 s.eq_ignore_ascii_case(&e.tag_name).then_some((0,0,1))
}
#[cfg(test)]
mod tests{
 use super::*; use wavecore_css::parse; use wavecore_html::parse as html;
 #[test] fn id_beats_class_and_tag(){let d=html("<p id=\"x\" class=\"note\">x</p>"); let s=style_tree(&d,&parse("p{color:black}.note{color:blue}#x{color:red}")); assert_eq!(s.children[0].properties.get("color").map(String::as_str),Some("red"));}
}
