mod clock;
mod power;
mod cache;
mod app_launcher;
mod gui;
mod config;
mod audio;

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write, Seek, SeekFrom, ErrorKind};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::process;
use libc;

use crate::gui::EframeGui;
use crate::config::load_config;
use crate::clock::get_current_time;

const PID_FILE_PATH: &str = "/tmp/tusk-launcher.pid";

struct PidFileGuard {
    _file: File,
}

impl Drop for PidFileGuard {
    fn drop(&mut self) {
        // Remove the PID file when the guard is dropped.
        let _ = std::fs::remove_file(PID_FILE_PATH);
    }
}

fn clean_stale_pid_file() -> io::Result<()> {
    if let Ok(content) = std::fs::read_to_string(PID_FILE_PATH) {
        if let Ok(pid) = content.trim().parse::<i32>() {
            // If kill returns non-zero, the process likely doesn't exist.
            if unsafe { libc::kill(pid, 0) } != 0 {
                let _ = std::fs::remove_file(PID_FILE_PATH);
            }
        } else {
            let _ = std::fs::remove_file(PID_FILE_PATH);
        }
    }
    Ok(())
}

fn acquire_pid_lock() -> io::Result<PidFileGuard> {
    clean_stale_pid_file()?;
    
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o644)
        .open(PID_FILE_PATH)?;

    // Acquire an exclusive, non-blocking lock.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(io::Error::new(ErrorKind::WouldBlock, "Another instance is running"));
    }

    // Now safely truncate and write our PID.
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    let pid = process::id().to_string();
    file.write_all(pid.as_bytes())?;
    file.flush()?;

    // Verify the written PID.
    file.seek(SeekFrom::Start(0))?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    if contents.trim() != pid {
        return Err(io::Error::new(ErrorKind::Other, "Failed to verify PID write"));
    }

    Ok(PidFileGuard { _file: file })
}

fn send_focus_signal() -> Result<(), Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(PID_FILE_PATH)?;
    let content = content.trim();
    if content.is_empty() {
        return Err(Box::new(io::Error::new(io::ErrorKind::InvalidData, "PID file is empty")));
    }
    let pid: libc::pid_t = content.parse().map_err(|e| {
        io::Error::new(io::ErrorKind::InvalidData, format!("Invalid PID format: {}", e))
    })?;

    if unsafe { libc::kill(pid, 0) } != 0 {
        let _ = std::fs::remove_file(PID_FILE_PATH);
        return Err(Box::new(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Process with PID {} does not exist", pid),
        )));
    }

    if unsafe { libc::kill(pid, libc::SIGUSR1) } != 0 {
        return Err(Box::new(io::Error::last_os_error()));
    }

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Attempt to acquire the PID lock.
    let pid_guard = match acquire_pid_lock() {
        Ok(guard) => guard,
        Err(e) if e.kind() == ErrorKind::WouldBlock => {
            // If the lock is held, try to send the focus signal.
            match send_focus_signal() {
                Ok(_) => {
                    println!("Successfully focused existing instance");
                    return Ok(());
                }
                Err(e) => {
                    eprintln!("Failed to focus existing instance: {}. Attempting to recover...", e);
                    clean_stale_pid_file()?;
                    acquire_pid_lock()?
                }
            }
        }
        Err(e) => return Err(e.into()),
    };

    // Keep the pid_guard alive for the duration of the GUI.
    let _pid_guard = pid_guard;

    let config = load_config();
    let current_time = get_current_time(&config);
    println!("Current time: {}", current_time);

    // Launch the GUI.
    let app = Box::new(app_launcher::AppLauncher::default());
    EframeGui::run(app)
}
