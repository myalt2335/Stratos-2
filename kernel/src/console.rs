#![allow(dead_code)]
#![allow(unused_variables)]

extern crate alloc;

use bootloader_api::info::{FrameBufferInfo, PixelFormat};
use bootloader_api::BootInfo;
use spin::Mutex;
use x86_64::instructions::interrupts;
use crate::font::VGA8_FONT;
use alloc::vec::Vec;
use alloc::vec;

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
    reserved_hud_rows: usize,
    hud_back: Option<Vec<u8>>,
    hud_back_stride: usize,
}

pub enum DrawPos {
    Char(usize, usize),
}

pub enum HudAlign {
    Left,
    Center,
    Right,
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
            reserved_hud_rows: 0,
            hud_back: None,
            hud_back_stride: 0,
        })
    }

    fn write_pixel(&mut self, off: usize, color: u32) {
        let r = ((color >> 16) & 0xFF) as u8;
        let g = ((color >> 8) & 0xFF) as u8;
        let b = (color & 0xFF) as u8;
        match (self.info.pixel_format, self.info.bytes_per_pixel) {
            (PixelFormat::Rgb, 4) => {
                self.fb[off] = r;
                self.fb[off + 1] = g;
                self.fb[off + 2] = b;
                self.fb[off + 3] = 0xFF;
            }
            (PixelFormat::Rgb, 3) => {
                self.fb[off] = r;
                self.fb[off + 1] = g;
                self.fb[off + 2] = b;
            }
            (PixelFormat::Bgr, 4) => {
                self.fb[off] = b;
                self.fb[off + 1] = g;
                self.fb[off + 2] = r;
                self.fb[off + 3] = 0xFF;
            }
            (PixelFormat::Bgr, 3) => {
                self.fb[off] = b;
                self.fb[off + 1] = g;
                self.fb[off + 2] = r;
            }
            _ => {}
        }
    }

    fn write_pixel_into(&self, buf: &mut [u8], buf_stride_px: usize, x: usize, y: usize, color: u32) {
        if x >= buf_stride_px || y >= self.info.height { return; }
        let bpp = self.info.bytes_per_pixel;
        let off = (y * buf_stride_px + x) * bpp;
        if off + bpp > buf.len() { return; }
        let r = ((color >> 16) & 0xFF) as u8;
        let g = ((color >> 8) & 0xFF) as u8;
        let b = (color & 0xFF) as u8;
        match (self.info.pixel_format, bpp) {
            (PixelFormat::Rgb, 4) => { buf[off] = r; buf[off+1] = g; buf[off+2] = b; buf[off+3] = 0xFF; }
            (PixelFormat::Rgb, 3) => { buf[off] = r; buf[off+1] = g; buf[off+2] = b; }
            (PixelFormat::Bgr, 4) => { buf[off] = b; buf[off+1] = g; buf[off+2] = r; buf[off+3] = 0xFF; }
            (PixelFormat::Bgr, 3) => { buf[off] = b; buf[off+1] = g; buf[off+2] = r; }
            _ => {}
        }
    }

    fn draw_glyph_into(&self, dst: &mut [u8], dst_stride_px: usize, x_char: usize, y_char: usize, c: char, fg: u32, bg: u32) {
        let glyph = if (c as u8) < 0x20 || (c as u8) > 0x7e {
            VGA8_FONT[0]
        } else {
            VGA8_FONT[(c as u8 - 0x20) as usize]
        };
        let s = self.scale;
        let base_px = x_char * 8 * s;
        let base_py = y_char * 8 * s;
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..8 {
                let bit = (bits >> (7 - col)) & 1;
                let pix = if bit == 1 { fg } else { bg };
                let px = base_px + col * s;
                let py = base_py + row * s;
                for dy in 0..s {
                    for dx in 0..s {
                        self.write_pixel_into(dst, dst_stride_px, px + dx, py + dy, pix);
                    }
                }
            }
        }
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
        if self.cursor_y >= self.text_area_height() {
            self.scroll();
            self.cursor_y = self.text_area_height() - 1;
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
        let visible_rows = self.text_area_height();
        let visible_px = visible_rows * 8 * self.scale;
        let shift = 8 * self.scale * self.info.stride * self.info.bytes_per_pixel;
        let copy_bytes = visible_px * self.info.stride * self.info.bytes_per_pixel;
        self.fb.copy_within(shift..copy_bytes, 0);
        let clear_start = copy_bytes - shift;
        for b in &mut self.fb[clear_start..copy_bytes] {
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

    pub fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    pub fn reserve_hud_rows(&mut self, rows: usize) {
        let rows = rows.min(self.height);
        self.reserved_hud_rows = rows;
        if rows > 0 {
            let s = self.scale;
            let hud_height_px = rows * 8 * s;
            let hud_width_px = self.info.width;
            self.hud_back_stride = hud_width_px;
            let bytes = hud_width_px * hud_height_px * self.info.bytes_per_pixel;
            self.hud_back = Some(vec![0u8; bytes]);
        } else {
            self.hud_back = None;
            self.hud_back_stride = 0;
        }
    }

    fn text_area_height(&self) -> usize {
        self.height.saturating_sub(self.reserved_hud_rows)
    }

    pub fn draw_text_at_char(&mut self, pos: DrawPos, s: &str) {
        match pos {
            DrawPos::Char(x, y) => {
                let old_x = self.cursor_x;
                let old_y = self.cursor_y;
                self.erase_cursor();
                let mut cx = x;
                for ch in s.chars() {
                    self.draw_glyph(cx, y, ch, self.fg);
                    cx += 1;
                }
                self.cursor_x = old_x;
                self.cursor_y = old_y;
                self.draw_cursor();
            }
        }
    }

    pub fn hud_begin(&mut self) {
        let (bg, info, stride_px) = (self.bg, &self.info, self.hud_back_stride);
        let scale = self.scale;
        let hud_h_px = self.reserved_hud_rows * 8 * scale;
        let mut local_buf_opt = None;
        core::mem::swap(&mut self.hud_back, &mut local_buf_opt);
        if let Some(mut buf) = local_buf_opt {
            for y in 0..hud_h_px {
                for x in 0..stride_px {
                    Self::write_pixel_into_static(&info, &mut buf, stride_px, x, y, bg);
                }
            }
            self.hud_back = Some(buf);
        }
    }

    fn write_pixel_into_static(info: &FrameBufferInfo, buf: &mut [u8], buf_stride_px: usize, x: usize, y: usize, color: u32) {
        if x >= buf_stride_px || y >= info.height { return; }
        let bpp = info.bytes_per_pixel;
        let off = (y * buf_stride_px + x) * bpp;
        if off + bpp > buf.len() { return; }
        let r = ((color >> 16) & 0xFF) as u8;
        let g = ((color >> 8) & 0xFF) as u8;
        let b = (color & 0xFF) as u8;
        match (info.pixel_format, bpp) {
            (PixelFormat::Rgb, 4) => { buf[off] = r; buf[off+1] = g; buf[off+2] = b; buf[off+3] = 0xFF; }
            (PixelFormat::Rgb, 3) => { buf[off] = r; buf[off+1] = g; buf[off+2] = b; }
            (PixelFormat::Bgr, 4) => { buf[off] = b; buf[off+1] = g; buf[off+2] = r; buf[off+3] = 0xFF; }
            (PixelFormat::Bgr, 3) => { buf[off] = b; buf[off+1] = g; buf[off+2] = r; }
            _ => {}
        }
    }

    pub fn hud_draw_text(&mut self, s: &str, fg: u32, align: HudAlign) {
        if self.reserved_hud_rows == 0 { return; }
        let (scale, info, bg, stride, rows) = (self.scale, &self.info, self.bg, self.hud_back_stride, self.reserved_hud_rows);
        let mut local_buf_opt = None;
        core::mem::swap(&mut self.hud_back, &mut local_buf_opt);
        if let Some(mut buf) = local_buf_opt {
            let char_w = 8 * scale;
            let text_chars = s.chars().count();
            let text_w_px = text_chars * char_w;
            let y_char = rows - 1;
            let x_char = match align {
                HudAlign::Left => 0,
                HudAlign::Center => ((info.width / (8 * scale)) / 2).saturating_sub(text_chars / 2),
                HudAlign::Right => (info.width.saturating_sub(text_w_px + 2 * scale)) / (8 * scale),
            };
            let mut cx = x_char;
            for ch in s.chars() {
                Self::draw_glyph_into_static(&info, scale, &mut buf, stride, cx, y_char, ch, fg, bg);
                cx += 1;
            }
            self.hud_back = Some(buf);
        }
    }

    fn draw_glyph_into_static(
        info: &FrameBufferInfo,
        scale: usize,
        dst: &mut [u8],
        dst_stride_px: usize,
        x_char: usize,
        y_char: usize,
        c: char,
        fg: u32,
        bg: u32,
    ) {
        let glyph = if (c as u8) < 0x20 || (c as u8) > 0x7e {
            VGA8_FONT[0]
        } else {
            VGA8_FONT[(c as u8 - 0x20) as usize]
        };
        let base_px = x_char * 8 * scale;
        let base_py = y_char * 8 * scale;
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..8 {
                let bit = (bits >> (7 - col)) & 1;
                let pix = if bit == 1 { fg } else { bg };
                let px = base_px + col * scale;
                let py = base_py + row * scale;
                for dy in 0..scale {
                    for dx in 0..scale {
                        Self::write_pixel_into_static(info, dst, dst_stride_px, px + dx, py + dy, pix);
                    }
                }
            }
        }
    }

    pub fn hud_present(&mut self) {
        if self.reserved_hud_rows == 0 { return; }
        let (buf, stride) = match (self.hud_back.as_ref(), self.hud_back_stride) {
            (Some(b), strd) => (b, strd),
            _ => return,
        };
        let bpp = self.info.bytes_per_pixel;
        let s = self.scale;
        let hud_h_px = self.reserved_hud_rows * 8 * s;
        let dst_y0 = self.info.height - hud_h_px;
        use core::ptr::copy_nonoverlapping;
        for y in (0..hud_h_px).rev() {
            let src_row = &buf[y * stride * bpp .. (y + 1) * stride * bpp];
            let dst_off = ((dst_y0 + y) * self.info.stride) * bpp;
            let dst_row = &mut self.fb[dst_off .. dst_off + stride * bpp];
            let len = src_row.len();
            let n_u64 = len / 8;
            let n_tail = len % 8;
            unsafe {
                let src64 = src_row.as_ptr() as *const u64;
                let dst64 = dst_row.as_mut_ptr() as *mut u64;
                copy_nonoverlapping(src64, dst64, n_u64);
                if n_tail > 0 {
                    let src_tail = src_row.as_ptr().add(n_u64 * 8);
                    let dst_tail = dst_row.as_mut_ptr().add(n_u64 * 8);
                    copy_nonoverlapping(src_tail, dst_tail, n_tail);
                }
            }
        }
    }

    pub fn clear_hud_row(&mut self) {
        self.hud_begin();
        self.hud_present();
    }

    pub fn erase_hud_box_for_len(&mut self, len: usize) {
        self.hud_begin();
        self.hud_present();
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
