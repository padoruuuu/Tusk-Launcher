mod clock;
mod power;
mod cache;
mod app_launcher;
mod gui;
mod audio;

use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use std::io::{self, ErrorKind};

use crate::gui::{EframeGui, load_theme};
use crate::clock::get_current_time;

// Using a local socket for instance management instead of a PID file
use std::net::{TcpListener, TcpStream};
use std::io::{Read, Write};

const PORT: u16 = 41758; // Arbitrary port number for our app
const CMD_FOCUS: &[u8] = b"FOCUS";
const CMD_CLOSE: &[u8] = b"CLOSE";
const CMD_TOGGLE: &[u8] = b"TOGGL";
const RESP_OK: &[u8] = b"DONE!";

struct InstanceServer {
    listener: Option<TcpListener>,
    shutdown: Arc<AtomicBool>,
}

impl InstanceServer {
    fn new() -> io::Result<Self> {
        // Try to bind to the port - if it fails, another instance is already running
        let listener = match TcpListener::bind(("127.0.0.1", PORT)) {
            Ok(listener) => listener,
            Err(e) if e.kind() == ErrorKind::AddrInUse => {
                // Another instance is already listening
                return Err(io::Error::new(ErrorKind::AddrInUse, "Another instance is already running"));
            },
            Err(e) => return Err(e),
        };
        
        // Make listener non-blocking
        listener.set_nonblocking(true)?;
        
        Ok(Self {
            listener: Some(listener),
            shutdown: Arc::new(AtomicBool::new(false)),
        })
    }
    
    fn run(&mut self) -> io::Result<()> {
        let listener = self.listener.take().expect("Listener was already taken");
        let shutdown = self.shutdown.clone();
        
        // Spawn a thread to handle incoming connections
        thread::spawn(move || {
            while !shutdown.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let mut buffer = [0; 5];
                        if let Ok(_) = stream.read_exact(&mut buffer) {
                            if &buffer == CMD_FOCUS {
                                // Send application to foreground
                                Self::bring_to_foreground();
                                // Confirm to the client
                                let _ = stream.write_all(RESP_OK);
                            } else if &buffer == CMD_CLOSE || &buffer == CMD_TOGGLE {
                                println!("Received close/toggle command");
                                // Confirm receipt before closing
                                let _ = stream.write_all(RESP_OK);
                                // Set shutdown flag
                                shutdown.store(true, Ordering::Relaxed);
                                // Exit the application
                                thread::spawn(|| {
                                    thread::sleep(Duration::from_millis(100));
                                    println!("Exiting on request");
                                    process::exit(0);
                                });
                                break;
                            }
                        }
                    },
                    Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                        // No connections available right now
                        thread::sleep(Duration::from_millis(50));
                        continue;
                    },
                    Err(_) => break,
                }
            }
        });
        
        Ok(())
    }
    
    fn bring_to_foreground() {
        // This would use whatever windowing system mechanism is appropriate
        // For example, sending a SIGUSR1 to self, or using X11/Wayland APIs
        
        // For demonstration, we'll simulate by printing a message
        println!("Instance requested to come to foreground");
        
        // In a real application, you would handle foreground focus using GUI toolkit mechanisms
        // For example:
        // - Using egui's request_focus() method
        // - Sending a signal to the main thread
        // - Using X11/Wayland APIs to raise the window
    }
}

impl Drop for InstanceServer {
    fn drop(&mut self) {
        // Signal the thread to shut down
        self.shutdown.store(true, Ordering::Relaxed);
        // Let the thread finish
        thread::sleep(Duration::from_millis(100));
    }
}

fn main() {
    // Check if we're being asked to perform a specific action
    let command = std::env::args().nth(1).unwrap_or_default();
    
    // Connect to any existing instance and handle according to command
    match TcpStream::connect(("127.0.0.1", PORT)) {
        Ok(mut stream) => {
            // If no specific command was given, treat a second launch as a close command
            // This ensures launching while open closes the app
            let cmd = match command.as_str() {
                "--close" => CMD_CLOSE,
                "--toggle" => CMD_TOGGLE,
                "--focus" => CMD_FOCUS,
                _ => {
                    // Default behavior: closing existing instance
                    println!("Found running instance, sending close command...");
                    CMD_CLOSE
                }
            };
            
            // Send the command
            if let Err(e) = stream.write_all(cmd) {
                eprintln!("Failed to send command: {}", e);
                std::process::exit(1);
            }
            
            // Wait for confirmation or timeout
            let mut buffer = [0; 5];
            match stream.read_exact(&mut buffer) {
                Ok(_) => {
                    if &buffer == RESP_OK {
                        if cmd == CMD_CLOSE {
                            println!("Close command acknowledged, existing instance will shut down");
                        } else if cmd == CMD_FOCUS {
                            println!("Focus command acknowledged, bringing existing instance to front");
                        } else {
                            println!("Command acknowledged by existing instance");
                        }
                    } else {
                        println!("Got unexpected response from existing instance");
                    }
                },
                Err(e) => {
                    // This could happen if the instance closes quickly
                    println!("Connection closed: {}", e);
                }
            }
            
            // For any command to an existing instance, we exit
            // This ensures that after closing, a new launch is required
            std::process::exit(0);
        },
        Err(_) => {
            // No instance running
            if command == "--close" {
                println!("No running instance found");
                std::process::exit(0);
            }
            // For other commands, continue to start a new instance
        }
    }
    
    // Set up our instance server
    // First handle the case where an instance might be in the middle of shutting down
    let mut retry_count = 0;
    let max_retries = 3;
    
    let mut server = loop {
        match InstanceServer::new() {
            Ok(server) => break server,
            Err(e) => {
                eprintln!("Failed to set up instance server: {}", e);
                
                // If port is in use and we haven't reached max retries, wait and try again
                if e.kind() == ErrorKind::AddrInUse && retry_count < max_retries {
                    retry_count += 1;
                    eprintln!("Another instance may be shutting down. Waiting... (attempt {}/{})", 
                             retry_count, max_retries);
                    thread::sleep(Duration::from_millis(500));
                    continue;
                }
                
                // If we get here, we've either reached max retries or encountered a different error
                if e.kind() == ErrorKind::AddrInUse {
                    eprintln!("Another instance is still running. Please close it first or try again later.");
                } else {
                    eprintln!("Unexpected error: {}", e);
                }
                std::process::exit(1);
            }
        }
    };
    
    if let Err(e) = server.run() {
        eprintln!("Failed to run instance server: {}", e);
        std::process::exit(1);
    }

    // Load the theme (which includes both theme rules and configuration)
    let theme = load_theme().expect("Failed to load theme");
    let config = theme.get_config();
    println!("Current time: {}", get_current_time(&config));

    let app = Box::new(app_launcher::AppLauncher::default());
    if let Err(e) = EframeGui::run(app) {
        eprintln!("Error running application: {}", e);
        std::process::exit(1);
    }
}