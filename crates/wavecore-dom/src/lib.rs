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

impl ElementData {
    pub fn id(&self) -> Option<&str> {
        self.attributes.get("id").map(String::as_str)
    }

    pub fn has_class(&self, class: &str) -> bool {
        self.attributes
            .get("class")
            .is_some_and(|v| v.split_ascii_whitespace().any(|c| c == class))
    }

    pub fn name(&self) -> Option<&str> {
        self.attributes.get("name").map(String::as_str)
    }

    pub fn value(&self) -> Option<&str> {
        self.attributes.get("value").map(String::as_str)
    }

    pub fn placeholder(&self) -> Option<&str> {
        self.attributes.get("placeholder").map(String::as_str)
    }

    pub fn input_type(&self) -> &str {
        self.attributes
            .get("type")
            .map(String::as_str)
            .unwrap_or("text")
    }

    pub fn is_form_control(&self) -> bool {
        matches!(
            self.tag_name.to_ascii_lowercase().as_str(),
            "input" | "button" | "textarea" | "select"
        )
    }

    pub fn get_attribute(&self, name: &str) -> Option<&str> {
        self.attributes.get(name).map(String::as_str)
    }

    pub fn set_attribute(&mut self, name: impl Into<String>, val: impl Into<String>) {
        self.attributes.insert(name.into(), val.into());
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub node_type: NodeType,
    pub children: Vec<Node>,
}

impl Node {
    pub fn document(children: Vec<Node>) -> Self {
        Self {
            node_type: NodeType::Document,
            children,
        }
    }

    pub fn text(value: impl Into<String>) -> Self {
        Self {
            node_type: NodeType::Text(value.into()),
            children: vec![],
        }
    }

    pub fn element(tag_name: impl Into<String>, children: Vec<Node>) -> Self {
        Self::element_with_attributes(tag_name, BTreeMap::new(), children)
    }

    pub fn element_with_attributes(
        tag_name: impl Into<String>,
        attributes: BTreeMap<String, String>,
        children: Vec<Node>,
    ) -> Self {
        Self {
            node_type: NodeType::Element(ElementData {
                tag_name: tag_name.into(),
                attributes,
            }),
            children,
        }
    }

    pub fn find_by_id(&self, id: &str) -> Option<&Node> {
        if let NodeType::Element(e) = &self.node_type {
            if e.id() == Some(id) {
                return Some(self);
            }
        }
        for c in &self.children {
            if let Some(n) = c.find_by_id(id) {
                return Some(n);
            }
        }
        None
    }

    pub fn find_by_id_mut(&mut self, id: &str) -> Option<&mut Node> {
        if let NodeType::Element(e) = &self.node_type {
            if e.id() == Some(id) {
                return Some(self);
            }
        }
        for c in &mut self.children {
            if let Some(n) = c.find_by_id_mut(id) {
                return Some(n);
            }
        }
        None
    }

    pub fn query_selector(&self, selector: &str) -> Option<&Node> {
        let s = selector.trim();
        if let Some(id) = s.strip_prefix('#') {
            return self.find_by_id(id);
        }
        if let NodeType::Element(e) = &self.node_type {
            if let Some(class) = s.strip_prefix('.') {
                if e.has_class(class) {
                    return Some(self);
                }
            } else if e.tag_name.eq_ignore_ascii_case(s) {
                return Some(self);
            }
        }
        for c in &self.children {
            if let Some(n) = c.query_selector(s) {
                return Some(n);
            }
        }
        None
    }

    pub fn query_selector_mut(&mut self, selector: &str) -> Option<&mut Node> {
        let s = selector.trim();
        if let Some(id) = s.strip_prefix('#') {
            return self.find_by_id_mut(id);
        }
        if let NodeType::Element(e) = &self.node_type {
            if let Some(class) = s.strip_prefix('.') {
                if e.has_class(class) {
                    return Some(self);
                }
            } else if e.tag_name.eq_ignore_ascii_case(s) {
                return Some(self);
            }
        }
        for c in &mut self.children {
            if let Some(n) = c.query_selector_mut(s) {
                return Some(n);
            }
        }
        None
    }

    pub fn inner_text(&self) -> String {
        match &self.node_type {
            NodeType::Text(s) => s.clone(),
            _ => {
                let mut buf = String::new();
                for c in &self.children {
                    buf.push_str(&c.inner_text());
                }
                buf
            }
        }
    }

    pub fn set_inner_text(&mut self, text: &str) {
        self.children = vec![Node::text(text)];
    }

    pub fn append_child(&mut self, child: Node) {
        self.children.push(child);
    }

    pub fn remove_child_at(&mut self, index: usize) -> Option<Node> {
        if index < self.children.len() {
            Some(self.children.remove(index))
        } else {
            None
        }
    }

    pub fn tag_name(&self) -> Option<&str> {
        if let NodeType::Element(e) = &self.node_type {
            Some(&e.tag_name)
        } else {
            None
        }
    }

    pub fn class_name(&self) -> Option<&str> {
        if let NodeType::Element(e) = &self.node_type {
            e.attributes.get("class").map(String::as_str)
        } else {
            None
        }
    }

    pub fn set_class_name(&mut self, class_name: &str) {
        if let NodeType::Element(e) = &mut self.node_type {
            e.set_attribute("class", class_name);
        }
    }

    pub fn add_class(&mut self, class: &str) {
        if let NodeType::Element(e) = &mut self.node_type {
            let mut classes: Vec<String> = e
                .attributes
                .get("class")
                .map(|s| s.split_whitespace().map(String::from).collect())
                .unwrap_or_default();
            if !classes.iter().any(|c| c == class) {
                classes.push(class.to_string());
                e.set_attribute("class", classes.join(" "));
            }
        }
    }

    pub fn remove_class(&mut self, class: &str) {
        if let NodeType::Element(e) = &mut self.node_type {
            if let Some(existing) = e.attributes.get("class") {
                let classes: Vec<&str> = existing
                    .split_whitespace()
                    .filter(|c| *c != class)
                    .collect();
                e.set_attribute("class", classes.join(" "));
            }
        }
    }

    pub fn toggle_class(&mut self, class: &str) -> bool {
        if let NodeType::Element(e) = &mut self.node_type {
            let mut classes: Vec<String> = e
                .attributes
                .get("class")
                .map(|s| s.split_whitespace().map(String::from).collect())
                .unwrap_or_default();
            if let Some(pos) = classes.iter().position(|c| c == class) {
                classes.remove(pos);
                e.set_attribute("class", classes.join(" "));
                false
            } else {
                classes.push(class.to_string());
                e.set_attribute("class", classes.join(" "));
                true
            }
        } else {
            false
        }
    }
}
