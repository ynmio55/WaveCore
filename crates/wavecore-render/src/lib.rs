use wavecore_layout::{LayoutBox, Rect};

#[derive(Debug, Clone)]
pub enum DisplayCommand {
    Text { text: String, rect: Rect },
}

pub fn build_display_list(layout: &LayoutBox) -> Vec<DisplayCommand> {
    let mut list = Vec::new();
    walk(layout, &mut list);
    list
}

fn walk(layout: &LayoutBox, list: &mut Vec<DisplayCommand>) {
    if let Some(text) = &layout.text {
        if !text.trim().is_empty() {
            list.push(DisplayCommand::Text { text: text.clone(), rect: layout.rect });
        }
    }
    for child in &layout.children { walk(child, list); }
}
