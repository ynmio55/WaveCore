use fontdb::{Database,Family,Query};
use rustybuzz::{Face,UnicodeBuffer};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Debug,Clone,PartialEq)]pub struct TextStyle{pub font_size:f32,pub line_height:f32,pub letter_spacing:f32,pub families:Vec<String>,pub weight:u16,pub italic:bool}
impl Default for TextStyle{fn default()->Self{Self{font_size:16.0,line_height:1.25,letter_spacing:0.0,families:vec!["sans-serif".into()],weight:400,italic:false}}}
#[derive(Debug,Clone,PartialEq)]pub struct Glyph{pub id:u32,pub x_advance:f32,pub x_offset:f32,pub y_offset:f32,pub cluster:u32}
#[derive(Debug,Clone,PartialEq)]pub struct TextLine{pub text:String,pub width:f32}
#[derive(Debug,Clone,PartialEq)]pub struct TextMetrics{pub lines:Vec<TextLine>,pub width:f32,pub height:f32,pub line_height:f32}

pub struct FontSystem{db:Database}
impl FontSystem{
 pub fn new()->Self{let mut db=Database::new();db.load_system_fonts();Self{db}}
 pub fn has_fonts(&self)->bool{self.db.faces().next().is_some()}
 pub fn shape(&self,text:&str,style:&TextStyle)->Option<Vec<Glyph>>{
  let generic=Family::SansSerif;let named=style.families.first().map(|s|Family::Name(s.as_str()));let families=[named.unwrap_or(generic),generic];
  let id=self.db.query(&Query{families:&families,..Query::default()})?;
  self.db.with_face_data(id,|data,index|{
   let face=Face::from_slice(data,index)?;let upem=face.units_per_em() as f32;let scale=style.font_size/upem;
   let mut buffer=UnicodeBuffer::new();buffer.push_str(text);let out=rustybuzz::shape(&face,&[],buffer);
   Some(out.glyph_infos().iter().zip(out.glyph_positions()).map(|(i,p)|Glyph{id:i.glyph_id,x_advance:p.x_advance as f32*scale+style.letter_spacing,x_offset:p.x_offset as f32*scale,y_offset:p.y_offset as f32*scale,cluster:i.cluster}).collect())
  }).flatten()
 }
 pub fn font_bytes_for(&self,style:&TextStyle)->Option<(Vec<u8>,u32)>{
  let family=style.families.first().map(|s|Family::Name(s.as_str())).unwrap_or(Family::SansSerif);let id=self.db.query(&Query{families:&[family,Family::SansSerif],..Query::default()})?;
  self.db.with_face_data(id,|d,i|(d.to_vec(),i))
 }
}
impl Default for FontSystem{fn default()->Self{Self::new()}}

fn estimated_width(g:&str,style:&TextStyle)->f32{UnicodeWidthStr::width(g).max(1)as f32*style.font_size*0.55+style.letter_spacing}
pub fn measure_and_wrap(text:&str,max_width:f32,style:&TextStyle)->TextMetrics{
 let line_height=(style.font_size*style.line_height).max(style.font_size);let mut lines=vec![];let mut current=String::new();let mut width=0.0;
 for g in UnicodeSegmentation::graphemes(text,true){if g=="\n"{lines.push(TextLine{text:std::mem::take(&mut current),width});width=0.0;continue}let gw=estimated_width(g,style);if max_width>0.0&&width+gw>max_width&&!current.is_empty(){lines.push(TextLine{text:std::mem::take(&mut current),width});width=0.0}current.push_str(g);width+=gw}
 if !current.is_empty()||lines.is_empty(){lines.push(TextLine{text:current,width})}let widest=lines.iter().map(|l|l.width).fold(0.0,f32::max);TextMetrics{height:line_height*lines.len()as f32,width:widest,lines,line_height}
}
pub fn grapheme_count(text:&str)->usize{UnicodeSegmentation::graphemes(text,true).count()}
#[cfg(test)]mod tests{use super::*;#[test]fn thai_and_emoji_clusters(){assert!(grapheme_count("สวัสดี 👨‍👩‍👧‍👦")<"สวัสดี 👨‍👩‍👧‍👦".chars().count());}#[test]fn wraps(){assert!(measure_and_wrap("Hello 世界 สวัสดี",45.0,&TextStyle::default()).lines.len()>1)}#[test]fn system_font_shaping_when_available(){let fs=FontSystem::new();if fs.has_fonts(){let g=fs.shape("ภาษาไทย Hello",&TextStyle::default()).unwrap();assert!(!g.is_empty())}}}
