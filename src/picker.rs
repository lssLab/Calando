//! A one-screen list picker for a terminal: ↑/↓ move, Enter picks, `a` picks all, Esc cancels.

use std::io::{self, Write};

#[derive(Debug, PartialEq)]
pub enum Pick {
    One(usize),
    All,
    Cancel,
}

enum Key {
    Up,
    Down,
    Enter,
    All,
    Cancel,
    Other,
}

fn draw(items: &[String], selected: usize, redraw: bool) {
    let mut out = io::stdout();
    if redraw {
        // Back to the first item line, then rewrite every line.
        let _ = write!(out, "\x1b[{}A", items.len());
    }
    for (index, item) in items.iter().enumerate() {
        let marker = if index == selected { ">" } else { " " };
        let _ = write!(out, "\r\x1b[2K {marker} {item}\r\n");
    }
    let _ = out.flush();
}

/// `None` when stdin is not an interactive terminal; the caller then asks for an argument.
pub fn pick(items: &[String]) -> Option<Pick> {
    let mut terminal = raw::Terminal::enter()?;
    println!("↑/↓ move · Enter resume · a all · Esc cancel");
    let mut selected = 0;
    draw(items, selected, false);
    let pick = loop {
        match terminal.key() {
            Key::Up => selected = selected.saturating_sub(1),
            Key::Down => selected = (selected + 1).min(items.len().saturating_sub(1)),
            Key::Enter => break Pick::One(selected),
            Key::All => break Pick::All,
            Key::Cancel => break Pick::Cancel,
            Key::Other => continue,
        }
        draw(items, selected, true);
    };
    drop(terminal);
    Some(pick)
}

#[cfg(unix)]
mod raw {
    use super::Key;
    use std::io::IsTerminal;

    pub struct Terminal {
        original: libc::termios,
    }

    impl Terminal {
        pub fn enter() -> Option<Self> {
            if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
                return None;
            }
            let mut original: libc::termios = unsafe { std::mem::zeroed() };
            if unsafe { libc::tcgetattr(0, &mut original) } != 0 {
                return None;
            }
            let mut raw = original;
            raw.c_lflag &= !(libc::ICANON | libc::ECHO | libc::ISIG);
            raw.c_cc[libc::VMIN] = 1;
            raw.c_cc[libc::VTIME] = 0;
            (unsafe { libc::tcsetattr(0, libc::TCSANOW, &raw) } == 0).then_some(Self { original })
        }

        fn byte(&self, wait_ms: i32) -> Option<u8> {
            let mut poll = libc::pollfd {
                fd: 0,
                events: libc::POLLIN,
                revents: 0,
            };
            if wait_ms >= 0 && unsafe { libc::poll(&mut poll, 1, wait_ms) } <= 0 {
                return None;
            }
            // Read the descriptor directly: std's buffered stdin would swallow the rest of an
            // escape sequence and make `poll` miss it.
            let mut byte = 0u8;
            (unsafe { libc::read(0, (&mut byte as *mut u8).cast(), 1) } == 1).then_some(byte)
        }

        pub fn key(&mut self) -> Key {
            match self.byte(-1) {
                Some(b'\r' | b'\n') => Key::Enter,
                Some(b'a' | b'A') => Key::All,
                // Ctrl+C, or end of input.
                Some(3) | None => Key::Cancel,
                // A lone Esc cancels; Esc [ A / Esc O A are arrow keys.
                Some(0x1b) => match self.byte(50) {
                    None => Key::Cancel,
                    Some(b'[' | b'O') => match self.byte(50) {
                        Some(b'A') => Key::Up,
                        Some(b'B') => Key::Down,
                        _ => Key::Other,
                    },
                    Some(_) => Key::Other,
                },
                Some(b'k') => Key::Up,
                Some(b'j') => Key::Down,
                Some(_) => Key::Other,
            }
        }
    }

    impl Drop for Terminal {
        fn drop(&mut self) {
            unsafe { libc::tcsetattr(0, libc::TCSANOW, &self.original) };
        }
    }
}

#[cfg(windows)]
mod raw {
    use super::Key;
    use std::io::IsTerminal;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::Console::{
        ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode, GetStdHandle, INPUT_RECORD, KEY_EVENT,
        ReadConsoleInputW, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, SetConsoleMode,
    };

    pub struct Terminal {
        input: HANDLE,
        output: HANDLE,
        input_mode: u32,
        output_mode: u32,
    }

    impl Terminal {
        pub fn enter() -> Option<Self> {
            if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
                return None;
            }
            let input = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
            let output = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
            let (mut input_mode, mut output_mode) = (0, 0);
            if unsafe { GetConsoleMode(input, &mut input_mode) } == 0
                || unsafe { GetConsoleMode(output, &mut output_mode) } == 0
            {
                return None;
            }
            // Key events arrive one by one (no line input or echo); output understands the
            // cursor sequences the picker draws with.
            unsafe {
                SetConsoleMode(input, 0);
                SetConsoleMode(output, output_mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
            }
            Some(Self {
                input,
                output,
                input_mode,
                output_mode,
            })
        }

        pub fn key(&mut self) -> Key {
            loop {
                let mut record: INPUT_RECORD = unsafe { std::mem::zeroed() };
                let mut read = 0;
                if unsafe { ReadConsoleInputW(self.input, &mut record, 1, &mut read) } == 0
                    || read == 0
                {
                    return Key::Cancel;
                }
                if u32::from(record.EventType) != KEY_EVENT {
                    continue;
                }
                let event = unsafe { record.Event.KeyEvent };
                if event.bKeyDown == 0 {
                    continue;
                }
                // VK_UP, VK_DOWN, VK_RETURN, VK_ESCAPE, 'A'.
                return match event.wVirtualKeyCode {
                    0x26 => Key::Up,
                    0x28 => Key::Down,
                    0x0D => Key::Enter,
                    0x1B => Key::Cancel,
                    0x41 => Key::All,
                    _ if unsafe { event.uChar.UnicodeChar } == 3 => Key::Cancel,
                    _ => Key::Other,
                };
            }
        }
    }

    impl Drop for Terminal {
        fn drop(&mut self) {
            unsafe {
                SetConsoleMode(self.input, self.input_mode);
                SetConsoleMode(self.output, self.output_mode);
            }
        }
    }
}
