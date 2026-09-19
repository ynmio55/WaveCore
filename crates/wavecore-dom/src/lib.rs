use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum NodeType {
    Document,
    Text(String),
    Element(ElementData),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ElementData {
    pub tag_name: String,
    pub attributes: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub node_type: NodeType,
    pub children: Vec<Node>,
}

impl Node {
    pub fn document(children: Vec<Node>) -> Self {
        Self { node_type: NodeType::Document, children }
    }

    pub fn text(value: impl Into<String>) -> Self {
        Self { node_type: NodeType::Text(value.into()), children: vec![] }
    }

    pub fn element(tag_name: impl Into<String>, children: Vec<Node>) -> Self {
        Self {
            node_type: NodeType::Element(ElementData {
                tag_name: tag_name.into(),
                attributes: BTreeMap::new(),
            }),
            children,
        }
    }
}
