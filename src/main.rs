mod clock;
mod power;
mod cache;
mod app_launcher;
mod gui;
mod config;
mod audio;

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write, Seek, SeekFrom, ErrorKind};
use std::os::unix::fs::OpenOptionsExt;
use std::process::{self, Command, Stdio};
use std::path::Path;

use crate::gui::EframeGui;
use crate::config::load_config;
use crate::clock::get_current_time;

const PID_FILE_PATH: &str = "/tmp/tusk-launcher.pid";

struct PidFileGuard {
    file: File,
}

impl Drop for PidFileGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(PID_FILE_PATH);
    }
}

/// Check if a process with the given PID exists
fn process_exists(pid: u32) -> bool {
    Command::new("kill")
        .args(&["-0", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn clean_stale_pid_file() -> io::Result<()> {
    if let Ok(content) = fs::read_to_string(PID_FILE_PATH) {
        if let Ok(pid) = content.trim().parse::<u32>() {
            if !process_exists(pid) {
                let _ = fs::remove_file(PID_FILE_PATH);
            }
        } else {
            // Invalid PID format
            let _ = fs::remove_file(PID_FILE_PATH);
        }
    }
    Ok(())
}

fn acquire_pid_lock() -> io::Result<PidFileGuard> {
    clean_stale_pid_file()?;
    
    // Try to create the PID file exclusively
    let file = match OpenOptions::new()
        .write(true)
        .read(true)
        .create_new(true)  // This is crucial - fails if file exists
        .mode(0o644)
        .open(PID_FILE_PATH) {
            Ok(file) => file,
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                // Check if the PID file belongs to a running process
                if let Ok(content) = fs::read_to_string(PID_FILE_PATH) {
                    if let Ok(pid) = content.trim().parse::<u32>() {
                        if process_exists(pid) {
                            return Err(io::Error::new(ErrorKind::WouldBlock, "Another instance is running"));
                        }
                    }
                }
                
                // Stale PID file, remove it and retry
                let _ = fs::remove_file(PID_FILE_PATH);
                
                OpenOptions::new()
                    .write(true)
                    .read(true)
                    .create_new(true)
                    .mode(0o644)
                    .open(PID_FILE_PATH)?
            },
            Err(e) => return Err(e),
        };
    
    // Write PID
    let pid = process::id().to_string();
    let mut file = file;
    file.write_all(pid.as_bytes())?;
    file.flush()?;

    // Verify
    file.seek(SeekFrom::Start(0))?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    if contents.trim() != pid {
        return Err(io::Error::new(ErrorKind::Other, "Failed to verify PID write"));
    }

    Ok(PidFileGuard { file })
}

fn send_focus_signal() -> Result<(), Box<dyn std::error::Error>> {
    if !Path::new(PID_FILE_PATH).exists() {
        return Err(Box::new(io::Error::new(
            io::ErrorKind::NotFound,
            "PID file does not exist"
        )));
    }

    let content = fs::read_to_string(PID_FILE_PATH)?.trim().to_string();
    if content.is_empty() {
        return Err(Box::new(io::Error::new(io::ErrorKind::InvalidData, "PID file is empty")));
    }
    
    let pid: u32 = content.parse().map_err(|e| {
        io::Error::new(io::ErrorKind::InvalidData, format!("Invalid PID format: {}", e))
    })?;

    // Check if process exists
    if !process_exists(pid) {
        let _ = fs::remove_file(PID_FILE_PATH);
        return Err(Box::new(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Process with PID {} does not exist", pid),
        )));
    }

    // Send SIGUSR1 signal using the kill command
    let status = Command::new("kill")
        .args(&["-SIGUSR1", &pid.to_string()])
        .status()
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
    
    if !status.success() {
        return Err(Box::new(io::Error::new(
            io::ErrorKind::Other, 
            format!("Failed to send signal to process {}", pid)
        )));
    }

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Attempt to acquire the PID lock
    let pid_guard = match acquire_pid_lock() {
        Ok(guard) => guard,
        Err(e) if e.kind() == ErrorKind::WouldBlock => {
            // If the lock is held, try to send the focus signal
            match send_focus_signal() {
                Ok(_) => {
                    println!("Successfully focused existing instance");
                    return Ok(());
                }
                Err(e) => {
                    eprintln!("Failed to focus existing instance: {}. Attempting to recover...", e);
                    clean_stale_pid_file()?;
                    match acquire_pid_lock() {
                        Ok(guard) => guard,
                        Err(e) => return Err(e.into()),
                    }
                }
            }
        }
        Err(e) => return Err(e.into()),
    };

    // Keep the pid_guard alive
    let _pid_guard = pid_guard;

    // Launch the application
    let config = load_config();
    let current_time = get_current_time(&config);
    println!("Current time: {}", current_time);

    let app = Box::new(app_launcher::AppLauncher::default());
    EframeGui::run(app)
}