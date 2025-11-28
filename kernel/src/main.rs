#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

extern crate alloc;
extern crate spin;
extern crate lazy_static;

mod console;
mod keyboard;
mod font;
mod font2;
mod font3;
mod boot_splash;
mod commands;
mod theme_presets;
mod history;
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
    pub mod utin;
}

use bootloader_api::{config::BootloaderConfig, entry_point, BootInfo};
use core::panic::PanicInfo;
use console::{init_console, with_console};
use keyboard::Keyboard;
use heapless::String;
use x86_64::instructions::interrupts as cpu_intr;

pub const OS_NAME: &str = "StratOS";
pub const OS_VERSION: &str = "1.A015.28.251130.EXENUS@cfc8a";

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
    thudmodules::utin::init();
    thudmodules::min::init();
    thudmodules::tin::init();

    interrupts::init_idt();
    pic::init_pic();
    timer::init_pit();
    cpu_intr::enable();
    time::init_time();
    wait::init();

    boot_splash::show();

    with_console(|c| {
        c.clear();
        c.write_line("==================================================\n");
        c.write_line(OS_NAME);
        c.write_line("Project Rejuvenescence");
        c.write_line("--------------------------------------------------\n");
        c.write_line("A product of Stratocompute Technologies\n");
        c.write_line(OS_VERSION);
        c.write_line("");
        c.write_line("==================================================\n");
        c.newline();
        c.put_char('>');
    });

    let mut kbd = Keyboard::new();
    let mut line = String::<128>::new();
    let mut draft_line = String::<128>::new();
    let mut history_index: Option<usize> = None;

    loop {
        if let Some(evt) = kbd.poll_event() {
            match evt {
                keyboard::KeyEvent::Char(ch) => {
                    if line.push(ch).is_ok() {
                        with_console(|c| c.put_char(ch));
                    }
                    history_index = None;
                }
                keyboard::KeyEvent::CtrlBackspace => {
                    let removed = delete_prev_word(&mut line);
                    if removed > 0 {
                        with_console(|c| {
                            for _ in 0..removed {
                                c.backspace();
                            }
                        });
                    }
                    history_index = None;
                }
                keyboard::KeyEvent::Backspace => {
                    if line.pop().is_some() {
                        with_console(|c| c.backspace());
                    }
                    history_index = None;
                }
                keyboard::KeyEvent::Up => {
                    let hist_len = history::len();
                    if hist_len == 0 {
                        continue;
                    }
                    if history_index.is_none() {
                        draft_line.clear();
                        let _ = draft_line.push_str(&line);
                    }
                    let new_idx = history_index
                        .map(|i| i.saturating_sub(1))
                        .unwrap_or_else(|| hist_len.saturating_sub(1));
                    if let Some(new_line) = history::entry(new_idx) {
                        history_index = Some(new_idx);
                        replace_input_line(&mut line, &new_line);
                    } else {
                        history_index = None;
                    }
                }
                keyboard::KeyEvent::Down => {
                    let hist_len = history::len();
                    if hist_len == 0 {
                        continue;
                    }
                    if let Some(idx) = history_index {
                        if idx + 1 < hist_len {
                            let new_idx = idx + 1;
                            if let Some(new_line) = history::entry(new_idx) {
                                history_index = Some(new_idx);
                                replace_input_line(&mut line, &new_line);
                            } else {
                                history_index = None;
                                replace_input_line(&mut line, &draft_line);
                            }
                        } else {
                            history_index = None;
                            replace_input_line(&mut line, &draft_line);
                        }
                    }
                }
                keyboard::KeyEvent::Enter => {
                    with_console(|c| c.newline());
                    commands::handle_line(&line);
                    history::push(&line);
                    line.clear();
                    draft_line.clear();
                    history_index = None;
                    with_console(|c| c.put_char('>'));
                }
            }
        }
    }
}

fn delete_prev_word(line: &mut String<128>) -> usize {
    let mut removed = 0;

    while let Some(ch) = line.chars().rev().next() {
        if ch.is_ascii_whitespace() {
            line.pop();
            removed += 1;
        } else {
            break;
        }
    }

    while let Some(ch) = line.chars().rev().next() {
        if !ch.is_ascii_whitespace() {
            line.pop();
            removed += 1;
        } else {
            break;
        }
    }

    removed
}

fn replace_input_line(line: &mut String<128>, new_content: &str) {
    with_console(|c| {
        for _ in 0..line.len() {
            c.backspace();
        }
        for ch in new_content.chars() {
            c.put_char(ch);
        }
    });
    line.clear();
    let _ = line.push_str(new_content);
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
