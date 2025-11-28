#![allow(dead_code)]
#![allow(unused_variables)]

extern crate alloc;

use bootloader_api::info::{FrameBufferInfo, PixelFormat};
use bootloader_api::BootInfo;
use spin::Mutex;
use x86_64::instructions::interrupts;
use crate::font::VGA8_FONT;
use crate::font2::TERMINUS_FONT;
use crate::font3::SPLEEN_FONT;
use alloc::vec::Vec;
use alloc::vec;

#[derive(Copy, Clone)]
struct Font {
    glyph: fn(u8) -> &'static [u8],
    width: usize,
    height: usize,
    name: &'static str,
}

impl Font {
    fn glyph(&self, c: char) -> &'static [u8] {
        let code = c as u8;
        let idx = if code < 0x20 || code > 0x7e { 0 } else { (code - 0x20) as usize };
        (self.glyph)(idx as u8)
    }
}

fn vga8_glyph(idx: u8) -> &'static [u8] {
    &VGA8_FONT[idx as usize]
}

fn terminus_glyph(idx: u8) -> &'static [u8] {
    &TERMINUS_FONT[idx as usize]
}

fn spleen_glyph(idx: u8) -> &'static [u8] {
    &SPLEEN_FONT[idx as usize]
}

static FONT_VGA8: Font = Font {
    glyph: vga8_glyph,
    width: 8,
    height: 8,
    name: "vga8",
};

static FONT_TERMINUS: Font = Font {
    glyph: terminus_glyph,
    width: 8,
    height: 16,
    name: "terminus",
};

static FONT_SPLEEN: Font = Font {
    glyph: spleen_glyph,
    width: 8,
    height: 16,
    name: "spleen",
};

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum FontKind {
    Vga8,
    Terminus8x16,
    Spleen8x16,
}

impl FontKind {
    fn face(self) -> &'static Font {
        match self {
            FontKind::Vga8 => &FONT_VGA8,
            FontKind::Terminus8x16 => &FONT_TERMINUS,
            FontKind::Spleen8x16 => &FONT_SPLEEN,
        }
    }

    fn default_scale(self) -> usize {
        match self {
            FontKind::Vga8 => 2,
            FontKind::Terminus8x16 => 1,
            FontKind::Spleen8x16 => 1,
        }
    }
}

pub struct Console {
    fb: &'static mut [u8],
    info: FrameBufferInfo,
    font_kind: FontKind,
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
    cursor_style: CursorStyle,
    cursor_blink: CursorBlink,
    cursor_visible: bool,
    cursor_intensity: u8,
    cursor_color: u32,
    blink_timer: u16,
}

pub enum DrawPos {
    Char(usize, usize),
}

pub enum HudAlign {
    Left,
    Center,
    Right,
}

#[derive(Copy, Clone)]
pub enum CursorStyle {
    Underscore,
    Line,
    Block,
    Hidden,
}

#[derive(Copy, Clone)]
pub enum CursorBlink {
    None,
    Pulse,
    Fade,
}

impl Console {
    fn font(&self) -> &'static Font {
        self.font_kind.face()
    }

    fn char_w(&self) -> usize {
        self.font().width * self.scale
    }

    fn char_h(&self) -> usize {
        self.font().height * self.scale
    }

    fn recompute_dimensions(&mut self) {
        self.width = self.info.width / self.char_w();
        self.height = self.info.height / self.char_h();

        let reserved = self.reserved_hud_rows;
        if reserved > 0 {
            self.reserve_hud_rows(reserved);
        }

        let text_h = self.text_area_height();
        if self.width > 0 {
            self.cursor_x = self.cursor_x.min(self.width - 1);
        } else {
            self.cursor_x = 0;
        }
        if text_h > 0 {
            self.cursor_y = self.cursor_y.min(text_h - 1);
        } else {
            self.cursor_y = 0;
        }
    }

    pub fn from_boot_info(boot: &'static mut BootInfo) -> Option<Self> {
        let fb = boot.framebuffer.as_mut()?;
        let info = fb.info();
        let slice = fb.buffer_mut();
        let font_kind = FontKind::Vga8;
        let scale = font_kind.default_scale();
        let font = font_kind.face();
        let width = info.width / (font.width * scale);
        let height = info.height / (font.height * scale);
        Some(Self {
            fb: slice,
            info,
            font_kind,
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
            cursor_style: CursorStyle::Line,
            cursor_blink: CursorBlink::Pulse,
            cursor_visible: true,
            cursor_intensity: 255,
            cursor_color: 0xFFFFFF,
            blink_timer: 0,
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

    pub fn framebuffer_info(&self) -> &FrameBufferInfo {
        &self.info
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
        let font = self.font();
        let glyph = font.glyph(c);
        let s = self.scale;
        let base_px = x_char * font.width * s;
        let base_py = y_char * font.height * s;
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..font.width {
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
        let font = self.font();
        let glyph = font.glyph(c);
        let s = self.scale;
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..font.width {
                let bit = (bits >> (7 - col)) & 1;
                let px = x * font.width * s + col * s;
                let py = y * font.height * s + row * s;
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
        self.cursor_visible = true;
        self.cursor_intensity = 255;
        self.draw_cursor();
    }

    pub fn put_char(&mut self, c: char) {
        if c == '\n' {
            self.newline();
            return;
        }
        self.erase_cursor();
        self.draw_glyph(self.cursor_x, self.cursor_y, c, self.fg);
        self.cursor_x += 1;
        if self.cursor_x >= self.width {
            self.newline();
            return;
        }
        self.cursor_visible = true;
        self.cursor_intensity = 255;
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
        self.cursor_visible = true;
        self.cursor_intensity = 255;
        self.draw_cursor();
    }

    pub fn backspace(&mut self) {
        if self.width == 0 {
            return;
        }

        if self.cursor_x == 0 {
            if self.cursor_y == 0 {
                return;
            }
            self.erase_cursor();
            self.cursor_y -= 1;
            self.cursor_x = self.width - 1;
        } else {
            self.erase_cursor();
            self.cursor_x -= 1;
        }

        self.draw_glyph(self.cursor_x, self.cursor_y, ' ', self.bg);
        self.cursor_visible = true;
        self.cursor_intensity = 255;
        self.draw_cursor();
    }

    fn scroll(&mut self) {
        let char_h_px = self.char_h();
        let visible_rows = self.text_area_height();
        if visible_rows == 0 {
            return;
        }

        let visible_px = visible_rows * char_h_px;
        let bpp = self.info.bytes_per_pixel;
        let stride = self.info.stride;
        let shift = char_h_px * stride * bpp;
        let copy_bytes = visible_px * stride * bpp;

        if copy_bytes <= shift {
            return;
        }

        self.fb.copy_within(shift..copy_bytes, 0);
        let clear_py = visible_px.saturating_sub(char_h_px);
        self.fill_rect(0, clear_py, self.info.width, char_h_px, self.bg);
    }

    fn apply_intensity(color: u32, intensity: u8) -> u32 {
        if intensity >= 255 {
            return color;
        }
        if intensity == 0 {
            return 0;
        }
        let r = ((color >> 16) & 0xFF) as u8;
        let g = ((color >> 8) & 0xFF) as u8;
        let b = (color & 0xFF) as u8;
        let scale = intensity as u32;
        let r2 = (r as u32 * scale / 255) as u8;
        let g2 = (g as u32 * scale / 255) as u8;
        let b2 = (b as u32 * scale / 255) as u8;
        ((r2 as u32) << 16) | ((g2 as u32) << 8) | (b2 as u32)
    }

    fn draw_cursor(&mut self) {
        if !self.cursor_visible {
            return;
        }
        let s = self.scale;
        let font = self.font();
        let px = self.cursor_x * font.width * s;
        let py = self.cursor_y * font.height * s;
        let color = Self::apply_intensity(self.cursor_color, self.cursor_intensity);
        match self.cursor_style {
            CursorStyle::Underscore => {
                self.fill_rect(px, py + (font.height - 1) * s, font.width * s, s, color);
            }
            CursorStyle::Line => {
                self.fill_rect(px, py, 2 * s, font.height * s, color);
            }
            CursorStyle::Block => {
                self.fill_rect(px, py, font.width * s, font.height * s, color);
            }
            CursorStyle::Hidden => {}
        }
    }

    fn erase_cursor(&mut self) {
        let s = self.scale;
        let font = self.font();
        let px = self.cursor_x * font.width * s;
        let py = self.cursor_y * font.height * s;
        match self.cursor_style {
            CursorStyle::Underscore => {
                self.fill_rect(px, py + (font.height - 1) * s, font.width * s, s, self.bg);
            }
            CursorStyle::Line => {
                self.fill_rect(px, py, 2 * s, font.height * s, self.bg);
            }
            CursorStyle::Block => {
                self.fill_rect(px, py, font.width * s, font.height * s, self.bg);
            }
            CursorStyle::Hidden => {}
        }
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
            let hud_height_px = rows * self.char_h();
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
                let old_visible = self.cursor_visible;
                self.erase_cursor();
                let mut cx = x;
                for ch in s.chars() {
                    self.draw_glyph(cx, y, ch, self.fg);
                    cx += 1;
                }
                self.cursor_x = old_x;
                self.cursor_y = old_y;
                self.cursor_visible = old_visible;
                self.draw_cursor();
            }
        }
    }

    pub fn hud_begin(&mut self) {
        let (bg, info, stride_px) = (self.bg, &self.info, self.hud_back_stride);
        let hud_h_px = self.reserved_hud_rows * self.char_h();
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
        let (scale, info, bg, stride, rows, font) = (self.scale, &self.info, self.bg, self.hud_back_stride, self.reserved_hud_rows, self.font());
        let mut local_buf_opt = None;
        core::mem::swap(&mut self.hud_back, &mut local_buf_opt);
        if let Some(mut buf) = local_buf_opt {
            let char_w = font.width * scale;
            let text_chars = s.chars().count();
            let y_char = rows - 1;
            let x_char = match align {
                HudAlign::Left => 0,
                HudAlign::Center => ((info.width / char_w) / 2).saturating_sub(text_chars / 2),
                HudAlign::Right => (info.width / char_w).saturating_sub(text_chars),
            };
            let mut cx = x_char;
            for ch in s.chars() {
                Self::draw_glyph_into_static(&info, font, scale, &mut buf, stride, cx, y_char, ch, fg, bg);
                cx += 1;
            }
            self.hud_back = Some(buf);
        }
    }

    fn draw_glyph_into_static(
        info: &FrameBufferInfo,
        font: &Font,
        scale: usize,
        dst: &mut [u8],
        dst_stride_px: usize,
        x_char: usize,
        y_char: usize,
        c: char,
        fg: u32,
        bg: u32,
    ) {
        let glyph = font.glyph(c);
        let base_px = x_char * font.width * scale;
        let base_py = y_char * font.height * scale;
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..font.width {
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
        let hud_h_px = self.reserved_hud_rows * self.char_h();
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

    pub fn set_cursor_style(&mut self, style: CursorStyle) {
        self.erase_cursor();
        self.cursor_style = style;
        self.cursor_visible = !matches!(style, CursorStyle::Hidden);
        self.cursor_intensity = 255;
        self.draw_cursor();
    }

    pub fn set_cursor_blink(&mut self, blink: CursorBlink) {
        self.erase_cursor();
        self.cursor_blink = blink;
        self.cursor_intensity = 255;
        self.cursor_visible = true;
        self.blink_timer = 0;
        self.draw_cursor();
    }

    pub fn set_cursor_color(&mut self, color: u32) {
        self.cursor_color = color;
    }

    pub fn set_font(&mut self, kind: FontKind) {
        if self.font_kind == kind {
            return;
        }
        self.font_kind = kind;
        self.scale = kind.default_scale();
        self.recompute_dimensions();
        self.clear();
    }

    pub fn current_font(&self) -> FontKind {
        self.font_kind
    }

    pub fn set_default_fg(&mut self, fg: u32) {
        self.fg = fg;
        self.cursor_color = fg;
    }

    pub fn set_default_bg(&mut self, bg: u32) {
        if self.bg == bg {
            return;
        }
        self.bg = bg;
        self.clear();
    }

    pub fn set_default_colors(&mut self, fg: u32, bg: u32) {
        let bg_changed = self.bg != bg;
        self.fg = fg;
        self.bg = bg;
        self.cursor_color = fg;
        if bg_changed {
            self.clear();
        }
    }

    pub fn default_colors(&self) -> (u32, u32) {
        (self.fg, self.bg)
    }

    pub fn tick(&mut self) {
        match self.cursor_blink {
            CursorBlink::None => {
                self.cursor_visible = true;
                self.cursor_intensity = 255;
            }
            CursorBlink::Pulse => {
                self.blink_timer = self.blink_timer.wrapping_add(1);
                if self.blink_timer % 60 == 0 {
                    self.cursor_visible = !self.cursor_visible;
                }
            }
            CursorBlink::Fade => {
                self.blink_timer = self.blink_timer.wrapping_add(1);
                let t = self.blink_timer as f32;
                let x = (t / 240.0) * core::f32::consts::PI * 2.0;
                let v = ((1.0 - libm::cosf(x)) * 0.5) * 255.0;
                self.cursor_intensity = v as u8;
                self.cursor_visible = self.cursor_intensity > 4;
            }
        }
        self.erase_cursor();
        self.draw_cursor();
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

pub fn set_cursor_style(style: CursorStyle) {
    with_console(|c| c.set_cursor_style(style));
}

pub fn set_cursor_blink(blink: CursorBlink) {
    with_console(|c| c.set_cursor_blink(blink));
}

pub fn set_cursor_color(color: u32) {
    with_console(|c| c.set_cursor_color(color));
}

pub fn set_font(kind: FontKind) {
    with_console(|c| c.set_font(kind));
}

pub fn current_font() -> FontKind {
    with_console(|c| c.current_font())
}

pub fn set_default_fg(color: u32) {
    with_console(|c| c.set_default_fg(color));
}

pub fn set_default_bg(color: u32) {
    with_console(|c| c.set_default_bg(color));
}

pub fn set_default_colors(fg: u32, bg: u32) {
    with_console(|c| c.set_default_colors(fg, bg));
}

pub fn default_colors() -> (u32, u32) {
    with_console(|c| c.default_colors())
}

pub fn default_fg() -> u32 {
    default_colors().0
}

pub fn default_bg() -> u32 {
    default_colors().1
}

pub fn size_chars() -> (usize, usize) {
    with_console(|c| (c.width, c.text_area_height()))
}

pub fn tick() {
    with_console(|c| c.tick());
}
