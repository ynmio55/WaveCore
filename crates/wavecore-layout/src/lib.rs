use wavecore_dom::NodeType;
use wavecore_style::StyledNode;
use wavecore_text::{measure_and_wrap, TextStyle};

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.width && py >= self.y && py <= self.y + self.height
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Edges {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxSizing {
    ContentBox,
    BorderBox,
}

#[derive(Debug, Clone)]
pub struct LayoutBox {
    pub rect: Rect,
    pub content: Rect,
    pub padding: Edges,
    pub border: Edges,
    pub margin: Edges,
    pub background: Option<String>,
    pub border_color: Option<String>,
    pub color: Option<String>,
    pub text: Option<String>,
    pub text_lines: Vec<String>,
    pub image_src: Option<String>,
    pub link_url: Option<String>,
    pub children: Vec<LayoutBox>,
}

impl LayoutBox {
    pub fn hit_test(&self, px: f32, py: f32) -> Option<&LayoutBox> {
        if !self.rect.contains(px, py) {
            return None;
        }
        for child in self.children.iter().rev() {
            if let Some(hit) = child.hit_test(px, py) {
                return Some(hit);
            }
        }
        Some(self)
    }

    pub fn find_link_at(&self, px: f32, py: f32) -> Option<String> {
        let b = self.hit_test(px, py)?;
        b.link_url.clone()
    }
}

pub fn layout(root: &StyledNode, viewport_width: f32) -> LayoutBox {
    layout_at(root, 0.0, 0.0, viewport_width, None, None, None)
}

fn length(v: Option<&String>, base: f32) -> Option<f32> {
    let s = v?.trim();
    if s == "auto" {
        return None;
    }
    if let Some(p) = s.strip_suffix('%') {
        return p.trim().parse::<f32>().ok().map(|n| base * n / 100.0);
    }
    s.strip_suffix("px").unwrap_or(s).parse().ok()
}

fn font_size(n: &StyledNode, parent: Option<f32>) -> f32 {
    if let Some(fs) = length(n.properties.get("font-size"), parent.unwrap_or(16.0)) {
        return fs;
    }
    if let NodeType::Element(e) = &n.node.node_type {
        match e.tag_name.to_ascii_lowercase().as_str() {
            "h1" => return 28.0,
            "h2" => return 22.0,
            "h3" => return 18.0,
            "h4" => return 16.0,
            "small" => return 12.0,
            _ => {}
        }
    }
    parent.unwrap_or(16.0)
}

fn line_height(n: &StyledNode) -> f32 {
    n.properties
        .get("line-height")
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(1.25)
}

fn shorthand(value: Option<&String>, base: f32) -> Edges {
    let v: Vec<f32> = value
        .map(|s| {
            s.split_whitespace()
                .filter_map(|x| length(Some(&x.to_string()), base))
                .collect()
        })
        .unwrap_or_default();
    match v.as_slice() {
        [a] => Edges { top: *a, right: *a, bottom: *a, left: *a },
        [v, h] => Edges { top: *v, right: *h, bottom: *v, left: *h },
        [t, h, b] => Edges { top: *t, right: *h, bottom: *b, left: *h },
        [t, r, b, l, ..] => Edges { top: *t, right: *r, bottom: *b, left: *l },
        _ => Edges::default(),
    }
}

fn edges(n: &StyledNode, prefix: &str, base: f32) -> Edges {
    let mut e = shorthand(n.properties.get(prefix), base);
    if let Some(v) = length(n.properties.get(&format!("{prefix}-top")), base) {
        e.top = v;
    }
    if let Some(v) = length(n.properties.get(&format!("{prefix}-right")), base) {
        e.right = v;
    }
    if let Some(v) = length(n.properties.get(&format!("{prefix}-bottom")), base) {
        e.bottom = v;
    }
    if let Some(v) = length(n.properties.get(&format!("{prefix}-left")), base) {
        e.left = v;
    }
    e
}

fn minmax(n: &StyledNode, name: &str, v: f32, base: f32) -> f32 {
    let min = length(n.properties.get(&format!("min-{name}")), base);
    let max = length(n.properties.get(&format!("max-{name}")), base);
    max.map_or(min.map_or(v, |m| v.max(m)), |m| {
        min.map_or(v.min(m), |lo| v.max(lo).min(m))
    })
}

fn is_hidden(node: &StyledNode) -> bool {
    if node.properties.get("display").map(|s| s.trim()) == Some("none") {
        return true;
    }
    if let NodeType::Element(e) = &node.node.node_type {
        let tag = e.tag_name.to_ascii_lowercase();
        if matches!(tag.as_str(), "head" | "style" | "script" | "title" | "meta" | "link") {
            return true;
        }
    }
    false
}

fn layout_at(
    node: &StyledNode,
    x: f32,
    y: f32,
    available: f32,
    inherited_font: Option<f32>,
    inherited_color: Option<String>,
    inherited_link: Option<String>,
) -> LayoutBox {
    if is_hidden(node) {
        return LayoutBox {
            rect: Rect::default(),
            content: Rect::default(),
            padding: Edges::default(),
            border: Edges::default(),
            margin: Edges::default(),
            background: None,
            border_color: None,
            color: None,
            text: None,
            text_lines: vec![],
            image_src: None,
            link_url: None,
            children: vec![],
        };
    }

    let fs = font_size(node, inherited_font);
    let current_color = node.properties.get("color").cloned().or(inherited_color);

    let (link_url, image_src, is_img) = match &node.node.node_type {
        NodeType::Element(e) => {
            let l = e.attributes.get("href").cloned().or(inherited_link.clone());
            let img = e.tag_name.eq_ignore_ascii_case("img");
            let src = if img { e.attributes.get("src").cloned() } else { None };
            (l, src, img)
        }
        _ => (inherited_link.clone(), None, false),
    };

    if let NodeType::Text(text) = &node.node.node_type {
        let metrics = measure_and_wrap(
            text,
            available,
            &TextStyle {
                font_size: fs,
                line_height: line_height(node),
                ..TextStyle::default()
            },
        );
        let h = metrics.height;
        return LayoutBox {
            rect: Rect { x, y, width: available, height: h },
            content: Rect { x, y, width: available, height: h },
            padding: Edges::default(),
            border: Edges::default(),
            margin: Edges::default(),
            background: None,
            border_color: None,
            color: current_color,
            text: Some(text.clone()),
            text_lines: metrics.lines.into_iter().map(|l| l.text).collect(),
            image_src: None,
            link_url,
            children: vec![],
        };
    }

    let margin = edges(node, "margin", available);
    let padding = edges(node, "padding", available);
    let mut border = edges(node, "border-width", available);
    if border == Edges::default() {
        if let Some(first) = node
            .properties
            .get("border")
            .and_then(|v| v.split_whitespace().next())
            .map(str::to_string)
            .and_then(|v| length(Some(&v), available))
        {
            border = Edges {
                top: first,
                right: first,
                bottom: first,
                left: first,
            };
        }
    }

    let sizing = if node
        .properties
        .get("box-sizing")
        .is_some_and(|v| v.trim() == "border-box")
    {
        BoxSizing::BorderBox
    } else {
        BoxSizing::ContentBox
    };

    let noncontent = padding.left + padding.right + border.left + border.right;
    let usable = (available - margin.left - margin.right).max(0.0);

    // Check width from CSS or HTML attribute
    let attr_w = match &node.node.node_type {
        NodeType::Element(e) => e.attributes.get("width").and_then(|v| length(Some(v), available)),
        _ => None,
    };
    let specified = length(node.properties.get("width"), available).or(attr_w);

    let mut cw = match (specified, sizing) {
        (Some(w), BoxSizing::BorderBox) => (w - noncontent).max(0.0),
        (Some(w), _) => w,
        (None, _) => {
            if is_img {
                200.0 // Default image fallback width
            } else {
                (usable - noncontent).max(0.0)
            }
        }
    };
    cw = minmax(node, "width", cw, available);

    let ox = x + margin.left;
    let oy = y + margin.top;
    let cx = ox + border.left + padding.left;
    let cy = oy + border.top + padding.top;
    let mut cursor = cy;
    let mut children = Vec::new();

    for child in &node.children {
        if is_hidden(child) {
            continue;
        }
        let b = layout_at(child, cx, cursor, cw, Some(fs), current_color.clone(), link_url.clone());
        if b.rect.width > 0.0 || b.rect.height > 0.0 || !b.children.is_empty() || b.image_src.is_some() || b.background.is_some() {
            cursor += b.margin.top + b.rect.height + b.margin.bottom;
            children.push(b);
        }
    }

    let natural = if is_img {
        // Default image fallback height
        match &node.node.node_type {
            NodeType::Element(e) => e.attributes.get("height").and_then(|v| length(Some(v), available)).unwrap_or(150.0),
            _ => 150.0,
        }
    } else {
        (cursor - cy).max(0.0)
    };

    let vert = padding.top + padding.bottom + border.top + border.bottom;
    let attr_h = match &node.node.node_type {
        NodeType::Element(e) => e.attributes.get("height").and_then(|v| length(Some(v), available)),
        _ => None,
    };
    let specified_h = length(node.properties.get("height"), natural).or(attr_h);
    let mut ch = match (specified_h, sizing) {
        (Some(h), BoxSizing::BorderBox) => (h - vert).max(0.0),
        (Some(h), _) => h,
        (None, _) => natural,
    };
    ch = minmax(node, "height", ch, natural.max(1.0));

    let rect = Rect {
        x: ox,
        y: oy,
        width: cw + noncontent,
        height: ch + vert,
    };
    let content = Rect {
        x: cx,
        y: cy,
        width: cw,
        height: ch,
    };

    LayoutBox {
        rect,
        content,
        padding,
        border,
        margin,
        background: node
            .properties
            .get("background-color")
            .cloned()
            .or_else(|| node.properties.get("background").cloned()),
        border_color: node
            .properties
            .get("border-color")
            .cloned()
            .or_else(|| {
                node.properties
                    .get("border")
                    .and_then(|v| v.split_whitespace().last().map(str::to_string))
            }),
        color: node.properties.get("color").cloned(),
        text: None,
        text_lines: vec![],
        image_src,
        link_url,
        children,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wavecore_css::parse;
    use wavecore_dom::Node;
    use wavecore_style::style_tree;

    fn box_for(css: &str) -> LayoutBox {
        let root = Node::element("div", vec![Node::text("สวัสดีครับ ภาษาไทย")]);
        layout(&style_tree(&root, &parse(css)), 800.0)
    }

    #[test]
    fn box_model_shorthand() {
        let b = box_for("div{width:100px;padding:10px 20px;margin:5px 6px 7px 8px;border-width:2px}");
        assert_eq!(b.rect.width, 144.0);
    }

    #[test]
    fn thai_text_wraps() {
        let b = box_for("div{width:60px;font-size:16px}");
        assert!(b.children[0].text_lines.len() > 1);
        assert!(b.children[0].rect.height > 20.0);
    }

    #[test]
    fn hit_test_and_link_detection() {
        let mut d = std::collections::BTreeMap::new();
        d.insert("href".to_string(), "https://example.com".to_string());
        let link_elem = Node::element_with_attributes("a", d, vec![Node::text("Click Here")]);
        let root = Node::element("div", vec![link_elem]);
        let l = layout(&style_tree(&root, &parse("a { display: block; width: 200px; }")), 800.0);
        assert_eq!(l.find_link_at(10.0, 10.0), Some("https://example.com".to_string()));
        assert_eq!(l.find_link_at(500.0, 500.0), None);
    }
}
