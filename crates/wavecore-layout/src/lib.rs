use wavecore_dom::NodeType;
use wavecore_style::StyledNode;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rect { pub x:f32,pub y:f32,pub width:f32,pub height:f32 }
#[derive(Debug,Clone,Copy,Default,PartialEq)]
pub struct Edges { pub top:f32,pub right:f32,pub bottom:f32,pub left:f32 }
#[derive(Debug,Clone)]
pub struct LayoutBox {
 pub rect:Rect, pub padding:Edges, pub border:Edges, pub margin:Edges,
 pub background:Option<String>, pub border_color:Option<String>,
 pub text:Option<String>, pub children:Vec<LayoutBox>,
}
pub fn layout(root:&StyledNode,viewport_width:f32)->LayoutBox{layout_at(root,0.0,0.0,viewport_width)}
fn px(v:Option<&String>)->Option<f32>{v?.trim().strip_suffix("px").unwrap_or(v?.trim()).parse().ok()}
fn edges(n:&StyledNode,prefix:&str)->Edges{
 let all=px(n.properties.get(prefix)).unwrap_or(0.0);
 Edges{
  top:px(n.properties.get(&format!("{prefix}-top"))).unwrap_or(all),
  right:px(n.properties.get(&format!("{prefix}-right"))).unwrap_or(all),
  bottom:px(n.properties.get(&format!("{prefix}-bottom"))).unwrap_or(all),
  left:px(n.properties.get(&format!("{prefix}-left"))).unwrap_or(all),
 }
}
fn layout_at(node:&StyledNode,x:f32,y:f32,available:f32)->LayoutBox{
 if let NodeType::Text(text)=&node.node.node_type{return LayoutBox{rect:Rect{x,y,width:available,height:20.0},padding:Edges::default(),border:Edges::default(),margin:Edges::default(),background:None,border_color:None,text:Some(text.clone()),children:vec![]};}
 let margin=edges(node,"margin"); let padding=edges(node,"padding"); let mut border=edges(node,"border-width");
 if border==Edges::default(){let w=px(node.properties.get("border")).unwrap_or(0.0); border=Edges{top:w,right:w,bottom:w,left:w};}
 let outer_x=x+margin.left; let outer_y=y+margin.top;
 let horizontal=margin.left+margin.right+padding.left+padding.right+border.left+border.right;
 let requested=px(node.properties.get("width"));
 let content_width=requested.unwrap_or((available-horizontal).max(0.0));
 let content_x=outer_x+border.left+padding.left; let content_y=outer_y+border.top+padding.top;
 let mut cursor=content_y; let mut children=Vec::new();
 for child in &node.children{let b=layout_at(child,content_x,cursor,content_width); cursor+=b.rect.height+b.margin.top+b.margin.bottom+b.padding.top+b.padding.bottom+b.border.top+b.border.bottom; children.push(b);}
 let content_height=px(node.properties.get("height")).unwrap_or((cursor-content_y).max(if children.is_empty(){20.0}else{0.0}));
 LayoutBox{rect:Rect{x:outer_x,y:outer_y,width:content_width+padding.left+padding.right+border.left+border.right,height:content_height+padding.top+padding.bottom+border.top+border.bottom},padding,border,margin,background:node.properties.get("background-color").cloned().or_else(||node.properties.get("background").cloned()),border_color:node.properties.get("border-color").cloned(),text:None,children}
}
#[cfg(test)]
mod tests{
 use super::*; use wavecore_css::parse; use wavecore_dom::Node; use wavecore_style::style_tree;
 #[test] fn applies_box_model(){let root=Node::element("div",vec![Node::text("x")]);let s=style_tree(&root,&parse("div{width:100px;padding:10px;margin:5px;border-width:2px;background-color:#fff}"));let b=layout(&s,800.0);assert_eq!(b.rect.width,124.0);assert_eq!(b.rect.x,5.0);assert_eq!(b.padding.left,10.0);assert_eq!(b.border.left,2.0);}
}
