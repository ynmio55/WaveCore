use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Debug,Clone,PartialEq)]
pub struct TextStyle{pub font_size:f32,pub line_height:f32,pub letter_spacing:f32}
impl Default for TextStyle{fn default()->Self{Self{font_size:16.0,line_height:1.25,letter_spacing:0.0}}}
#[derive(Debug,Clone,PartialEq)]pub struct TextLine{pub text:String,pub width:f32}
#[derive(Debug,Clone,PartialEq)]pub struct TextMetrics{pub lines:Vec<TextLine>,pub width:f32,pub height:f32,pub line_height:f32}

fn grapheme_width(g:&str,style:&TextStyle)->f32{
 let columns=UnicodeWidthStr::width(g).max(1) as f32;
 columns*style.font_size*0.55+style.letter_spacing
}
pub fn measure_and_wrap(text:&str,max_width:f32,style:&TextStyle)->TextMetrics{
 let line_height=(style.font_size*style.line_height).max(style.font_size);let mut lines=Vec::new();let mut current=String::new();let mut width=0.0;
 for g in UnicodeSegmentation::graphemes(text,true){
  if g=="\n"{lines.push(TextLine{text:std::mem::take(&mut current),width});width=0.0;continue}
  let gw=grapheme_width(g,style);
  if max_width>0.0&&width+gw>max_width&&!current.is_empty(){lines.push(TextLine{text:std::mem::take(&mut current),width});width=0.0;}
  current.push_str(g);width+=gw;
 }
 if !current.is_empty()||lines.is_empty(){lines.push(TextLine{text:current,width});}
 let widest=lines.iter().map(|l|l.width).fold(0.0,f32::max);
 TextMetrics{height:line_height*lines.len() as f32,width:widest,lines,line_height}
}
pub fn grapheme_count(text:&str)->usize{UnicodeSegmentation::graphemes(text,true).count()}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn keeps_thai_graphemes(){let s="สวัสดีครับ ภาษาไทย";let m=measure_and_wrap(s,90.0,&TextStyle::default());let joined=m.lines.iter().map(|l|l.text.as_str()).collect::<String>();assert_eq!(joined,s);}
 #[test]fn handles_emoji_clusters(){let s="A👨‍👩‍👧‍👦B";assert_eq!(grapheme_count(s),3);let m=measure_and_wrap(s,500.0,&TextStyle::default());assert_eq!(m.lines[0].text,s);}
 #[test]fn wraps_unicode(){let m=measure_and_wrap("Hello 世界 สวัสดี",45.0,&TextStyle::default());assert!(m.lines.len()>1);}
 #[test]fn explicit_newlines(){let m=measure_and_wrap("หนึ่ง\nสอง",500.0,&TextStyle::default());assert_eq!(m.lines.len(),2);}
}
