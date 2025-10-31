#![allow(dead_code)]

use x86_64::instructions::hlt;
use crate::timer;

static mut INITIALIZED: bool = false;

pub fn init() {
    unsafe {
        if INITIALIZED {
            return;
        }
        INITIALIZED = true;
    }
}

pub fn bsec(seconds: u64) {
    let start = timer::ticks();
    let end = start + seconds * timer::frequency() as u64;
    while timer::ticks() < end {
        hlt();
    }
}

pub fn bms(ms: u64) {
    let start = timer::ticks();
    let ticks = (ms * timer::frequency() as u64) / 1000;
    while timer::ticks() - start < ticks {
        hlt();
    }
}

pub struct Wait {
    target_tick: u64,
}

impl Wait {
    pub fn sec(seconds: u64) -> Self {
        Self {
            target_tick: timer::ticks() + seconds * timer::frequency() as u64,
        }
    }

    pub fn ms(ms: u64) -> Self {
        Self {
            target_tick: timer::ticks() + (ms * timer::frequency() as u64) / 1000,
        }
    }

    pub fn done(&self) -> bool {
        timer::ticks() >= self.target_tick
    }

    pub fn remaining(&self) -> u64 {
        self.target_tick.saturating_sub(timer::ticks())
    }
}
