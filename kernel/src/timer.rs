#![allow(dead_code)]

use x86_64::instructions::port::Port;
use x86_64::structures::idt::InterruptStackFrame;

use crate::{commands, time};

const PIT_FREQUENCY: u32 = 1193182;
const DESIRED_FREQUENCY: u32 = 100;
const PIT_COMMAND_PORT: u16 = 0x43;
const PIT_CHANNEL0_PORT: u16 = 0x40;

pub fn init_pit() {
    let divisor: u16 = (PIT_FREQUENCY / DESIRED_FREQUENCY) as u16;

    unsafe {
        let mut cmd: Port<u8> = Port::new(PIT_COMMAND_PORT);
        let mut data: Port<u8> = Port::new(PIT_CHANNEL0_PORT);

        cmd.write(0x36);

        data.write((divisor & 0xFF) as u8);
        data.write((divisor >> 8) as u8);
    }
}

static mut SUBSECOND_TICKS: u64 = 0;
static mut TICKS: u64 = 0;

pub extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
    commands::tick();

    unsafe {
        TICKS = TICKS.wrapping_add(1);
        SUBSECOND_TICKS = SUBSECOND_TICKS.wrapping_add(1);

        if SUBSECOND_TICKS >= (DESIRED_FREQUENCY as u64) {
            SUBSECOND_TICKS = 0;
            time::tick_second();
        }
    }

    crate::thud::on_100hz_tick();
    crate::thud::poll_draw();

    unsafe {
        let mut port = Port::<u8>::new(0x20);
        port.write(0x20);
    }
}

pub fn ticks() -> u64 {
    unsafe { TICKS }
}

pub fn seconds() -> u64 {
    unsafe { TICKS / (DESIRED_FREQUENCY as u64) }
}

pub fn frequency() -> u32 {
    DESIRED_FREQUENCY
}