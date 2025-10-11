use crate::console;
use alloc::format;
use heapless::{String as HString, Vec, LinearMap};
use spin::Mutex;
use raw_cpuid::CpuId;
use core::fmt::Write;

static mut TICKS: u64 = 0;

const DEFAULT_BG: u32 = 0x000000;

static ALIASES: Mutex<LinearMap<HString<32>, HString<32>, 32>> =
    Mutex::new(LinearMap::new());

pub fn add_alias(alias: &str, command: &str) {
    let mut aliases = ALIASES.lock();

    let alias_lower = alias.to_ascii_lowercase();
    let command_lower = command.to_ascii_lowercase();

    let mut alias_str: HString<32> = HString::new();
    let mut command_str: HString<32> = HString::new();

    if alias_str.push_str(&alias_lower).is_err() || command_str.push_str(&command_lower).is_err() {
        console::write_line("Alias too long (max 32 chars).");
        return;
    }

    if aliases.contains_key(&alias_str) || aliases.values().any(|v| v == &alias_str) {
        console::write_line("Alias already exists or conflicts.");
        return;
    }

    aliases.insert(alias_str.clone(), command_str.clone()).ok();
    console::write_line(&format!("Alias added: {} -> {}", alias_lower, command_lower));
}

pub fn remove_alias(alias: &str) {
    let mut aliases = ALIASES.lock();

    let alias_lower = alias.to_ascii_lowercase();
    let mut alias_str: HString<32> = HString::new();
    let _ = alias_str.push_str(&alias_lower);

    if aliases.remove(&alias_str).is_some() {
        console::write_line(&format!("Alias removed: {}", alias_lower));
    } else {
        console::write_line("Alias not found.");
    }
}

pub fn list_aliases() {
    let aliases = ALIASES.lock();
    if aliases.is_empty() {
        console::write_line("No aliases defined.");
    } else {
        console::write_line("Aliases:");
        for (alias, target) in aliases.iter() {
            console::write_line(&format!("  {} -> {}", alias, target));
        }
    }
}

pub fn resolve_alias(cmd: &str) -> HString<32> {
    let aliases = ALIASES.lock();

    let cmd_lower = cmd.to_ascii_lowercase();
    let mut key: HString<32> = HString::new();
    let _ = key.push_str(&cmd_lower);

    if let Some(target) = aliases.get(&key) {
        target.clone()
    } else {
        key
    }
}

pub fn tick() {
    unsafe { TICKS += 1; }
}

pub fn wait_ticks(ticks: u64) {
    let start = unsafe { TICKS };
    while unsafe { TICKS } - start < ticks {
        unsafe { x86::halt(); }
    }
}

pub fn echo(args: &[&str]) {
    let mut s = HString::<128>::new();
    for (i, word) in args.iter().enumerate() {
        if i > 0 {
            let _ = s.push(' ');
        }
        let _ = s.push_str(word);
    }
    console::write_line(&s);
}

fn parse_rgb_hex(s: &str) -> Option<u32> {
    let h = s.trim();
    if h.len() == 3 {
        let mut buf = [0u8; 6];
        for (i, b) in h.bytes().enumerate() {
            let hi = b;
            buf[i * 2] = hi;
            buf[i * 2 + 1] = hi;
        }
        let expanded = core::str::from_utf8(&buf).ok()?;
        return u32::from_str_radix(expanded, 16).ok();
    }
    if h.len() == 6 {
        return u32::from_str_radix(h, 16).ok();
    }
    None
}

pub fn cecho(args: &[&str]) {
    if args.len() < 2 {
        console::write_line("Usage: cecho <hex> <text>");
        return;
    }

    let fg = match parse_rgb_hex(args[0]) {
        Some(v) if v <= 0xFFFFFF => v,
        _ => {
            console::write_line("cecho: invalid hex. Use 3 or 6 hex digits, e.g., FF0000");
            return;
        }
    };

    let mut s = HString::<128>::new();
    for (i, word) in args[1..].iter().enumerate() {
        if i > 0 { let _ = s.push(' '); }
        let _ = s.push_str(word);
    }

    console::cwrite_line(&s, fg, DEFAULT_BG);
}

pub fn whatthecreatorofstratosis() {
    console::write_line("");
    console::cwrite_line("                    ", 0x058069, 0x058069);
    console::cwrite_line("                    ", 0x058069, 0x058069);
    console::cwrite_line("                    ", 0x20be9f, 0x20be9f);
    console::cwrite_line("                    ", 0x20be9f, 0x20be9f);
    console::cwrite_line("                    ", 0x8ed8b2, 0x8ed8b2);
    console::cwrite_line("                    ", 0x8ed8b2, 0x8ed8b2);
    console::cwrite_line("                    ", 0xf1f0f0, 0xf1f0f0);
    console::cwrite_line("                    ", 0xf1f0f0, 0xf1f0f0);
    console::cwrite_line("                    ", 0x71a2d1, 0x71a2d1);
    console::cwrite_line("                    ", 0x71a2d1, 0x71a2d1);
    console::cwrite_line("                    ", 0x4b43b8, 0x4b43b8);
    console::cwrite_line("                    ", 0x4b43b8, 0x4b43b8);
    console::cwrite_line("                    ", 0x38196f, 0x38196f);
    console::cwrite_line("                    ", 0x38196f, 0x38196f);
    console::write_line("");
}
// ^ test of console::cwrite_line

pub fn rainbowband() {
    console::cwrite("     ", 0xf10103, 0xf10103);
    console::cwrite("     ", 0xff8001, 0xff8001);
    console::cwrite("     ", 0xffff00, 0xffff00);
    console::cwrite("     ", 0x007940, 0x007940);
    console::cwrite("     ", 0x403ffe, 0x403ffe);
    console::cwrite("     ", 0xa100bf, 0xa100bf);
    console::write("\n");
}
// ^ test of console::cwrite

pub fn clear() {
    console::clear_screen();
}

pub fn help() {
    console::write_line("\nAvailable commands:");
    console::write_line("  help          - Show this help");
    console::write_line("  echo <text>   - Print text");
    console::write_line("  clear         - Clear the screen");
    console::write_line("  time          - Show uptime since boot");
    console::write_line("  reboot        - Reboot the machine");
    console::write_line("  shutdown      - Power down the machine");
    console::write_line("  meminfo       - Show memory info");
    console::write_line("  memtest       - Test the memory");
    console::write_line("  cpuinfo       - Show CPU info");
    console::write_line("  version       - Show OS version");
    console::write_line("  alias         - Create an alias");
    console::write_line("  unalias       - Remove an alias");
    console::write_line("  aliases       - List all aliases\n");
}

/* Delisted commands:
- Panic
- Halt
- whatthecreatorofstratosis
probably nothing else.
*/

pub fn time() {
    unsafe {
        let secs = TICKS / 100;
        let mins = secs / 60;
        let hours = mins / 60;
        console::write_line(&format!(
            "Uptime: {:02}:{:02}:{:02}",
            hours,
            mins % 60,
            secs % 60
        ));
    }
}

pub fn version() {
    console::write_line("StratOS Shell 1.A004.22.251001.EXENUS@cfc8a");
    console::write_line("Built with Rust.");
}

pub fn reboot() {
    console::write_line("Attempting to reboot...");
    wait_ticks(20);

    let ok = unsafe {
        x86::io::outb(0x64, 0xFE);
        true
    };

    if !ok {
        console::write_line("Something went wrong, attempting to restart the machine\n");
    }
}

pub fn shutdown() -> ! {
    console::write_line("Attempting to shut down...");

    let ok = unsafe {
        x86::io::outw(0x604, 0x2000);
        true
    };

    if !ok {
        console::write_line("\nSomething went wrong attempting to shut down the machine.");
        console::write_line("Halting to allow for safe machine shutdown....\n");
    }

    loop {
        unsafe { x86::halt(); }
    }
}

fn format_bytes<const N: usize>(bytes: usize) -> HString<N> {
    let mut s: HString<N> = HString::new();
    if bytes >= 1024 * 1024 {
        let _ = write!(s, "{} MB", bytes / 1024 / 1024);
    } else if bytes >= 1024 {
        let _ = write!(s, "{} KB", bytes / 1024);
    } else {
        let _ = write!(s, "{} B", bytes);
    }
    s
}

fn mem_selftest() {
    use crate::memory::{
        register_app, unregister_app, app_alloc, app_dealloc,
        app_stats, kalloc, kdealloc,
    };
    use crate::console;

    const DUMMY_APP: u32 = 42;

    console::write_line("=== Memory self-test starting ===");

    unsafe {
        let p = kalloc(128, 8);
        if !p.is_null() {
            console::write_line("Kernel alloc: success (128 B)");
            kdealloc(p, 128, 8);
            console::write_line("Kernel dealloc: success");
        } else {
            console::write_line("Kernel alloc FAILED");
        }
    }

    if register_app(DUMMY_APP, 64 * 1024) {
        unsafe {
            let ptr = app_alloc(DUMMY_APP, 4096, 8);
            if !ptr.is_null() {
                console::write_line("App alloc: success (4 KiB)");
                app_dealloc(DUMMY_APP, ptr, 4096, 8);
                console::write_line("App dealloc: success");
            } else {
                console::write_line("App alloc FAILED");
            }
        }

        if let Some(stats) = app_stats(DUMMY_APP) {
            console::write_line(&format!(
                "App {} stats: total={} used={} free={} allocs={} deallocs={}",
                DUMMY_APP,
                stats.total, stats.used, stats.free,
                stats.alloc_count, stats.dealloc_count
            ));
        }

        unregister_app(DUMMY_APP);
    } else {
        console::write_line("App register FAILED");
    }

    console::write_line("=== Memory self-test complete ===");
}

pub fn meminfo() {
    use crate::memory::memory_overview;
    use crate::console;

    let mo = memory_overview();

    console::write_line(&format!(
        "System memory:\n  Total: {}\n  Reserved: {}\n  Free: {}",
        format_bytes::<32>(mo.system.total),
        format_bytes::<32>(mo.system.reserved),
        format_bytes::<32>(mo.system.free),
    ));

    console::write_line(&format!(
        "\nKernel heap:\n  Total: {}\n  Used: {}\n  Free: {}\n  Peak: {}\n  Allocs: {}\n  Deallocs: {}",
        format_bytes::<32>(mo.kernel_heap.total),
        format_bytes::<32>(mo.kernel_heap.used),
        format_bytes::<32>(mo.kernel_heap.free),
        format_bytes::<32>(mo.kernel_heap.peak_used),
        mo.kernel_heap.alloc_count,
        mo.kernel_heap.dealloc_count,
    ));

    console::write_line(&format!(
        "\nUser arena:\n  Total: {}\n  Free for new regions: {}",
        format_bytes::<32>(mo.user_arena_total),
        format_bytes::<32>(mo.user_arena_free_for_new_regions),
    ));

    for e in mo.apps.iter().flatten() {
        let (id, st) = *e;
        console::write_line(&format!(
            "\nApp {}:\n  Total: {}\n  Used: {}\n  Free: {}\n  Peak: {}\n  Allocs: {}\n  Deallocs: {}",
            id,
            format_bytes::<32>(st.total),
            format_bytes::<32>(st.used),
            format_bytes::<32>(st.free),
            format_bytes::<32>(st.peak_used),
            st.alloc_count,
            st.dealloc_count,
        ));
    }
}

pub fn cpuinfo() {
    let cpuid = CpuId::new();

    if let Some(vf) = cpuid.get_vendor_info() {
        console::write_line(&format!("CPU Vendor: {}", vf.as_str()));
    }

    if let Some(fi) = cpuid.get_feature_info() {
        console::write_line(&format!(
            "Model: {} Family: {} Stepping: {}",
            fi.model_id(),
            fi.family_id(),
            fi.stepping_id()
        ));
        console::write_line(&format!(
            "Features: SSE={} SSE2={} SSE3={} AVX={}",
            fi.has_sse(),
            fi.has_sse2(),
            fi.has_sse3(),
            fi.has_avx()
        ));
    }

    if let Some(pf) = cpuid.get_processor_brand_string() {
        let brand: &str = pf.as_str();
        console::write_line(&format!("Brand: {}", brand));
    }
}

pub fn halt_cmd(args: &[&str]) {
    if args.len() == 1 && args[0] == "yes-i-know" {
        console::write_line("System halted.");
        loop {
            unsafe { x86::halt(); }
        }
    } else {
        console::write_line("Refusing to halt. Use: halt yes-i-know");
    }
}

pub fn panic_cmd(args: &[&str]) {
    if args.len() == 1 && args[0] == "yes-i-know" {
        panic!("Manual panic triggered from shell");
    } else {
        console::write_line("Refusing to panic. Use: panic yes-i-know");
    }
}

pub fn handle_command(input: &str) {
    let mut parts: Vec<&str, 16> = Vec::new();
    for word in input.split_whitespace() {
        let _ = parts.push(word);
    }

    if parts.is_empty() {
        return;
    }

    let command = resolve_alias(&parts[0]).to_ascii_lowercase();

    match command.as_str() {
        "echo" => echo(&parts[1..]),
        "cecho" => cecho(&parts[1..]),
        "version" => version(),
        "clear" => clear(),
        "rainbow" => rainbowband(),
        "cls" => clear(),
        "fag" => whatthecreatorofstratosis(),
        "help" => help(),
        "time" => time(),
        "reboot" => reboot(),
        "shutdown" => shutdown(),
        "meminfo" => meminfo(),
        "memtest" => mem_selftest(),
        "cpuinfo" => cpuinfo(),
        "halt" => halt_cmd(&parts[1..]),
        "panic" => panic_cmd(&parts[1..]),

        "alias" => {
            if parts.len() == 3 {
                add_alias(parts[2], parts[1]);
            } else {
                console::write_line("Usage: alias <command> <alias>");
            }
        }
        "unalias" => {
            if parts.len() == 2 {
                remove_alias(parts[1]);
            } else {
                console::write_line("Usage: unalias <alias>");
            }
        }
        "aliases" => list_aliases(),

        _ => console::write_line(&format!("Unknown command: {}", parts[0])),
    }
}
