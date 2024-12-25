mod clock;
mod power;
mod cache;
mod app_launcher;
mod gui;
mod config;
mod audio;

use app_launcher::AppLauncher;
use crate::gui::{EframeGui, GuiFramework};
use crate::config::load_config; // To load the config
use crate::clock::get_current_time; // To get the current time

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load the configuration
    let config = load_config();

    // Get and display the current time based on the configuration's timezone
    let current_time = get_current_time(&config);
    println!("Current time: {}", current_time);

    // Initialize and run the GUI
    let app = Box::new(AppLauncher::default());
    EframeGui::run(app)
}
