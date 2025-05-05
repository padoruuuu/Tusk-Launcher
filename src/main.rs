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
};

use crate::gui::{EframeGui, load_theme};
use crate::clock::get_current_time;

const PORT: u16 = 42069;
const EXIT_CMD: &[u8] = b"EXIT";

fn main() {
    let addr = SocketAddr::from(([127, 0, 0, 1], PORT));

    // 1) Tell any existing instance to exit (best‑effort).
    if let Ok(mut s) = TcpStream::connect(addr) {
        let _ = s.write_all(EXIT_CMD);
    }

    // 2) Busy‑retry bind until the old instance truly releases the port.
    let listener = loop {
        match TcpListener::bind(addr) {
            Ok(l) => break l,
            Err(e) if e.kind() == ErrorKind::AddrInUse => {
                // spin until the port is free
                continue;
            }
            Err(e) => {
                eprintln!("Failed to bind control port {}: {}", PORT, e);
                process::exit(1);
            }
        }
    };

    println!("Starting primary instance");

    // 3) Spawn a thread to handle future EXIT_CMDs
    let exit_listener = listener.try_clone().expect("Failed to clone listener");
    thread::spawn(move || {
        for stream in exit_listener.incoming() {
            if let Ok(mut s) = stream {
                let mut buf = [0u8; 4];
                if s.read(&mut buf).is_ok() && &buf == EXIT_CMD {
                    println!("Exit command received, shutting down");
                    process::exit(0);
                }
            }
        }
    });

    // 4) Spawn a thread to load theme & print time so GUI comes up instantly
    thread::spawn(|| {
        let theme = load_theme().unwrap_or_else(|e| {
            eprintln!("Failed to load theme: {}", e);
            process::exit(1);
        });
        let cfg = theme.get_config();
        println!("Current time: {}", get_current_time(&cfg));
    });

    // 5) Run the GUI on the main thread (required for event loop)
    let app = Box::new(app_cache::AppLauncher::default());
    if let Err(e) = EframeGui::run(app) {
        eprintln!("Error running GUI: {}", e);
        process::exit(1);
    }

    println!("Application exiting normally");
}
