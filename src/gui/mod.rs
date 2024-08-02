pub mod egui_gui;

use crate::app_launcher::AppLauncher;

pub trait Gui {
    fn run(&mut self) -> eframe::Result<()>;
}