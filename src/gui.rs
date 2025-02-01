use std::error::Error;
use std::collections::HashMap;
use std::fs::File;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use eframe::egui;
use libc::{self, pid_t, SIGUSR1};
use std::os::fd::FromRawFd;
use crate::{config::Config, app_launcher::AppLaunchOptions, audio::AudioController};

static FOCUS_REQUESTED: AtomicBool = AtomicBool::new(false);

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
    fn get_icon_path(&self, app_name: &str) -> Option<String>;
}

pub struct EframeGui;

impl EframeGui {
    pub fn run(app: Box<dyn AppInterface>, pid_file: File) -> Result<(), Box<dyn Error>> {
        unsafe {
            libc::signal(SIGUSR1, handle_sigusr1 as libc::sighandler_t);
        }

        let native_options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([350.0, 500.0])
                .with_always_on_top()
                .with_decorations(true)
                .with_active(true),
            ..Default::default()
        };

        let audio_controller = {
            let config = app.get_config();
            let ctrl = AudioController::new(config)?;
            ctrl.start_polling(config);
            ctrl
        };

        thread::spawn(|| {
            loop {
                if FOCUS_REQUESTED.load(Ordering::Relaxed) {
                    Self::focus_window();
                    FOCUS_REQUESTED.store(false, Ordering::Relaxed);
                }
                thread::sleep(Duration::from_millis(500));
            }
        });

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
                    icon_textures: HashMap::new(),
                    pid_file,
                })
            }),
        )?;
        Ok(())
    }

    fn focus_window() {
        let ctx = eframe::egui::Context::default();
        let viewport_id = ctx.viewport_id();
        ctx.send_viewport_cmd_to(viewport_id, egui::ViewportCommand::Focus);
    }
}

struct EframeWrapper {
    app: Box<dyn AppInterface>,
    audio_controller: AudioController,
    current_volume: f32,
    editing: Option<(String, String)>,
    focused: bool,
    icon_textures: HashMap<String, egui::TextureHandle>,
    pid_file: File,
}

impl eframe::App for EframeWrapper {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.app.update();

        if self.audio_controller.update_volume().is_ok() {
            self.current_volume = self.audio_controller.get_volume();
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical(|ui| {
                // Search query text field.
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

                // Scrollable search results.
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for app_name in self.app.get_search_results() {
                        ui.horizontal(|ui| {
                            let mut settings_clicked = false;

                            // Settings button (icon or text)
                            if self.app.get_config().enable_icons {
                                if let Some(icon_path) = self.app.get_icon_path(&app_name) {
                                    // Load or retrieve the icon texture.
                                    let texture = self.icon_textures
                                        .entry(icon_path.clone())
                                        .or_insert_with(|| {
                                            let image = match image::open(&icon_path) {
                                                Ok(img) => {
                                                    let img = img.to_rgba8();
                                                    let size = [img.width() as usize, img.height() as usize];
                                                    egui::ColorImage::from_rgba_unmultiplied(size, &img)
                                                }
                                                Err(_) => egui::ColorImage::new([1, 1], egui::Color32::TRANSPARENT)
                                            };
                                            ctx.load_texture("icon", image, Default::default())
                                        });
                                    
                                    // Use a slightly smaller size for a compact UI.
                                    let desired_size = egui::Vec2::new(18.0, 18.0);
                                    let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click());
                                    if response.clicked() && self.editing.is_none() {
                                        settings_clicked = true;
                                    }
                                    if ui.is_rect_visible(rect) {
                                        // Draw the application icon.
                                        ui.painter().image(
                                            texture.id(),
                                            rect,
                                            egui::Rect::from_min_max(egui::Pos2::new(0.0, 0.0), egui::Pos2::new(1.0, 1.0)),
                                            egui::Color32::WHITE,
                                        );
                                        // Draw a tiny gear overlay in the top-right corner.
                                        let gear_text = "⚙";
                                        let gear_font = egui::TextStyle::Button.resolve(ui.style());
                                        let gear_galley = ui.fonts(|fonts| {
                                            fonts.layout_no_wrap(gear_text.to_string(), gear_font.clone(), egui::Color32::WHITE)
                                        });
                                        let gear_size = gear_galley.size();
                                        let gear_pos = egui::Pos2::new(rect.max.x - gear_size.x, rect.min.y);
                                        ui.painter().text(gear_pos, egui::Align2::LEFT_TOP, gear_text, gear_font, egui::Color32::WHITE);
                                    }
                                } else {
                                    // Fallback to text button with fixed size if no icon available.
                                    let response = ui.add_sized(
                                        egui::Vec2::new(18.0, 18.0),
                                        egui::Button::new("⚙").frame(false)
                                    );
                                    if response.clicked() && self.editing.is_none() {
                                        settings_clicked = true;
                                    }
                                }
                            } else {
                                // Icons disabled; use a text button with fixed size.
                                let response = ui.add_sized(
                                    egui::Vec2::new(18.0, 18.0),
                                    egui::Button::new("⚙").frame(false)
                                );
                                if response.clicked() && self.editing.is_none() {
                                    settings_clicked = true;
                                }
                            }

                            // Application launch button.
                            // We set a min height so the row is compact yet tall enough for its text.
                            let app_button = ui.add(egui::Button::new(&app_name).min_size(egui::Vec2::new(0.0, 20.0)));
                            if app_button.clicked() {
                                self.app.launch_app(&app_name);
                            }

                            if settings_clicked {
                                let options = if let Some(_) = self.app.get_launch_options(&app_name) {
                                    self.app.start_launch_options_edit(&app_name)
                                } else {
                                    String::new()
                                };
                                self.editing = Some((app_name.clone(), options));
                            }
                        });
                    }
                });

                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
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

                    if self.audio_controller.is_enabled() {
                        ui.horizontal(|ui| {
                            ui.label("Volume:");
                            let mut volume = self.current_volume;
                            if ui.add(egui::Slider::new(&mut volume, 0.0..=self.app.get_config().max_volume)).changed() {
                                let _ = self.audio_controller.set_volume(volume);
                            }
                        });
                        ui.add_space(5.0);
                    }

                    if self.app.get_config().show_time {
                        ui.label(self.app.get_time());
                        ui.add_space(5.0);
                    }
                });
            });

            // Editing window for launch options.
            if let Some((app_name, options)) = self.editing.take() {
                let mut new_options = options.clone();
                let mut save_pressed = false;
                let mut cancel_pressed = false;
                
                egui::Window::new(format!("Launch Options for {}", app_name))
                    .collapsible(false)
                    .show(ctx, |ui| {
                        ui.label("Custom command and environment variables:");
                        ui.text_edit_singleline(&mut new_options);
                        
                        ui.horizontal(|ui| {
                            if ui.button("Save").clicked() {
                                save_pressed = true;
                            }
                            if ui.button("Cancel").clicked() {
                                cancel_pressed = true;
                            }
                        });
                    });

                if save_pressed {
                    self.app.handle_input(&format!("LAUNCH_OPTIONS:{}:{}", app_name, new_options));
                } else if !cancel_pressed {
                    // If neither button was pressed, restore editing so the window remains open.
                    self.editing = Some((app_name, options));
                }
                // If "Cancel" was pressed, the editing window will close.
            }
        });

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

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Properly take ownership of the pid_file before dropping it.
        let _ = std::mem::replace(&mut self.pid_file, unsafe { File::from_raw_fd(-1) });
        let _ = std::fs::remove_file("/tmp/your_app.pid");
    }
}

extern "C" fn handle_sigusr1(_: libc::c_int) {
    FOCUS_REQUESTED.store(true, Ordering::Relaxed);
}

pub fn send_focus_signal() -> Result<(), Box<dyn Error>> {
    let pid = std::fs::read_to_string("/tmp/your_app.pid")?
        .trim()
        .parse::<pid_t>()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    let result = unsafe { libc::kill(pid, SIGUSR1) };
    if result != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}
