use eframe::egui;
use crate::gui_trait::{GuiFramework, AppInterface};

pub struct EframeGui;

impl GuiFramework for EframeGui {
    fn run(app: Box<dyn AppInterface>) -> Result<(), Box<dyn std::error::Error>> {
        let native_options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([300.0, 400.0]),
            ..Default::default()
        };
        eframe::run_native(
            "Application Launcher",
            native_options,
            Box::new(|_cc| Box::new(EframeWrapper {
                app,
                focused: false,
            })),
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
            ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
                // Search bar
                let mut query = self.app.get_query();
                let search_response = ui.add(egui::TextEdit::singleline(&mut query).hint_text("Search..."));

                // Request focus on the search bar if not yet focused
                if !self.focused {
                    search_response.request_focus();
                    self.focused = true;
                }

                if search_response.changed() {
                    self.app.handle_input(&query);
                }
                
                ui.add_space(10.0);

                // Search results
                for result in self.app.get_search_results() {
                    if ui.button(&result).clicked() {
                        self.app.launch_app(&result);
                    }
                }
            });

            // Push everything else to the bottom
            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                // Power, Restart, Logout buttons
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

                // Time display
                ui.label(self.app.get_time());
            });
        });

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.app.handle_input("ESC");
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            self.app.handle_input("ENTER");
        }

        if self.app.should_quit() {
            ctx.request_repaint(); // Ensure the UI is updated immediately
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}
