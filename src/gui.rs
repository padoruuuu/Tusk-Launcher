use std::{
    collections::HashMap,
    error::Error,
    fs::{read_to_string, create_dir_all, OpenOptions},
    io::Write,
    sync::atomic::{AtomicBool, Ordering},
};
use eframe;
use libc::SIGUSR1;
use crate::{config::Config, app_launcher::AppLaunchOptions, audio::AudioController, cache::IconManager};
use xdg;

static FOCUS_REQUESTED: AtomicBool = AtomicBool::new(false);

struct Rule {
    class_name: String,
    props: HashMap<String, String>,
}
const DEFAULT_THEME: &str = "";
struct Theme {
    rules: Vec<Rule>,
}

impl Theme {
    fn load_or_create() -> Result<Self, Box<dyn Error>> {
        let dirs = xdg::BaseDirectories::new()?;
        let path = dirs.place_config_file("tusk-launcher/theme.css")?;
        if let Some(p) = path.parent() {
            create_dir_all(p)?;
        }
        if !path.exists() {
            let mut file = OpenOptions::new().write(true).create(true).open(&path)?;
            file.write_all(DEFAULT_THEME.as_bytes())?;
        }
        let css = read_to_string(&path)?;
        Ok(Self {
            rules: Self::parse_css_rules(&css),
        })
    }

    /// Parser that processes every `.class { ... }` rule, including position/layout.
    fn parse_css_rules(css: &str) -> Vec<Rule> {
        let mut rules = Vec::new();
        let mut rest = css;
        loop {
            rest = rest.trim_start();
            let dot = match rest.find('.') {
                Some(i) => i,
                None => break,
            };
            rest = &rest[dot + 1..];
            let class_end = match rest.find(|c: char| c.is_whitespace() || c == '{') {
                Some(i) => i,
                None => break,
            };
            if class_end == 0 {
                break;
            }
            let class_sel = rest[..class_end].trim();
            let class_name = class_sel.to_string();
            if let Some(brace) = rest.find('{') {
                let after = &rest[brace + 1..];
                if let Some(end) = after.find('}') {
                    let block = &after[..end];
                    let props = block.split(';').filter_map(|decl| {
                        let decl = decl.trim();
                        if decl.is_empty() {
                            None
                        } else {
                            let mut parts = decl.splitn(2, ':');
                            Some((
                                parts.next()?.trim().to_string(),
                                parts.next()?.trim().to_string(),
                            ))
                        }
                    }).collect();
                    rules.push(Rule { class_name, props });
                    rest = &after[end + 1..];
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        rules
    }

    fn get_style(&self, class: &str, prop: &str) -> Option<String> {
        self.rules
            .iter()
            .find(|r| r.class_name == class)?
            .props
            .get(prop)
            .cloned()
    }

    fn parse_color(&self, s: &str) -> Option<egui::Color32> {
        let s = s.trim().to_lowercase();
        if s.starts_with("rgba(") {
            let inner = s.strip_prefix("rgba(")?.strip_suffix(")")?.trim();
            let vals: Vec<_> = inner.split(',').map(|v| v.trim()).collect();
            if vals.len() == 4 {
                let r = vals[0].parse().ok()?;
                let g = vals[1].parse().ok()?;
                let b = vals[2].parse().ok()?;
                let a = vals[3].parse::<f32>().ok()?;
                return Some(egui::Color32::from_rgba_unmultiplied(r, g, b, (a * 255.0) as u8));
            }
        } else if s.starts_with('#') {
            let hex = s.strip_prefix('#')?;
            if hex.len() == 6 {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                return Some(egui::Color32::from_rgb(r, g, b));
            }
        }
        None
    }

    fn get_order(&self, section: &str) -> i32 {
        self.get_style(section, "order")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
    }
    fn get_layout(&self, section: &str) -> Option<String> {
        self.get_style(section, "layout")
    }
    fn apply_style(&self, ui: &mut egui::Ui, class: &str) {
        let style = ui.style_mut();
        if let Some(bg) = self
            .get_style(class, "background-color")
            .and_then(|s| self.parse_color(&s))
        {
            style.visuals.window_fill = bg;
            style.visuals.panel_fill = bg;
        }
        if let Some(col) = self
            .get_style(class, "color")
            .and_then(|s| self.parse_color(&s))
        {
            style.visuals.override_text_color = Some(col);
        }
        if let Some(pad) = self
            .get_style(class, "padding")
            .and_then(|s| s.replace("px", "").parse::<f32>().ok())
        {
            style.spacing.item_spacing = egui::Vec2::splat(pad);
            style.spacing.window_margin = egui::Margin::same(pad);
        }
        if let Some(rad) = self
            .get_style(class, "border-radius")
            .and_then(|s| s.replace("px", "").parse::<f32>().ok())
        {
            let r = egui::Rounding::same(rad);
            style.visuals.window_rounding = r;
            style.visuals.widgets.noninteractive.rounding = r;
            style.visuals.widgets.inactive.rounding = r;
            style.visuals.widgets.hovered.rounding = r;
            style.visuals.widgets.active.rounding = r;
        }
        if let Some(sz) = self
            .get_style(class, "font-size")
            .and_then(|s| s.replace("px", "").parse::<f32>().ok())
        {
            if let Some(text) = ui.style_mut().text_styles.get_mut(&egui::TextStyle::Body) {
                text.size = sz;
            }
        }
    }
}

fn with_custom_style<R>(
    ui: &mut egui::Ui,
    modify: impl FnOnce(&mut egui::Style),
    f: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let old = ui.style().clone();
    modify(ui.style_mut());
    let res = f(ui);
    *ui.style_mut() = (*old).clone();
    res
}

/// Modified alignment helper: for "app-list", we use a vertical centered layout.
fn with_alignment<R>(
    ui: &mut egui::Ui,
    theme: &Theme,
    section: &str,
    f: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    if section == "app-list" {
        // Use vertical_centered and extract the inner value.
        return ui.vertical_centered(f).inner;
    }
    if let Some(pos) = theme.get_style(section, "position") {
        let layout = match pos.as_str() {
            "center" => egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
            "left" => egui::Layout::left_to_right(egui::Align::Min),
            "right" => egui::Layout::right_to_left(egui::Align::Max),
            _ => egui::Layout::default(),
        };
        ui.with_layout(layout, f).inner
    } else if let Some(align) = theme.get_style(section, "align") {
        let layout = match align.as_str() {
            "center" => egui::Layout::left_to_right(egui::Align::Center),
            "right" => egui::Layout::right_to_left(egui::Align::Center),
            "left" => egui::Layout::left_to_right(egui::Align::Min),
            _ => egui::Layout::default(),
        };
        ui.with_layout(layout, f).inner
    } else {
        f(ui)
    }
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
    fn start_launch_options_edit(&mut self, app_name: &str) -> String;
    fn get_launch_options(&self, app_name: &str) -> Option<&AppLaunchOptions>;
    fn get_icon_path(&self, app_name: &str) -> Option<String>;
}

pub struct EframeGui;
impl EframeGui {
    pub fn run(app: Box<dyn AppInterface>) -> Result<(), Box<dyn Error>> {
        let theme = Theme::load_or_create()?;
        unsafe {
            libc::signal(SIGUSR1, handle_sigusr1 as libc::sighandler_t);
        }
        let native_options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([300.0, 200.0])
                .with_always_on_top()
                .with_decorations(false)
                .with_resizable(false)
                .with_active(true)
                .with_transparent(true),
            ..Default::default()
        };
        let cfg = app.get_config();
        let audio = AudioController::new(cfg)?;
        audio.start_polling(cfg);
        eframe::run_native("Application Launcher", native_options, Box::new(|cc| {
            cc.egui_ctx.request_repaint();
            Box::new(EframeWrapper {
                app,
                audio_controller: audio,
                current_volume: 0.0,
                editing: None,
                focused: false,
                icon_manager: IconManager::new(),
                theme,
            })
        }))?;
        Ok(())
    }
}

struct EframeWrapper {
    app: Box<dyn AppInterface>,
    audio_controller: AudioController,
    current_volume: f32,
    editing: Option<(String, String)>,
    focused: bool,
    icon_manager: IconManager,
    theme: Theme,
}

impl EframeWrapper {
    fn render_search_bar(&mut self, ui: &mut egui::Ui) {
        with_alignment(ui, &self.theme, "search-bar", |ui| {
            self.theme.apply_style(ui, "search-bar");
            let bg = self
                .theme
                .get_style("search-bar", "background-color")
                .and_then(|s| self.theme.parse_color(&s))
                .unwrap_or(ui.visuals().window_fill);
            let rounding = self
                .theme
                .get_style("search-bar", "border-radius")
                .and_then(|s| s.replace("px", "").parse::<f32>().ok())
                .map(egui::Rounding::same)
                .unwrap_or_default();
            egui::Frame::none().fill(bg).rounding(rounding).show(ui, |ui| {
                with_custom_style(ui, |s| {
                    if let Some(c) = self
                        .theme
                        .get_style("search-bar", "color")
                        .and_then(|s| self.theme.parse_color(&s))
                    {
                        s.visuals.override_text_color = Some(c);
                    }
                }, |ui| {
                    let mut query = self.app.get_query();
                    let resp = ui.add(egui::TextEdit::singleline(&mut query).hint_text("Search..."));
                    if !self.focused {
                        resp.request_focus();
                        self.focused = true;
                    }
                    if resp.changed() && !query.starts_with("LAUNCH_OPTIONS:") {
                        self.app.handle_input(&query);
                    }
                });
            });
        });
    }

    fn render_volume_slider(&mut self, ui: &mut egui::Ui) {
        with_alignment(ui, &self.theme, "volume-slider", |ui| {
            self.theme.apply_style(ui, "volume-slider");
            ui.horizontal(|ui| {
                if let Some(gap) = self
                    .theme
                    .get_style("volume-slider", "gap")
                    .and_then(|s| s.replace("px", "").parse::<f32>().ok())
                {
                    ui.spacing_mut().item_spacing.x = gap;
                }
                ui.label("Volume:");
                let vol = &mut self.current_volume;
                let slider_visuals = {
                    let mut v = ui.style().visuals.widgets.inactive.clone();
                    if let Some(bg) = self
                        .theme
                        .get_style("volume-slider", "background-color")
                        .and_then(|s| self.theme.parse_color(&s))
                    {
                        v.bg_fill = bg;
                    }
                    v.rounding = self
                        .theme
                        .get_style("volume-slider", "border-radius")
                        .and_then(|s| s.replace("px", "").parse::<f32>().ok())
                        .map(egui::Rounding::same)
                        .unwrap_or_default();
                    v
                };
                with_custom_style(ui, |s| {
                    s.visuals.widgets.inactive = slider_visuals.clone();
                    s.visuals.widgets.hovered = slider_visuals.clone();
                    s.visuals.widgets.active = slider_visuals;
                    s.visuals.widgets.inactive.bg_stroke = egui::Stroke::NONE;
                    s.visuals.widgets.hovered.bg_stroke = egui::Stroke::NONE;
                    s.visuals.widgets.active.bg_stroke = egui::Stroke::NONE;
                    s.visuals.widgets.hovered.expansion = 0.0;
                    s.visuals.widgets.active.expansion = 0.0;
                }, |ui| {
                    let slider = egui::Slider::new(vol, 0.0..=self.app.get_config().max_volume)
                        .custom_formatter(|n, _| format!("{:.0}%", n * 100.0))
                        .custom_parser(|s| {
                            s.trim()
                                .trim_end_matches('%')
                                .parse::<f64>()
                                .ok()
                                .map(|n| n / 100.0)
                        });
                    if ui.add(slider).changed() {
                        let _ = self.audio_controller.set_volume(*vol);
                    }
                });
            });
        });
    }

    fn render_app_list(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        with_alignment(ui, &self.theme, "app-list", |ui| {
            self.theme.apply_style(ui, "app-list");
            egui::ScrollArea::vertical().show(ui, |ui| {
                for app_name in self.app.get_search_results() {
                    let item_bg = self
                        .theme
                        .get_style("app-item", "background-color")
                        .and_then(|s| self.theme.parse_color(&s))
                        .unwrap_or(egui::Color32::TRANSPARENT);
                    let rounding = self
                        .theme
                        .get_style("app-item", "border-radius")
                        .and_then(|s| s.replace("px", "").parse::<f32>().ok())
                        .map(egui::Rounding::same)
                        .unwrap_or_default();
                    egui::Frame::none()
                        .fill(item_bg)
                        .stroke(egui::Stroke::NONE)
                        .rounding(rounding)
                        .inner_margin(egui::Margin::symmetric(6.0, 4.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let mut settings_clicked = false;
                                let icon_size = egui::Vec2::splat(18.0);
                                let (icon_rect, icon_resp) =
                                    ui.allocate_exact_size(icon_size, egui::Sense::click());
                                if icon_resp.clicked() && self.editing.is_none() {
                                    settings_clicked = true;
                                }
                                if ui.is_rect_visible(icon_rect) {
                                    if let Some(tex) = self
                                        .app
                                        .get_icon_path(&app_name)
                                        .and_then(|p| self.icon_manager.get_texture(ctx, &p))
                                    {
                                        ui.painter().image(
                                            tex.id(),
                                            icon_rect,
                                            egui::Rect::from_min_max(
                                                egui::Pos2::ZERO,
                                                egui::Pos2::new(1.0, 1.0),
                                            ),
                                            egui::Color32::WHITE,
                                        );
                                    }
                                    let gear = "⚙";
                                    let gear_font = egui::TextStyle::Button.resolve(ui.style());
                                    let gear_size = ui.fonts(|f| {
                                        f.layout_no_wrap(
                                            gear.to_owned(),
                                            gear_font.clone(),
                                            egui::Color32::WHITE,
                                        )
                                        .size()
                                    });
                                    let gear_pos = egui::Pos2::new(
                                        icon_rect.max.x - gear_size.x,
                                        icon_rect.min.y - gear_size.y * 0.2,
                                    );
                                    ui.painter().text(
                                        gear_pos,
                                        egui::Align2::RIGHT_TOP,
                                        gear,
                                        gear_font,
                                        egui::Color32::from_rgb(64, 64, 64),
                                    );
                                }
                                if settings_clicked {
                                    let opts = if self.app.get_launch_options(&app_name).is_some() {
                                        self.app.start_launch_options_edit(&app_name)
                                    } else {
                                        String::new()
                                    };
                                    self.editing = Some((app_name.clone(), opts));
                                } else {
                                    // Render the app list button similarly to the power buttons.
                                    with_custom_style(ui, |s| {
                                        let bg = self.theme.get_style("app-button", "background-color")
                                            .and_then(|v| self.theme.parse_color(&v))
                                            .unwrap_or(egui::Color32::TRANSPARENT);
                                        s.visuals.widgets.inactive.bg_fill = bg;
                                        s.visuals.widgets.hovered.bg_fill = bg;
                                        s.visuals.widgets.active.bg_fill = bg;
                                        s.visuals.widgets.inactive.weak_bg_fill = bg;
                                        s.visuals.widgets.hovered.weak_bg_fill = bg;
                                        s.visuals.widgets.active.weak_bg_fill = bg;
                                        s.visuals.widgets.inactive.bg_stroke = egui::Stroke::NONE;
                                        s.visuals.widgets.hovered.bg_stroke = egui::Stroke::NONE;
                                        s.visuals.widgets.active.bg_stroke = egui::Stroke::NONE;
                                        s.visuals.widgets.hovered.expansion = 0.0;
                                        s.visuals.widgets.active.expansion = 0.0;
                                        if let Some(r) = self
                                            .theme
                                            .get_style("app-button", "border-radius")
                                            .and_then(|v| v.trim().replace("px", "").parse::<f32>().ok())
                                        {
                                            let rr = egui::Rounding::same(r);
                                            s.visuals.widgets.inactive.rounding = rr;
                                            s.visuals.widgets.hovered.rounding = rr;
                                            s.visuals.widgets.active.rounding = rr;
                                        }
                                        if let Some(tc) = self
                                            .theme
                                            .get_style("app-button", "color")
                                            .and_then(|v| self.theme.parse_color(&v))
                                        {
                                            s.visuals.override_text_color = Some(tc);
                                        }
                                    }, |ui| {
                                        if ui.add(egui::Button::new(&app_name)
                                            .min_size(egui::Vec2::new(0.0, 15.0)))
                                            .clicked()
                                        {
                                            self.app.launch_app(&app_name);
                                        }
                                    });
                                }
                            });
                        });
                }
            });
        });
    }

    fn render_time_display(&mut self, ui: &mut egui::Ui) {
        with_alignment(ui, &self.theme, "time-display", |ui| {
            self.theme.apply_style(ui, "time-display");
            ui.label(self.app.get_time());
        });
    }

    fn render_power_button(&mut self, ui: &mut egui::Ui) {
        with_alignment(ui, &self.theme, "power-button", |ui| {
            if let Some(bg) = self
                .theme
                .get_style("power-button", "background-color")
                .and_then(|s| self.theme.parse_color(&s))
            {
                with_custom_style(ui, |st| {
                    st.visuals.widgets.inactive.bg_fill = bg;
                    st.visuals.widgets.hovered.bg_fill = bg;
                    st.visuals.widgets.active.bg_fill = bg;
                    st.visuals.widgets.inactive.weak_bg_fill = bg;
                    st.visuals.widgets.hovered.weak_bg_fill = bg;
                    st.visuals.widgets.active.weak_bg_fill = bg;
                    st.visuals.widgets.inactive.bg_stroke = egui::Stroke::NONE;
                    st.visuals.widgets.hovered.bg_stroke = egui::Stroke::NONE;
                    st.visuals.widgets.active.bg_stroke = egui::Stroke::NONE;
                    st.visuals.widgets.hovered.expansion = 0.0;
                    st.visuals.widgets.active.expansion = 0.0;
                    if let Some(tc) = self
                        .theme
                        .get_style("power-button", "color")
                        .and_then(|s| self.theme.parse_color(&s))
                    {
                        st.visuals.override_text_color = Some(tc);
                    }
                }, |ui| {
                    ui.horizontal(|ui| {
                        for &(label, cmd) in &[("Power", "P"), ("Restart", "R"), ("Logout", "L")] {
                            if ui.button(label).clicked() {
                                self.app.handle_input(cmd);
                            }
                        }
                    });
                });
            } else {
                ui.horizontal(|ui| {
                    for &(label, cmd) in &[("Power", "P"), ("Restart", "R"), ("Logout", "L")] {
                        if ui.button(label).clicked() {
                            self.app.handle_input(cmd);
                        }
                    }
                });
            }
        });
    }

    fn render_section(&mut self, ui: &mut egui::Ui, sec: &str, ctx: &egui::Context) {
        match sec {
            "search-bar" => self.render_search_bar(ui),
            "volume-slider" => self.render_volume_slider(ui),
            "app-list" => self.render_app_list(ui, ctx),
            "time-display" => self.render_time_display(ui),
            "power-button" => self.render_power_button(ui),
            _ => {}
        }
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
        let bg = self
            .theme
            .get_style("main-window", "background-color")
            .and_then(|s| self.theme.parse_color(&s))
            .unwrap_or(egui::Color32::BLACK);
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(bg))
            .show(ctx, |ui| {
                self.theme.apply_style(ui, "main-window");
                let mut secs = vec![
                    ("search-bar", self.theme.get_order("search-bar"), self.theme.get_layout("search-bar")),
                    ("app-list", self.theme.get_order("app-list"), self.theme.get_layout("app-list")),
                    ("volume-slider", self.theme.get_order("volume-slider"), self.theme.get_layout("volume-slider")),
                    ("time-display", self.theme.get_order("time-display"), self.theme.get_layout("time-display")),
                    ("power-button", self.theme.get_order("power-button"), self.theme.get_layout("power-button")),
                ];
                secs.sort_by_key(|&(_, order, _)| order);
                let mut i = 0;
                while i < secs.len() {
                    if secs[i].2.as_deref() == Some("horizontal") {
                        let mut group = vec![secs[i].0];
                        i += 1;
                        while i < secs.len() && secs[i].2.as_deref() == Some("horizontal") {
                            group.push(secs[i].0);
                            i += 1;
                        }
                        ui.horizontal(|ui| {
                            for sec in group {
                                self.render_section(ui, sec, ctx);
                            }
                        });
                    } else {
                        self.render_section(ui, secs[i].0, ctx);
                        i += 1;
                    }
                }
            });
        if let Some((app_name, mut opts)) = self.editing.take() {
            let (mut save, mut cancel) = (false, false);
            egui::Window::new(format!("Launch Options for {}", app_name))
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label("Custom command and environment variables:");
                    ui.text_edit_singleline(&mut opts);
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            save = true;
                        }
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                    });
                });
            if save {
                self.app.handle_input(&format!("LAUNCH_OPTIONS:{}:{}", app_name, opts));
            } else if !cancel {
                self.editing = Some((app_name, opts));
            }
        }
        let (esc, enter) = ctx.input(|i| {
            (i.key_pressed(egui::Key::Escape), i.key_pressed(egui::Key::Enter))
        });
        match (esc, enter) {
            (true, _) => {
                if self.editing.is_some() {
                    self.editing = None;
                } else {
                    self.app.handle_input("ESC");
                }
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
}

extern "C" fn handle_sigusr1(_: libc::c_int) {
    FOCUS_REQUESTED.store(true, Ordering::Relaxed);
}
