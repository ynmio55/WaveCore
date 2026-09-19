use std::collections::BTreeMap;
use wavecore_css::Stylesheet;
use wavecore_dom::{Node, NodeType};

#[derive(Debug, Clone)]
pub struct StyledNode {
    pub node: Node,
    pub properties: BTreeMap<String, String>,
    pub children: Vec<StyledNode>,
}

pub fn style_tree(node: &Node, sheet: &Stylesheet) -> StyledNode {
    let mut properties = BTreeMap::new();
    if let NodeType::Element(element) = &node.node_type {
        for rule in &sheet.rules {
            if rule.selector == "*" || rule.selector.eq_ignore_ascii_case(&element.tag_name) {
                properties.extend(rule.declarations.clone());
            }
        }
    }
    StyledNode {
        node: node.clone(),
        properties,
        children: node.children.iter().map(|child| style_tree(child, sheet)).collect(),
    }
}
