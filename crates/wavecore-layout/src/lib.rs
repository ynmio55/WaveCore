use wavecore_dom::NodeType;
use wavecore_style::StyledNode;

#[derive(Debug,Clone,Copy,Default,PartialEq)] pub struct Rect{pub x:f32,pub y:f32,pub width:f32,pub height:f32}
#[derive(Debug,Clone,Copy,Default,PartialEq)] pub struct Edges{pub top:f32,pub right:f32,pub bottom:f32,pub left:f32}
#[derive(Debug,Clone,Copy,PartialEq,Eq)] pub enum BoxSizing{ContentBox,BorderBox}
#[derive(Debug,Clone)] pub struct LayoutBox{
 pub rect:Rect,pub content:Rect,pub padding:Edges,pub border:Edges,pub margin:Edges,
 pub background:Option<String>,pub border_color:Option<String>,pub text:Option<String>,pub children:Vec<LayoutBox>,
}
pub fn layout(root:&StyledNode,viewport_width:f32)->LayoutBox{layout_at(root,0.0,0.0,viewport_width)}
fn length(v:Option<&String>,base:f32)->Option<f32>{let s=v?.trim();if s=="auto"{return None}if let Some(p)=s.strip_suffix('%'){return p.trim().parse::<f32>().ok().map(|n|base*n/100.0)}s.strip_suffix("px").unwrap_or(s).parse().ok()}
fn shorthand(value:Option<&String>,base:f32)->Edges{
 let v:Vec<f32>=value.map(|s|s.split_whitespace().filter_map(|x|length(Some(&x.to_string()),base)).collect()).unwrap_or_default();
 match v.as_slice(){[a]=>Edges{top:*a,right:*a,bottom:*a,left:*a},[v,h]=>Edges{top:*v,right:*h,bottom:*v,left:*h},[t,h,b]=>Edges{top:*t,right:*h,bottom:*b,left:*h},[t,r,b,l,..]=>Edges{top:*t,right:*r,bottom:*b,left:*l},_=>Edges::default()}
}
fn edges(n:&StyledNode,prefix:&str,base:f32)->Edges{
 let mut e=shorthand(n.properties.get(prefix),base);
 if let Some(v)=length(n.properties.get(&format!("{prefix}-top")),base){e.top=v} if let Some(v)=length(n.properties.get(&format!("{prefix}-right")),base){e.right=v}
 if let Some(v)=length(n.properties.get(&format!("{prefix}-bottom")),base){e.bottom=v} if let Some(v)=length(n.properties.get(&format!("{prefix}-left")),base){e.left=v} e
}
fn minmax(n:&StyledNode,name:&str,v:f32,base:f32)->f32{
 let min=length(n.properties.get(&format!("min-{name}")),base);let max=length(n.properties.get(&format!("max-{name}")),base);
 max.map_or(min.map_or(v,|m|v.max(m)),|m|min.map_or(v.min(m),|lo|v.max(lo).min(m)))
}
fn layout_at(node:&StyledNode,x:f32,y:f32,available:f32)->LayoutBox{
 if let NodeType::Text(text)=&node.node.node_type{return LayoutBox{rect:Rect{x,y,width:available,height:20.0},content:Rect{x,y,width:available,height:20.0},padding:Edges::default(),border:Edges::default(),margin:Edges::default(),background:None,border_color:None,text:Some(text.clone()),children:vec![]};}
 let margin=edges(node,"margin",available);let padding=edges(node,"padding",available);let mut border=edges(node,"border-width",available);
 if border==Edges::default(){if let Some(first)=node.properties.get("border").and_then(|v|v.split_whitespace().next()).map(str::to_string).and_then(|v|length(Some(&v),available)){border=Edges{top:first,right:first,bottom:first,left:first}}}
 let sizing=if node.properties.get("box-sizing").is_some_and(|v|v.trim()=="border-box"){BoxSizing::BorderBox}else{BoxSizing::ContentBox};
 let noncontent=padding.left+padding.right+border.left+border.right;let usable=(available-margin.left-margin.right).max(0.0);
 let specified=length(node.properties.get("width"),available);let mut content_width=match (specified,sizing){(Some(w),BoxSizing::BorderBox)=>(w-noncontent).max(0.0),(Some(w),_)=>w,(None,_)=> (usable-noncontent).max(0.0)};
 content_width=minmax(node,"width",content_width,available);
 let outer_x=x+margin.left;let outer_y=y+margin.top;let content_x=outer_x+border.left+padding.left;let content_y=outer_y+border.top+padding.top;
 let mut cursor=content_y;let mut children=Vec::new();for child in &node.children{let b=layout_at(child,content_x,cursor,content_width);cursor+=b.margin.top+b.rect.height+b.margin.bottom;children.push(b);}
 let natural=(cursor-content_y).max(if children.is_empty(){20.0}else{0.0});let vert=padding.top+padding.bottom+border.top+border.bottom;
 let specified_h=length(node.properties.get("height"),natural);let mut content_height=match(specified_h,sizing){(Some(h),BoxSizing::BorderBox)=>(h-vert).max(0.0),(Some(h),_)=>h,(None,_)=>natural};content_height=minmax(node,"height",content_height,natural.max(1.0));
 let rect=Rect{x:outer_x,y:outer_y,width:content_width+noncontent,height:content_height+vert};let content=Rect{x:content_x,y:content_y,width:content_width,height:content_height};
 LayoutBox{rect,content,padding,border,margin,background:node.properties.get("background-color").cloned().or_else(||node.properties.get("background").cloned()),border_color:node.properties.get("border-color").cloned().or_else(||node.properties.get("border").and_then(|v|v.split_whitespace().last().map(str::to_string))),text:None,children}
}
#[cfg(test)]mod tests{
 use super::*;use wavecore_css::parse;use wavecore_dom::Node;use wavecore_style::style_tree;
 fn box_for(css:&str)->LayoutBox{let root=Node::element("div",vec![Node::text("x")]);layout(&style_tree(&root,&parse(css)),800.0)}
 #[test]fn box_model_shorthand(){let b=box_for("div{width:100px;padding:10px 20px;margin:5px 6px 7px 8px;border-width:2px}");assert_eq!(b.rect.width,144.0);assert_eq!(b.rect.x,8.0);assert_eq!(b.padding.left,20.0);assert_eq!(b.margin.bottom,7.0)}
 #[test]fn border_box_keeps_declared_width(){let b=box_for("div{box-sizing:border-box;width:100px;padding:10px;border-width:2px}");assert_eq!(b.rect.width,100.0);assert_eq!(b.content.width,76.0)}
 #[test]fn percent_and_constraints(){let b=box_for("div{width:50%;min-width:450px;max-width:500px}");assert_eq!(b.content.width,450.0)}
}
