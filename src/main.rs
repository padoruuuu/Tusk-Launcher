mod clock;
mod power;
mod app_cache;
mod gui;
mod audio;
use std::{
    io::{Read, Write, ErrorKind},
    net::{SocketAddr, TcpListener, TcpStream},
    process,
    thread,
    time::Duration,
};
use crate::gui::{EframeGui, load_theme};
use crate::clock::get_current_time;

const PORT: u16 = 42069;
const EXIT_CMD: &[u8] = b"EXIT";
const MAX_RETRY_ATTEMPTS: u32 = 5;
const RETRY_DELAY_MS: u64 = 200;

fn main() {
    let addr = SocketAddr::from(([127, 0, 0, 1], PORT));
    
    // 1) Check if another instance is running and tell it to exit if so
    if let Ok(mut stream) = TcpStream::connect(&addr) {
        println!("Found existing instance, sending exit command...");
        let _ = stream.write_all(EXIT_CMD);
        // Properly close our connection
        let _ = stream.flush();
        drop(stream); // Explicitly drop the connection
        
        println!("Exit command sent. Application is closing the existing instance.");
        // Exit this new instance - don't start a new one
        process::exit(0);
    }

    // 2) Attempt to bind with retries and proper error handling
    let listener = match bind_with_retry(addr, MAX_RETRY_ATTEMPTS) {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!("Failed to bind to port {} after multiple attempts: {}", PORT, e);
            process::exit(1);
        }
    };

    println!("Starting primary instance");
    
    // 4) Set up exit handler
    let exit_listener = match listener.try_clone() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to clone listener: {}", e);
            process::exit(1);
        }
    };
    
    thread::spawn(move || {
        for stream in exit_listener.incoming() {
            if let Ok(mut s) = stream {
                let mut buf = [0u8; 4];
                if s.read(&mut buf).is_ok() && &buf == EXIT_CMD {
                    println!("Exit command received, shutting down");
                    // Send acknowledgment to ensure the sender knows we're exiting
                    let _ = s.write_all(b"ACK");
                    let _ = s.flush();
                    // Properly exit the process
                    process::exit(0);
                }
            }
        }
    });

    // 5) Load theme in background to keep GUI responsive
    thread::spawn(|| {
        let theme = load_theme().unwrap_or_else(|e| {
            eprintln!("Failed to load theme: {}", e);
            process::exit(1);
        });
        let cfg = theme.get_config();
        println!("Current time: {}", get_current_time(&cfg));
    });

    // 6) Run the GUI on the main thread (required for event loop)
    let app = Box::new(app_cache::AppLauncher::default());
    if let Err(e) = EframeGui::run(app) {
        eprintln!("Error running GUI: {}", e);
        process::exit(1);
    }

    println!("Application exiting normally");
}

/// Try to bind to the socket with multiple retries
fn bind_with_retry(addr: SocketAddr, max_attempts: u32) -> Result<TcpListener, std::io::Error> {
    let mut attempt = 0;
    let mut last_error = None;

    while attempt < max_attempts {
        match TcpListener::bind(addr) {
            Ok(listener) => return Ok(listener),
            Err(e) if e.kind() == ErrorKind::AddrInUse => {
                attempt += 1;
                last_error = Some(e);
                println!("Port still in use, retry attempt {}/{}", attempt, max_attempts);
                thread::sleep(Duration::from_millis(RETRY_DELAY_MS));
            },
            Err(e) => return Err(e),
        }
    }

    Err(last_error.unwrap_or_else(|| {
        std::io::Error::new(
            ErrorKind::Other,
            "Failed to bind after maximum retry attempts"
        )
    }))
}
