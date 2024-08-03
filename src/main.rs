mod clock;
mod power;
mod cache;
mod app_launcher;
mod gui_trait;
mod eframe_impl;

use gui_trait::GuiFramework;
use eframe_impl::EframeGui;
use app_launcher::AppLauncher;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = Box::new(AppLauncher::default());
    EframeGui::run(app)
}