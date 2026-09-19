use std::sync::Arc;
use fontdue::{Font, FontSettings};
use rustybuzz::{Face, UnicodeBuffer};
use wavecore_render::DisplayCommand;
use wavecore_text::FontSystem;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);

pub struct FontFace {
    pub data: Arc<Vec<u8>>,
    pub index: u32,
    pub font: Arc<Font>,
}

#[derive(Clone)]
pub struct Surface {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<Rgba>,
    fonts: Vec<Arc<FontFace>>,
}

impl Surface {
    pub fn new(width: u32, height: u32) -> Self {
        let font_system = FontSystem::new();
        let mut fonts = Vec::new();
        for (data, index) in font_system.fallback_fonts() {
            if let Ok(f) = Font::from_bytes(
                data.as_slice(),
                FontSettings {
                    collection_index: index,
                    ..FontSettings::default()
                },
            ) {
                fonts.push(Arc::new(FontFace {
                    data: Arc::new(data),
                    index,
                    font: Arc::new(f),
                }));
            }
        }

        Self {
            width,
            height,
            pixels: vec![Rgba(255, 255, 255, 255); (width * height) as usize],
            fonts,
        }
    }

    pub fn clear(&mut self, color: Rgba) {
        self.pixels.fill(color);
    }

    pub fn paint(&mut self, list: &[DisplayCommand]) {
        self.paint_offset(list, 0.0, 0.0);
    }

    pub fn paint_offset(&mut self, list: &[DisplayCommand], offset_x: f32, offset_y: f32) {
        for c in list {
            match c {
                DisplayCommand::FillRect { rect, color } => {
                    let c = parse_color(color).unwrap_or(Rgba(240, 240, 240, 255));
                    self.fill_rect(rect.x + offset_x, rect.y + offset_y, rect.width, rect.height, c);
                }
                DisplayCommand::Border { rect, widths, color } => {
                    let c = parse_color(color).unwrap_or(Rgba(0, 0, 0, 255));
                    let rx = rect.x + offset_x;
                    let ry = rect.y + offset_y;
                    self.fill_rect(rx, ry, rect.width, widths.top, c);
                    self.fill_rect(rx, ry + rect.height - widths.bottom, rect.width, widths.bottom, c);
                    self.fill_rect(rx, ry, widths.left, rect.height, c);
                    self.fill_rect(rx + rect.width - widths.right, ry, widths.right, rect.height, c);
                }
                DisplayCommand::Text { text, rect, font_size, line_height, color } => {
                    let c = parse_color(color).unwrap_or(Rgba(30, 30, 30, 255));
                    self.draw_text(text, rect.x + offset_x, rect.y + offset_y, *font_size, *line_height, rect.width, c);
                }
            }
        }
    }

    pub fn draw_text(&mut self, text: &str, x: f32, y: f32, font_size: f32, line_height: f32, max_width: f32, color: Rgba) {
        if self.fonts.is_empty() {
            self.fill_rect(x, y + font_size * 0.8, max_width.min(font_size * 12.0), (font_size / 10.0).max(1.0), color);
            return;
        }

        let has_thai = text.chars().any(|c| (0x0E00..=0x0E7F).contains(&(c as u32)));
        let selected_font = if has_thai {
            self.fonts
                .iter()
                .find(|f| f.font.lookup_glyph_index('ก') != 0)
                .cloned()
                .unwrap_or_else(|| self.fonts[0].clone())
        } else {
            self.fonts[0].clone()
        };

        let Some(face) = Face::from_slice(&selected_font.data, selected_font.index) else {
            return;
        };

        let upem = face.units_per_em() as f32;
        let scale = font_size / upem;
        let baseline_y = y + (line_height - font_size) * 0.5 + font_size * 0.75;
        let mut cur_x = x;

        let mut buffer = UnicodeBuffer::new();
        buffer.push_str(text);
        buffer.guess_segment_properties();
        let glyph_buffer = rustybuzz::shape(&face, &[], buffer);

        let infos = glyph_buffer.glyph_infos();
        let positions = glyph_buffer.glyph_positions();

        for (info, pos) in infos.iter().zip(positions) {
            let glyph_id = info.glyph_id as u16;
            let x_offset = pos.x_offset as f32 * scale;
            let y_offset = pos.y_offset as f32 * scale;
            let x_advance = pos.x_advance as f32 * scale;

            let (metrics, bitmap) = selected_font.font.rasterize_indexed(glyph_id, font_size);

            let glyph_left = cur_x + x_offset + metrics.xmin as f32;
            let glyph_top = (baseline_y - y_offset) - (metrics.ymin as f32 + metrics.height as f32);

            for row in 0..metrics.height {
                for col in 0..metrics.width {
                    let alpha = bitmap[row * metrics.width + col];
                    if alpha > 0 {
                        let px = (glyph_left + col as f32).round() as i32;
                        let py = (glyph_top + row as f32).round() as i32;
                        self.blend_pixel(px, py, color, alpha);
                    }
                }
            }

            cur_x += x_advance;
        }
    }

    pub fn blend_pixel(&mut self, x: i32, y: i32, color: Rgba, alpha_mask: u8) {
        if alpha_mask == 0 || x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let idx = (y as u32 * self.width + x as u32) as usize;
        let bg = self.pixels[idx];
        let a = (alpha_mask as u32 * color.3 as u32) / 255;
        let inv_a = 255 - a;
        let r = ((color.0 as u32 * a + bg.0 as u32 * inv_a) / 255) as u8;
        let g = ((color.1 as u32 * a + bg.1 as u32 * inv_a) / 255) as u8;
        let b = ((color.2 as u32 * a + bg.2 as u32 * inv_a) / 255) as u8;
        self.pixels[idx] = Rgba(r, g, b, 255);
    }

    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Rgba) {
        if w <= 0.0 || h <= 0.0 {
            return;
        }
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

    pub fn to_u32_buffer(&self) -> Vec<u32> {
        self.pixels
            .iter()
            .map(|Rgba(r, g, b, _)| ((r.to_owned() as u32) << 16) | ((g.to_owned() as u32) << 8) | (b.to_owned() as u32))
            .collect()
    }

    pub fn to_ppm(&self) -> Vec<u8> {
        let mut o = format!("P6\n{} {}\n255\n", self.width, self.height).into_bytes();
        for Rgba(r, g, b, _) in &self.pixels {
            o.extend_from_slice(&[*r, *g, *b]);
        }
        o
    }
}

fn parse_color(s: &str) -> Option<Rgba> {
    let s = s.trim().to_ascii_lowercase();
    match s.as_str() {
        "transparent" => Some(Rgba(0, 0, 0, 0)),
        "white" => Some(Rgba(255, 255, 255, 255)),
        "black" => Some(Rgba(0, 0, 0, 255)),
        "red" => Some(Rgba(255, 0, 0, 255)),
        "green" => Some(Rgba(0, 128, 0, 255)),
        "blue" => Some(Rgba(0, 0, 255, 255)),
        "gray" | "grey" => Some(Rgba(128, 128, 128, 255)),
        "yellow" => Some(Rgba(255, 255, 0, 255)),
        "purple" => Some(Rgba(128, 0, 128, 255)),
        "orange" => Some(Rgba(255, 165, 0, 255)),
        "cyan" => Some(Rgba(0, 255, 255, 255)),
        _ => {
            let h = s.strip_prefix('#')?;
            match h.len() {
                3 => Some(Rgba(
                    u8::from_str_radix(&h[0..1].repeat(2), 16).ok()?,
                    u8::from_str_radix(&h[1..2].repeat(2), 16).ok()?,
                    u8::from_str_radix(&h[2..3].repeat(2), 16).ok()?,
                    255,
                )),
                6 => Some(Rgba(
                    u8::from_str_radix(&h[0..2], 16).ok()?,
                    u8::from_str_radix(&h[2..4], 16).ok()?,
                    u8::from_str_radix(&h[4..6], 16).ok()?,
                    255,
                )),
                8 => Some(Rgba(
                    u8::from_str_radix(&h[0..2], 16).ok()?,
                    u8::from_str_radix(&h[2..4], 16).ok()?,
                    u8::from_str_radix(&h[4..6], 16).ok()?,
                    u8::from_str_radix(&h[6..8], 16).ok()?,
                )),
                _ => None,
            }
        }
    }
}
