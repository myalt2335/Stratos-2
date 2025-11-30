#![allow(dead_code)]

use alloc::vec::Vec;
use heapless::String;
use spin::Mutex;

const HISTORY_LIMIT: usize = 64;

static HISTORY: Mutex<Vec<String<128>>> = Mutex::new(Vec::new());
static ENABLED: Mutex<bool> = Mutex::new(true);

pub fn push(cmd: &str) {
    if !is_enabled() {
        return;
    }
    if cmd.is_empty() {
        return;
    }
    let mut history = HISTORY.lock();
    if let Some(pos) = history.iter().position(|h| h == cmd) {
        history.remove(pos);
    }
    if history.len() >= HISTORY_LIMIT {
        history.remove(0);
    }
    let mut s = String::<128>::new();
    let _ = s.push_str(cmd);
    history.push(s);
}

pub fn len() -> usize {
    HISTORY.lock().len()
}

pub fn is_empty() -> bool {
    HISTORY.lock().is_empty()
}

pub fn entry(idx: usize) -> Option<String<128>> {
    HISTORY.lock().get(idx).cloned()
}

pub fn clear() {
    let mut history = HISTORY.lock();
    *history = Vec::new();
}

pub fn is_enabled() -> bool {
    *ENABLED.lock()
}

pub fn set_enabled(enabled: bool) {
    let mut flag = ENABLED.lock();
    *flag = enabled;
    if !enabled {
        clear();
    }
}

pub fn toggle_enabled() -> bool {
    let currently = is_enabled();
    set_enabled(!currently);
    !currently
}
