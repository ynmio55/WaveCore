use wavecore_layout::{Edges,LayoutBox,Rect};
#[derive(Debug,Clone)]pub enum DisplayCommand{FillRect{rect:Rect,color:String},Border{rect:Rect,widths:Edges,color:String},Text{text:String,rect:Rect}}
pub fn build_display_list(layout:&LayoutBox)->Vec<DisplayCommand>{let mut v=vec![];walk(layout,&mut v);v}
fn walk(b:&LayoutBox,v:&mut Vec<DisplayCommand>){if let Some(bg)=&b.background{v.push(DisplayCommand::FillRect{rect:b.rect,color:bg.clone()});}if b.border!=Edges::default(){v.push(DisplayCommand::Border{rect:b.rect,widths:b.border,color:b.border_color.clone().unwrap_or_else(||"#000000".into())});}if let Some(t)=&b.text{if !t.trim().is_empty(){v.push(DisplayCommand::Text{text:t.clone(),rect:b.content});}}for c in &b.children{walk(c,v)}}
