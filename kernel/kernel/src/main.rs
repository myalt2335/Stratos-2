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
mod help;
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
use heapless::{String, Vec};
use x86_64::instructions::interrupts as cpu_intr;

pub const OS_NAME: &str = "StratOS";
pub const OS_VERSION: &str = "1.A015.92.251130.EXENUS@cfc8a";
const SHOWSPLASH: bool = true;

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

    if SHOWSPLASH {
    boot_splash::show();
    }

    let mut input_origin = with_console(|c| {
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
        c.cursor_position()
    });

    let mut kbd = Keyboard::new();
    let mut line = String::<128>::new();
    let mut draft_line = String::<128>::new();
    let mut history_index: Option<usize> = None;
    let mut cursor_pos: usize = 0;
    let mut rendered_len: usize = 0;

    loop {
        if let Some(evt) = kbd.poll_event() {
            match evt {
                keyboard::KeyEvent::Char(ch) => {
                    if insert_char_at(&mut line, cursor_pos, ch) {
                        cursor_pos += 1;
                        redraw_input_line(&line, cursor_pos, input_origin, &mut rendered_len);
                    }
                    history_index = None;
                }
                keyboard::KeyEvent::CtrlBackspace => {
                    if delete_prev_word(&mut line, &mut cursor_pos) {
                        redraw_input_line(&line, cursor_pos, input_origin, &mut rendered_len);
                    }
                    history_index = None;
                }
                keyboard::KeyEvent::Backspace => {
                    if cursor_pos > 0 && remove_char_at(&mut line, cursor_pos - 1) {
                        cursor_pos -= 1;
                        redraw_input_line(&line, cursor_pos, input_origin, &mut rendered_len);
                    }
                    history_index = None;
                }
                keyboard::KeyEvent::Delete => {
                    if remove_char_at(&mut line, cursor_pos) {
                        redraw_input_line(&line, cursor_pos, input_origin, &mut rendered_len);
                    }
                    history_index = None;
                }
                keyboard::KeyEvent::Left => {
                    if cursor_pos > 0 {
                        cursor_pos -= 1;
                        with_console(|c| {
                            let target_x = input_origin.0.saturating_add(cursor_pos);
                            c.move_cursor_to(target_x, input_origin.1);
                        });
                    }
                }
                keyboard::KeyEvent::Right => {
                    let len = line.chars().count();
                    if cursor_pos < len {
                        cursor_pos += 1;
                        with_console(|c| {
                            let target_x = input_origin.0.saturating_add(cursor_pos);
                            c.move_cursor_to(target_x, input_origin.1);
                        });
                    }
                }
                keyboard::KeyEvent::CtrlLeft => {
                    if move_cursor_word_left(&line, &mut cursor_pos) {
                        with_console(|c| {
                            let target_x = input_origin.0.saturating_add(cursor_pos);
                            c.move_cursor_to(target_x, input_origin.1);
                        });
                    }
                }
                keyboard::KeyEvent::CtrlRight => {
                    if move_cursor_word_right(&line, &mut cursor_pos) {
                        with_console(|c| {
                            let target_x = input_origin.0.saturating_add(cursor_pos);
                            c.move_cursor_to(target_x, input_origin.1);
                        });
                    }
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
                        set_input_line(&mut line, &new_line, &mut cursor_pos, input_origin, &mut rendered_len);
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
                                set_input_line(&mut line, &new_line, &mut cursor_pos, input_origin, &mut rendered_len);
                            } else {
                                history_index = None;
                                set_input_line(&mut line, &draft_line, &mut cursor_pos, input_origin, &mut rendered_len);
                            }
                        } else {
                            history_index = None;
                            set_input_line(&mut line, &draft_line, &mut cursor_pos, input_origin, &mut rendered_len);
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
                    cursor_pos = 0;
                    rendered_len = 0;
                    input_origin = with_console(|c| {
                        c.put_char('>');
                        c.cursor_position()
                    });
                }
            }
        }
    }
}

fn insert_char_at(line: &mut String<128>, idx: usize, ch: char) -> bool {
    let len = line.chars().count();
    if idx > len {
        return false;
    }
    let mut new_line = String::<128>::new();
    let mut inserted = false;
    for (i, existing) in line.chars().enumerate() {
        if i == idx {
            if new_line.push(ch).is_err() { return false; }
            inserted = true;
        }
        if new_line.push(existing).is_err() { return false; }
    }
    if !inserted && new_line.push(ch).is_err() {
        return false;
    }
    *line = new_line;
    true
}

fn remove_char_at(line: &mut String<128>, idx: usize) -> bool {
    let len = line.chars().count();
    if idx >= len {
        return false;
    }
    let mut new_line = String::<128>::new();
    for (i, ch) in line.chars().enumerate() {
        if i == idx {
            continue;
        }
        if new_line.push(ch).is_err() {
            return false;
        }
    }
    *line = new_line;
    true
}

fn delete_prev_word(line: &mut String<128>, cursor_pos: &mut usize) -> bool {
    if *cursor_pos == 0 {
        return false;
    }
    let mut chars = Vec::<char, 128>::new();
    for ch in line.chars() {
        let _ = chars.push(ch);
    }
    let mut idx = (*cursor_pos).min(chars.len());
    while idx > 0 && chars[idx - 1].is_ascii_whitespace() {
        idx -= 1;
    }
    while idx > 0 && !chars[idx - 1].is_ascii_whitespace() {
        idx -= 1;
    }
    if idx == *cursor_pos {
        return false;
    }
    let remove_count = *cursor_pos - idx;
    for _ in 0..remove_count {
        chars.remove(idx);
    }
    line.clear();
    for ch in chars.iter() {
        let _ = line.push(*ch);
    }
    *cursor_pos = idx;
    true
}

fn move_cursor_word_left(line: &String<128>, cursor_pos: &mut usize) -> bool {
    if *cursor_pos == 0 {
        return false;
    }
    let chars: Vec<char, 128> = line.chars().collect();
    let mut idx = (*cursor_pos).min(chars.len());
    while idx > 0 && chars[idx - 1].is_ascii_whitespace() {
        idx -= 1;
    }
    while idx > 0 && !chars[idx - 1].is_ascii_whitespace() {
        idx -= 1;
    }
    if idx == *cursor_pos {
        return false;
    }
    *cursor_pos = idx;
    true
}

fn move_cursor_word_right(line: &String<128>, cursor_pos: &mut usize) -> bool {
    let chars: Vec<char, 128> = line.chars().collect();
    if *cursor_pos >= chars.len() {
        return false;
    }
    let mut idx = *cursor_pos;
    while idx < chars.len() && !chars[idx].is_ascii_whitespace() {
        idx += 1;
    }
    while idx < chars.len() && chars[idx].is_ascii_whitespace() {
        idx += 1;
    }
    if idx == *cursor_pos {
        return false;
    }
    *cursor_pos = idx;
    true
}

fn set_input_line(
    line: &mut String<128>,
    new_content: &str,
    cursor_pos: &mut usize,
    origin: (usize, usize),
    rendered_len: &mut usize,
) {
    line.clear();
    for ch in new_content.chars() {
        if line.push(ch).is_err() {
            break;
        }
    }
    *cursor_pos = line.chars().count();
    redraw_input_line(line, *cursor_pos, origin, rendered_len);
}

fn redraw_input_line(
    line: &String<128>,
    cursor_pos: usize,
    origin: (usize, usize),
    rendered_len: &mut usize,
) {
    let new_len = console::render_line_at(origin.0, origin.1, line.as_str(), *rendered_len, cursor_pos);
    *rendered_len = new_len;
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
