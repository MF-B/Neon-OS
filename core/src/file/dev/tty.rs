//! Device filesystem module for handling terminal I/O

use alloc::vec;
use axerrno::{AxResult, ax_err};
use axfs_vfs::{VfsNodeAttr, VfsNodeOps, VfsNodePerm, VfsNodeType, VfsResult};
use axio::{BufReader, PollState, prelude::*};
use axsync::Mutex;
use linux_raw_sys::{
    general::{
        BRKINT, CREAD, ECHO, ECHOE, ECHONL, HUPCL, ICANON, ICRNL, IEXTEN, IMAXBEL, ISIG, IUTF8,
        IXANY, IXON, ONLCR, OPOST, termios, winsize,
    },
    ioctl::{
        TCGETA, TCGETS, TCSETS, TCSETSF, TCSETSW, TIOCGPGRP, TIOCGWINSZ, TIOCSPGRP, TIOCSWINSZ,
    },
};

/// Represents the type of TTY device
struct TtyState {
    termios: termios,
    pgid: u32,
    winsize: winsize,
}

impl TtyState {
    pub fn new() -> Self {
        Self {
            termios: termios {
                c_iflag: IMAXBEL | IUTF8 | IXON | IXANY | ICRNL | BRKINT,
                c_oflag: OPOST | ONLCR,
                c_cflag: CREAD | HUPCL | 0x77,
                c_lflag: ISIG | ICANON | ECHO | ECHOE | ECHONL | IEXTEN,
                c_line: 0,
                c_cc: [
                    3,   // VINTR Ctrl-C
                    28,  // VQUIT
                    127, // VERASE
                    21,  // VKILL
                    4,   // VEOF Ctrl-D
                    0,   // VTIME
                    1,   // VMIN
                    0,   // VSWTC
                    17,  // VSTART
                    19,  // VSTOP
                    26,  // VSUSP Ctrl-Z
                    255, // VEOL
                    18,  // VREPAINT
                    15,  // VDISCARD
                    23,  // VWERASE
                    22,  // VLNEXT
                    255, // VEOL2
                    0, 0, // Unused
                ],
            },
            pgid: 2,
            winsize: winsize {
                ws_row: 24,
                ws_col: 140,
                ws_xpixel: 0,
                ws_ypixel: 0,
            },
        }
    }

    pub fn ioctl(&mut self, cmd: u32, arg: *mut u8) -> VfsResult<isize> {
        match cmd {
            TCGETS | TCGETA => {
                unsafe {
                    (arg as *mut termios).write_volatile(self.termios);
                }
                Ok(0)
            }
            TCSETS | TCSETSW | TCSETSF => {
                unsafe { self.termios = *(arg as *mut termios).as_mut().unwrap() }
                Ok(0)
            }
            TIOCGPGRP => match unsafe { (arg as *mut u32).as_mut() } {
                Some(pgid) => {
                    *pgid = self.pgid;
                    Ok(0)
                }
                None => ax_err!(InvalidInput),
            },
            TIOCSPGRP => match unsafe { (arg as *mut u32).as_mut() } {
                Some(pgid) => {
                    self.pgid = *pgid;
                    Ok(0)
                }
                None => ax_err!(InvalidInput),
            },
            TIOCGWINSZ => {
                unsafe {
                    *(arg as *mut winsize).as_mut().unwrap() = self.winsize;
                }
                Ok(0)
            }
            TIOCSWINSZ => {
                unsafe {
                    self.winsize = *(arg as *mut winsize).as_mut().unwrap();
                }
                Ok(0)
            }
            _ => ax_err!(Unsupported),
        }
    }
}

impl Default for TtyState {
    fn default() -> Self {
        Self::new()
    }
}

fn console_read_bytes(buf: &mut [u8]) -> AxResult<usize> {
    let mut kernel_buf = vec![0u8; buf.len()];
    let len = axhal::console::read_bytes(&mut kernel_buf);
    buf.copy_from_slice(&kernel_buf);
    for c in &mut buf[..len] {
        if *c == b'\r' {
            *c = b'\n';
        }
    }
    Ok(len)
}

fn console_write_bytes(buf: &[u8]) -> AxResult<usize> {
    axhal::console::write_bytes(buf);
    Ok(buf.len())
}

/// Represents the standard input (stdin) and output (stdout) for TTY devices
pub struct TtyRaw;

impl Read for TtyRaw {
    fn read(&mut self, buf: &mut [u8]) -> AxResult<usize> {
        let mut read_len = 0;
        while read_len < buf.len() {
            let len = console_read_bytes(buf[read_len..].as_mut())?;
            if len == 0 {
                break;
            }
            read_len += len;
        }
        Ok(read_len)
    }
}

impl Write for TtyRaw {
    fn write(&mut self, buf: &[u8]) -> AxResult<usize> {
        console_write_bytes(buf)
    }

    fn flush(&mut self) -> AxResult {
        Ok(())
    }
}

/// Represents a TTY device in the device filesystem
pub struct Tty {
    stdin: Mutex<BufReader<TtyRaw>>,
    stdout: Mutex<TtyRaw>,
    state: Mutex<TtyState>,
}

impl Tty {
    /// Creates a new TTY device with the specified type
    pub fn new() -> Tty {
        Tty {
            stdin: Mutex::new(BufReader::new(TtyRaw)),
            stdout: Mutex::new(TtyRaw),
            state: Mutex::new(TtyState::new()),
        }
    }

    fn read_blocked(&self, buf: &mut [u8]) -> AxResult<usize> {
        let read_len = self.stdin.lock().read(buf)?;
        if buf.is_empty() || read_len > 0 {
            return Ok(read_len);
        }
        loop {
            let read_len = self.stdin.lock().read(buf)?;
            if read_len > 0 {
                return Ok(read_len);
            }
            axtask::yield_now();
        }
    }
}

impl Default for Tty {
    fn default() -> Self {
        Self::new()
    }
}

impl VfsNodeOps for Tty {
    fn get_attr(&self) -> VfsResult<VfsNodeAttr> {
        let perm = VfsNodePerm::from_bits_truncate(0o777);
        Ok(VfsNodeAttr::new(perm, VfsNodeType::CharDevice, 0, 0))
    }

    fn read_at(&self, _offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        self.read_blocked(buf)
    }

    fn write_at(&self, _offset: u64, buf: &[u8]) -> VfsResult<usize> {
        self.stdout.lock().write(buf)
    }

    fn poll(&self) -> VfsResult<PollState> {
        let mut inner = self.stdin.lock();
        if inner.has_data_left()? {
            return Ok(PollState {
                readable: true,
                writable: false,
            });
        }

        let buf = inner.fill_buf()?;
        if !buf.is_empty() {
            Ok(PollState {
                readable: true,
                writable: false,
            })
        } else {
            Ok(PollState {
                readable: false,
                writable: false,
            })
        }
    }

    fn ioctl(&self, op: usize, arg: *mut u8) -> VfsResult<isize> {
        self.state.lock().ioctl(op as u32, arg)
    }

    axfs_vfs::impl_vfs_non_dir_default! {}
}
