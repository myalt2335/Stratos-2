#![allow(unused_unsafe)]

use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;
use x86_64::instructions::port::Port;
use heapless::String as HString;

pub static DISPLAY_24H: AtomicBool = AtomicBool::new(true);

static BASE_TIME: Mutex<Option<DateTime>> = Mutex::new(None);
static UPTIME_SECONDS: Mutex<u64> = Mutex::new(0);

#[derive(Copy, Clone)]
pub struct DateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

fn is_leap_year(year: u64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

const MONTH_DAYS: [u64; 12] = [31,28,31,30,31,30,31,31,30,31,30,31];

fn days_in_year(year: u64) -> u64 {
    if is_leap_year(year) { 366 } else { 365 }
}

fn days_in_month(year: u64, month: u64) -> u64 {
    let mut days = MONTH_DAYS[(month - 1) as usize];
    if month == 2 && is_leap_year(year) {
        days += 1;
    }
    days
}

fn read_rtc_register(reg: u8) -> u8 {
    unsafe {
        let mut cmos_address = Port::<u8>::new(0x70);
        let mut cmos_data = Port::<u8>::new(0x71);
        cmos_address.write(reg);
        cmos_data.read()
    }
}

fn bcd_to_binary(value: u8) -> u8 {
    ((value / 16) * 10) + (value & 0xF)
}

fn read_rtc_time() -> DateTime {
    let second = bcd_to_binary(read_rtc_register(0x00));
    let minute = bcd_to_binary(read_rtc_register(0x02));
    let hour = bcd_to_binary(read_rtc_register(0x04));
    let day = bcd_to_binary(read_rtc_register(0x07));
    let month = bcd_to_binary(read_rtc_register(0x08));
    let year = bcd_to_binary(read_rtc_register(0x09)) as u16 + 2000;

    DateTime { year, month, day, hour, minute, second }
}

pub fn init_time() {
    let rtc = read_rtc_time();
    let mut base = BASE_TIME.lock();
    *base = Some(rtc);
    let mut uptime = UPTIME_SECONDS.lock();
    *uptime = 0;
}

pub fn tick_second() {
    let mut uptime = UPTIME_SECONDS.lock();
    *uptime += 1;
}

pub fn current_time_secs() -> Option<u64> {
    let base = BASE_TIME.lock();
    base.as_ref().map(|b| {
        let base_seconds = ymd_hms_to_secs(b.year as u64, b.month as u64, b.day as u64, b.hour as u64, b.minute as u64, b.second as u64);
        let uptime = *UPTIME_SECONDS.lock();
        base_seconds + uptime
    })
}

fn ymd_hms_to_secs(y: u64, m: u64, d: u64, h: u64, min: u64, s: u64) -> u64 {
    let mut days = 0u64;

    for year in 1970..y {
        days += days_in_year(year);
    }

    for month in 1..m {
        days += days_in_month(y, month);
    }

    days += d - 1;

    days * 86400 + h * 3600 + min * 60 + s
}

fn secs_to_ymd_hms(mut secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    let mut year = 1970u64;
    let mut days = secs / 86400;
    secs %= 86400;

    loop {
        let dy = days_in_year(year);
        if days >= dy {
            days -= dy;
            year += 1;
        } else {
            break;
        }
    }

    let mut month = 1u64;
    loop {
        let dm = days_in_month(year, month);
        if days >= dm {
            days -= dm;
            month += 1;
        } else {
            break;
        }
    }

    let day = days + 1;

    let hour = secs / 3600;
    secs %= 3600;
    let minute = secs / 60;
    let second = secs % 60;

    (year, month, day, hour, minute, second)
}


pub fn time_cmd(args: &[&str]) {
    match args.get(0).copied() {
        Some("help") => {
            crate::console::write_line("Usage: time [12hr|24hr|sync|help]");
            crate::console::write_line("  12hr   Set display format to 12-hour mode");
            crate::console::write_line("  24hr   Set display format to 24-hour mode");
            crate::console::write_line("  sync   Resync OS time to RTC time if drift detected");
            crate::console::write_line("  help   Show this message");
        }
        Some("24hr") => {
            DISPLAY_24H.store(true, Ordering::Relaxed);
            crate::console::write_line("Set time format: 24-hour");
        }
        Some("12hr") => {
            DISPLAY_24H.store(false, Ordering::Relaxed);
            crate::console::write_line("Set time format: 12-hour");
        }
        Some("sync") => {
            if let Some(current_secs) = current_time_secs() {
                let rtc = read_rtc_time();
                let rtc_secs = ymd_hms_to_secs(
                    rtc.year as u64,
                    rtc.month as u64,
                    rtc.day as u64,
                    rtc.hour as u64,
                    rtc.minute as u64,
                    rtc.second as u64,
                );
                let drift = if rtc_secs > current_secs {
                    rtc_secs - current_secs
                } else {
                    current_secs - rtc_secs
                };

                if drift > 2 {
                    let mut base = BASE_TIME.lock();
                    *base = Some(rtc);
                    let mut uptime = UPTIME_SECONDS.lock();
                    *uptime = 0;
                    crate::console::write_line("Time re-synced to RTC.");
                } else {
                    crate::console::write_line("Clock is in sync with RTC.");
                }
            } else {
                crate::console::write_line("Time not initialized yet, initializing...");
                init_time();
            }
        }
        _ => {
            let current = current_time_secs();
            match current {
                Some(secs) => {
                    let (y, m, d, h, min, s) = secs_to_ymd_hms(secs);
                    let mut buf = heapless::String::<32>::new();
                    unsafe {
                        if DISPLAY_24H.load(Ordering::Relaxed) {
                            use core::fmt::Write;
                            let _ = write!(&mut buf, "{y:04}-{m:02}-{d:02} {h:02}:{min:02}:{s:02}");
                        } else {
                            let (disp_h, ampm) = if h == 0 {
                                (12, "AM")
                            } else if h < 12 {
                                (h, "AM")
                            } else if h == 12 {
                                (12, "PM")
                            } else {
                                (h - 12, "PM")
                            };
                            use core::fmt::Write;
                            let _ = write!(
                                &mut buf,
                                "{y:04}-{m:02}-{d:02} {disp_h:02}:{min:02}:{s:02} {ampm}"
                            );
                        }
                    }
                    crate::console::write_line(buf.as_str());
                }
                None => crate::console::write_line("Time not initialized yet."),
            }
        }
    }
}

pub fn format_hud_time() -> HString<32> {
    let mut out: HString<32> = HString::new();

    if let Some(secs) = current_time_secs() {
        let (y, m, d, h, min, s) = secs_to_ymd_hms(secs);
        let is_24 = DISPLAY_24H.load(Ordering::Relaxed);

        if is_24 {
            let _ = core::fmt::write(&mut out, format_args!("{:02}/{:02}/{:04} {:02}:{:02}:{:02}", m, d, y, h, min, s));
        } else {
            let (disp_h, ampm) = if h == 0 {
                (12, "AM")
            } else if h < 12 {
                (h, "AM")
            } else if h == 12 {
                (12, "PM")
            } else {
                (h - 12, "PM")
            };
            let _ = core::fmt::write(&mut out, format_args!("{:02}/{:02}/{:04} {:02}:{:02}:{:02} {}", m, d, y, disp_h, min, s, ampm));
        }
    } else {
        let _ = out.push_str("--/--/---- --:--:--");
    }

    out
}
