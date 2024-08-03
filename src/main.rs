mod clock;
mod power;
mod cache;
mod app_launcher;

use eframe::egui;
use app_launcher::AppLauncher;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        ..Default::default()
    };
    eframe::run_native(
        "Application Launcher",
        native_options,
        Box::new(|_cc| Box::new(AppLauncher::default())),
    )
}
