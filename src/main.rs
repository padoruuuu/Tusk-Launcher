mod app_launcher;
mod gui;
mod desktop_entry;
mod recent_apps;
mod utils;

use app_launcher::AppLauncher;
use gui::egui_gui::EguiGui;

fn main() -> eframe::Result<()> {
    let app_launcher = AppLauncher::new();
    let mut gui = EguiGui::new(app_launcher);
    gui.run()
}