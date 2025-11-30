use pc_keyboard::{
    layouts::Us104Key, DecodedKey, HandleControl, Keyboard as PcKeyboard, KeyCode,
    KeyEvent as PcKeyEvent, KeyState, ScancodeSet1,
};
use x86_64::instructions::port::Port;

pub enum KeyEvent {
    Char(char),
    Backspace,
    CtrlBackspace,
    Delete,
    Enter,
    Up,
    Down,
    Left,
    Right,
    CtrlLeft,
    CtrlRight,
}

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
    ctrl_down: bool,
}

impl Keyboard {
    pub fn new() -> Self { Self { inner: KeyboardState::new(), ctrl_down: false } }

    fn update_ctrl_state(&mut self, evt: &PcKeyEvent) {
        if matches!(evt.code, KeyCode::LControl | KeyCode::RControl) {
            self.ctrl_down = matches!(evt.state, KeyState::Down | KeyState::SingleShot);
        }
    }

    fn translate_backspace(&self) -> KeyEvent {
        if self.ctrl_down { KeyEvent::CtrlBackspace } else { KeyEvent::Backspace }
    }

    pub fn poll_event(&mut self) -> Option<KeyEvent> {
        if let Some(sc) = self.inner.read_scancode() {
            if let Ok(Some(evt)) = self.inner.kb.add_byte(sc) {
                self.update_ctrl_state(&evt);
                if let Some(key) = self.inner.kb.process_keyevent(evt) {
                    match key {
                        DecodedKey::Unicode(c) => match c {
                            '\n' | '\r' => Some(KeyEvent::Enter),
                            '\x08' => Some(self.translate_backspace()),
                            '\u{7f}' => Some(KeyEvent::Delete),
                            _ => Some(KeyEvent::Char(c)),
                        },
                        DecodedKey::RawKey(k) => {
                            match k {
                                KeyCode::Return => Some(KeyEvent::Enter),
                                KeyCode::Backspace => Some(self.translate_backspace()),
                                KeyCode::Delete => Some(KeyEvent::Delete),
                                KeyCode::ArrowUp => Some(KeyEvent::Up),
                                KeyCode::ArrowDown => Some(KeyEvent::Down),
                                KeyCode::ArrowLeft => {
                                    if self.ctrl_down { Some(KeyEvent::CtrlLeft) } else { Some(KeyEvent::Left) }
                                }
                                KeyCode::ArrowRight => {
                                    if self.ctrl_down { Some(KeyEvent::CtrlRight) } else { Some(KeyEvent::Right) }
                                }
                                _ => None,
                            }
                        }
                    }
                } else { None }
            } else { None }
        } else { None }
    }
}

