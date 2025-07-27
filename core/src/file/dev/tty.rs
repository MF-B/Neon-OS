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

struct TtyStdinRaw;
struct TtyStdoutRaw;

impl Read for TtyStdinRaw {
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

impl Write for TtyStdoutRaw {
    fn write(&mut self, buf: &[u8]) -> AxResult<usize> {
        console_write_bytes(buf)
    }

    fn flush(&mut self) -> AxResult {
        Ok(())
    }
}

/// Represents the standard input (stdin) and output (stdout) for TTY devices
pub struct TtyStdin {
    inner: &'static Mutex<BufReader<TtyStdinRaw>>,
    state: Mutex<TtyState>,
}

impl TtyStdin {
    fn read_blocked(&self, buf: &mut [u8]) -> AxResult<usize> {
        let read_len = self.inner.lock().read(buf)?;
        if buf.is_empty() || read_len > 0 {
            return Ok(read_len);
        }
        loop {
            let read_len = self.inner.lock().read(buf)?;
            if read_len > 0 {
                return Ok(read_len);
            }
            axtask::yield_now();
        }
    }

    /// Handles polling for TTY stdin.
    pub fn poll(&self) -> VfsResult<PollState> {
        let mut inner = self.inner.lock();
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

    /// Handles ioctl commands for TTY stdin.
    pub fn ioctl(&self, cmd: usize, arg: *mut u8) -> VfsResult<isize> {
        self.state.lock().ioctl(cmd as u32, arg)
    }
}

/// Represents the standard output (stdout) for TTY devices
pub struct TtyStdout {
    inner: &'static Mutex<TtyStdoutRaw>,
    state: Mutex<TtyState>,
}

impl TtyStdout {
    /// Handles polling for TTY stdout.
    pub fn poll(&self) -> VfsResult<PollState> {
        Ok(PollState {
            readable: false,
            writable: true,
        })
    }

    /// Handles ioctl commands for TTY stdout.
    pub fn ioctl(&self, cmd: usize, arg: *mut u8) -> VfsResult<isize> {
        self.state.lock().ioctl(cmd as u32, arg)
    }

    fn write_bytes(&self, buf: &[u8]) -> VfsResult<usize> {
        self.inner.lock().write(buf)
    }
}

fn tty_stdin() -> TtyStdin {
    static INSTANCE: Mutex<BufReader<TtyStdinRaw>> = Mutex::new(BufReader::new(TtyStdinRaw));
    TtyStdin {
        inner: &INSTANCE,
        state: Mutex::new(TtyState::new()),
    }
}

fn tty_stdout() -> TtyStdout {
    static INSTANCE: Mutex<TtyStdoutRaw> = Mutex::new(TtyStdoutRaw);
    TtyStdout {
        inner: &INSTANCE,
        state: Mutex::new(TtyState::new()),
    }
}

/// Represents the type of TTY device
pub enum TtyType {
    /// Standard input (stdin)
    Stdin,
    /// Standard output (stdout)
    Stdout,
}

/// Represents a TTY device in the device filesystem
pub struct Tty {
    tty_type: TtyType,
}

impl Tty {
    /// Creates a new TTY device with the specified type
    pub fn new(tty_type: TtyType) -> Tty {
        Tty { tty_type }
    }

    /// Returns the TTY type
    pub fn stdin() -> Tty {
        Self::new(TtyType::Stdin)
    }

    /// Returns the TTY type for stdout
    pub fn stdout() -> Tty {
        Self::new(TtyType::Stdout)
    }
}

impl Default for Tty {
    fn default() -> Self {
        Self::new(TtyType::Stdin)
    }
}

impl VfsNodeOps for Tty {
    fn get_attr(&self) -> VfsResult<VfsNodeAttr> {
        let perm = match self.tty_type {
            TtyType::Stdin => VfsNodePerm::from_bits_truncate(0o444), // r--r--r--
            TtyType::Stdout => VfsNodePerm::from_bits_truncate(0o220), // -w--w----
        };
        Ok(VfsNodeAttr::new(perm, VfsNodeType::CharDevice, 0, 0))
    }

    fn read_at(&self, _offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        match self.tty_type {
            TtyType::Stdin => {
                let stdin = tty_stdin();
                stdin.read_blocked(buf)
            }
            TtyType::Stdout => ax_err!(PermissionDenied), // stdout can't be read
        }
    }

    fn write_at(&self, _offset: u64, buffer: &[u8]) -> VfsResult<usize> {
        match self.tty_type {
            TtyType::Stdin => ax_err!(PermissionDenied), // stdin can't be written
            TtyType::Stdout => {
                let stdout = tty_stdout();
                stdout.write_bytes(buffer)
            }
        }
    }

    fn poll(&self) -> VfsResult<PollState> {
        match self.tty_type {
            TtyType::Stdin => {
                let stdin = tty_stdin();
                stdin.poll()
            }
            TtyType::Stdout => {
                let stdout = tty_stdout();
                stdout.poll()
            }
        }
    }

    fn ioctl(&self, cmd: usize, arg: *mut u8) -> VfsResult<isize> {
        match self.tty_type {
            TtyType::Stdin => {
                let stdin = tty_stdin();
                Ok(stdin.ioctl(cmd, arg)?)
            }
            TtyType::Stdout => {
                let stdout = tty_stdout();
                Ok(stdout.ioctl(cmd, arg)?)
            }
        }
    }

    axfs_vfs::impl_vfs_non_dir_default! {}
}
