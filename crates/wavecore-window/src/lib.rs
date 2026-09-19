use minifb::{Key, KeyRepeat, MouseButton, MouseMode, Window, WindowOptions};
use wavecore_layout::Rect;
use wavecore_pixels::Surface;

#[derive(Debug, Clone, PartialEq)]
pub enum WindowEvent {
    None,
    Click { x: f32, y: f32 },
    TextInput(char),
    Backspace,
    Enter,
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
    pub back_buffer: Vec<u32>,
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
            back_buffer: vec![0; width * height],
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
        let shift = self.window.is_key_down(Key::LeftShift) || self.window.is_key_down(Key::RightShift);

        if self.window.is_key_pressed(Key::Backspace, KeyRepeat::No) {
            events.push(WindowEvent::Backspace);
        } else if alt && self.window.is_key_pressed(Key::Left, KeyRepeat::No) {
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

        if self.window.is_key_pressed(Key::Enter, KeyRepeat::No) {
            events.push(WindowEvent::Enter);
        }

        // Text typing keys
        for key in self.window.get_keys_pressed(KeyRepeat::Yes) {
            if let Some(ch) = key_to_char(key, shift) {
                events.push(WindowEvent::TextInput(ch));
            }
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
            self.back_buffer = vec![0; w * h];
            Some((w, h))
        } else {
            None
        }
    }

    /// Blit surface pixels to the window buffer
    pub fn present(&mut self, surface: &Surface) -> Result<(), minifb::Error> {
        self.back_buffer = surface.to_u32_buffer();
        self.window.update_with_buffer(&self.back_buffer, self.width, self.height)
    }

    /// Blit only the damaged regions to the window buffer
    pub fn present_damage(&mut self, surface: &Surface, damage: &[Rect]) -> Result<(), minifb::Error> {
        if damage.is_empty() {
            return self.window.update_with_buffer(&self.back_buffer, self.width, self.height);
        }

        let sw = surface.width as usize;
        let sh = surface.height as usize;
        let bw = self.width;
        let bh = self.height;

        for d in damage {
            let x0 = (d.x.max(0.0) as usize).min(bw);
            let y0 = (d.y.max(0.0) as usize).min(bh);
            let x1 = ((d.x + d.width).max(0.0) as usize).min(bw);
            let y1 = ((d.y + d.height).max(0.0) as usize).min(bh);

            for y in y0..y1 {
                if y >= sh {
                    break;
                }
                let src_row = y * sw;
                let dst_row = y * bw;
                for x in x0..x1 {
                    if x >= sw {
                        break;
                    }
                    let pixel = surface.pixels[src_row + x];
                    let u = ((pixel.0 as u32) << 16) | ((pixel.1 as u32) << 8) | (pixel.2 as u32);
                    self.back_buffer[dst_row + x] = u;
                }
            }
        }

        self.window.update_with_buffer(&self.back_buffer, self.width, self.height)
    }
}

fn key_to_char(k: Key, shift: bool) -> Option<char> {
    match k {
        Key::A => Some(if shift { 'A' } else { 'a' }),
        Key::B => Some(if shift { 'B' } else { 'b' }),
        Key::C => Some(if shift { 'C' } else { 'c' }),
        Key::D => Some(if shift { 'D' } else { 'd' }),
        Key::E => Some(if shift { 'E' } else { 'e' }),
        Key::F => Some(if shift { 'F' } else { 'f' }),
        Key::G => Some(if shift { 'G' } else { 'g' }),
        Key::H => Some(if shift { 'H' } else { 'h' }),
        Key::I => Some(if shift { 'I' } else { 'i' }),
        Key::J => Some(if shift { 'J' } else { 'j' }),
        Key::K => Some(if shift { 'K' } else { 'k' }),
        Key::L => Some(if shift { 'L' } else { 'l' }),
        Key::M => Some(if shift { 'M' } else { 'm' }),
        Key::N => Some(if shift { 'N' } else { 'n' }),
        Key::O => Some(if shift { 'O' } else { 'o' }),
        Key::P => Some(if shift { 'P' } else { 'p' }),
        Key::Q => Some(if shift { 'Q' } else { 'q' }),
        Key::R => Some(if shift { 'R' } else { 'r' }),
        Key::S => Some(if shift { 'S' } else { 's' }),
        Key::T => Some(if shift { 'T' } else { 't' }),
        Key::U => Some(if shift { 'U' } else { 'u' }),
        Key::V => Some(if shift { 'V' } else { 'v' }),
        Key::W => Some(if shift { 'W' } else { 'w' }),
        Key::X => Some(if shift { 'X' } else { 'x' }),
        Key::Y => Some(if shift { 'Y' } else { 'y' }),
        Key::Z => Some(if shift { 'Z' } else { 'z' }),
        Key::Key0 => Some(if shift { ')' } else { '0' }),
        Key::Key1 => Some(if shift { '!' } else { '1' }),
        Key::Key2 => Some(if shift { '@' } else { '2' }),
        Key::Key3 => Some(if shift { '#' } else { '3' }),
        Key::Key4 => Some(if shift { '$' } else { '4' }),
        Key::Key5 => Some(if shift { '%' } else { '5' }),
        Key::Key6 => Some(if shift { '^' } else { '6' }),
        Key::Key7 => Some(if shift { '&' } else { '7' }),
        Key::Key8 => Some(if shift { '*' } else { '8' }),
        Key::Key9 => Some(if shift { '(' } else { '9' }),
        Key::Space => Some(' '),
        Key::Period => Some(if shift { '>' } else { '.' }),
        Key::Comma => Some(if shift { '<' } else { ',' }),
        Key::Slash => Some(if shift { '?' } else { '/' }),
        Key::Minus => Some(if shift { '_' } else { '-' }),
        Key::Equal => Some(if shift { '+' } else { '=' }),
        _ => None,
    }
}
