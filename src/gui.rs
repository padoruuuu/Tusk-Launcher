use std::error::Error;
use eframe::egui;
use crate::config::Config;
use crate::app_launcher::AppLaunchOptions;

pub trait GuiFramework {
    fn run(app: Box<dyn AppInterface>) -> Result<(), Box<dyn Error>>;
}

pub trait AppInterface {
    fn update(&mut self);
    fn handle_input(&mut self, input: &str);
    fn should_quit(&self) -> bool;
    fn get_query(&self) -> String;
    fn get_search_results(&self) -> Vec<String>;
    fn get_time(&self) -> String;
    fn launch_app(&mut self, app_name: &str);
    fn get_config(&self) -> &Config;
    
    // New methods for launch options
    fn start_launch_options_edit(&mut self, app_name: &str) -> String;
    fn get_launch_options(&self, app_name: &str) -> Option<&AppLaunchOptions>;
}

pub struct EframeGui;

impl GuiFramework for EframeGui {
    fn run(app: Box<dyn AppInterface>) -> Result<(), Box<dyn std::error::Error>> {
        let native_options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([350.0, 500.0])  // Slightly wider to accommodate launch options
                .with_always_on_top()
                .with_decorations(true)
                .with_transparent(false),
            ..Default::default()
        };
        eframe::run_native(
            "Application Launcher",
            native_options,
            Box::new(|cc| {
                cc.egui_ctx.request_repaint();
                Box::new(EframeWrapper {
                    app,
                    focused: false,
                    launch_options_input: String::new(),
                    editing_app: None,
                })
            }),
        )?;
        Ok(())
    }
}

struct EframeWrapper {
    app: Box<dyn AppInterface>,
    focused: bool,
    launch_options_input: String,
    editing_app: Option<String>,
}

impl eframe::App for EframeWrapper {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.app.update();
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical(|ui| {
                // Search bar
                let mut query = self.app.get_query();
                let search_response = ui.add(egui::TextEdit::singleline(&mut query).hint_text("Search..."));

                if !self.focused {
                    search_response.request_focus();
                    self.focused = true;
                }

                if search_response.changed() && !query.starts_with("LAUNCH_OPTIONS:") {
                    self.app.handle_input(&query);
                }

                ui.add_space(10.0);

                // Display search results with launch options buttons
                self.display_search_results(ui);

                // Launch options editing window (if active)
                if self.editing_app.is_some() {
                    self.display_launch_options_edit(ui);
                }

                // Bottom panel for power options and time
                self.display_bottom_panel(ui);
            });
        });

        // Handle key presses
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            if self.editing_app.is_some() {
                self.editing_app = None;
            } else {
                self.app.handle_input("ESC");
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            if self.editing_app.is_some() {
                let app_name = self.editing_app.take().unwrap();
                let save_input = format!("LAUNCH_OPTIONS:{}:{}", app_name, self.launch_options_input);
                self.app.handle_input(&save_input);
                self.launch_options_input = String::new(); // Reset input
            } else {
                self.app.handle_input("ENTER");
            }
        }

        // Handle quit request
        if self.app.should_quit() {
            ctx.request_repaint();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

impl EframeWrapper {
    fn display_search_results(&mut self, ui: &mut egui::Ui) {
        for result in self.app.get_search_results() {
            ui.horizontal(|ui| {
                // App launch button
                if ui.button(&result).clicked() {
                    self.app.launch_app(&result);
                }

                // Launch options button
                if ui.button("⚙").clicked() {
                    // Temporarily take ownership to resolve borrowing
                    let formatted_options = {
                        // First check if launch options exist
                        let has_options = self.app.get_launch_options(&result).is_some();
                        
                        // If options exist, get formatted options
                        if has_options {
                            // Clone the result to avoid borrowing conflicts
                            let result_clone = result.clone();
                            self.app.start_launch_options_edit(&result_clone)
                        } else {
                            String::new()
                        }
                    };

                    self.editing_app = Some(result.clone());
                    self.launch_options_input = formatted_options;
                }
            });
        }
    }

    fn display_launch_options_edit(&mut self, ui: &mut egui::Ui) {
        let app_name = self.editing_app.as_ref().unwrap();
        
        egui::Window::new(format!("Launch Options for {}", app_name))
            .collapsible(false)
            .show(ui.ctx(), |ui| {
                ui.label("Enter launch command (format: command;;working_directory;;env_vars):");
                ui.text_edit_singleline(&mut self.launch_options_input);
                
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        let app_name = self.editing_app.take().unwrap();
                        let save_input = format!("LAUNCH_OPTIONS:{}:{}", app_name, self.launch_options_input);
                        self.app.handle_input(&save_input);
                        self.launch_options_input = String::new(); // Reset input
                    }
                    if ui.button("Cancel").clicked() {
                        self.editing_app = None;
                        self.launch_options_input = String::new(); // Reset input
                    }
                });
            });
    }

    fn display_bottom_panel(&mut self, ui: &mut egui::Ui) {
        // Fetch config before entering the closure
        let config = self.app.get_config().clone(); // Clone the config to avoid borrowing `self`

        ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
            // Power options
            if config.enable_power_options {
                ui.horizontal(|ui| {
                    if ui.button("Power").clicked() {
                        self.app.handle_input("P");
                    }
                    if ui.button("Restart").clicked() {
                        self.app.handle_input("R");
                    }
                    if ui.button("Logout").clicked() {
                        self.app.handle_input("L");
                    }
                });
                ui.add_space(5.0);
            }

            // Display the current time if enabled
            if config.show_time {
                ui.label(format!("{}", self.app.get_time()));
            }
        });
    }
}