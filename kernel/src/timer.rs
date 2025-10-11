use x86_64::instructions::port::Port;
use x86_64::structures::idt::InterruptStackFrame;

use crate::commands;

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

pub extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
    commands::tick();

    unsafe {
        let mut port = Port::<u8>::new(0x20);
        port.write(0x20);
    }
}
