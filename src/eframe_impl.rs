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
            Box::new(|_cc| Box::new(EframeWrapper(app))),
        )?;
        Ok(())
    }
}

struct EframeWrapper(Box<dyn AppInterface>);

impl eframe::App for EframeWrapper {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.0.update();
        
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
                // Search bar
                let mut query = self.0.get_query();
                ui.add(egui::TextEdit::singleline(&mut query).hint_text("Search..."));
                if query != self.0.get_query() {
                    self.0.handle_input(&query);
                }
                
                ui.add_space(10.0);
                
                // Search results
                for result in self.0.get_search_results() {
                    if ui.button(&result).clicked() {
                        self.0.launch_app(&result);
                    }
                }
                
                // Push everything else to the bottom
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    // Power, Restart, Logout buttons
                    ui.horizontal(|ui| {
                        if ui.button("Power").clicked() {
                            self.0.handle_input("P");
                        }
                        if ui.button("Restart").clicked() {
                            self.0.handle_input("R");
                        }
                        if ui.button("Logout").clicked() {
                            self.0.handle_input("L");
                        }
                    });
                    
                    ui.add_space(5.0);
                    
                    // Time display
                    ui.label(self.0.get_time());
                });
            });
        });

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.0.handle_input("ESC");
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            self.0.handle_input("ENTER");
        }

        if self.0.should_quit() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}