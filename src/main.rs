mod clock;
mod power;
mod cache;
mod app_launcher;
mod gui;
mod config;
mod audio;

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::process;
use libc;
use crate::gui::EframeGui;
use crate::config::load_config;
use crate::clock::get_current_time;

fn acquire_pid_lock() -> io::Result<File> {
    let pid_file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)  // Fixed: Truncate the file to clear previous contents
        .mode(0o644)
        .open("/tmp/your_app.pid")?;

    let ret = unsafe { libc::flock(pid_file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if ret != 0 {
        return Err(io::Error::last_os_error());
    }

    write!(&pid_file, "{}", process::id())?;
    Ok(pid_file)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pid_file = match acquire_pid_lock() {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
            if let Err(e) = gui::send_focus_signal() {
                eprintln!("Failed to focus existing instance: {}", e);
            }
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };

    let config = load_config();
    let current_time = get_current_time(&config);
    println!("Current time: {}", current_time);

    let app = Box::new(app_launcher::AppLauncher::default());
    let result = EframeGui::run(app, pid_file);

    result
}