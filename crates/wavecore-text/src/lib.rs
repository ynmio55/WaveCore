use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Debug,Clone,PartialEq)]
pub struct TextStyle { pub font_size:f32, pub line_height:f32 }
impl Default for TextStyle { fn default()->Self{Self{font_size:16.0,line_height:1.25}} }

#[derive(Debug,Clone,PartialEq)]
pub struct TextLine { pub text:String, pub width:f32 }

#[derive(Debug,Clone,PartialEq)]
pub struct TextMetrics { pub lines:Vec<TextLine>, pub width:f32, pub height:f32, pub line_height:f32 }

pub fn measure_and_wrap(text:&str,max_width:f32,style:&TextStyle)->TextMetrics{
 let advance=(style.font_size*0.55).max(1.0); let line_height=(style.font_size*style.line_height).max(style.font_size);
 let mut lines=Vec::new(); let mut current=String::new(); let mut width=0.0;
 for g in UnicodeSegmentation::graphemes(text,true){
  if g=="\n"{lines.push(TextLine{text:std::mem::take(&mut current),width});width=0.0;continue}
  let gw=(UnicodeWidthStr::width(g) as f32*advance).max(if g.chars().all(|c|c.is_whitespace()){advance}else{0.0});
  if max_width>0.0 && width+gw>max_width && !current.is_empty(){lines.push(TextLine{text:std::mem::take(&mut current),width});width=0.0;}
  current.push_str(g);width+=gw;
 }
 if !current.is_empty()||lines.is_empty(){lines.push(TextLine{text:current,width});}
 let widest=lines.iter().map(|l|l.width).fold(0.0,f32::max);
 TextMetrics{height:line_height*lines.len() as f32,width:widest.min(max_width.max(widest)),lines,line_height}
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn keeps_thai_graphemes(){let m=measure_and_wrap("สวัสดีครับ ภาษาไทย",90.0,&TextStyle::default());assert!(!m.lines.is_empty());assert_eq!(m.lines.concat(),"สวัสดีครับ ภาษาไทย");}
 #[test]fn handles_emoji_clusters(){let s="A👨‍👩‍👧‍👦B";let m=measure_and_wrap(s,500.0,&TextStyle::default());assert_eq!(m.lines[0].text,s);}
 #[test]fn wraps_unicode(){let m=measure_and_wrap("Hello 世界 สวัสดี",45.0,&TextStyle::default());assert!(m.lines.len()>1);}
}
