use pc_keyboard::{layouts::Us104Key, DecodedKey, HandleControl, Keyboard as PcKeyboard, ScancodeSet1};
use x86_64::instructions::port::Port;

pub enum KeyEvent { Char(char), Backspace, Enter }

pub struct KeyboardState {
    kb: PcKeyboard<Us104Key, ScancodeSet1>,
    data: Port<u8>,
    status: Port<u8>,
}

impl KeyboardState {
    fn new() -> Self {
        Self {
            kb: PcKeyboard::new(ScancodeSet1::new(), Us104Key, HandleControl::Ignore),
            data: Port::new(0x60),
            status: Port::new(0x64),
        }
    }

    fn read_scancode(&mut self) -> Option<u8> {
        let status: u8 = unsafe { self.status.read() };
        if status & 1 == 0 {
            return None;
        }
        let sc: u8 = unsafe { self.data.read() };
        Some(sc)
    }
}

pub struct Keyboard {
    inner: KeyboardState,
}

impl Keyboard {
    pub fn new() -> Self { Self { inner: KeyboardState::new() } }

    pub fn poll_event(&mut self) -> Option<KeyEvent> {
        if let Some(sc) = self.inner.read_scancode() {
            if let Ok(Some(evt)) = self.inner.kb.add_byte(sc) {
                if let Some(key) = self.inner.kb.process_keyevent(evt) {
                    match key {
                        DecodedKey::Unicode(c) => match c {
                            '\n' | '\r' => Some(KeyEvent::Enter),
                            '\x08' => Some(KeyEvent::Backspace),
                            _ => Some(KeyEvent::Char(c)),
                        },
                        DecodedKey::RawKey(k) => {
                            use pc_keyboard::KeyCode;
                            match k {
                                KeyCode::Return => Some(KeyEvent::Enter),
                                KeyCode::Backspace => Some(KeyEvent::Backspace),
                                _ => None,
                            }
                        }
                    }
                } else { None }
            } else { None }
        } else { None }
    }
}

