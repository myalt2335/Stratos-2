#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

extern crate alloc;
extern crate spin;
extern crate lazy_static;

mod console;
mod keyboard;
mod font;
mod commands;
mod memory;
mod timer;
mod interrupts;
mod pic;
mod serial;
mod time;
mod thud;
mod wait;
mod thudmodules {
    pub mod tin;
    pub mod min;
}

use bootloader_api::{config::BootloaderConfig, entry_point, BootInfo};
use core::panic::PanicInfo;
use console::{init_console, with_console};
use keyboard::Keyboard;
use heapless::String;
use x86_64::instructions::interrupts as cpu_intr;

static BOOTLOADER_CONFIG: BootloaderConfig = {
    let cfg = BootloaderConfig::new_default();
    cfg
};

entry_point!(kernel_main, config = &BOOTLOADER_CONFIG);

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    serial::write("Hello from kernel!");
    memory::init_memory(boot_info);

    init_console(boot_info);
    with_console(|c| c.reserve_hud_rows(1));
    thud::init();
    thudmodules::min::init();
    thudmodules::tin::init();

    interrupts::init_idt();
    pic::init_pic();
    timer::init_pit();
    cpu_intr::enable();
    time::init_time();
    wait::init();

    with_console(|c| {
        c.clear();
        c.write_line("==================================================\n");
        c.write_line("StratOS");
        c.write_line("Project Rejuvenescence");
        c.write_line("--------------------------------------------------\n");
        c.write_line("Ancient look, modern architecture. (x64)");
        c.write_line("A product of Stratocompute Technologies\n");
        c.write_line("1.A009.02.251031.EXENUS@cfc8a\n");
        c.write_line("==================================================\n");
        c.newline();
        c.put_char('>');
    });

    let mut kbd = Keyboard::new();
    let mut line = String::<128>::new();

    loop {
        if let Some(evt) = kbd.poll_event() {
            match evt {
                keyboard::KeyEvent::Char(ch) => {
                    if line.push(ch).is_ok() {
                        with_console(|c| c.put_char(ch));
                    }
                }
                keyboard::KeyEvent::Backspace => {
                    if line.pop().is_some() {
                        with_console(|c| c.backspace());
                    }
                }
                keyboard::KeyEvent::Enter => {
                    with_console(|c| c.newline());
                    commands::handle_line(&line);
                    line.clear();
                    with_console(|c| c.put_char('>'));
                }
            }
        }
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    with_console(|c| {
        c.write_line("");
        c.cwrite_line("=== KERNEL PANIC ===", 0xFF0000, 0x000000);
        let msg = alloc_str(info);
        c.cwrite_line(&msg, 0xFFFF8F, 0x000000);
        c.write_line("");
        c.cwrite_line("Attempting to fix via reboot...", 0x0047AB, 0x000000);
    });

    crate::commands::wait_ticks(300);

    crate::commands::reboot();

    with_console(|c| {
        c.write_line("Reboot failed! Halting...");
    });

    loop {
        unsafe { x86::halt(); }
    }
}

fn alloc_str(info: &PanicInfo) -> heapless::String<256> {
    use core::fmt::Write;
    let mut s = heapless::String::<256>::new();
    let _ = write!(&mut s, "{info}");
    s
}
