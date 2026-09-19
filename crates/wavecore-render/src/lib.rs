use wavecore_layout::{Edges, LayoutBox, Rect};

#[derive(Debug, Clone, PartialEq)]
pub enum DisplayCommand {
    FillRect { rect: Rect, color: String },
    Border { rect: Rect, widths: Edges, color: String },
    Text { text: String, rect: Rect, font_size: f32, line_height: f32, color: String },
}

pub fn build_display_list(layout: &LayoutBox) -> Vec<DisplayCommand> {
    let mut v = Vec::new();
    walk(layout, &mut v);
    v
}

fn walk(b: &LayoutBox, v: &mut Vec<DisplayCommand>) {
    if let Some(bg) = &b.background {
        v.push(DisplayCommand::FillRect { rect: b.rect, color: bg.clone() });
    }
    if b.border != Edges::default() {
        v.push(DisplayCommand::Border {
            rect: b.rect,
            widths: b.border,
            color: b.border_color.clone().unwrap_or_else(|| "#000000".into()),
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
    for c in &b.children {
        walk(c, v);
    }
}
