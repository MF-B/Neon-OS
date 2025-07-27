//! Device filesystem module

mod tty;
use alloc::sync::Arc;

pub use tty::*;

/// Initialize the device filesystem by setting up /dev directories.
pub fn init_devfs() {
    let opts = axfs::fops::OpenOptions::new().set_read(true);
    let devfs = axfs::fops::Directory::open_dir("/dev", &opts).unwrap();

    let stdin = Tty::stdin();
    let stdout = Tty::stdout();
    let stderr = Tty::stdout();
    let tty = Tty::stdout();
    let _ = devfs.add_node("stdin", Arc::new(stdin));
    let _ = devfs.add_node("stdout", Arc::new(stdout));
    let _ = devfs.add_node("stderr", Arc::new(stderr));
    let _ = devfs.add_node("tty", Arc::new(tty));
}
