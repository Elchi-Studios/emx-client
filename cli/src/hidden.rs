// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Reading a token at the terminal without showing it.

/// Reads a line from the terminal without echo, where the terminal
/// allows it, and gives the terminal back as it was, also when the
/// person presses Ctrl-C, which ends `emx` with status 130.
pub fn read_line() -> Result<String, String> {
    imp::read_line()
}

#[cfg(unix)]
mod imp {
    use std::io::Read;
    use std::process::{Command, Stdio};

    /// The terminal's settings, put back when dropped, however reading
    /// ends.
    struct Restore(String);

    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = Command::new("stty").arg(&self.0).stdin(Stdio::inherit()).status();
        }
    }

    pub fn read_line() -> Result<String, String> {
        let saved = Command::new("stty")
            .arg("-g")
            .stdin(Stdio::inherit())
            .stderr(Stdio::null())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty());
        let Some(saved) = saved else {
            // No stty: read as typed, with echo.
            let mut s = String::new();
            std::io::stdin().read_line(&mut s).map_err(|e| e.to_string())?;
            return Ok(s);
        };
        // Without echo, and byte by byte with signals off, so that Ctrl-C
        // arrives here as a byte and the terminal is put back before
        // `emx` ends, instead of being left without echo.
        let quiet = Command::new("stty")
            .args(["-echo", "-icanon", "-isig", "min", "1", "time", "0"])
            .stdin(Stdio::inherit())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        let restore = Restore(saved);
        if !quiet {
            drop(restore);
            let mut s = String::new();
            std::io::stdin().read_line(&mut s).map_err(|e| e.to_string())?;
            return Ok(s);
        }
        let mut line = Vec::new();
        let mut stdin = std::io::stdin().lock();
        let mut byte = [0u8; 1];
        let outcome = loop {
            match stdin.read(&mut byte) {
                Ok(0) => break Ok(()),
                Ok(_) => match edit(&mut line, byte[0]) {
                    Key::Go => {}
                    Key::Done => break Ok(()),
                    Key::Cancel => break Err(()),
                },
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => {
                    drop(restore);
                    eprintln!();
                    return Err(e.to_string());
                }
            }
        };
        drop(restore);
        eprintln!();
        if outcome.is_err() {
            std::process::exit(130);
        }
        Ok(String::from_utf8_lossy(&line).into_owned())
    }

    pub(super) enum Key {
        Go,
        Done,
        Cancel,
    }

    /// Takes one byte typed into `line`: Enter ends it, Ctrl-C ends
    /// reading, Ctrl-D ends an empty line, Backspace takes back a
    /// character, Ctrl-U the whole line.
    pub(super) fn edit(line: &mut Vec<u8>, b: u8) -> Key {
        match b {
            b'\n' | b'\r' => Key::Done,
            0x03 => Key::Cancel,
            0x04 if line.is_empty() => Key::Done,
            0x7f | 0x08 => {
                // A whole UTF-8 character: its continuation bytes too.
                while let Some(last) = line.pop() {
                    if last & 0xc0 != 0x80 {
                        break;
                    }
                }
                Key::Go
            }
            0x15 => {
                line.clear();
                Key::Go
            }
            b if b < 0x20 => Key::Go,
            b => {
                line.push(b);
                Key::Go
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn typed(keys: &[u8]) -> (String, bool) {
            let mut line = Vec::new();
            for &b in keys {
                match edit(&mut line, b) {
                    Key::Go => {}
                    Key::Done => return (String::from_utf8(line).unwrap(), true),
                    Key::Cancel => return (String::new(), false),
                }
            }
            (String::from_utf8(line).unwrap(), false)
        }

        #[test]
        fn a_line_is_read_with_its_corrections() {
            assert_eq!(typed(b"emx_abc\n"), ("emx_abc".into(), true));
            assert_eq!(typed(b"emx_abd\x7fc\r"), ("emx_abc".into(), true));
            assert_eq!(typed("emx_\u{e4}\x7fa\n".as_bytes()), ("emx_a".into(), true));
            assert_eq!(typed(b"wrong\x15emx_x\n"), ("emx_x".into(), true));
            assert_eq!(typed(b"emx_\x1bx\n"), ("emx_x".into(), true));
        }

        #[test]
        fn ctrl_c_cancels() {
            assert_eq!(typed(b"emx_a\x03b\n"), (String::new(), false));
        }
    }
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;

    const STD_INPUT_HANDLE: u32 = -10i32 as u32;
    const ENABLE_ECHO_INPUT: u32 = 0x0004;

    #[link(name = "kernel32")]
    extern "system" {
        fn GetStdHandle(which: u32) -> *mut c_void;
        fn GetConsoleMode(console: *mut c_void, mode: *mut u32) -> i32;
        fn SetConsoleMode(console: *mut c_void, mode: u32) -> i32;
    }

    /// The console's mode, put back when dropped.
    struct Restore(*mut c_void, u32);

    impl Drop for Restore {
        fn drop(&mut self) {
            // SAFETY: the handle came from GetStdHandle and stays valid
            // for the life of the process.
            unsafe {
                SetConsoleMode(self.0, self.1);
            }
        }
    }

    pub fn read_line() -> Result<String, String> {
        // SAFETY: plain calls on the process's own standard input handle;
        // `mode` is a valid place for one u32.
        let restore = unsafe {
            let handle = GetStdHandle(STD_INPUT_HANDLE);
            let mut mode = 0u32;
            if !handle.is_null()
                && GetConsoleMode(handle, &mut mode) != 0
                && SetConsoleMode(handle, mode & !ENABLE_ECHO_INPUT) != 0
            {
                Some(Restore(handle, mode))
            } else {
                None
            }
        };
        let mut s = String::new();
        let r = std::io::stdin().read_line(&mut s);
        if restore.is_some() {
            drop(restore);
            eprintln!();
        }
        r.map_err(|e| e.to_string())?;
        Ok(s)
    }
}

#[cfg(not(any(unix, windows)))]
mod imp {
    pub fn read_line() -> Result<String, String> {
        let mut s = String::new();
        std::io::stdin().read_line(&mut s).map_err(|e| e.to_string())?;
        Ok(s)
    }
}
