#![allow(dead_code)]

use bootloader_api::info::{FrameBufferInfo, PixelFormat};
use bootloader_api::BootInfo;
use spin::Mutex;
use x86_64::instructions::interrupts;

use crate::font::VGA8_FONT;

pub struct Console {
    fb: &'static mut [u8],
    info: FrameBufferInfo,
    width: usize,
    height: usize,
    cursor_x: usize,
    cursor_y: usize,
    scale: usize,
    fg: u32,
    bg: u32,
}

impl Console {
    pub fn from_boot_info(boot: &'static mut BootInfo) -> Option<Self> {
        let fb = boot.framebuffer.as_mut()?;
        let info = fb.info();
        let slice = fb.buffer_mut();

        let scale = 2;
        let width = info.width / (8 * scale);
        let height = info.height / (8 * scale);

        Some(Self {
            fb: slice,
            info,
            width,
            height,
            cursor_x: 0,
            cursor_y: 0,
            scale,
            fg: 0xCCCCCC,
            bg: 0x000000,
        })
    }

    fn draw_glyph(&mut self, x: usize, y: usize, c: char, color: u32) {
        let glyph = if (c as u8) < 0x20 || (c as u8) > 0x7e {
            VGA8_FONT[0]
        } else {
            VGA8_FONT[(c as u8 - 0x20) as usize]
        };

        let s = self.scale;
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..8 {
                let bit = (bits >> (7 - col)) & 1;
                let px = x * 8 * s + col * s;
                let py = y * 8 * s + row * s;
                let pix = if bit == 1 { color } else { self.bg };
                self.fill_rect(px, py, s, s, pix);
            }
        }
    }

    fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: u32) {
        let bytes_per_pixel = self.info.bytes_per_pixel;
        let stride = self.info.stride;
        for dy in 0..h {
            for dx in 0..w {
                let px = x + dx;
                let py = y + dy;
                if px < self.info.width && py < self.info.height {
                    let off = (py * stride + px) * bytes_per_pixel;
                    self.write_pixel(off, color);
                }
            }
        }
    }

    fn write_pixel(&mut self, off: usize, color: u32) {
        let r = ((color >> 16) & 0xFF) as u8;
        let g = ((color >> 8) & 0xFF) as u8;
        let b = (color & 0xFF) as u8;

        match self.info.pixel_format {
            PixelFormat::Rgb => {
                self.fb[off]     = r;
                self.fb[off + 1] = g;
                self.fb[off + 2] = b;
            }
            PixelFormat::Bgr => {
                self.fb[off]     = b;
                self.fb[off + 1] = g;
                self.fb[off + 2] = r;
            }
            _ => {}
        }
    }

    pub fn clear(&mut self) {
        self.fill_rect(0, 0, self.info.width, self.info.height, self.bg);
        self.cursor_x = 0;
        self.cursor_y = 0;
    }

    pub fn put_char(&mut self, c: char) {
        self.erase_cursor();
        if c == '\n' {
            self.newline();
        } else {
            self.draw_glyph(self.cursor_x, self.cursor_y, c, self.fg);
            self.cursor_x += 1;
            if self.cursor_x >= self.width {
                self.newline();
            }
        }
        self.draw_cursor();
    }

    pub fn write_line(&mut self, s: &str) {
        for c in s.chars() {
            self.put_char(c);
        }
        self.put_char('\n');
    }

    pub fn write(&mut self, s: &str) {
        for c in s.chars() {
            self.put_char(c);
        }
    }

    pub fn newline(&mut self) {
        self.erase_cursor();
        self.cursor_x = 0;
        self.cursor_y += 1;
        if self.cursor_y >= self.height {
            self.scroll();
            self.cursor_y = self.height - 1;
        }
        self.draw_cursor();
    }

    pub fn backspace(&mut self) {
        if self.cursor_x > 0 {
            self.erase_cursor();
            self.cursor_x -= 1;
            self.draw_glyph(self.cursor_x, self.cursor_y, ' ', self.bg);
            self.draw_cursor();
        }
    }

    fn scroll(&mut self) {
        let row_bytes = self.info.stride * self.info.bytes_per_pixel * 8 * self.scale;
        let total_bytes = self.info.stride * self.info.bytes_per_pixel * self.info.height;
        let shift = row_bytes * self.scale;

        self.fb.copy_within(shift..total_bytes, 0);
        let clear_start = total_bytes - shift;
        for b in &mut self.fb[clear_start..] {
            *b = 0;
        }
    }

    fn draw_cursor(&mut self) {
        let s = self.scale;
        let px = self.cursor_x * 8 * s;
        let py = self.cursor_y * 8 * s + (7 * s);
        self.fill_rect(px, py, 8 * s, s, 0xFFFFFF);
    }

    fn erase_cursor(&mut self) {
        let s = self.scale;
        let px = self.cursor_x * 8 * s;
        let py = self.cursor_y * 8 * s + (7 * s);
        self.fill_rect(px, py, 8 * s, s, self.bg);
    }

    pub fn cput_char(&mut self, c: char, fg: u32, bg: u32) {
        let old_fg = self.fg;
        let old_bg = self.bg;

        self.fg = fg;
        self.bg = bg;
        self.put_char(c);

        self.fg = old_fg;
        self.bg = old_bg;
    }

    pub fn cwrite(&mut self, s: &str, fg: u32, bg: u32) {
        let old_fg = self.fg;
        let old_bg = self.bg;

        self.fg = fg;
        self.bg = bg;
        for c in s.chars() {
            self.put_char(c);
        }

        self.fg = old_fg;
        self.bg = old_bg;
    }

    pub fn cwrite_line(&mut self, s: &str, fg: u32, bg: u32) {
        let old_fg = self.fg;
        let old_bg = self.bg;

        self.fg = fg;
        self.bg = bg;
        for c in s.chars() {
            self.put_char(c);
        }
        self.fg = old_fg;
        self.bg = old_bg;
        
        self.put_char('\n');
    }
}

pub static CONSOLE: Mutex<Option<Console>> = Mutex::new(None);

pub fn init_console(boot: &'static mut BootInfo) {
    if let Some(console) = Console::from_boot_info(boot) {
        *CONSOLE.lock() = Some(console);
    }
}

pub fn with_console<F, R>(f: F) -> R
where
    F: FnOnce(&mut Console) -> R,
{
    interrupts::without_interrupts(|| {
        let mut lock = CONSOLE.lock();
        let con = lock.as_mut().expect("Console not init");
        f(con)
    })
}

pub fn write_line(s: &str) {
    with_console(|c| c.write_line(s));
}

pub fn clear_screen() {
    with_console(|c| c.clear());
}

pub fn cwrite_line(s: &str, fg: u32, bg: u32) {
    with_console(|c| c.cwrite_line(s, fg, bg));
}

pub fn cput_char(c: char, fg: u32, bg: u32) {
    with_console(|con| con.cput_char(c, fg, bg));
}

pub fn cwrite(s: &str, fg: u32, bg: u32) {
    with_console(|c| c.cwrite(s, fg, bg));
}

pub fn write(s: &str) {
    with_console(|c| c.write(s));
}
