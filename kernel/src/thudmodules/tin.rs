#![allow(dead_code)]

use heapless::String as HString;
use crate::{thud::{HudModule, register}, time};
use crate::console::HudAlign;
use alloc::boxed::Box;

pub struct Tin;

impl HudModule for Tin {
    fn name(&self) -> &'static str { "tin" }

    fn alignment(&self) -> HudAlign { HudAlign::Right }

    fn update(&mut self) {
    }

    fn render(&self) -> HString<64> {
        let mut out: HString<64> = HString::new();
        let s = time::format_hud_time();
        let _ = out.push_str(s.as_str());
        out
    }
}

pub fn init() {
    register(Box::new(Tin));
}
