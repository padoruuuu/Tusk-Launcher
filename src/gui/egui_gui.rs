use crate::gui::Gui;
use crate::app_launcher::AppLauncher;
use eframe::egui;
use std::process::Command;
use chrono::prelude::*;
use std::time::SystemTime;

#[derive(Clone)]
pub struct EguiGui {
    app_launcher: AppLauncher,
    query: String,
    search_results: Vec<(String, String)>,
    is_quit: bool,
    focus_set: bool,
}

impl EguiGui {
    pub fn new(app_launcher: AppLauncher) -> Self {
        Self {
            app_launcher,
            query: String::new(),
            search_results: Vec::new(),
            is_quit: false,
            focus_set: false,
        }
    }
}

impl Gui for EguiGui {
    fn run(&mut self) -> eframe::Result<()> {
        let native_options = eframe::NativeOptions::default();
        eframe::run_native(
            "Application Launcher",
            native_options,
            Box::new(|_cc| Box::new(self.clone())),
        )
    }
}

impl eframe::App for EguiGui {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint();

        if self.is_quit {
            std::process::exit(0);
        }

        ctx.input(|i| {
            if i.key_pressed(egui::Key::Escape) {
                self.is_quit = true;
            }
            if i.key_pressed(egui::Key::Enter) {
                if let Some((app_name, exec_cmd)) = self.search_results.first() {
                    if let Err(err) = self.app_launcher.launch_app(app_name, exec_cmd) {
                        eprintln!("Failed to launch app: {}", err);
                    } else {
                        self.is_quit = true;
                    }
                }
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                let response = ui.add(egui::TextEdit::singleline(&mut self.query).hint_text("Search..."));
                
                if !self.focus_set {
                    response.request_focus();
                    self.focus_set = true;
                }

                if response.changed() {
                    self.search_results = self.app_launcher.search_applications(&self.query);
                }

                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (app_name, exec_cmd) in &self.search_results {
                        if ui.button(app_name).clicked() {
                            if let Err(err) = self.app_launcher.launch_app(app_name, exec_cmd) {
                                eprintln!("Failed to launch app: {}", err);
                            } else {
                                self.is_quit = true;
                            }
                        }
                    }
                });
            });

            ui.add_space(ui.available_height() - 100.0);

            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                ui.horizontal(|ui| {
                    if ui.button("Power").clicked() {
                        Command::new("shutdown").arg("-h").arg("now").spawn().expect("Failed to execute shutdown command");
                    }
                    if ui.button("Restart").clicked() {
                        Command::new("reboot").spawn().expect("Failed to execute reboot command");
                    }
                    if ui.button("Logout").clicked() {
                        Command::new("logout").spawn().expect("Failed to execute logout command");
                    }
                });

                ui.separator();

                let datetime: DateTime<Local> = SystemTime::now().into();
                ui.label(datetime.format("%I:%M %p %m/%d/%Y").to_string());
            });
        });

        ctx.output_mut(|o| o.cursor_icon = egui::CursorIcon::Default);
    }
}