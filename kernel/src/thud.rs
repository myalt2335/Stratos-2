#![allow(dead_code)]

use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;
use heapless::{String as HString, Vec};
use crate::console::{with_console, HudAlign};
use alloc::boxed::Box;
use core::fmt::Write;

pub trait HudModule {
    fn name(&self) -> &'static str;
    fn alignment(&self) -> HudAlign { HudAlign::Right }
    fn update(&mut self);
    fn render(&self) -> HString<64>;
}

static ENABLED: AtomicBool = AtomicBool::new(false);
static NEEDS_REDRAW: AtomicBool = AtomicBool::new(false);
static MODULES: Mutex<Vec<Box<dyn HudModule + Send>, 8>> = Mutex::new(Vec::new());

static mut TICK_COUNT: u64 = 0;

pub fn init() {
}

pub fn register(module: Box<dyn HudModule + Send>) {
    let mut mods = MODULES.lock();
    if mods.len() < mods.capacity() {
        mods.push(module).ok();
    }
}

pub fn enable() {
    ENABLED.store(true, Ordering::Release);
    NEEDS_REDRAW.store(true, Ordering::Release);
}

pub fn disable() {
    ENABLED.store(false, Ordering::Release);
    with_console(|c| c.clear_hud_row());
}

pub fn on_100hz_tick() {
    if !ENABLED.load(Ordering::Relaxed) { return; }
    unsafe {
        TICK_COUNT = TICK_COUNT.wrapping_add(1);
        if TICK_COUNT % 100 == 0 {
            NEEDS_REDRAW.store(true, Ordering::Release);
            poll_draw();
        }
    }
}

pub fn poll_draw() {
    if !ENABLED.load(Ordering::Acquire) { return; }
    if !NEEDS_REDRAW.swap(false, Ordering::AcqRel) { return; }

    let mut left_buf = HString::<128>::new();
    let mut center_buf = HString::<128>::new();
    let mut right_buf = HString::<128>::new();

    let mut modules = MODULES.lock();
    for m in modules.iter_mut() {
        m.update();
        let part = m.render();
        match m.alignment() {
            HudAlign::Left => { let _ = write!(left_buf, "{}  ", part); }
            HudAlign::Center => { let _ = write!(center_buf, "{}  ", part); }
            HudAlign::Right => { let _ = write!(right_buf, "{}  ", part); }
        }
    }

    trim_trailing_ws(&mut left_buf);
    trim_trailing_ws(&mut center_buf);
    trim_trailing_ws(&mut right_buf);

    with_console(|c| {
        let (fg, _) = c.default_colors();
        c.hud_begin();
        c.hud_draw_text(left_buf.as_str(), fg, HudAlign::Left);
        c.hud_draw_text(center_buf.as_str(), fg, HudAlign::Center);
        c.hud_draw_text(right_buf.as_str(), fg, HudAlign::Right);
        c.hud_present();
    });
}

fn trim_trailing_ws(s: &mut HString<128>) {
    while let Some(ch) = s.chars().rev().next() {
        if ch.is_whitespace() {
            s.pop();
        } else {
            break;
        }
    }
}
