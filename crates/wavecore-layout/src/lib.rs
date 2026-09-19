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

    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        let x1 = self.x.max(other.x);
        let y1 = self.y.max(other.y);
        let x2 = (self.x + self.width).min(other.x + other.width);
        let y2 = (self.y + self.height).min(other.y + other.height);

        if x2 > x1 && y2 > y1 {
            Some(Rect {
                x: x1,
                y: y1,
                width: x2 - x1,
                height: y2 - y1,
            })
        } else {
            None
        }
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
    pub is_form_control: bool,
    pub form_id: Option<String>,
    pub form_control_type: Option<String>,
    pub placeholder: Option<String>,
    pub overflow_hidden: bool,
    pub is_media: bool,
    pub is_canvas: bool,
    pub is_svg: bool,
    pub is_dirty: bool,
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

    pub fn find_form_control_at(&self, px: f32, py: f32) -> Option<&LayoutBox> {
        let b = self.hit_test(px, py)?;
        if b.is_form_control {
            Some(b)
        } else {
            None
        }
    }
}

pub fn layout(root: &StyledNode, viewport_width: f32) -> LayoutBox {
    layout_at(root, 0.0, 0.0, viewport_width, None, None, None)
}

#[derive(Default, Clone)]
pub struct LayoutCache {
    pub cached_root: Option<LayoutBox>,
    pub last_viewport_width: Option<f32>,
}

impl LayoutCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn layout(&mut self, root: &StyledNode, viewport_width: f32, is_dirty: bool) -> LayoutBox {
        if !is_dirty && self.last_viewport_width == Some(viewport_width) {
            if let Some(cached) = &self.cached_root {
                return cached.clone();
            }
        }

        let computed = layout(root, viewport_width);
        self.cached_root = Some(computed.clone());
        self.last_viewport_width = Some(viewport_width);
        computed
    }
}

pub struct LayoutWorkerPool;

impl LayoutWorkerPool {
    pub fn compute_async<F, T>(task: F) -> std::sync::mpsc::Receiver<T>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let res = task();
            let _ = tx.send(res);
        });
        rx
    }
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
            is_form_control: false,
            form_id: None,
            form_control_type: None,
            placeholder: None,
            overflow_hidden: false,
            is_media: false,
            is_canvas: false,
            is_svg: false,
            is_dirty: false,
            children: vec![],
        };
    }

    let fs = font_size(node, inherited_font);
    let current_color = node.properties.get("color").cloned().or(inherited_color);

    let (link_url, image_src, is_img, is_form_control, form_id, form_control_type, placeholder, form_val, is_media, is_canvas, is_svg) = match &node.node.node_type {
        NodeType::Element(e) => {
            let l = e.attributes.get("href").cloned().or(inherited_link.clone());
            let img = e.tag_name.eq_ignore_ascii_case("img");
            let src = if img { e.attributes.get("src").cloned() } else { None };
            let is_form = e.is_form_control();
            let fid = e.id().map(String::from);
            let ftype = Some(e.tag_name.clone());
            let ph = e.placeholder().map(String::from);
            let val = e.value().map(String::from);
            let media = e.tag_name.eq_ignore_ascii_case("video") || e.tag_name.eq_ignore_ascii_case("audio");
            let canvas = e.tag_name.eq_ignore_ascii_case("canvas");
            let svg = e.tag_name.eq_ignore_ascii_case("svg");
            (l, src, img, is_form, fid, ftype, ph, val, media, canvas, svg)
        }
        _ => (inherited_link.clone(), None, false, false, None, None, None, None, false, false, false),
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
            is_form_control: false,
            form_id: None,
            form_control_type: None,
            placeholder: None,
            overflow_hidden: false,
            is_media: false,
            is_canvas: false,
            is_svg: false,
            is_dirty: false,
            children: vec![],
        };
    }

    let margin = edges(node, "margin", available);
    let mut padding = edges(node, "padding", available);
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

    // Default form control styling if not set by CSS
    if is_form_control {
        if border == Edges::default() {
            border = Edges { top: 1.0, right: 1.0, bottom: 1.0, left: 1.0 };
        }
        if padding == Edges::default() {
            padding = Edges { top: 6.0, right: 12.0, bottom: 6.0, left: 12.0 };
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

    let display_mode = node.properties.get("display").map(|s| s.trim()).unwrap_or("block");
    let is_flex = display_mode == "flex";
    let is_grid = display_mode == "grid";

    // Width calculation
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
                200.0
            } else if is_media {
                300.0
            } else if is_canvas || is_svg {
                300.0
            } else if is_form_control && form_control_type.as_deref() == Some("input") {
                220.0
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

    let gap = length(node.properties.get("gap").or_else(|| node.properties.get("grid-gap")), cw).unwrap_or(0.0);

    let mut children = Vec::new();
    let natural_h: f32;

    if is_flex {
        let flex_dir = node.properties.get("flex-direction").map(|s| s.trim()).unwrap_or("row");
        if flex_dir == "row" {
            let flex_wrap = node
                .properties
                .get("flex-wrap")
                .map(|s| s.trim())
                .unwrap_or("nowrap")
                == "wrap";
            let mut cursor_x = cx;
            let mut cursor_y = cy;
            let mut row_h: f32 = 0.0;
            let child_nodes: Vec<&StyledNode> =
                node.children.iter().filter(|c| !is_hidden(c)).collect();
            let count = child_nodes.len().max(1) as f32;
            let total_gap = (count - 1.0) * gap;
            let default_item_w = ((cw - total_gap) / count).max(40.0);

            for child in child_nodes {
                let grow = child
                    .properties
                    .get("flex-grow")
                    .and_then(|v| v.trim().parse::<f32>().ok())
                    .unwrap_or(0.0);
                let base_w = length(child.properties.get("width"), cw).unwrap_or(default_item_w);
                let item_w = if grow > 0.0 {
                    (base_w + grow * 40.0).min(cw)
                } else {
                    base_w
                };

                if flex_wrap && (cursor_x + item_w - cx > cw) && cursor_x > cx {
                    cursor_x = cx;
                    cursor_y += row_h + gap;
                    row_h = 0.0;
                }

                let b = layout_at(
                    child,
                    cursor_x,
                    cursor_y,
                    item_w,
                    Some(fs),
                    current_color.clone(),
                    link_url.clone(),
                );
                row_h = row_h.max(b.rect.height + b.margin.top + b.margin.bottom);
                cursor_x += b.rect.width + b.margin.left + b.margin.right + gap;
                children.push(b);
            }
            natural_h = (cursor_y + row_h - cy).max(0.0);
        } else {
            // flex-direction: column
            let mut cursor_y = cy;
            for child in &node.children {
                if is_hidden(child) { continue; }
                let b = layout_at(child, cx, cursor_y, cw, Some(fs), current_color.clone(), link_url.clone());
                cursor_y += b.margin.top + b.rect.height + b.margin.bottom + gap;
                children.push(b);
            }
            natural_h = (cursor_y - cy).max(0.0);
        }
    } else if is_grid {
        let cols = parse_grid_cols(node.properties.get("grid-template-columns"));
        let col_count = cols.max(1);
        let total_gap = (col_count - 1) as f32 * gap;
        let col_w = ((cw - total_gap) / col_count as f32).max(40.0);

        let active_children: Vec<&StyledNode> = node.children.iter().filter(|c| !is_hidden(c)).collect();
        let mut row_cursor = cy;
        let mut max_row_h: f32 = 0.0;

        for (idx, child) in active_children.into_iter().enumerate() {
            let col_idx = idx % col_count;
            if col_idx == 0 && idx > 0 {
                row_cursor += max_row_h + gap;
                max_row_h = 0.0;
            }
            let col_x = cx + col_idx as f32 * (col_w + gap);
            let b = layout_at(child, col_x, row_cursor, col_w, Some(fs), current_color.clone(), link_url.clone());
            max_row_h = max_row_h.max(b.rect.height);
            children.push(b);
        }
        natural_h = (row_cursor + max_row_h - cy).max(0.0);
    } else {
        // Normal block layout
        let mut cursor = cy;
        for child in &node.children {
            if is_hidden(child) {
                continue;
            }
            let b = layout_at(child, cx, cursor, cw, Some(fs), current_color.clone(), link_url.clone());
            if b.rect.width > 0.0 || b.rect.height > 0.0 || !b.children.is_empty() || b.image_src.is_some() || b.background.is_some() || b.is_form_control {
                cursor += b.margin.top + b.rect.height + b.margin.bottom;
                children.push(b);
            }
        }
        natural_h = (cursor - cy).max(0.0);
    }

    let natural = if is_img {
        match &node.node.node_type {
            NodeType::Element(e) => e.attributes.get("height").and_then(|v| length(Some(v), available)).unwrap_or(150.0),
            _ => 150.0,
        }
    } else if is_media {
        match &node.node.node_type {
            NodeType::Element(e) => {
                if e.tag_name.eq_ignore_ascii_case("audio") {
                    48.0
                } else {
                    e.attributes.get("height").and_then(|v| length(Some(v), available)).unwrap_or(180.0)
                }
            }
            _ => 180.0,
        }
    } else if is_canvas || is_svg {
        match &node.node.node_type {
            NodeType::Element(e) => e.attributes.get("height").and_then(|v| length(Some(v), available)).unwrap_or(150.0),
            _ => 150.0,
        }
    } else if is_form_control {
        if form_control_type.as_deref() == Some("textarea") {
            80.0
        } else {
            36.0
        }
    } else {
        natural_h
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

    // Handle relative positioning offset
    let mut final_x = ox;
    let mut final_y = oy;
    let mut final_cx = cx;
    let mut final_cy = cy;
    let position = node.properties.get("position").map(|s| s.trim()).unwrap_or("static");
    if position == "fixed" {
        let top_off = length(node.properties.get("top"), available).unwrap_or(0.0);
        let left_off = length(node.properties.get("left"), available).unwrap_or(0.0);
        final_x = left_off;
        final_cx = left_off + padding.left + border.left;
        final_y = top_off;
        final_cy = top_off + padding.top + border.top;
    } else if position == "relative" {
        let top_off = length(node.properties.get("top"), available).unwrap_or(0.0);
        let left_off = length(node.properties.get("left"), available).unwrap_or(0.0);
        final_x += left_off;
        final_cx += left_off;
        final_y += top_off;
        final_cy += top_off;
    }

    let overflow_hidden = node
        .properties
        .get("overflow")
        .map(|s| s.trim())
        == Some("hidden");

    let rect = Rect {
        x: final_x,
        y: final_y,
        width: cw + noncontent,
        height: ch + vert,
    };
    let content = Rect {
        x: final_cx,
        y: final_cy,
        width: cw,
        height: ch,
    };

    // Default form control text & colors
    let (form_text, form_lines) = if is_form_control {
        let display_val = form_val.or(placeholder.clone()).unwrap_or_default();
        if !display_val.is_empty() {
            (Some(display_val.clone()), vec![display_val])
        } else {
            (None, vec![])
        }
    } else {
        (None, vec![])
    };

    let bg_color = node
        .properties
        .get("background-color")
        .cloned()
        .or_else(|| node.properties.get("background").cloned())
        .or_else(|| {
            if is_form_control {
                if form_control_type.as_deref() == Some("button") {
                    Some("#21262d".to_string())
                } else {
                    Some("#161b22".to_string())
                }
            } else {
                None
            }
        });

    let b_color = node
        .properties
        .get("border-color")
        .cloned()
        .or_else(|| {
            node.properties
                .get("border")
                .and_then(|v| v.split_whitespace().last().map(str::to_string))
        })
        .or_else(|| {
            if is_form_control {
                Some("#30363d".to_string())
            } else {
                None
            }
        });

    LayoutBox {
        rect,
        content,
        padding,
        border,
        margin,
        background: bg_color,
        border_color: b_color,
        color: node.properties.get("color").cloned().or_else(|| {
            if is_form_control {
                Some("#e6edf3".to_string())
            } else {
                None
            }
        }),
        text: form_text,
        text_lines: form_lines,
        image_src,
        link_url,
        is_form_control,
        form_id,
        form_control_type,
        placeholder,
        overflow_hidden,
        is_media,
        is_canvas,
        is_svg,
        is_dirty: false,
        children,
    }
}

fn parse_grid_cols(prop: Option<&String>) -> usize {
    let Some(s) = prop else { return 2; };
    let s = s.trim();
    if let Some(inner) = s.strip_prefix("repeat(").and_then(|r| r.strip_suffix(')')) {
        if let Some((n, _)) = inner.split_once(',') {
            return n.trim().parse::<usize>().unwrap_or(2);
        }
    }
    s.split_whitespace().count().max(1)
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

    #[test]
    fn flexbox_row_layout() {
        let item1 = Node::element("div", vec![Node::text("Item 1")]);
        let item2 = Node::element("div", vec![Node::text("Item 2")]);
        let root = Node::element("div", vec![item1, item2]);
        let css = "div { display: flex; flex-direction: row; gap: 10px; width: 400px; }";
        let l = layout(&style_tree(&root, &parse(css)), 800.0);
        assert_eq!(l.children.len(), 2);
        assert!(l.children[1].rect.x > l.children[0].rect.x);
    }

    #[test]
    fn form_control_layout() {
        let mut d = std::collections::BTreeMap::new();
        d.insert("type".to_string(), "text".to_string());
        d.insert("placeholder".to_string(), "Enter name...".to_string());
        let input = Node::element_with_attributes("input", d, vec![]);
        let root = Node::element("div", vec![input]);
        let l = layout(&style_tree(&root, &parse("div { width: 400px; }")), 800.0);
        assert!(l.children[0].is_form_control);
        assert_eq!(l.children[0].form_control_type.as_deref(), Some("input"));
    }

    #[test]
    fn flex_wrap_and_overflow_hidden() {
        let item1 = Node::element("div", vec![Node::text("Card 1")]);
        let item2 = Node::element("div", vec![Node::text("Card 2")]);
        let root = Node::element("div", vec![item1, item2]);
        let css = "div { display: flex; flex-direction: row; flex-wrap: wrap; width: 200px; overflow: hidden; } div div { width: 150px; height: 50px; }";
        let l = layout(&style_tree(&root, &parse(css)), 800.0);
        assert!(l.overflow_hidden);
        assert_eq!(l.children.len(), 2);
        // Because item1 is 150px and container is 200px, item2 (150px) cannot fit on same line with flex-wrap: wrap
        assert!(l.children[1].rect.y > l.children[0].rect.y);
    }

    #[test]
    fn fixed_positioning_layout() {
        let fixed_node = Node::element("div", vec![Node::text("Fixed Header")]);
        let root = Node::element("div", vec![fixed_node]);
        let css = "div div { position: fixed; top: 15px; left: 25px; width: 300px; height: 40px; }";
        let l = layout(&style_tree(&root, &parse(css)), 800.0);
        assert_eq!(l.children[0].rect.x, 25.0);
        assert_eq!(l.children[0].rect.y, 15.0);
    }

    #[test]
    fn incremental_layout_cache_and_worker_pool() {
        let node = Node::element("div", vec![Node::text("Cached content")]);
        let styled = style_tree(&node, &parse("div { width: 350px; height: 120px; }"));

        let mut cache = LayoutCache::new();
        let l1 = cache.layout(&styled, 800.0, true);
        assert_eq!(l1.content.width, 350.0);

        // When not dirty and same viewport, returns cached without recalculating
        let l2 = cache.layout(&styled, 800.0, false);
        assert_eq!(l2.content.width, 350.0);

        // Async worker computation
        let rx = LayoutWorkerPool::compute_async(move || {
            let n = Node::element("div", vec![Node::text("Worker layout")]);
            layout(&style_tree(&n, &parse("div { width: 500px; }")), 1000.0)
        });
        let res = rx.recv().expect("Worker thread failed");
        assert_eq!(res.content.width, 500.0);
    }

    #[test]
    fn box_sizing_and_media_element_layout() {
        let video_node = Node::element("video", vec![]);
        let root = Node::element("div", vec![video_node]);
        let css = "video { width: 400px; height: 200px; padding: 10px; border: 5px solid black; box-sizing: border-box; }";
        let l = layout(&style_tree(&root, &parse(css)), 800.0);
        let vid = &l.children[0];
        assert!(vid.is_media);
        // In border-box: content.width = 400 - (10*2 + 5*2) = 370.0
        assert_eq!(vid.content.width, 370.0);
        // content.height = 200 - (10*2 + 5*2) = 170.0
        assert_eq!(vid.content.height, 170.0);
        // Total box width = 400.0
        assert_eq!(vid.rect.width, 400.0);
    }
}
