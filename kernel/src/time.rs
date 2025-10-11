use crate::console;
use alloc::format;
use heapless::Vec;

extern "Rust" {
    pub static mut TICKS: u64;
}

static mut BASE_TIME: u64 = 0;

static mut DISPLAY_24H: bool = false;

static mut TIME_INITIALIZED: bool = false;

fn is_leap(year: u64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

fn days_in_year(year: u64) -> u64 {
    if is_leap(year) { 366 } else { 365 }
}

fn days_in_month(year: u64, month: u64) -> u64 {
    match month {
        1 => 31,
        2 => if is_leap(year) { 29 } else { 28 },
        3 => 31,
        4 => 30,
        5 => 31,
        6 => 30,
        7 => 31,
        8 => 31,
        9 => 30,
        10 => 31,
        11 => 30,
        12 => 31,
        _ => 30,
    }
}

fn ymd_hms_to_secs(y: u64, m: u64, d: u64, h: u64, min: u64, s: u64) -> u64 {
    let mut days = 0;

    for year in 2000..y {
        days += days_in_year(year);
    }

    for month in 1..m {
        days += days_in_month(y, month);
    }

    days += d - 1;

    days * 86400 + h * 3600 + min * 60 + s
}

fn secs_to_ymd_hms(mut secs: u64) -> (u64,u64,u64,u64,u64,u64) {
    let s = secs % 60; secs /= 60;
    let min = secs % 60; secs /= 60;
    let h = secs % 24; secs /= 24;

    let mut year = 2000;
    while secs >= days_in_year(year) {
        secs -= days_in_year(year);
        year += 1;
    }

    let mut month = 1;
    while secs >= days_in_month(year, month) {
        secs -= days_in_month(year, month);
        month += 1;
    }

    let day = secs + 1;

    (year, month, day, h, min, s)
}

fn bcd_to_bin(x: u8) -> u8 {
    (x & 0x0F) + ((x >> 4) * 10)
}

fn rtc_read_time() -> Option<(u64,u64,u64,u64,u64,u64)> {
    use x86::io::{inb, outb};

    unsafe {
        loop {
            outb(0x70, 0x0A);
            let status_a = inb(0x71);
            if status_a & 0x80 != 0 { continue; }

            outb(0x70, 0x00); let sec = inb(0x71);
            outb(0x70, 0x02); let min = inb(0x71);
            outb(0x70, 0x04); let hour = inb(0x71);
            outb(0x70, 0x07); let day = inb(0x71);
            outb(0x70, 0x08); let month = inb(0x71);
            outb(0x70, 0x09); let year = inb(0x71);

            outb(0x70, 0x0B);
            let status_b = inb(0x71);

            let is_binary = status_b & 0x04 != 0;
            let is_24h = status_b & 0x02 != 0;

            let sec = if is_binary { sec } else { bcd_to_bin(sec) };
            let min = if is_binary { min } else { bcd_to_bin(min) };
            let mut hour = if is_binary { hour } else { bcd_to_bin(hour) };
            let day = if is_binary { day } else { bcd_to_bin(day) };
            let month = if is_binary { month } else { bcd_to_bin(month) };
            let year = if is_binary { year } else { bcd_to_bin(year) };

            if !is_24h {
                let pm = (hour & 0x80) != 0;
                hour &= 0x7F;
                if pm && hour < 12 {
                    hour = hour.wrapping_add(12);
                } else if !pm && hour == 12 {
                    hour = 0;
                }
            }

            return Some((2000 + year as u64, month as u64, day as u64,
                        hour as u64, min as u64, sec as u64));
        }
    }
}

fn set_base_time(secs: u64) {
    unsafe {
        BASE_TIME = secs.saturating_sub(TICKS / 100);
        TIME_INITIALIZED = true;
    }
}

pub fn init() {
    if let Some((y,m,d,h,min,s)) = rtc_read_time() {
        let secs = ymd_hms_to_secs(y,m,d,h,min,s);
        set_base_time(secs);
        console::write_line("Time initialized from RTC.");
    } else {
        console::write_line("RTC not available, time not initialized.");
    }
}

fn current_time_secs() -> Option<u64> {
    unsafe {
        if !TIME_INITIALIZED {
            None
        } else {
            Some(BASE_TIME + (TICKS / 100))
        }
    }
}

fn parse_date(s: &str) -> Option<(u64,u64,u64)> {
    let mut parts: Vec<&str, 3> = Vec::new();
    for p in s.split('-') { let _ = parts.push(p); }
    if parts.len() != 3 { return None; }
    Some((parts[0].parse().ok()?, parts[1].parse().ok()?, parts[2].parse().ok()?))
}

fn parse_time(s: &str) -> Option<(u64,u64,u64)> {
    let mut parts: Vec<&str, 3> = Vec::new();
    for p in s.split(':') { let _ = parts.push(p); }
    if parts.len() != 3 { return None; }
    Some((parts[0].parse().ok()?, parts[1].parse().ok()?, parts[2].parse().ok()?))
}

pub fn time_cmd(args: &[&str]) {
    if args.is_empty() {
        match current_time_secs() {
            Some(secs) => {
                let (y,m,d,h,min,s) = secs_to_ymd_hms(secs);
                unsafe {
                    if DISPLAY_24H {
                        console::write_line(&format!(
                            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                            y,m,d,h,min,s
                        ));
                    } else {
                        let mut disp_h = h % 12;
                        if disp_h == 0 { disp_h = 12; }
                        let ampm = if h >= 12 { "PM" } else { "AM" };
                        console::write_line(&format!(
                            "{:04}-{:02}-{:02} {:02}:{:02}:{:02} {}",
                            y,m,d,disp_h,min,s,ampm
                        ));
                    }
                }
            }
            None => console::write_line("Error: No valid time set"),
        }
    } else if args[0] == "set" && args.len() == 3 {
        if let (Some((y,m,d)), Some((h,min,s))) = (parse_date(args[1]), parse_time(args[2])) {
            let secs = ymd_hms_to_secs(y,m,d,h,min,s);
            set_base_time(secs);
            console::write_line("Time manually set.");
        } else {
            console::write_line("Usage: time set YYYY-MM-DD HH:MM:SS");
        }
    } else if args[0] == "12hr" {
        unsafe { DISPLAY_24H = false; }
        console::write_line("Time display set to 12-hour (AM/PM).");
    } else if args[0] == "24hr" {
        unsafe { DISPLAY_24H = true; }
        console::write_line("Time display set to 24-hour.");
    } else if args[0] == "sync" {
        if let Some((y,m,d,h,min,s)) = rtc_read_time() {
            let secs = ymd_hms_to_secs(y,m,d,h,min,s);
            set_base_time(secs);
            console::write_line("Time re-synced from RTC.");
        } else {
            console::write_line("RTC not available.");
        }
    } else {
        console::write_line("Usage: time [set YYYY-MM-DD HH:MM:SS | 12hr | 24hr | sync]");
    }
}
