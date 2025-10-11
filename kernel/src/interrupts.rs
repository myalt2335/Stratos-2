use x86_64::structures::idt::InterruptDescriptorTable;
use lazy_static::lazy_static;

use crate::timer;

lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();

        idt[32].set_handler_fn(timer::timer_interrupt_handler);

        idt
    };
}

pub fn init_idt() {
    IDT.load();
}
