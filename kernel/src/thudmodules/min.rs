#![allow(dead_code)]

use heapless::String as HString;
use crate::thud::{HudModule, register};
use crate::console::HudAlign;
use alloc::boxed::Box;
use crate::memory::memory_overview;

pub struct Mem;

impl HudModule for Mem {
    fn name(&self) -> &'static str { "mem" }

    fn alignment(&self) -> HudAlign { HudAlign::Center }

    fn update(&mut self) {
    }

    fn render(&self) -> HString<64> {
        let mo = memory_overview();

        let used = mo.kernel_heap.used as u64;
        let total = mo.kernel_heap.total as u64;

        let used_str = format_bytes::<32>(used);
        let total_str = format_bytes::<32>(total);

        let mut out: HString<64> = HString::new();
        let _ = out.push_str("Heap: ");
        let _ = out.push_str(used_str.as_str());
        let _ = out.push_str(" / ");
        let _ = out.push_str(total_str.as_str());
        out
    }
}

pub fn init() {
    register(Box::new(Mem));
}

fn format_bytes<const N: usize>(b: u64) -> HString<N> {
    use core::fmt::Write;
    let mut s = HString::<N>::new();
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if b >= GB {
        let _ = write!(s, "{:.1} GiB", b as f64 / GB as f64);
    } else if b >= MB {
        let _ = write!(s, "{:.1} MiB", b as f64 / MB as f64);
    } else if b >= KB {
        let _ = write!(s, "{:.1} KiB", b as f64 / KB as f64);
    } else {
        let _ = write!(s, "{} B", b);
    }
    s
}
