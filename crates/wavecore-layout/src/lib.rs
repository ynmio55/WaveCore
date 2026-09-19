use wavecore_dom::NodeType;
use wavecore_style::StyledNode;

#[derive(Debug, Clone, Copy, Default)]
pub struct Rect { pub x: f32, pub y: f32, pub width: f32, pub height: f32 }

#[derive(Debug, Clone)]
pub struct LayoutBox {
    pub rect: Rect,
    pub text: Option<String>,
    pub children: Vec<LayoutBox>,
}

pub fn layout(root: &StyledNode, viewport_width: f32) -> LayoutBox {
    layout_at(root, 0.0, 0.0, viewport_width)
}

fn layout_at(node: &StyledNode, x: f32, y: f32, width: f32) -> LayoutBox {
    if let NodeType::Text(text) = &node.node.node_type {
        return LayoutBox {
            rect: Rect { x, y, width, height: 20.0 },
            text: Some(text.clone()),
            children: vec![],
        };
    }

    let mut cursor_y = y;
    let mut children = Vec::new();
    for child in &node.children {
        let child_box = layout_at(child, x, cursor_y, width);
        cursor_y += child_box.rect.height;
        children.push(child_box);
    }

    let own_height = (cursor_y - y).max(if children.is_empty() { 20.0 } else { 0.0 });
    LayoutBox { rect: Rect { x, y, width, height: own_height }, text: None, children }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wavecore_css::parse;
    use wavecore_dom::Node;
    use wavecore_style::style_tree;

    #[test]
    fn stacks_children_vertically() {
        let root = Node::element("body", vec![Node::text("one"), Node::text("two")]);
        let styled = style_tree(&root, &parse(""));
        let result = layout(&styled, 800.0);
        assert_eq!(result.rect.height, 40.0);
        assert_eq!(result.children[1].rect.y, 20.0);
    }
}
