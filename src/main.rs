mod clock;
mod power;
mod cache;
mod app_launcher;
mod gui;
mod audio;

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write, ErrorKind};
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::process::{self, Command, Stdio};
use xdg::BaseDirectories;

use crate::gui::{EframeGui, load_theme}; // load_theme() is a public function re-exporting Theme::load_or_create()
use crate::clock::get_current_time;

/// Determine the PID file path using the xdg runtime directory.
fn get_pid_file_path() -> io::Result<PathBuf> {
    let xdg_dirs = BaseDirectories::with_prefix("Tusk-Launcher")
        .map_err(|e| io::Error::new(ErrorKind::Other, e))?;
    // place_runtime_file creates (if needed) and returns the full path for the given file.
    xdg_dirs.place_runtime_file("tusk-launcher.pid")
}

/// A guard that holds the PID file.
/// When dropped, it removes the PID file.
struct PidFileGuard {
    file: File,
    path: PathBuf,
}

impl Drop for PidFileGuard {
    fn drop(&mut self) {
        // Actively use the file field to ensure the lock remains held until drop.
        let _ = self.file.as_raw_fd();
        // Remove the PID file on drop.
        let _ = fs::remove_file(&self.path);
    }
}

/// Check if a process with the given PID exists by sending a 0 signal.
fn process_exists(pid: u32) -> bool {
    Command::new("kill")
        .args(&["-0", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Read the PID from the given PID file path.
fn read_pid_from_file(path: &PathBuf) -> io::Result<u32> {
    let content = fs::read_to_string(path)?;
    content.trim().parse::<u32>().map_err(|e| {
        io::Error::new(ErrorKind::InvalidData, format!("Failed to parse PID from file: {}", e))
    })
}

/// Acquire a lock by atomically creating a PID file.
/// If the file already exists, check if the process is running.
/// If it is, attempt to send a focus signal and exit;
/// if not, remove the stale file and retry.
fn acquire_pid_lock() -> io::Result<PidFileGuard> {
    let pid_path = get_pid_file_path()?;
    loop {
        match OpenOptions::new().write(true).create_new(true).open(&pid_path) {
            Ok(mut file) => {
                // Successfully created the PID file; write our PID.
                let pid = process::id();
                file.write_all(pid.to_string().as_bytes())?;
                file.flush()?;
                return Ok(PidFileGuard { file, path: pid_path });
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                // PID file already exists; check if the process is alive.
                match read_pid_from_file(&pid_path) {
                    Ok(existing_pid) => {
                        if process_exists(existing_pid) {
                            // Send a focus signal to the running instance.
                            if send_focus_signal(existing_pid).is_ok() {
                                println!("Focused existing instance (PID {})", existing_pid);
                                std::process::exit(0);
                            }
                            return Err(io::Error::new(ErrorKind::AlreadyExists, "Instance already running"));
                        } else {
                            // Stale PID file found; remove it and retry.
                            let _ = fs::remove_file(&pid_path);
                            continue;
                        }
                    }
                    Err(_) => {
                        // If we can’t read the PID, assume the file is stale.
                        let _ = fs::remove_file(&pid_path);
                        continue;
                    }
                }
            }
            Err(e) => return Err(e),
        }
    }
}

/// Send a focus signal (SIGUSR1) to the process with the given PID.
fn send_focus_signal(pid: u32) -> io::Result<()> {
    let status = Command::new("kill")
        .args(&["-SIGUSR1", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::new(ErrorKind::Other, "Failed to send focus signal"))
    }
}

fn main() {
    // Attempt to acquire the PID lock.
    let _pid_guard = match acquire_pid_lock() {
        Ok(guard) => guard,
        Err(e) => {
            eprintln!("Failed to acquire PID lock: {}", e);
            std::process::exit(1);
        }
    };

    // Load the theme (which includes both theme rules and configuration).
    let theme = load_theme().expect("Failed to load theme");
    let config = theme.get_config();
    println!("Current time: {}", get_current_time(&config));

    let app = Box::new(app_launcher::AppLauncher::default());
    if let Err(e) = EframeGui::run(app) {
        eprintln!("Error running application: {}", e);
        std::process::exit(1);
    }
}
