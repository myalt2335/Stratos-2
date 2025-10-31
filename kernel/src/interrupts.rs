#![allow(dead_code)]
#![allow(unused_variables)]

use lazy_static::lazy_static;
use x86_64::{
    instructions::hlt,
    registers::control::Cr2,
    structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode},
};
use crate::{console, timer, serial};

use alloc::format;

use alloc::string::String;
use core::fmt::Write;

pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();

        idt.divide_error.set_handler_fn(exc_divide_error);
        idt.debug.set_handler_fn(exc_debug);
        idt.non_maskable_interrupt.set_handler_fn(exc_nmi);
        idt.breakpoint.set_handler_fn(exc_breakpoint);
        idt.overflow.set_handler_fn(exc_overflow);
        idt.bound_range_exceeded.set_handler_fn(exc_bound);
        idt.invalid_opcode.set_handler_fn(exc_invalid_opcode);
        idt.device_not_available.set_handler_fn(exc_device_na);
        unsafe{
        idt.double_fault
            .set_handler_fn(exc_double_fault)
            .set_stack_index(DOUBLE_FAULT_IST_INDEX);
        }
        idt.invalid_tss.set_handler_fn(exc_invalid_tss);
        idt.segment_not_present.set_handler_fn(exc_segment_not_present);
        idt.stack_segment_fault.set_handler_fn(exc_stack_fault);
        idt.general_protection_fault.set_handler_fn(exc_gpf);
        idt.page_fault.set_handler_fn(exc_page_fault);
        idt.alignment_check.set_handler_fn(exc_alignment_check);

        idt.x87_floating_point.set_handler_fn(exc_default);
        idt.machine_check.set_handler_fn(exc_machine_check);
        idt.simd_floating_point.set_handler_fn(exc_default);
        idt.virtualization.set_handler_fn(exc_default);

        idt[32].set_handler_fn(timer::timer_interrupt_handler);

        idt
    };
}

pub fn init_idt() {
    IDT.load();
}

fn print_line(msg: &str) {
    console::cwrite_line(msg, 0xFFFFFF, 0x000000);
}

fn strip_ascii_whitespace(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for b in s.bytes() {
        if !b.is_ascii_whitespace() {
            let _ = result.write_char(b as char);
        }
    }
    result
}

fn print_err(msg: &str) {
    console::cwrite_line(msg, 0xFF0000, 0x000000);
}

macro_rules! simple_exc {
    ($name:ident, $msg:expr) => {
        extern "x86-interrupt" fn $name(stack_frame: InterruptStackFrame) {
            print_err("=== CPU EXCEPTION ===");
            print_line(concat!($msg, " detected, halting..."));
            print_line(&format!("{:#?}", stack_frame));
            loop { hlt(); }
        }
    };
}

simple_exc!(exc_divide_error, "#DE Divide Error");
simple_exc!(exc_debug, "#DB Debug");
simple_exc!(exc_nmi, "Non-Maskable Interrupt");
simple_exc!(exc_bound, "BOUND Range Exceeded");
simple_exc!(exc_invalid_opcode, "#UD Invalid Opcode");
simple_exc!(exc_device_na, "Device Not Available");
simple_exc!(exc_default, "Unknown/Reserved Exception");

macro_rules! errcode_exc {
    ($name:ident, $msg:expr) => {
        extern "x86-interrupt" fn $name(stack_frame: InterruptStackFrame, _error_code: u64) {
            print_err("=== CPU EXCEPTION ===");
            print_line(concat!($msg, " detected, halting..."));
            print_line(&format!("{:#?}", stack_frame));
            print_line(&format!("Error code: {:#x}", _error_code));
            loop { hlt(); }
        }
    };
}

errcode_exc!(exc_invalid_tss, "#TS Invalid TSS");
errcode_exc!(exc_segment_not_present, "#NP Segment Not Present");
errcode_exc!(exc_stack_fault, "#SS Stack Segment Fault");
errcode_exc!(exc_gpf, "#GP General Protection Fault");
errcode_exc!(exc_alignment_check, "#AC Alignment Check");

extern "x86-interrupt" fn exc_page_fault(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    let addr = Cr2::read();
    print_err("=== PAGE FAULT ===");
    {
        print_line(&format!("Accessed address: {:?}", addr));
        print_line(&format!("Error code: {:?}", error_code));
        print_line(&format!("{:#?}", stack_frame));
    }
    loop { hlt(); }
}

extern "x86-interrupt" fn exc_machine_check(_stack_frame: InterruptStackFrame) -> ! {
    console::cwrite_line("=== MACHINE CHECK ===", 0xFF0000, 0x000000);
    console::cwrite_line("Fatal hardware error, halting.", 0xFFFFFF, 0x000000);
    loop { x86_64::instructions::hlt(); }
}

extern "x86-interrupt" fn exc_double_fault(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    print_err("=== DOUBLE FAULT ===");
    print_line("CPU failed to deliver exceptions correctly, halting.");
    print_line(&format!("{:#?}", stack_frame));
    loop { hlt(); }
}

extern "x86-interrupt" fn exc_breakpoint(stack_frame: InterruptStackFrame) {
    serial::write("INT3 detected");
    
    let formatted = format!("Stack frame: {:#?}", stack_frame);
    let compact = strip_ascii_whitespace(&formatted);
    serial::write(&compact);
}

extern "x86-interrupt" fn exc_overflow(stack_frame: InterruptStackFrame) {
    print_line("INT4 (#OF) detected!");
    print_line(&format!("Stack frame: {:#?}", stack_frame));
}