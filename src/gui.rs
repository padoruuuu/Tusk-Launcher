use std::{
    collections::HashMap,
    error::Error,
    fs::{read_to_string, create_dir_all, OpenOptions},
    io::Write,
};
use eframe;
use crate::{config::Config, app_launcher::AppLaunchOptions, audio::AudioController, cache::IconManager};
use xdg;

const DEFAULT_THEME: &str = "";

struct Rule {
    class_name: String,
    props: HashMap<String, String>,
}

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
            OpenOptions::new()
                .write(true)
                .create(true)
                .open(&path)?
                .write_all(DEFAULT_THEME.as_bytes())?;
        }
        Ok(Self { rules: Self::parse_css_rules(&read_to_string(&path)?) })
    }

    fn parse_css_rules(css: &str) -> Vec<Rule> {
        let mut rules = Vec::new();
        let mut rest = css;
        while let Some(dot) = rest.find('.') {
            rest = &rest[dot + 1..];
            let class_end = match rest.find(|c: char| c.is_whitespace() || c == '{') {
                Some(i) => i,
                None => break,
            };
            if class_end == 0 {
                break;
            }
            let class_name = rest[..class_end].trim().to_string();
            if let Some(brace) = rest.find('{') {
                let after = &rest[brace + 1..];
                if let Some(end) = after.find('}') {
                    let block = &after[..end];
                    let props = block.split(';')
                        .filter_map(|decl| {
                            let decl = decl.trim();
                            if decl.is_empty() {
                                None
                            } else {
                                let mut parts = decl.splitn(2, ':');
                                Some((parts.next()?.trim().to_string(), parts.next()?.trim().to_string()))
                            }
                        })
                        .collect();
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
        self.rules.iter().find(|r| r.class_name == class)?.props.get(prop).cloned()
    }

    fn get_combined_style(&self, classes: &[&str], prop: &str) -> Option<String> {
        let mut result = None;
        for &class in classes {
            if let Some(val) = self.get_style(class, prop) {
                result = Some(val);
            }
        }
        result
    }

    fn parse_color(&self, s: &str) -> Option<egui::Color32> {
        let s = s.trim().to_lowercase();
        if s == "transparent" {
            return Some(egui::Color32::TRANSPARENT);
        }
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
            let hex = &s[1..];
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
        self.get_style(section, "order").and_then(|s| s.parse().ok()).unwrap_or(0)
    }

    fn apply_style(&self, ui: &mut egui::Ui, class: &str) {
        let style = ui.style_mut();
        if let Some(bg) = self.get_style(class, "background-color").and_then(|s| self.parse_color(&s)) {
            style.visuals.window_fill = bg;
            style.visuals.panel_fill = bg;
        }
        if let Some(col) = self.get_style(class, "text-color").and_then(|s| self.parse_color(&s)) {
            style.visuals.override_text_color = Some(col);
        }
        if let Some(pad) = self.get_style(class, "padding").and_then(|s| s.replace("px", "").parse::<f32>().ok()) {
            style.spacing.item_spacing = egui::Vec2::splat(pad);
            style.spacing.window_margin = egui::Margin::symmetric(pad, pad);
        }
        if let Some(rad) = self.get_style(class, "border-radius").and_then(|s| s.replace("px", "").parse::<f32>().ok()) {
            let r = egui::Rounding::same(rad);
            style.visuals.window_rounding = r;
            style.visuals.widgets.noninteractive.rounding = r;
            style.visuals.widgets.inactive.rounding = r;
            style.visuals.widgets.hovered.rounding = r;
            style.visuals.widgets.active.rounding = r;
        }
        if let Some(sz) = self.get_style(class, "font-size").and_then(|s| s.replace("px", "").parse::<f32>().ok()) {
            if let Some(text) = ui.style_mut().text_styles.get_mut(&egui::TextStyle::Body) {
                text.size = sz;
            }
        }
    }

    fn get_frame_properties(&self, class: &str, default_fill: egui::Color32) -> (egui::Color32, Option<egui::Color32>, egui::Rounding) {
        let base = self.get_style(class, "background-color")
            .and_then(|s| self.parse_color(&s))
            .unwrap_or(default_fill);
        let hover = self.get_style(class, "hover-background-color")
            .and_then(|s| self.parse_color(&s));
        let rounding = self.get_style(class, "border-radius")
            .and_then(|s| s.replace("px", "").parse::<f32>().ok())
            .map(egui::Rounding::same)
            .unwrap_or_default();
        (base, hover, rounding)
    }

    fn get_px_value(&self, class: &str, prop: &str) -> Option<f32> {
        self.get_style(class, prop).and_then(|s| s.trim_end_matches("px").parse::<f32>().ok())
    }

    fn apply_widget_style(&self, style: &mut egui::Style, class: &str) {
        if let Some(bg) = self.get_style(class, "background-color").and_then(|s| self.parse_color(&s)) {
            style.visuals.widgets.inactive.bg_fill = bg;
            if let Some(hover_bg) = self.get_style(class, "hover-background-color").and_then(|s| self.parse_color(&s)) {
                style.visuals.widgets.hovered.bg_fill = hover_bg;
                style.visuals.widgets.hovered.weak_bg_fill = hover_bg;
            } else {
                style.visuals.widgets.hovered.bg_fill = bg;
                style.visuals.widgets.hovered.weak_bg_fill = bg;
            }
            style.visuals.widgets.active.bg_fill = bg;
            style.visuals.widgets.inactive.weak_bg_fill = bg;
            style.visuals.widgets.active.weak_bg_fill = bg;
            style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(0.0, egui::Color32::TRANSPARENT);
            style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(0.0, egui::Color32::TRANSPARENT);
            style.visuals.widgets.active.bg_stroke = egui::Stroke::new(0.0, egui::Color32::TRANSPARENT);
            style.visuals.widgets.hovered.expansion = 0.0;
            style.visuals.widgets.active.expansion = 0.0;
        }
        if let Some(tc) = self.get_style(class, "text-color").and_then(|s| self.parse_color(&s)) {
            style.visuals.override_text_color = Some(tc);
        }
    }

    fn apply_combined_widget_style(&self, style: &mut egui::Style, classes: &[&str]) {
        let default_fill = egui::Color32::TRANSPARENT;
        let base = self.get_combined_style(classes, "container-background-color")
            .or_else(|| self.get_combined_style(classes, "background-color"))
            .and_then(|s| self.parse_color(&s))
            .unwrap_or(default_fill);
        let hover = self.get_combined_style(classes, "hover-background-color")
            .and_then(|s| self.parse_color(&s))
            .unwrap_or(base);
        style.visuals.widgets.inactive.bg_fill = base;
        style.visuals.widgets.hovered.bg_fill = hover;
        style.visuals.widgets.hovered.weak_bg_fill = hover;
        style.visuals.widgets.active.bg_fill = base;
        style.visuals.widgets.inactive.weak_bg_fill = base;
        style.visuals.widgets.active.weak_bg_fill = base;
        style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(0.0, egui::Color32::TRANSPARENT);
        style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(0.0, egui::Color32::TRANSPARENT);
        style.visuals.widgets.active.bg_stroke = egui::Stroke::new(0.0, egui::Color32::TRANSPARENT);
        style.visuals.widgets.hovered.expansion = 0.0;
        style.visuals.widgets.active.expansion = 0.0;
        if let Some(tc) = self.get_combined_style(classes, "text-color").and_then(|s| self.parse_color(&s)) {
            style.visuals.override_text_color = Some(tc);
        }
    }
}

fn with_custom_style<R>(ui: &mut egui::Ui, modify: impl FnOnce(&mut egui::Style), f: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let old = ui.style().clone();
    modify(ui.style_mut());
    let res = f(ui);
    *ui.style_mut() = (*old).clone();
    res
}

fn with_alignment<R>(ui: &mut egui::Ui, theme: &Theme, section: &str, f: impl FnOnce(&mut egui::Ui) -> R) -> R {
    if section == "app-list" {
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
        let width = theme.get_style("main-window", "width")
            .or_else(|| theme.get_style("env-window", "width"))
            .and_then(|s| s.trim_end_matches("px").parse::<f32>().ok())
            .unwrap_or(300.0);
        let height = theme.get_style("main-window", "height")
            .or_else(|| theme.get_style("env-window", "height"))
            .and_then(|s| s.trim_end_matches("px").parse::<f32>().ok())
            .unwrap_or(200.0);
        let viewport_builder = egui::ViewportBuilder::default()
            .with_inner_size([width, height])
            .with_always_on_top()
            .with_decorations(false)
            .with_resizable(false)
            .with_active(true)
            .with_transparent(true);
        let native_options = eframe::NativeOptions {
            viewport: viewport_builder,
            ..Default::default()
        };
        let cfg = app.get_config();
        let audio = AudioController::new(cfg)?;
        audio.start_polling(cfg);
        eframe::run_native("Application Launcher", native_options, Box::new(|cc| {
            if let Some(scaling_str) = theme.get_style("env-window", "scaling") {
                if let Ok(scaling) = scaling_str.trim().parse::<f32>() {
                    cc.egui_ctx.set_pixels_per_point(scaling);
                }
            }
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
            let (base_bg, hover_bg, rounding) = self.theme.get_frame_properties("search-bar", ui.visuals().window_fill);
            let rect = ui.available_rect_before_wrap();
            let resp = ui.interact(rect, ui.id().with("search-bar"), egui::Sense::hover());
            let fill = if resp.hovered() { hover_bg.unwrap_or(base_bg) } else { base_bg };
            egui::Frame::none().fill(fill).rounding(rounding).show(ui, |ui| {
                with_custom_style(ui, |s| {
                    if let Some(tc) = self.theme.get_style("search-bar", "text-color")
                        .and_then(|s| self.theme.parse_color(&s)) {
                        s.visuals.override_text_color = Some(tc);
                    }
                }, |ui| {
                    let mut query = self.app.get_query();
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut query)
                            .hint_text("Search...")
                            .frame(false)
                    );
                    if !self.focused {
                        resp.request_focus();
                        self.focused = true;
                    }
                    if resp.changed() && !query.starts_with("LAUNCH_OPTIONS:") {
                        self.app.handle_input(&query);
                    }
                })
            });
        });
    }

    fn render_volume_slider(&mut self, ui: &mut egui::Ui) {
        with_alignment(ui, &self.theme, "volume-slider", |ui| {
            self.theme.apply_style(ui, "volume-slider");
            ui.horizontal(|ui| {
                if let Some(gap) = self.theme.get_px_value("volume-slider", "gap") {
                    ui.spacing_mut().item_spacing.x = gap;
                }
                ui.label("Volume:");
                let (base, hover, rounding) = self.theme.get_frame_properties("volume-slider", ui.style().visuals.widgets.inactive.bg_fill);
                let mut slider_visuals = ui.style().visuals.widgets.inactive.clone();
                slider_visuals.bg_fill = base;
                slider_visuals.rounding = rounding;
                with_custom_style(ui, |s| {
                    s.visuals.widgets.inactive = slider_visuals.clone();
                    s.visuals.widgets.hovered.bg_fill = hover.unwrap_or(base);
                    s.visuals.widgets.hovered.weak_bg_fill = hover.unwrap_or(base);
                    s.visuals.widgets.active = slider_visuals.clone();
                    s.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(0.0, egui::Color32::TRANSPARENT);
                    s.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(0.0, egui::Color32::TRANSPARENT);
                    s.visuals.widgets.active.bg_stroke = egui::Stroke::new(0.0, egui::Color32::TRANSPARENT);
                    s.visuals.widgets.hovered.expansion = 0.0;
                    s.visuals.widgets.active.expansion = 0.0;
                }, |ui| {
                    let slider = egui::Slider::new(&mut self.current_volume, 0.0..=self.app.get_config().max_volume)
                        .custom_formatter(|n, _| format!("{:.0}%", n * 100.0))
                        .custom_parser(|s| s.trim().trim_end_matches('%').parse::<f64>().ok().map(|n| n / 100.0));
                    if ui.add(slider).changed() {
                        let _ = self.audio_controller.set_volume(self.current_volume);
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
                    ui.horizontal(|ui| {
                        let mut settings_clicked = false;
                        let icon_size = egui::Vec2::splat(18.0);
                        let (icon_rect, icon_resp) = ui.allocate_exact_size(icon_size, egui::Sense::click());
                        if icon_resp.clicked() && self.editing.is_none() {
                            settings_clicked = true;
                        }
                        if ui.is_rect_visible(icon_rect) {
                            if let Some(tex) = self.app.get_icon_path(&app_name)
                                .and_then(|p| self.icon_manager.get_texture(ctx, &p)) {
                                ui.painter().image(
                                    tex.id(),
                                    icon_rect,
                                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(1.0, 1.0)),
                                    egui::Color32::WHITE,
                                );
                            }
                            let gear = "⚙";
                            let gear_color = if icon_resp.hovered() {
                                self.theme.get_style("settings-button", "hover-color")
                                    .and_then(|s| self.theme.parse_color(&s))
                                    .unwrap_or(egui::Color32::from_rgb(64, 64, 64))
                            } else {
                                self.theme.get_style("settings-button", "text-color")
                                    .and_then(|s| self.theme.parse_color(&s))
                                    .unwrap_or(egui::Color32::from_rgb(64, 64, 64))
                            };
                            if let Some(hitbox_color) = self.theme.get_combined_style(&["settings-button"], "hitbox-color")
                                .and_then(|s| self.theme.parse_color(&s)) {
                                ui.painter().rect_filled(icon_rect, 0.0, hitbox_color);
                            }
                            let gear_font = egui::TextStyle::Button.resolve(ui.style());
                            let center_align = egui::Align2([egui::Align::Center, egui::Align::Center]);
                            let gear_size = ui.fonts(|f| f.layout_no_wrap(gear.to_owned(), gear_font.clone(), gear_color).size());
                            let gear_pos = egui::Pos2::new(
                                icon_rect.center().x - gear_size.x / 2.0,
                                icon_rect.center().y - gear_size.y / 2.0
                            );
                            ui.painter().text(gear_pos, center_align, gear, gear_font, gear_color);
                        }
                        if settings_clicked {
                            let opts = if self.app.get_launch_options(&app_name).is_some() {
                                self.app.start_launch_options_edit(&app_name)
                            } else {
                                String::new()
                            };
                            self.editing = Some((app_name.clone(), opts));
                        } else {
                            with_custom_style(ui, |s| {
                                self.theme.apply_combined_widget_style(s, &["app-item", "app-button"]);
                            }, |ui| {
                                if ui.button(&app_name).clicked() {
                                    self.app.launch_app(&app_name);
                                }
                            });
                        }
                    });
                    ui.add_space(4.0);
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
            with_custom_style(ui, |s| {
                self.theme.apply_widget_style(s, "power-button");
            }, |ui| {
                ui.horizontal(|ui| {
                    for &(label, cmd) in &[("Power", "P"), ("Restart", "R"), ("Logout", "L")] {
                        if ui.button(label).clicked() {
                            self.app.handle_input(cmd);
                        }
                    }
                });
            });
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
        self.app.update();
        let cfg = self.app.get_config().clone();
        if cfg.enable_audio_control {
            if self.audio_controller.update_volume().is_ok() {
                self.current_volume = self.audio_controller.get_volume();
            }
        }
        let bg = self.theme.get_style("env-window", "background-color")
            .or_else(|| self.theme.get_style("main-window", "background-color"))
            .and_then(|s| self.theme.parse_color(&s))
            .unwrap_or(egui::Color32::BLACK);
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(bg))
            .show(ctx, |ui| {
                let mut secs = vec!["search-bar", "app-list"];
                if cfg.enable_audio_control { secs.push("volume-slider"); }
                if cfg.show_time { secs.push("time-display"); }
                if cfg.enable_power_options { secs.push("power-button"); }
                secs.sort_by_key(|sec| self.theme.get_order(sec));
                for sec in secs {
                    if let (Some(x), Some(y)) = (
                        self.theme.get_px_value(sec, "x"),
                        self.theme.get_px_value(sec, "y")
                    ) {
                        let w = self.theme.get_px_value(sec, "width");
                        let h = self.theme.get_px_value(sec, "height");
                        egui::Area::new(sec.into())
                            .order(egui::Order::Foreground)
                            .fixed_pos(egui::Pos2::new(x, y))
                            .show(ctx, |ui| {
                                if let Some(w) = w { ui.set_width(w); }
                                if let Some(h) = h { ui.set_height(h); }
                                self.render_section(ui, sec, ctx);
                            });
                    } else {
                        self.render_section(ui, sec, ctx);
                    }
                }
            });
        if let Some((app_name, mut opts)) = self.editing.take() {
            let (mut save, mut cancel) = (false, false);
            let width = self.theme.get_px_value("env-window", "width").unwrap_or(300.0);
            let height = self.theme.get_px_value("env-window", "height").unwrap_or(200.0);
            let x = self.theme.get_px_value("env-window", "x")
                .unwrap_or((ctx.input(|i| i.screen_rect().width()) - width) / 2.0);
            let y = self.theme.get_px_value("env-window", "y")
                .unwrap_or((ctx.input(|i| i.screen_rect().height()) - height) / 2.0);
            egui::Area::new("env-window".into())
                .order(egui::Order::Foreground)
                .movable(true)
                .fixed_pos(egui::Pos2::new(x, y))
                .show(ctx, |ui| {
                    ui.set_width(width);
                    ui.set_height(height);
                    ui.vertical(|ui| {
                        // Header drag-handle: allocate a 20px tall region with click_and_drag sense.
                        let header_height = 20.0;
                        let (header_rect, _header_resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), header_height), egui::Sense::click_and_drag());
                        ui.painter().text(
                            header_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            format!("Environment Variables for {}", app_name),
                            ui.style().text_styles.get(&egui::TextStyle::Body).cloned().unwrap_or_else(|| egui::FontId::default()),
                            ui.visuals().override_text_color.unwrap_or(egui::Color32::WHITE),
                        );
                        ui.add_space(4.0);
                        // env-input area: apply background from CSS for "env-input"
                        let env_input_bg = self.theme.get_style("env-input", "background-color")
                            .and_then(|s| self.theme.parse_color(&s))
                            .unwrap_or(egui::Color32::TRANSPARENT);
                        egui::Frame::none()
                            .fill(env_input_bg)
                            .show(ui, |ui| {
                                if let (Some(w), Some(h)) = (self.theme.get_px_value("env-input", "width"), self.theme.get_px_value("env-input", "height")) {
                                    ui.set_width(w);
                                    ui.set_height(h);
                                }
                                with_alignment(ui, &self.theme, "env-input", |ui| {
                                    self.theme.apply_style(ui, "env-input");
                                    ui.add(egui::TextEdit::singleline(&mut opts)
                                        .hint_text("Enter env variables...")
                                        .frame(false)
                                    );
                                });
                            });
                        ui.add_space(4.0);
                        // Save and Cancel buttons with themed styling
                        ui.horizontal(|ui| {
                            with_custom_style(ui, |s| { self.theme.apply_widget_style(s, "edit-button"); }, |ui| {
                                if ui.button("Save").clicked() { save = true; }
                                if ui.button("Cancel").clicked() { cancel = true; }
                            });
                        });
                    });
                });
            if save {
                self.app.handle_input(&format!("LAUNCH_OPTIONS:{}:{}", app_name, opts));
            } else if !cancel {
                self.editing = Some((app_name, opts));
            }
        }
        let (esc, enter) = ctx.input(|i| (i.key_pressed(egui::Key::Escape), i.key_pressed(egui::Key::Enter)));
        match (esc, enter) {
            (true, _) => {
                if self.editing.is_some() { self.editing = None; }
                else { self.app.handle_input("ESC"); }
            }
            (_, true) => {
                if let Some((n, o)) = self.editing.take() {
                    self.app.handle_input(&format!("LAUNCH_OPTIONS:{}:{}", n, o));
                } else { self.app.handle_input("ENTER"); }
            }
            _ => {}
        }
        if self.app.should_quit() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}
