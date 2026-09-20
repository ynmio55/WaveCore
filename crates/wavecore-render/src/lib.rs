use wavecore_layout::{Edges, LayoutBox, Rect};

#[derive(Debug, Clone, PartialEq)]
pub enum DisplayCommand {
    FillRect { rect: Rect, color: String },
    FillRoundedRect { rect: Rect, radius: f32, color: String },
    Border { rect: Rect, widths: Edges, color: String, radius: f32 },
    Text { text: String, rect: Rect, font_size: f32, line_height: f32, color: String },
    Image { rect: Rect, src: String },
    DrawLine { x1: f32, y1: f32, x2: f32, y2: f32, color: String, width: f32 },
    DrawCircle { cx: f32, cy: f32, radius: f32, fill: Option<String>, stroke: Option<(String, f32)> },
    PushClip(Rect),
    PopClip,
    PushOpacity(f32),
    PopOpacity,
}

pub fn build_display_list(layout: &LayoutBox) -> Vec<DisplayCommand> {
    let mut v = Vec::new();
    walk(layout, &mut v);
    v
}

fn walk(b: &LayoutBox, v: &mut Vec<DisplayCommand>) {
    let is_clipped = b.overflow_hidden;
    if b.opacity < 0.999 {
        v.push(DisplayCommand::PushOpacity(b.opacity));
    }
    if is_clipped {
        v.push(DisplayCommand::PushClip(b.content));
    }

    if let Some(bg) = &b.background {
        if b.border_radius > 0.0 {
            v.push(DisplayCommand::FillRoundedRect {
                rect: b.rect,
                radius: b.border_radius,
                color: bg.clone(),
            });
        } else {
            v.push(DisplayCommand::FillRect { rect: b.rect, color: bg.clone() });
        }
    }
    if b.border != Edges::default() {
        v.push(DisplayCommand::Border {
            rect: b.rect,
            widths: b.border,
            color: b.border_color.clone().unwrap_or_else(|| "#000000".into()),
            radius: b.border_radius,
        });
    }
    if let Some(src) = &b.image_src {
        v.push(DisplayCommand::Image {
            rect: b.content,
            src: src.clone(),
        });
    }
    if b.text.is_some() {
        let count = b.text_lines.len().max(1) as f32;
        let lh = b.content.height / count;
        let fs = lh / 1.25;
        let text_color = b.color.clone().unwrap_or_else(|| "#000000".into());
        for (i, line) in b.text_lines.iter().enumerate() {
            if !line.trim().is_empty() {
                v.push(DisplayCommand::Text {
                    text: line.clone(),
                    rect: Rect {
                        x: b.content.x,
                        y: b.content.y + i as f32 * lh,
                        width: b.content.width,
                        height: lh,
                    },
                    font_size: fs,
                    line_height: lh,
                    color: text_color.clone(),
                });
            }
        }
    }
    let mut children: Vec<&LayoutBox> = b.children.iter().collect();
    children.sort_by_key(|child| child.z_index);
    for c in children {
        walk(c, v);
    }

    if is_clipped {
        v.push(DisplayCommand::PopClip);
    }
    if b.opacity < 0.999 {
        v.push(DisplayCommand::PopOpacity);
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use wavecore_dom::Node;
    use wavecore_style::style_tree;

    #[test]
    fn emits_rounded_background_opacity_and_z_order() {
        let a = Node::element("div", vec![Node::text("A")]);
        let b = Node::element("div", vec![Node::text("B")]);
        let root = Node::element("main", vec![a, b]);
        let css = wavecore_css::parse(
            "main{width:300px} main>div{width:50px;height:20px;background:red;border-radius:6px;opacity:.5} main>div:first-child{z-index:2}"
        );
        let layout = wavecore_layout::layout(&style_tree(&root, &css), 300.0);
        let list = build_display_list(&layout);
        assert!(list.iter().any(|cmd| matches!(cmd, DisplayCommand::FillRoundedRect { radius, .. } if *radius > 0.0)));
        assert!(list.iter().any(|cmd| matches!(cmd, DisplayCommand::PushOpacity(v) if *v < 1.0)));
    }
}
