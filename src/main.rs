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

struct PidFileGuard {
    _file: File,
}

impl Drop for PidFileGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file("/tmp/tusk-launcher.pid");
    }
}

fn clean_stale_pid_file() -> io::Result<()> {
    if let Ok(mut file) = File::open("/tmp/tusk-launcher.pid") {
        let mut content = String::new();
        if file.read_to_string(&mut content).is_ok() {
            if let Ok(pid) = content.trim().parse::<i32>() {
                if unsafe { libc::kill(pid, 0) } != 0 {
                    let _ = std::fs::remove_file("/tmp/tusk-launcher.pid");
                }
            } else {
                let _ = std::fs::remove_file("/tmp/tusk-launcher.pid");
            }
        }
    }
    Ok(())
}

fn acquire_pid_lock() -> io::Result<PidFileGuard> {
    clean_stale_pid_file()?;

    let mut pid_file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o644)
        .open("/tmp/tusk-launcher.pid")?;

    let ret = unsafe { 
        libc::flock(pid_file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) 
    };

    if ret != 0 {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "Another instance is running"
        ));
    }

    let pid = process::id().to_string();
    pid_file.write_all(pid.as_bytes())?;
    pid_file.flush()?;
    
    pid_file.seek(SeekFrom::Start(0))?;
    let mut contents = String::new();
    pid_file.read_to_string(&mut contents)?;
    
    if contents.trim() != pid {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "Failed to verify PID write"
        ));
    }

    Ok(PidFileGuard { _file: pid_file })
}

fn send_focus_signal() -> Result<(), Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string("/tmp/tusk-launcher.pid")?;
    let content = content.trim();

    if content.is_empty() {
        return Err(Box::new(io::Error::new(
            io::ErrorKind::InvalidData,
            "PID file is empty"
        )));
    }

    let pid: libc::pid_t = content.parse().map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Invalid PID format: {}", e)
        )
    })?;

    if unsafe { libc::kill(pid, 0) } != 0 {
        let _ = std::fs::remove_file("/tmp/tusk-launcher.pid");
        return Err(Box::new(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Process with PID {} does not exist", pid)
        )));
    }

    if unsafe { libc::kill(pid, libc::SIGUSR1) } != 0 {
        return Err(Box::new(std::io::Error::last_os_error()));
    }

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    match acquire_pid_lock() {
        Ok(_pid_guard) => {
            let config = load_config();
            let current_time = get_current_time(&config);
            println!("Current time: {}", current_time);
            
            let app = Box::new(app_launcher::AppLauncher::default());
            EframeGui::run(app)
        },
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
            match send_focus_signal() {
                Ok(_) => {
                    println!("Successfully focused existing instance");
                    Ok(())
                },
                Err(e) => {
                    eprintln!("Failed to focus existing instance: {}", e);
                    clean_stale_pid_file()?;
                    match acquire_pid_lock() {
                        Ok(_pid_guard) => {
                            let _config = load_config();
                            let app = Box::new(app_launcher::AppLauncher::default());
                            EframeGui::run(app)
                        },
                        Err(e) => Err(e.into())
                    }
                }
            }
        },
        Err(e) => Err(e.into()),
    }
}
