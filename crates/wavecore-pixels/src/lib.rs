use wavecore_render::DisplayCommand;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);

pub struct Surface {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<Rgba>,
}

impl Surface {
    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height, pixels: vec![Rgba(255, 255, 255, 255); (width * height) as usize] }
    }

    pub fn paint(&mut self, list: &[DisplayCommand]) {
        for command in list {
            match command {
                DisplayCommand::Text { rect, .. } => {
                    // Temporary text-run visualization until glyph rasterization lands.
                    self.fill_rect(rect.x, rect.y, rect.width.min(240.0), 2.0, Rgba(30, 30, 30, 255));
                }
            }
        }
    }

    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Rgba) {
        let x0 = x.max(0.0) as u32;
        let y0 = y.max(0.0) as u32;
        let x1 = (x + w).max(0.0).min(self.width as f32) as u32;
        let y1 = (y + h).max(0.0).min(self.height as f32) as u32;
        for py in y0..y1 {
            for px in x0..x1 {
                self.pixels[(py * self.width + px) as usize] = color;
            }
        }
    }

    pub fn to_ppm(&self) -> Vec<u8> {
        let mut out = format!("P6\n{} {}\n255\n", self.width, self.height).into_bytes();
        for Rgba(r, g, b, _) in &self.pixels { out.extend_from_slice(&[*r, *g, *b]); }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn creates_white_surface() {
        let s = Surface::new(2, 2);
        assert_eq!(s.pixels.len(), 4);
        assert_eq!(s.pixels[0], Rgba(255,255,255,255));
    }
}
