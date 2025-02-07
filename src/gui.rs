use std::{
    error::Error,
    collections::HashMap,
    fs::{File, read, metadata, remove_file, read_to_string},
    mem,
    os::fd::FromRawFd,
    sync::atomic::{AtomicBool, Ordering},
    time,
};
use eframe::egui;
use libc::{self, pid_t, SIGUSR1};
use resvg::usvg::{self, TreeParsing, TreeTextToPath};
use resvg::tiny_skia::Pixmap;
use resvg::Tree;
use image;
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
        unsafe { libc::signal(SIGUSR1, handle_sigusr1 as libc::sighandler_t); }
        let native_options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([300.0, 200.0])
                .with_always_on_top()
                .with_decorations(true)
                .with_resizable(false)
                .with_active(true),
            ..Default::default()
        };
        let config = app.get_config();
        let audio_controller = AudioController::new(config)?;
        audio_controller.start_polling(config);
        eframe::run_native("Application Launcher", native_options, Box::new(|cc| {
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
        }))?;
        Ok(())
    }
}

#[derive(Default)]
struct IconCache {
    texture: Option<egui::TextureHandle>,
    last_modified: Option<time::SystemTime>,
}

struct EframeWrapper {
    app: Box<dyn AppInterface>,
    audio_controller: AudioController,
    current_volume: f32,
    editing: Option<(String, String)>,
    focused: bool,
    icon_textures: HashMap<String, IconCache>,
    pid_file: File,
}

impl EframeWrapper {
    fn get_texture(&mut self, ctx: &egui::Context, icon_path: &str) -> Option<egui::TextureHandle> {
        let reload = self.icon_textures.get(icon_path).map_or(true, |cache| {
            metadata(icon_path)
                .map(|m| cache.last_modified.map_or(true, |lm| lm != m.modified().unwrap_or(lm)))
                .unwrap_or(true)
        });
        if reload {
            let img = if icon_path.ends_with(".svg") {
                Self::load_svg(icon_path).unwrap_or_else(|_| Self::create_placeholder())
            } else {
                image::open(icon_path)
                    .map(|img| {
                        let img = img.into_rgba8();
                        egui::ColorImage::from_rgba_unmultiplied(
                            [img.width() as usize, img.height() as usize],
                            &img,
                        )
                    })
                    .unwrap_or_else(|_| Self::create_placeholder())
            };
            let tex = ctx.load_texture(icon_path, img, Default::default());
            self.icon_textures.insert(
                icon_path.to_owned(),
                IconCache {
                    texture: Some(tex.clone()),
                    last_modified: metadata(icon_path).and_then(|m| m.modified()).ok(),
                },
            );
            Some(tex)
        } else {
            self.icon_textures.get(icon_path).and_then(|cache| cache.texture.clone())
        }
    }

    fn load_svg(path: &str) -> Result<egui::ColorImage, Box<dyn Error>> {
        let data = read(path)?;
        let mut tree = usvg::Tree::from_data(&data, &usvg::Options::default())?;
        tree.convert_text(&usvg::fontdb::Database::new());
        let pixmap_size = tree.size.to_int_size();
        let mut pixmap = Pixmap::new(pixmap_size.width(), pixmap_size.height())
            .ok_or("Failed to create pixmap")?;
        {
            let mut pm = pixmap.as_mut();
            Tree::from_usvg(&tree).render(usvg::Transform::default(), &mut pm);
        }
        Ok(egui::ColorImage::from_rgba_unmultiplied(
            [pixmap_size.width() as usize, pixmap_size.height() as usize],
            pixmap.data(),
        ))
    }

    fn create_placeholder() -> egui::ColorImage {
        egui::ColorImage::from_rgba_unmultiplied([16, 16], &[127u8; 16 * 16 * 4])
    }
}

impl eframe::App for EframeWrapper {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if FOCUS_REQUESTED.swap(false, Ordering::Relaxed) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        self.app.update();
        if self.audio_controller.update_volume().is_ok() {
            self.current_volume = self.audio_controller.get_volume();
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical(|ui| {
                let mut query = self.app.get_query();
                let resp = ui.add(egui::TextEdit::singleline(&mut query).hint_text("Search..."));
                if !self.focused {
                    resp.request_focus();
                    self.focused = true;
                }
                if resp.changed() && !query.starts_with("LAUNCH_OPTIONS:") {
                    self.app.handle_input(&query);
                }
                ui.add_space(10.0);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for app_name in self.app.get_search_results() {
                        ui.horizontal(|ui| {
                            let mut settings_clicked = false;
                            let icon_size = egui::Vec2::splat(18.0);
                            let (icon_rect, icon_resp) = ui.allocate_exact_size(icon_size, egui::Sense::click());
                            if icon_resp.clicked() && self.editing.is_none() {
                                settings_clicked = true;
                            }
                            if ui.is_rect_visible(icon_rect) {
                                let tex = self.app.get_icon_path(&app_name)
                                    .and_then(|p| self.get_texture(ctx, &p))
                                    .or_else(|| Some(ctx.load_texture("placeholder", Self::create_placeholder(), Default::default())));
                                if let Some(tex) = tex {
                                    ui.painter().image(
                                        tex.id(),
                                        icon_rect,
                                        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(1.0, 1.0)),
                                        egui::Color32::WHITE,
                                    );
                                }
                                let gear = "⚙";
                                let gear_font = egui::TextStyle::Button.resolve(ui.style());
                                let gear_size = ui.fonts(|f| f.layout_no_wrap(gear.to_owned(), gear_font.clone(), egui::Color32::WHITE)).size();
                                let gear_pos = egui::Pos2::new(icon_rect.max.x - gear_size.x, icon_rect.min.y - gear_size.y * 0.2);
                                ui.painter().text(gear_pos, egui::Align2::RIGHT_TOP, gear, gear_font, egui::Color32::from_rgb(64, 64, 64));
                            }
                            if ui.add(egui::Button::new(&app_name).min_size(egui::Vec2::new(0.0, 15.0))).clicked() {
                                self.app.launch_app(&app_name);
                            }
                            if settings_clicked {
                                let opts = if self.app.get_launch_options(&app_name).is_some() {
                                    self.app.start_launch_options_edit(&app_name)
                                } else {
                                    String::new()
                                };
                                self.editing = Some((app_name.clone(), opts));
                            }
                        });
                    }
                });
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    if self.app.get_config().enable_power_options {
                        ui.horizontal(|ui| {
                            for (l, c) in [("Power", "P"), ("Restart", "R"), ("Logout", "L")] {
                                if ui.button(l).clicked() {
                                    self.app.handle_input(c);
                                }
                            }
                        });
                        ui.add_space(5.0);
                    }
                    if self.audio_controller.is_enabled() {
                        ui.horizontal(|ui| {
                            ui.label("Volume:");
                            let mut vol = self.current_volume;
                            if ui.add(egui::Slider::new(&mut vol, 0.0..=self.app.get_config().max_volume)).changed() {
                                let _ = self.audio_controller.set_volume(vol);
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
            if let Some((app_name, mut opts)) = self.editing.take() {
                let (mut save, mut cancel) = (false, false);
                egui::Window::new(format!("Launch Options for {}", app_name))
                    .collapsible(false)
                    .show(ctx, |ui| {
                        ui.label("Custom command and environment variables:");
                        ui.text_edit_singleline(&mut opts);
                        ui.horizontal(|ui| {
                            if ui.button("Save").clicked() { save = true; }
                            if ui.button("Cancel").clicked() { cancel = true; }
                        });
                    });
                if save {
                    self.app.handle_input(&format!("LAUNCH_OPTIONS:{}:{}", app_name, opts));
                } else if !cancel {
                    self.editing = Some((app_name, opts));
                }
            }
        });
        let (esc, enter) = ctx.input(|i| (i.key_pressed(egui::Key::Escape), i.key_pressed(egui::Key::Enter)));
        match (esc, enter) {
            (true, _) => {
                if self.editing.is_some() { self.editing = None; }
                else { self.app.handle_input("ESC"); }
            }
            (_, true) => {
                if let Some((n, o)) = self.editing.take() {
                    self.app.handle_input(&format!("LAUNCH_OPTIONS:{}:{}", n, o));
                } else {
                    self.app.handle_input("ENTER");
                }
            }
            _ => {}
        }
        if self.app.should_quit() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        let _ = mem::replace(&mut self.pid_file, unsafe { File::from_raw_fd(-1) });
        let _ = remove_file("/tmp/your_app.pid");
    }
}

extern "C" fn handle_sigusr1(_: libc::c_int) {
    FOCUS_REQUESTED.store(true, Ordering::Relaxed);
}

pub fn send_focus_signal() -> Result<(), Box<dyn Error>> {
    let pid: pid_t = read_to_string("/tmp/your_app.pid")?
        .trim()
        .parse()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    if unsafe { libc::kill(pid, SIGUSR1) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}
