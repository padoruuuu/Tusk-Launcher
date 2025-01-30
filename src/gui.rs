use std::error::Error;
use eframe::egui;
use crate::{config::Config, app_launcher::AppLaunchOptions, audio::AudioController};

pub trait AppInterface {
    fn update(&mut self);
    fn handle_input(&mut self, input: &str);
    fn should_quit(&self) -> bool;
    fn get_query(&self) -> String;
    fn get_search_results(&self) -> Vec<String>;
    fn get_time(&self) -> String;
    fn launch_app(&mut self, app_name: &str);
    fn get_config(&self) -> &Config;
    fn start_launch_options_edit(&mut self, app_name: &str) -> String;
    fn get_launch_options(&self, app_name: &str) -> Option<&AppLaunchOptions>;
}

pub struct EframeGui;

impl EframeGui {
    pub fn run(app: Box<dyn AppInterface>) -> Result<(), Box<dyn Error>> {
        let native_options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([350.0, 500.0])
                .with_always_on_top()
                .with_decorations(true),
            ..Default::default()
        };

        let audio_controller = {
            let config = app.get_config();
            let ctrl = AudioController::new(config)?;
            ctrl.start_polling(config);
            ctrl
        };

        eframe::run_native(
            "Application Launcher",
            native_options,
            Box::new(|cc| {
                cc.egui_ctx.request_repaint();
                Box::new(EframeWrapper {
                    app,
                    audio_controller,
                    current_volume: 0.0,
                    editing: None,
                    focused: false,
                })
            }),
        )?;
        Ok(())
    }
}

struct EframeWrapper {
    app: Box<dyn AppInterface>,
    audio_controller: AudioController,
    current_volume: f32,
    editing: Option<(String, String)>,
    focused: bool,
}

impl eframe::App for EframeWrapper {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.app.update();
        
        if self.audio_controller.update_volume().is_ok() {
            self.current_volume = self.audio_controller.get_volume();
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical(|ui| {
                // Search bar at the top
                let mut query = self.app.get_query();
                let response = ui.add(egui::TextEdit::singleline(&mut query).hint_text("Search..."));
                if !self.focused {
                    response.request_focus();
                    self.focused = true;
                }
                if response.changed() && !query.starts_with("LAUNCH_OPTIONS:") {
                    self.app.handle_input(&query);
                }
                ui.add_space(10.0);

                // Application results
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for result in self.app.get_search_results() {
                        ui.horizontal(|ui| {
                            if ui.button(&result).clicked() {
                                self.app.launch_app(&result);
                            }
                            
                            if ui.button("⚙").clicked() && self.editing.is_none() {
                                let options = match self.app.get_launch_options(&result) {
                                    Some(_) => self.app.start_launch_options_edit(&result),
                                    None => String::new(),
                                };
                                self.editing = Some((result, options));
                            }
                        });
                    }
                });

                // Push everything else to the bottom using available space
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    // Power options at the bottom
                    if self.app.get_config().enable_power_options {
                        ui.horizontal(|ui| {
                            for (label, cmd) in [("Power", "P"), ("Restart", "R"), ("Logout", "L")] {
                                if ui.button(label).clicked() {
                                    self.app.handle_input(cmd);
                                }
                            }
                        });
                        ui.add_space(5.0);
                    }

                    // Volume control above power options
                    if self.audio_controller.is_enabled() {
                        ui.horizontal(|ui| {
                            ui.label("Volume:");
                            let mut volume = self.current_volume;
                            if ui.add(egui::Slider::new(&mut volume, 0.0..=self.app.get_config().max_volume)).changed() {
                                if self.audio_controller.set_volume(volume).is_ok() {
                                    self.current_volume = volume;
                                }
                            }
                        });
                        ui.add_space(5.0);
                    }

                    // Time display above volume
                    if self.app.get_config().show_time {
                        ui.label(self.app.get_time());
                        ui.add_space(5.0);
                    }
                });
            });

            // Launch options window
            if let Some((ref app_name, ref options)) = self.editing.clone() {
                let mut input = options.to_string();
                egui::Window::new(format!("Launch Options for {}", app_name))
                    .collapsible(false)
                    .show(ctx, |ui| {
                        ui.label("Enter launch command (format: command;;working_directory;;env_vars):");
                        ui.text_edit_singleline(&mut input);
                        
                        ui.horizontal(|ui| {
                            if ui.button("Save").clicked() {
                                self.app.handle_input(&format!("LAUNCH_OPTIONS:{}:{}", app_name, input));
                                self.editing = None;
                            }
                            if ui.button("Cancel").clicked() {
                                self.editing = None;
                            }
                        });
                    });
            }
        });

        // Handle key events
        let (esc_pressed, enter_pressed) = ctx.input(|i| (
            i.key_pressed(egui::Key::Escape),
            i.key_pressed(egui::Key::Enter)
        ));

        match (esc_pressed, enter_pressed) {
            (true, _) => {
                if self.editing.is_some() {
                    self.editing = None;
                } else {
                    self.app.handle_input("ESC");
                }
            },
            (_, true) => {
                if let Some((app_name, options)) = self.editing.take() {
                    self.app.handle_input(&format!("LAUNCH_OPTIONS:{}:{}", app_name, options));
                } else {
                    self.app.handle_input("ENTER");
                }
            },
            _ => {}
        }

        if self.app.should_quit() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}