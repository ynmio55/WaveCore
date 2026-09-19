use std::sync::Arc;
use fontdue::{Font, FontSettings};
use rustybuzz::{Face, UnicodeBuffer};
use wavecore_layout::Rect;
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
    pub clip_stack: Vec<Rect>,
    pub damage_rects: Vec<Rect>,
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
            clip_stack: Vec::new(),
            damage_rects: Vec::new(),
            fonts,
        }
    }

    pub fn clear(&mut self, color: Rgba) {
        self.pixels.fill(color);
        self.clip_stack.clear();
        self.damage_rects.clear();
    }

    pub fn active_clip(&self) -> Option<Rect> {
        self.clip_stack.last().copied()
    }

    pub fn push_clip(&mut self, rect: Rect) {
        let effective = if let Some(current) = self.active_clip() {
            current.intersection(&rect).unwrap_or(Rect { x: 0.0, y: 0.0, width: 0.0, height: 0.0 })
        } else {
            rect
        };
        self.clip_stack.push(effective);
    }

    pub fn pop_clip(&mut self) {
        self.clip_stack.pop();
    }

    pub fn mark_damage(&mut self, rect: Rect) {
        self.damage_rects.push(rect);
    }

    pub fn paint(&mut self, list: &[DisplayCommand]) {
        self.paint_offset(list, 0.0, 0.0);
    }

    pub fn paint_damage(&mut self, list: &[DisplayCommand], damage: &[Rect]) {
        self.paint_damage_offset(list, damage, 0.0, 0.0);
    }

    pub fn paint_damage_offset(&mut self, list: &[DisplayCommand], damage: &[Rect], offset_x: f32, offset_y: f32) {
        if damage.is_empty() {
            return;
        }

        for d in damage {
            let clip_d = Rect {
                x: d.x + offset_x,
                y: d.y + offset_y,
                width: d.width,
                height: d.height,
            };
            self.push_clip(clip_d);

            for c in list {
                if let Some(b) = command_bounds(c) {
                    let offset_b = Rect {
                        x: b.x + offset_x,
                        y: b.y + offset_y,
                        width: b.width,
                        height: b.height,
                    };
                    if clip_d.intersection(&offset_b).is_none() {
                        continue;
                    }
                }

                match c {
                    DisplayCommand::PushClip(rect) => {
                        self.push_clip(Rect {
                            x: rect.x + offset_x,
                            y: rect.y + offset_y,
                            width: rect.width,
                            height: rect.height,
                        });
                    }
                    DisplayCommand::PopClip => {
                        self.pop_clip();
                    }
                    DisplayCommand::FillRect { rect, color } => {
                        let col = parse_color(color).unwrap_or(Rgba(240, 240, 240, 255));
                        self.fill_rect(rect.x + offset_x, rect.y + offset_y, rect.width, rect.height, col);
                    }
                    DisplayCommand::Border { rect, widths, color } => {
                        let col = parse_color(color).unwrap_or(Rgba(0, 0, 0, 255));
                        let rx = rect.x + offset_x;
                        let ry = rect.y + offset_y;
                        self.fill_rect(rx, ry, rect.width, widths.top, col);
                        self.fill_rect(rx, ry + rect.height - widths.bottom, rect.width, widths.bottom, col);
                        self.fill_rect(rx, ry, widths.left, rect.height, col);
                        self.fill_rect(rx + rect.width - widths.right, ry, widths.right, rect.height, col);
                    }
                    DisplayCommand::Text { text, rect, font_size, line_height, color } => {
                        let col = parse_color(color).unwrap_or(Rgba(30, 30, 30, 255));
                        self.draw_text(text, rect.x + offset_x, rect.y + offset_y, *font_size, *line_height, rect.width, col);
                    }
                    DisplayCommand::Image { rect, src } => {
                        self.draw_image(src, rect.x + offset_x, rect.y + offset_y, rect.width, rect.height);
                    }
                }
            }

            self.pop_clip();
        }
    }

    pub fn paint_offset(&mut self, list: &[DisplayCommand], offset_x: f32, offset_y: f32) {
        for c in list {
            match c {
                DisplayCommand::PushClip(rect) => {
                    self.push_clip(Rect {
                        x: rect.x + offset_x,
                        y: rect.y + offset_y,
                        width: rect.width,
                        height: rect.height,
                    });
                }
                DisplayCommand::PopClip => {
                    self.pop_clip();
                }
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
                DisplayCommand::Image { rect, src } => {
                    self.draw_image(src, rect.x + offset_x, rect.y + offset_y, rect.width, rect.height);
                }
            }
        }
    }

    pub fn draw_image(&mut self, src: &str, x: f32, y: f32, width: f32, height: f32) {
        if width <= 0.0 || height <= 0.0 {
            return;
        }

        let bytes = match std::fs::read(src) {
            Ok(b) => b,
            Err(_) => {
                // Fallback placeholder box
                self.fill_rect(x, y, width, height, Rgba(235, 238, 242, 255));
                return;
            }
        };

        let Ok(img) = image::load_from_memory(&bytes) else {
            self.fill_rect(x, y, width, height, Rgba(235, 238, 242, 255));
            return;
        };

        let rgba_img = img.to_rgba8();
        let orig_w = rgba_img.width() as f32;
        let orig_h = rgba_img.height() as f32;
        let scale_x = orig_w / width;
        let scale_y = orig_h / height;

        let target_w = width as u32;
        let target_h = height as u32;

        for dy in 0..target_h {
            let sy = ((dy as f32 * scale_y) as u32).min(rgba_img.height() - 1);
            for dx in 0..target_w {
                let sx = ((dx as f32 * scale_x) as u32).min(rgba_img.width() - 1);
                let pixel = rgba_img.get_pixel(sx, sy);
                let color = Rgba(pixel[0], pixel[1], pixel[2], pixel[3]);
                self.blend_pixel((x + dx as f32) as i32, (y + dy as f32) as i32, color, pixel[3]);
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
        if let Some(clip) = self.active_clip() {
            let fx = x as f32;
            let fy = y as f32;
            if fx < clip.x || fx >= clip.x + clip.width || fy < clip.y || fy >= clip.y + clip.height {
                return;
            }
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
        let mut rx0 = x;
        let mut ry0 = y;
        let mut rx1 = x + w;
        let mut ry1 = y + h;

        if let Some(clip) = self.active_clip() {
            rx0 = rx0.max(clip.x);
            ry0 = ry0.max(clip.y);
            rx1 = rx1.min(clip.x + clip.width);
            ry1 = ry1.min(clip.y + clip.height);
            if rx1 <= rx0 || ry1 <= ry0 {
                return;
            }
        }

        let x0 = rx0.max(0.0) as u32;
        let y0 = ry0.max(0.0) as u32;
        let x1 = rx1.max(0.0).min(self.width as f32) as u32;
        let y1 = ry1.max(0.0).min(self.height as f32) as u32;
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

pub fn command_bounds(cmd: &DisplayCommand) -> Option<Rect> {
    match cmd {
        DisplayCommand::FillRect { rect, .. } => Some(*rect),
        DisplayCommand::Border { rect, .. } => Some(*rect),
        DisplayCommand::Text { rect, .. } => Some(*rect),
        DisplayCommand::Image { rect, .. } => Some(*rect),
        DisplayCommand::PushClip(_) | DisplayCommand::PopClip => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipping_scissoring() {
        let mut surface = Surface::new(100, 100);
        surface.clear(Rgba(255, 255, 255, 255));

        // Push clip of 10..30 x 10..30
        let clip_rect = Rect { x: 10.0, y: 10.0, width: 20.0, height: 20.0 };
        surface.paint(&[
            DisplayCommand::PushClip(clip_rect),
            DisplayCommand::FillRect {
                rect: Rect { x: 0.0, y: 0.0, width: 50.0, height: 50.0 },
                color: "#ff0000".to_string(),
            },
            DisplayCommand::PopClip,
        ]);

        // (5, 5) should remain white because it is outside the clip
        assert_eq!(surface.pixels[5 * 100 + 5], Rgba(255, 255, 255, 255));
        // (15, 15) should be red because it is inside the clip
        assert_eq!(surface.pixels[15 * 100 + 15], Rgba(255, 0, 0, 255));
        // (40, 40) should remain white because it is outside the clip
        assert_eq!(surface.pixels[40 * 100 + 40], Rgba(255, 255, 255, 255));
    }

    #[test]
    fn partial_paint_damage_skips_unaffected_commands() {
        let mut surface = Surface::new(100, 100);
        surface.clear(Rgba(255, 255, 255, 255));

        let commands = vec![
            DisplayCommand::FillRect {
                rect: Rect { x: 0.0, y: 0.0, width: 20.0, height: 20.0 },
                color: "#ff0000".to_string(),
            },
            DisplayCommand::FillRect {
                rect: Rect { x: 60.0, y: 60.0, width: 20.0, height: 20.0 },
                color: "#00ff00".to_string(),
            },
        ];

        // Damage rect only covers region (50..90 x 50..90)
        let damage = vec![Rect { x: 50.0, y: 50.0, width: 40.0, height: 40.0 }];
        surface.paint_damage(&commands, &damage);

        // (10, 10) was NOT painted because red command does not intersect damage rect
        assert_eq!(surface.pixels[10 * 100 + 10], Rgba(255, 255, 255, 255));
        // (70, 70) WAS painted green because it intersects damage rect
        assert_eq!(surface.pixels[70 * 100 + 70], Rgba(0, 255, 0, 255));
    }
}
