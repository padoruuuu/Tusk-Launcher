mod clock;
mod power;
mod cache;
mod app_launcher;
mod gui;
mod config;
mod audio;

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write, Seek, SeekFrom};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::process;
use libc;
use crate::gui::EframeGui;
use crate::config::load_config;
use crate::clock::get_current_time;

fn clean_stale_pid_file() -> io::Result<()> {
    if let Ok(mut file) = File::open("/tmp/your_app.pid") {
        let mut content = String::new();
        if file.read_to_string(&mut content).is_ok() {
            if let Ok(pid) = content.trim().parse::<i32>() {
                if unsafe { libc::kill(pid, 0) } != 0 {
                    // Process doesn't exist, remove the file
                    let _ = std::fs::remove_file("/tmp/your_app.pid");
                }
            } else {
                // Invalid PID format, remove the file
                let _ = std::fs::remove_file("/tmp/your_app.pid");
            }
        }
    }
    Ok(())
}

fn acquire_pid_lock() -> io::Result<File> {
    // Clean up any stale PID file first
    clean_stale_pid_file()?;

    let mut pid_file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o644)
        .open("/tmp/tusk-launcher.pid")?;

    // Attempt to acquire an exclusive non-blocking lock
    let ret = unsafe { 
        libc::flock(pid_file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) 
    };

    if ret != 0 {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "Another instance is running"
        ));
    }

    // Write our process id
    let pid = process::id().to_string();
    pid_file.write_all(pid.as_bytes())?;
    pid_file.flush()?;
    
    // Verify what we wrote
    pid_file.seek(SeekFrom::Start(0))?;
    let mut contents = String::new();
    pid_file.read_to_string(&mut contents)?;
    
    if contents.trim() != pid {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "Failed to verify PID write"
        ));
    }

    Ok(pid_file)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    match acquire_pid_lock() {
        Ok(pid_file) => {
            let _config = load_config();  // Prefixed with underscore to indicate intentionally unused
            let current_time = get_current_time(&_config);
            println!("Current time: {}", current_time);
            
            let app = Box::new(app_launcher::AppLauncher::default());
            EframeGui::run(app, pid_file)
        },
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
            // Try to focus existing instance
            match gui::send_focus_signal() {
                Ok(_) => {
                    println!("Successfully focused existing instance");
                    Ok(())
                },
                Err(e) => {
                    eprintln!("Failed to focus existing instance: {}", e);
                    // If we failed to focus, clean up and try to start a new instance
                    clean_stale_pid_file()?;
                    // Try one more time to acquire the lock
                    match acquire_pid_lock() {
                        Ok(pid_file) => {
                            let _config = load_config();  // Prefixed with underscore here too
                            let app = Box::new(app_launcher::AppLauncher::default());
                            EframeGui::run(app, pid_file)
                        },
                        Err(e) => Err(e.into())
                    }
                }
            }
        },
        Err(e) => Err(e.into()),
    }
}