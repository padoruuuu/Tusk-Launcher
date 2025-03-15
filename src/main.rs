mod clock;
mod power;
mod cache;
mod app_launcher;
mod gui;
mod config;
mod audio;

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write, ErrorKind};
use std::os::unix::fs::OpenOptionsExt;
use std::process::{self, Command, Stdio};

use crate::gui::EframeGui;
use crate::config::load_config;
use crate::clock::get_current_time;

const PID_FILE_PATH: &str = "/tmp/tusk-launcher.pid";

#[allow(dead_code)] // `file` is used in `Drop` for cleanup
struct PidFileGuard {
    file: File,
}

impl Drop for PidFileGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(PID_FILE_PATH); // Fast cleanup on exit
    }
}

/// Check if a process exists using `kill -0`.
fn process_exists(pid: u32) -> bool {
    Command::new("kill")
        .args(&["-0", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Get the PID if the process is running, else clean up and return None.
fn get_running_pid() -> io::Result<Option<u32>> {
    match fs::read_to_string(PID_FILE_PATH) {
        Ok(content) => match content.trim().parse::<u32>() {
            Ok(pid) => {
                if process_exists(pid) {
                    Ok(Some(pid))
                } else {
                    let _ = fs::remove_file(PID_FILE_PATH); // Remove stale PID file
                    Ok(None)
                }
            }
            Err(_) => {
                let _ = fs::remove_file(PID_FILE_PATH); // Remove invalid PID file
                Ok(None)
            }
        },
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Acquire PID lock, removing stale files and retrying on race conditions.
fn acquire_pid_lock() -> io::Result<PidFileGuard> {
    loop {
        match get_running_pid()? {
            Some(pid) => return Err(io::Error::new(ErrorKind::WouldBlock, format!("Instance running with PID {}", pid))),
            None => match OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o644)
                .open(PID_FILE_PATH)
            {
                Ok(mut file) => {
                    file.write_all(process::id().to_string().as_bytes())?;
                    file.flush()?;
                    return Ok(PidFileGuard { file });
                }
                Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                    // Race condition: another process wrote the file; check again
                    if let Some(_) = get_running_pid()? {
                        return Err(io::Error::new(ErrorKind::WouldBlock, "Instance running"));
                    }
                    let _ = fs::remove_file(PID_FILE_PATH); // Force remove if stale
                    continue;
                }
                Err(e) => return Err(e),
            },
        }
    }
}

/// Send SIGUSR1 to focus an existing instance.
fn send_focus_signal(pid: u32) -> io::Result<()> {
    let status = Command::new("kill")
        .args(&["-SIGUSR1", &pid.to_string()])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::new(ErrorKind::Other, "Failed to send focus signal"))
    }
}

fn main() {
    let pid_guard = match acquire_pid_lock() {
        Ok(guard) => guard,
        Err(e) if e.kind() == ErrorKind::WouldBlock => {
            if let Ok(Some(pid)) = get_running_pid() {
                if send_focus_signal(pid).is_ok() {
                    println!("Focused existing instance");
                    std::process::exit(0);
                }
            }
            match acquire_pid_lock() {
                Ok(guard) => guard,
                Err(e) => {
                    eprintln!("Failed to acquire PID lock after focus attempt: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to acquire PID lock: {}", e);
            std::process::exit(1);
        }
    };

    let _pid_guard = pid_guard;

    let config = load_config();
    println!("Current time: {}", get_current_time(&config));

    let app = Box::new(app_launcher::AppLauncher::default());
    if let Err(e) = EframeGui::run(app) {
        eprintln!("Error running application: {}", e);
        std::process::exit(1);
    }
}
