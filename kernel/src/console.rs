#![allow(dead_code)]
#![allow(unused_variables)]

extern crate alloc;
use bootloader_api::info::{FrameBufferInfo, PixelFormat};
use bootloader_api::BootInfo;
use core::mem::MaybeUninit;
use core::ptr::{self, addr_of_mut};
use spin::Mutex;
use x86_64::instructions::interrupts;
use crate::font::VGA8_FONT;
use crate::font2::TERMINUS_FONT;
use crate::font3::SPLEEN_FONT;
use crate::wait;

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
    back_buffer: &'static mut [u8],
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
    dirty: Option<(usize, usize, usize, usize)>,
    cursor_style: CursorStyle,
    cursor_blink: CursorBlink,
    cursor_visible: bool,
    cursor_intensity: u8,
    cursor_color: u32,
    blink_timer: u16,
    cursor_saved: Option<CursorSave>,
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

const MAX_BACKBUFFER_BYTES: usize = 32 * 1024 * 1024;
static mut BACK_BUFFER_STORAGE: MaybeUninit<[u8; MAX_BACKBUFFER_BYTES]> = MaybeUninit::uninit();
static mut SNAPSHOT_STORAGE: MaybeUninit<[u8; MAX_BACKBUFFER_BYTES]> = MaybeUninit::uninit();
// Snapshot of the pixels under the cursor so we can draw over existing text without losing it.
const CURSOR_SNAPSHOT_MAX: usize = 8192;

struct CursorSave {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    len: usize,
    data: [u8; CURSOR_SNAPSHOT_MAX],
}

fn alloc_back_buffer(len: usize) -> Option<&'static mut [u8]> {
    if len > MAX_BACKBUFFER_BYTES {
        return None;
    }
    unsafe {
        let ptr = addr_of_mut!(BACK_BUFFER_STORAGE) as *mut u8;
        Some(core::slice::from_raw_parts_mut(ptr, len))
    }
}

#[derive(Copy, Clone)]
pub struct DisplayBufferStats {
    pub framebuffer_bytes: usize,
    pub backbuffer_bytes: usize,
    pub width_px: usize,
    pub height_px: usize,
    pub stride_px: usize,
    pub bytes_per_pixel: usize,
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

    fn mark_dirty(&mut self, x: usize, y: usize, w: usize, h: usize) {
        if w == 0 || h == 0 {
            return;
        }
        let max_x = self.info.width;
        let max_y = self.info.height;
        if x >= max_x || y >= max_y {
            return;
        }
        let x1 = (x + w).min(max_x);
        let y1 = (y + h).min(max_y);
        if x1 <= x || y1 <= y {
            return;
        }
        match self.dirty {
            Some((dx0, dy0, dx1, dy1)) => {
                self.dirty = Some((dx0.min(x), dy0.min(y), dx1.max(x1), dy1.max(y1)));
            }
            None => {
                self.dirty = Some((x, y, x1, y1));
            }
        }
    }

    fn present(&mut self) {
        if let Some((x0, y0, x1, y1)) = self.dirty.take() {
            self.present_rect(x0, y0, x1 - x0, y1 - y0);
        }
    }

    fn present_rect(&mut self, x: usize, y: usize, w: usize, h: usize) {
        if w == 0 || h == 0 {
            return;
        }
        let max_x = self.info.width;
        let max_y = self.info.height;
        if x >= max_x || y >= max_y {
            return;
        }
        let x1 = (x + w).min(max_x);
        let y1 = (y + h).min(max_y);
        let bpp = self.info.bytes_per_pixel;
        let stride = self.info.stride;
        for row in y..y1 {
            let off = (row * stride + x) * bpp;
            let len = (x1 - x) * bpp;
            let src = &self.back_buffer[off..off + len];
            let dst = &mut self.fb[off..off + len];
            dst.copy_from_slice(src);
        }
    }

    fn present_full(&mut self) {
        self.present_rect(0, 0, self.info.width, self.info.height);
        self.dirty = None;
    }

    fn buffer_stats(&self) -> DisplayBufferStats {
        DisplayBufferStats {
            framebuffer_bytes: self.fb.len(),
            backbuffer_bytes: self.back_buffer.len(),
            width_px: self.info.width,
            height_px: self.info.height,
            stride_px: self.info.stride,
            bytes_per_pixel: self.info.bytes_per_pixel,
        }
    }

    pub fn from_boot_info(boot: &'static mut BootInfo) -> Option<Self> {
        let fb = boot.framebuffer.as_mut()?;
        let info = fb.info();
        let slice = fb.buffer_mut();
        let back_buffer = alloc_back_buffer(slice.len())?;
        back_buffer.copy_from_slice(slice);
        let font_kind = FontKind::Vga8;
        let scale = font_kind.default_scale();
        let font = font_kind.face();
        let width = info.width / (font.width * scale);
        let height = info.height / (font.height * scale);
        Some(Self {
            fb: slice,
            back_buffer,
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
            dirty: None,
            cursor_style: CursorStyle::Line,
            cursor_blink: CursorBlink::Pulse,
            cursor_visible: true,
            cursor_intensity: 255,
            cursor_color: 0xFFFFFF,
            blink_timer: 0,
            cursor_saved: None,
        })
    }

    fn write_pixel_to_back(&mut self, off: usize, color: u32) {
        let r = ((color >> 16) & 0xFF) as u8;
        let g = ((color >> 8) & 0xFF) as u8;
        let b = (color & 0xFF) as u8;
        match (self.info.pixel_format, self.info.bytes_per_pixel) {
            (PixelFormat::Rgb, 4) => {
                self.back_buffer[off] = r;
                self.back_buffer[off + 1] = g;
                self.back_buffer[off + 2] = b;
                self.back_buffer[off + 3] = 0xFF;
            }
            (PixelFormat::Rgb, 3) => {
                self.back_buffer[off] = r;
                self.back_buffer[off + 1] = g;
                self.back_buffer[off + 2] = b;
            }
            (PixelFormat::Bgr, 4) => {
                self.back_buffer[off] = b;
                self.back_buffer[off + 1] = g;
                self.back_buffer[off + 2] = r;
                self.back_buffer[off + 3] = 0xFF;
            }
            (PixelFormat::Bgr, 3) => {
                self.back_buffer[off] = b;
                self.back_buffer[off + 1] = g;
                self.back_buffer[off + 2] = r;
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
        let base_px = x * font.width * s;
        let base_py = y * font.height * s;
        self.mark_dirty(base_px, base_py, font.width * s, font.height * s);
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..font.width {
                let bit = (bits >> (7 - col)) & 1;
                let px = base_px + col * s;
                let py = base_py + row * s;
                let pix = if bit == 1 { color } else { self.bg };
                self.fill_rect_raw(px, py, s, s, pix);
            }
        }
    }

    fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: u32) {
        self.mark_dirty(x, y, w, h);
        self.fill_rect_raw(x, y, w, h, color);
    }

    fn fill_rect_raw(&mut self, x: usize, y: usize, w: usize, h: usize, color: u32) {
        let bytes_per_pixel = self.info.bytes_per_pixel;
        let stride = self.info.stride;
        for dy in 0..h {
            let py = y + dy;
            if py >= self.info.height {
                break;
            }
            for dx in 0..w {
                let px = x + dx;
                if px >= self.info.width {
                    break;
                }
                let off = (py * stride + px) * bytes_per_pixel;
                self.write_pixel_to_back(off, color);
            }
        }
    }

    pub fn clear(&mut self) {
        self.erase_cursor();
        self.fill_rect(0, 0, self.info.width, self.info.height, self.bg);
        self.cursor_x = 0;
        self.cursor_y = 0;
        self.cursor_visible = true;
        self.cursor_intensity = 255;
        self.draw_cursor();
        self.present();
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
        self.present();
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
        self.present();
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
        self.present();
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

        self.back_buffer.copy_within(shift..copy_bytes, 0);
        self.mark_dirty(0, 0, self.info.width, visible_px);
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

    fn cursor_rect(&self) -> Option<(usize, usize, usize, usize)> {
        let s = self.scale;
        let font = self.font();
        let px = self.cursor_x * font.width * s;
        let py = self.cursor_y * font.height * s;
        match self.cursor_style {
            CursorStyle::Underscore => Some((px, py + (font.height - 1) * s, font.width * s, s)),
            CursorStyle::Line => Some((px, py, 2 * s, font.height * s)),
            CursorStyle::Block => Some((px, py, font.width * s, font.height * s)),
            CursorStyle::Hidden => None,
        }
    }

    fn save_cursor_area(&mut self, x: usize, y: usize, w: usize, h: usize) {
        let bpp = self.info.bytes_per_pixel;
        let stride = self.info.stride;
        let max_w = self.info.width.saturating_sub(x);
        let copy_w = w.min(max_w);
        if copy_w == 0 || h == 0 {
            self.cursor_saved = None;
            return;
        }
        let max_bytes = copy_w
            .saturating_mul(h)
            .saturating_mul(bpp);
        if max_bytes == 0 || max_bytes > CURSOR_SNAPSHOT_MAX {
            self.cursor_saved = None;
            return;
        }
        let mut snap = CursorSave { x, y, w: copy_w, h, len: 0, data: [0; CURSOR_SNAPSHOT_MAX] };
        for row in 0..h {
            let py = y + row;
            if py >= self.info.height {
                break;
            }
            let off = (py * stride + x) * bpp;
            let row_bytes = copy_w * bpp;
            let src_end = off + row_bytes;
            let dst_end = snap.len + row_bytes;
            if src_end > self.back_buffer.len() || dst_end > snap.data.len() {
                break;
            }
            snap.data[snap.len..dst_end].copy_from_slice(&self.back_buffer[off..src_end]);
            snap.len = dst_end;
        }
        self.cursor_saved = if snap.len > 0 { Some(snap) } else { None };
    }

    fn restore_cursor_area(&mut self) {
        if let Some(save) = self.cursor_saved.take() {
            let bpp = self.info.bytes_per_pixel;
            let stride = self.info.stride;
            let mut src_off = 0;
            let copy_w = save.w.min(self.info.width.saturating_sub(save.x));
            if copy_w == 0 || save.h == 0 {
                return;
            }
            for row in 0..save.h {
                if src_off >= save.len {
                    break;
                }
                let py = save.y + row;
                if py >= self.info.height {
                    break;
                }
                let row_bytes = copy_w * bpp;
                let dst_off = (py * stride + save.x) * bpp;
                let dst_end = dst_off + row_bytes;
                let src_end = src_off + row_bytes;
                if dst_end > self.back_buffer.len() || src_end > save.len {
                    break;
                }
                self.back_buffer[dst_off..dst_end].copy_from_slice(&save.data[src_off..src_end]);
                src_off = src_end;
            }
            self.mark_dirty(save.x, save.y, copy_w, save.h);
        }
    }

    fn draw_cursor(&mut self) {
        if !self.cursor_visible {
            return;
        }
        if let Some((px, py, w, h)) = self.cursor_rect() {
            let color = Self::apply_intensity(self.cursor_color, self.cursor_intensity);
            self.save_cursor_area(px, py, w, h);
            if self.cursor_saved.is_none() {
                return;
            }
            self.mark_dirty(px, py, w, h);
            self.fill_rect_raw(px, py, w, h, color);
        }
    }

    fn erase_cursor(&mut self) {
        self.restore_cursor_area();
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

    pub fn cursor_position(&self) -> (usize, usize) {
        (self.cursor_x, self.cursor_y)
    }

    pub fn move_cursor_to(&mut self, x: usize, y: usize) {
        self.erase_cursor();
        let max_x = self.width.saturating_sub(1);
        let max_y = self.text_area_height().saturating_sub(1);
        self.cursor_x = x.min(max_x);
        self.cursor_y = y.min(max_y);
        self.cursor_visible = true;
        self.cursor_intensity = 255;
        self.draw_cursor();
        self.present();
    }

    pub fn render_line_at(
        &mut self,
        origin_x: usize,
        origin_y: usize,
        content: &str,
        prev_render_len: usize,
        cursor_offset: usize,
    ) -> usize {
        self.erase_cursor();
        let max_x = self.width;
        let max_y = self.text_area_height();
        if max_x == 0 || max_y == 0 {
            return 0;
        }
        let origin_x = origin_x.min(max_x - 1);
        let y = origin_y.min(max_y - 1);
        let mut x = origin_x;
        let mut drawn = 0;
        for ch in content.chars() {
            if x >= max_x {
                break;
            }
            self.draw_glyph(x, y, ch, self.fg);
            x += 1;
            drawn += 1;
        }
        let trailing = prev_render_len.saturating_sub(drawn);
        for _ in 0..trailing {
            if x >= max_x {
                break;
            }
            self.draw_glyph(x, y, ' ', self.bg);
            x += 1;
        }
        self.cursor_x = origin_x.saturating_add(cursor_offset).min(max_x - 1);
        self.cursor_y = y;
        self.cursor_visible = true;
        self.cursor_intensity = 255;
        self.draw_cursor();
        self.present();
        drawn
    }

    pub fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    pub fn reserve_hud_rows(&mut self, rows: usize) {
        let rows = rows.min(self.height);
        self.reserved_hud_rows = rows;
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
        self.present();
    }

    pub fn hud_begin(&mut self) {
        if self.reserved_hud_rows == 0 {
            return;
        }
        let hud_h_px = self.reserved_hud_rows * self.char_h();
        let start_y = self.info.height.saturating_sub(hud_h_px);
        self.fill_rect(0, start_y, self.info.width, hud_h_px, self.bg);
    }

    pub fn hud_draw_text(&mut self, s: &str, fg: u32, align: HudAlign) {
        if self.reserved_hud_rows == 0 { return; }
        let scale = self.scale;
        let font = self.font();
        let char_w = font.width * scale;
        if char_w == 0 {
            return;
        }
        let text_chars = s.chars().count();
        let y_char = self.height.saturating_sub(self.reserved_hud_rows);
        let x_char = match align {
            HudAlign::Left => 0,
            HudAlign::Center => ((self.info.width / char_w) / 2).saturating_sub(text_chars / 2),
            HudAlign::Right => (self.info.width / char_w).saturating_sub(text_chars),
        };
        let mut cx = x_char;
        for ch in s.chars() {
            self.draw_glyph(cx, y_char, ch, fg);
            cx += 1;
        }
    }

    pub fn hud_present(&mut self) {
        if self.reserved_hud_rows == 0 { return; }
        self.present();
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
        self.present();
    }

    pub fn set_cursor_blink(&mut self, blink: CursorBlink) {
        self.erase_cursor();
        self.cursor_blink = blink;
        self.cursor_intensity = 255;
        self.cursor_visible = true;
        self.blink_timer = 0;
        self.draw_cursor();
        self.present();
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
        self.present();
    }

    fn blit_image_scaled(&mut self, image: &[u8], img_w: usize, img_h: usize, channels: usize) {
        if img_w == 0 || img_h == 0 {
            return;
        }
        if channels != 3 && channels != 4 {
            return;
        }
        if image.len() < img_w.saturating_mul(img_h).saturating_mul(channels) {
            return;
        }

        // Clear to black before drawing the image.
        self.fill_rect_raw(0, 0, self.info.width, self.info.height, 0x000000);

        let scale_x = self.info.width as f32 / img_w as f32;
        let scale_y = self.info.height as f32 / img_h as f32;
        let mut scale = scale_x.min(scale_y);
        if scale > 1.0 {
            scale = 1.0;
        }
        if scale <= 0.0 {
            return;
        }

        let target_w = libm::roundf(img_w as f32 * scale) as usize;
        let target_h = libm::roundf(img_h as f32 * scale) as usize;
        if target_w == 0 || target_h == 0 {
            return;
        }

        let offset_x = (self.info.width.saturating_sub(target_w)) / 2;
        let offset_y = (self.info.height.saturating_sub(target_h)) / 2;
        let bpp = self.info.bytes_per_pixel;
        let stride = self.info.stride;

        for ty in 0..target_h {
            let sy = ty * img_h / target_h;
            for tx in 0..target_w {
                let sx = tx * img_w / target_w;
                let src_idx = (sy * img_w + sx) * channels;
                if src_idx + (channels - 1) >= image.len() {
                    continue;
                }
                let r = image[src_idx] as u32;
                let g = image[src_idx + 1] as u32;
                let b = image[src_idx + 2] as u32;
                let dst_x = offset_x + tx;
                let dst_y = offset_y + ty;
                if dst_x >= self.info.width || dst_y >= self.info.height {
                    continue;
                }
                let off = (dst_y * stride + dst_x) * bpp;
                self.write_pixel_to_back(off, (r << 16) | (g << 8) | b);
            }
        }

        self.mark_dirty(0, 0, self.info.width, self.info.height);
        self.present_full();
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

pub fn render_line_at(
    origin_x: usize,
    origin_y: usize,
    content: &str,
    prev_render_len: usize,
    cursor_offset: usize,
) -> usize {
    with_console(|c| c.render_line_at(origin_x, origin_y, content, prev_render_len, cursor_offset))
}

pub fn tick() {
    with_console(|c| c.tick());
}

pub fn display_buffer_stats() -> Option<DisplayBufferStats> {
    interrupts::without_interrupts(|| {
        let lock = CONSOLE.lock();
        lock.as_ref().map(|c| c.buffer_stats())
    })
}

fn infer_image_dims(image: &[u8], fb_w: usize, fb_h: usize) -> Option<(usize, usize, usize)> {
    let channels = if image.len() % 4 == 0 { 4 } else if image.len() % 3 == 0 { 3 } else { return None };
    let total_px = image.len() / channels;
    let target_aspect = fb_w as f32 / fb_h as f32;
    let mut best: Option<(usize, usize, f32)> = None;
    let limit = libm::sqrtf(total_px as f32) as usize + 1;
    for w in 1..=limit {
        if total_px % w != 0 {
            continue;
        }
        let h = total_px / w;
        let aspect = w as f32 / h as f32;
        let diff = libm::fabsf(aspect - target_aspect);
        match best {
            None => best = Some((w, h, diff)),
            Some((_, _, best_diff)) if diff < best_diff => best = Some((w, h, diff)),
            _ => {}
        }
    }
    best.map(|(w, h, _)| (w, h, channels))
}

pub fn showimage(image: &[u8], width: usize, height: usize, seconds: u64) {
    let (snapshot_len, prev_style, prev_visible, prev_blink) = interrupts::without_interrupts(|| {
        let mut lock = CONSOLE.lock();
        let con = lock.as_mut().expect("Console not init");
        let len = con.back_buffer.len();
        if len > MAX_BACKBUFFER_BYTES {
            return (0, con.cursor_style, con.cursor_visible, con.cursor_blink);
        }
        let prev_cursor_style = con.cursor_style;
        let prev_cursor_visible = con.cursor_visible;
        let prev_cursor_blink = con.cursor_blink;

        con.erase_cursor();
        con.cursor_style = CursorStyle::Hidden;
        con.cursor_visible = false;
        con.cursor_blink = CursorBlink::None;

        let snap_ptr = addr_of_mut!(SNAPSHOT_STORAGE) as *mut u8;
        unsafe { ptr::copy_nonoverlapping(con.back_buffer.as_ptr(), snap_ptr, len); }
        let (w, h, channels) = if width.saturating_mul(height).saturating_mul(4) == image.len() {
            (width, height, 4)
        } else if width.saturating_mul(height).saturating_mul(3) == image.len() {
            (width, height, 3)
        } else {
            infer_image_dims(image, con.info.width, con.info.height).unwrap_or((0, 0, 0))
        };
        if w == 0 || h == 0 || channels == 0 {
            return (0, prev_cursor_style, prev_cursor_visible, prev_cursor_blink);
        }
        con.blit_image_scaled(image, w, h, channels);
        (len, prev_cursor_style, prev_cursor_visible, prev_cursor_blink)
    });

    if snapshot_len == 0 {
        return;
    }

    wait::bsec(seconds);

    interrupts::without_interrupts(|| {
        let mut lock = CONSOLE.lock();
        if let Some(con) = lock.as_mut() {
            let len = core::cmp::min(snapshot_len, con.back_buffer.len());
            let snap_ptr = core::ptr::addr_of!(SNAPSHOT_STORAGE) as *const u8;
            unsafe { ptr::copy_nonoverlapping(snap_ptr, con.back_buffer.as_mut_ptr(), len); }
            con.cursor_style = prev_style;
            con.cursor_visible = prev_visible;
            con.cursor_blink = prev_blink;
            con.cursor_intensity = 255;
            con.draw_cursor();
            con.present_full();
        }
    });
}
