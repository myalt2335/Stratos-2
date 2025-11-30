#![allow(dead_code)]

use heapless::String as HString;
use crate::{thud::{HudModule, register}, console::HudAlign};
use alloc::boxed::Box;

extern "C" {
    static mut TICKS: u64;
}

pub struct UptimeHud;

impl HudModule for UptimeHud {
    fn name(&self) -> &'static str { "uptime" }

    fn alignment(&self) -> HudAlign { HudAlign::Left }

    fn update(&mut self) {}

    fn render(&self) -> HString<64> {
        let mut out: HString<64> = HString::new();

        let _ = out.push_str("Uptime: ");

        let (hours, mins, secs) = unsafe {
            let secs = TICKS / 100;
            let mins = secs / 60;
            let hours = mins / 60;
            (hours, mins % 60, secs % 60)
        };

        push_2digits(&mut out, hours as u32);
        let _ = out.push(':');
        push_2digits(&mut out, mins as u32);
        let _ = out.push(':');
        push_2digits(&mut out, secs as u32);

        out
    }
}

pub fn init() {
    register(Box::new(UptimeHud));
}

fn push_2digits(out: &mut HString<64>, v: u32) {
    let tens = ((v % 100) / 10) as u8;
    let ones = (v % 10) as u8;

    let _ = out.push(char::from(b'0' + tens));
    let _ = out.push(char::from(b'0' + ones));
}
