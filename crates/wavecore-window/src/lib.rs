use minifb::{Key, KeyRepeat, MouseButton, MouseMode, Window, WindowOptions};
use wavecore_pixels::Surface;

#[derive(Debug, Clone, PartialEq)]
pub enum WindowEvent {
    None,
    Click { x: f32, y: f32 },
    NavigateBack,
    NavigateForward,
    Reload,
    Scroll(f32),
}

pub struct BrowserWindow {
    window: Window,
    pub width: usize,
    pub height: usize,
    pub scroll_y: f32,
    mouse_was_down: bool,
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
        window.set_target_fps(60);
        Ok(Self {
            window,
            width,
            height,
            scroll_y: 0.0,
            mouse_was_down: false,
        })
    }

    pub fn set_title(&mut self, title: &str) {
        self.window.set_title(title);
    }

    pub fn is_open(&self) -> bool {
        self.window.is_open() && !self.window.is_key_down(Key::Escape)
    }

    pub fn update(&mut self) -> bool {
        self.window.update();
        self.is_open()
    }

    pub fn poll_events(&mut self) -> Vec<WindowEvent> {
        let mut events = Vec::new();

        // Detect single mouse click (mouse down transition)
        let mouse_down = self.window.get_mouse_down(MouseButton::Left);
        if mouse_down && !self.mouse_was_down {
            if let Some((mx, my)) = self.window.get_mouse_pos(MouseMode::Pass) {
                events.push(WindowEvent::Click {
                    x: mx,
                    y: my + self.scroll_y,
                });
            }
        }
        self.mouse_was_down = mouse_down;

        // Navigation keyboard shortcuts
        let alt = self.window.is_key_down(Key::LeftAlt) || self.window.is_key_down(Key::RightAlt);
        let ctrl = self.window.is_key_down(Key::LeftCtrl) || self.window.is_key_down(Key::RightCtrl);

        if self.window.is_key_pressed(Key::Backspace, KeyRepeat::No)
            || (alt && self.window.is_key_pressed(Key::Left, KeyRepeat::No))
        {
            events.push(WindowEvent::NavigateBack);
        }

        if alt && self.window.is_key_pressed(Key::Right, KeyRepeat::No) {
            events.push(WindowEvent::NavigateForward);
        }

        if self.window.is_key_pressed(Key::F5, KeyRepeat::No)
            || (ctrl && self.window.is_key_pressed(Key::R, KeyRepeat::No))
        {
            events.push(WindowEvent::Reload);
        }

        // Keyboard scrolling
        if self.window.is_key_pressed(Key::Down, KeyRepeat::Yes) {
            self.scroll_y = (self.scroll_y + 40.0).max(0.0);
            events.push(WindowEvent::Scroll(self.scroll_y));
        }
        if self.window.is_key_pressed(Key::Up, KeyRepeat::Yes) {
            self.scroll_y = (self.scroll_y - 40.0).max(0.0);
            events.push(WindowEvent::Scroll(self.scroll_y));
        }
        if self.window.is_key_pressed(Key::PageDown, KeyRepeat::Yes) {
            self.scroll_y = (self.scroll_y + 300.0).max(0.0);
            events.push(WindowEvent::Scroll(self.scroll_y));
        }
        if self.window.is_key_pressed(Key::PageUp, KeyRepeat::Yes) {
            self.scroll_y = (self.scroll_y - 300.0).max(0.0);
            events.push(WindowEvent::Scroll(self.scroll_y));
        }

        // Mouse wheel scrolling
        if let Some((_x, y)) = self.window.get_scroll_wheel() {
            if y.abs() > 0.01 {
                self.scroll_y = (self.scroll_y - y * 35.0).max(0.0);
                events.push(WindowEvent::Scroll(self.scroll_y));
            }
        }

        events
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
