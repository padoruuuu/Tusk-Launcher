use std::error::Error;
use eframe::egui;
use crate::config::Config;

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
}

pub struct EframeGui;

impl GuiFramework for EframeGui {
    fn run(app: Box<dyn AppInterface>) -> Result<(), Box<dyn std::error::Error>> {
        let native_options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([300.0, 400.0])
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
                })
            }),
        )?;
        Ok(())
    }
}

struct EframeWrapper {
    app: Box<dyn AppInterface>,
    focused: bool,
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

                if search_response.changed() {
                    self.app.handle_input(&query);
                }

                ui.add_space(10.0);

                // Display search results
                self.display_search_results(ui);

                // Bottom panel for power options and time
                self.display_bottom_panel(ui);
            });
        });

        // Handle key presses
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.app.handle_input("ESC");
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            self.app.handle_input("ENTER");
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
            if ui.button(&result).clicked() {
                self.app.launch_app(&result);
            }
        }
    }

    fn display_bottom_panel(&mut self, ui: &mut egui::Ui) {
        // Fetch config before entering the closure
        let config = self.app.get_config().clone(); // Clone the config to avoid borrowing `self`

        ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
            // Power options
            if config.enable_power_options {
                ui.horizontal(|ui| {
                    if ui.button("Power").clicked() {
                        // Now we can safely borrow `self` mutably here
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
