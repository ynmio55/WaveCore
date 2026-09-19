use wavecore_render::DisplayCommand;
#[derive(Debug,Clone,Copy,PartialEq,Eq)] pub struct Rgba(pub u8,pub u8,pub u8,pub u8);
pub struct Surface{pub width:u32,pub height:u32,pub pixels:Vec<Rgba>}
impl Surface{
 pub fn new(width:u32,height:u32)->Self{Self{width,height,pixels:vec![Rgba(255,255,255,255);(width*height)as usize]}}
 pub fn paint(&mut self,list:&[DisplayCommand]){for c in list{match c{
  DisplayCommand::FillRect{rect,color}=>{let c=parse_color(color).unwrap_or(Rgba(240,240,240,255));self.fill_rect(rect.x,rect.y,rect.width,rect.height,c);}
  DisplayCommand::Border{rect,widths,color}=>{let c=parse_color(color).unwrap_or(Rgba(0,0,0,255));self.fill_rect(rect.x,rect.y,rect.width,widths.top,c);self.fill_rect(rect.x,rect.y+rect.height-widths.bottom,rect.width,widths.bottom,c);self.fill_rect(rect.x,rect.y,widths.left,rect.height,c);self.fill_rect(rect.x+rect.width-widths.right,rect.y,widths.right,rect.height,c);}
  DisplayCommand::Text{rect,..}=>self.fill_rect(rect.x,rect.y,rect.width.min(240.0),2.0,Rgba(30,30,30,255)),
 }}}
 pub fn fill_rect(&mut self,x:f32,y:f32,w:f32,h:f32,color:Rgba){let x0=x.max(0.0)as u32;let y0=y.max(0.0)as u32;let x1=(x+w).max(0.0).min(self.width as f32)as u32;let y1=(y+h).max(0.0).min(self.height as f32)as u32;for py in y0..y1{for px in x0..x1{self.pixels[(py*self.width+px)as usize]=color;}}}
 pub fn to_ppm(&self)->Vec<u8>{let mut o=format!("P6\n{} {}\n255\n",self.width,self.height).into_bytes();for Rgba(r,g,b,_)in &self.pixels{o.extend_from_slice(&[*r,*g,*b]);}o}
}
fn parse_color(s:&str)->Option<Rgba>{let s=s.trim();match s{"white"=>Some(Rgba(255,255,255,255)),"black"=>Some(Rgba(0,0,0,255)),"red"=>Some(Rgba(255,0,0,255)),"green"=>Some(Rgba(0,128,0,255)),"blue"=>Some(Rgba(0,0,255,255)),_=>{let h=s.strip_prefix('#')?;if h.len()==6{Some(Rgba(u8::from_str_radix(&h[0..2],16).ok()?,u8::from_str_radix(&h[2..4],16).ok()?,u8::from_str_radix(&h[4..6],16).ok()?,255))}else{None}}}}
#[cfg(test)]mod tests{use super::*;#[test]fn parses_hex(){assert_eq!(parse_color("#ff0080"),Some(Rgba(255,0,128,255)));}}
