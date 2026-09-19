use minifb::{Key, Window, WindowOptions};
use wavecore_pixels::Surface;

pub struct BrowserWindow {
    window: Window,
    pub width: usize,
    pub height: usize,
    pub scroll_y: f32,
}

impl BrowserWindow {
    pub fn new(title: &str, width: usize, height: usize) -> Result<Self, minifb::Error> {
        let mut window = Window::new(
            title,
            width,
            height,
            WindowOptions {
                resize: true,
                ..WindowOptions::default()
            },
        )?;
        // 60 FPS target
        window.set_target_fps(60);
        Ok(Self {
            window,
            width,
            height,
            scroll_y: 0.0,
        })
    }

    pub fn is_open(&self) -> bool {
        self.window.is_open() && !self.window.is_key_down(Key::Escape)
    }

    pub fn update(&mut self) -> bool {
        self.window.update();
        self.is_open()
    }

    /// Process mouse scroll wheel, update scroll_y, and return current scroll offset
    pub fn handle_scroll(&mut self) -> f32 {
        if let Some((_x, y)) = self.window.get_scroll_wheel() {
            if y.abs() > 0.01 {
                self.scroll_y = (self.scroll_y - y * 30.0).max(0.0);
            }
        }
        self.scroll_y
    }

    /// Check if window was resized, returning (new_width, new_height) if changed
    pub fn check_resize(&mut self) -> Option<(usize, usize)> {
        let (w, h) = self.window.get_size();
        if w != self.width || h != self.height {
            self.width = w;
            self.height = h;
            Some((w, h))
        } else {
            None
        }
    }

    /// Blit surface pixels to the window buffer
    pub fn present(&mut self, surface: &Surface) -> Result<(), minifb::Error> {
        let buffer = surface.to_u32_buffer();
        self.window.update_with_buffer(&buffer, surface.width as usize, surface.height as usize)
    }
}
