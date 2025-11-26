mod clock;
mod power;
mod app_launcher;
mod gui;
mod audio;

use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    process,
    thread,
};
use crate::gui::{EframeGui, load_theme};
use crate::clock::get_current_time;

const PORT: u16 = 42069;
const EXIT_CMD: &[u8] = b"EXIT";

fn main() {
    let addr = SocketAddr::from(([127, 0, 0, 1], PORT));
    
    // Check if another instance is running
    if let Ok(mut stream) = TcpStream::connect(&addr) {
        // Found another instance, tell it to exit
        let _ = stream.write_all(EXIT_CMD);
        let _ = stream.flush();
        return; // Exit this instance
    }

    // Try to start our instance
    let listener = match TcpListener::bind(addr) {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!("Failed to bind to port {}: {}", PORT, e);
            process::exit(1);
        }
    };

    // Set up exit handler
    thread::spawn(move || {
        for stream in listener.incoming() {
            if let Ok(mut stream) = stream {
                let mut buf = [0u8; 4];
                if stream.read(&mut buf).is_ok() && &buf == EXIT_CMD {
                    println!("Exit command received, shutting down");
                    process::exit(0);
                }
            }
        }
    });

    // Load theme and run GUI
    let theme = load_theme();
    println!("Current time: {}", get_current_time(&theme.get_config()));
    
    // Run the GUI on the main thread
    let app = Box::new(app_launcher::AppLauncher::default());
    if let Err(e) = EframeGui::run(app) {
        eprintln!("Error running GUI: {}", e);
        process::exit(1);
    }

    println!("Application exiting normally");
}