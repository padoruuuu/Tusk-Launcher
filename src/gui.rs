use std::{
    collections::HashMap,
    error::Error,
    fs::{read_to_string, create_dir_all, OpenOptions},
    io::Write,
};

use eframe::egui;
use eframe::glow::HasContext;
use libc::SIGUSR1;
use crate::{config::Config, app_launcher::AppLaunchOptions, audio::AudioController, cache::IconManager};
use xdg;

static FOCUS_REQUESTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// A simple CSS rule structure.
struct Rule {
    class_name: String,
    props: HashMap<String, String>,
}

/// Default theme – feel free to modify the colors here.
const DEFAULT_THEME: &str = r#"/* Tusk Launcher Theme CSS - Optimized for compact UI layout */
/* Tusk Launcher Theme CSS - Optimized for compact UI layout */
.main-window {
    background-color: rgba(30, 30, 30, 0.95);
    font-family: "Sans-serif", Arial;
    padding: 8px;
    border-radius: 5px;
    width: 250px;
    display: flex;
    flex-direction: column;
    align-items: center;
    /* Add max-height to ensure content fits */
    max-height: 100vh;
    box-sizing: border-box;
}

/* Section: Search Bar */
.search-bar {
    background-color: rgba(0, 0, 0, 0.85);
    color: rgba(255, 255, 255, 1.0);
    border: none;
    padding: 4px 8px;
    width: 90%;
    margin-bottom: 6px;
    border-radius: 3px;
}

/* Section: App List (Menu) */
.app-list {
    background-color: rgba(34, 34, 34, 0.8);
    padding: 6px;
    border-radius: 5px;
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 4px;
    /* Add max-height and scrolling for app list */
    max-height: calc(100vh - 120px);
    overflow-y: auto;
    box-sizing: border-box;
}

.app-item {
    display: flex;
    align-items: center;
    padding: 4px 6px;
    background-color: rgba(50, 50, 50, 0.9);
    border-radius: 3px;
}

.app-item:hover {
    background-color: rgba(70, 70, 70, 0.9);
}

/* Section: Volume Control */
.volume-slider {
    background-color: rgba(34, 34, 34, 0.8);
    color: rgba(255, 255, 255, 0.9);
    padding: 1px 1px;
    border-radius: 3px;
    width: 50%;
    margin: 1px 0;
    display: flex;
    align-items: center;
    gap: 8px;
}

/* Section: Time Display */
.time-display {
    text-align: center;
    font-size: 13px;
    color: rgba(255, 255, 255, 0.9);
    margin: 8px 0 4px 0;
    width: 100%;
    /* Ensure time display stays visible */
    flex-shrink: 0;
}

/* Section: Power Options */
.power-container {
    width: 100%;
    display: flex;
    justify-content: space-between;
    gap: 4px;
    /* Ensure power options stay at bottom */
    flex-shrink: 0;
    margin-top: auto;
}

.power-button {
    flex: 1;
    text-align: center;
    background-color: rgba(90, 90, 90, 0.9);
    padding: 4px 6px;
    border-radius: 3px;
    cursor: pointer;
}

.power-button:hover {
    background-color: rgba(110, 110, 110, 0.9);
}

"#;

/// A theme loader that reads (or creates) the CSS file and parses it into rules.
struct Theme {
    rules: Vec<Rule>,
}

impl Theme {
    fn load_or_create() -> Result<Self, Box<dyn Error>> {
        let xdg_dirs = xdg::BaseDirectories::new()?;
        let config_path = xdg_dirs.place_config_file("tusk-launcher/theme.css")?;

        if let Some(parent) = config_path.parent() {
            create_dir_all(parent)?;
        }

        if !config_path.exists() {
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .open(&config_path)?;
            file.write_all(DEFAULT_THEME.as_bytes())?;
        }

        let css = read_to_string(&config_path)?;
        let rules = Self::parse_css_rules(&css);
        Ok(Theme { rules })
    }

    fn parse_css_rules(css: &str) -> Vec<Rule> {
        let mut rules = Vec::new();
        let mut rest = css;
        while let Some(dot_index) = rest.find('.') {
            rest = &rest[dot_index + 1..];
            let class_end = rest.find(|c: char| c.is_whitespace() || c == '{').unwrap_or(0);
            if class_end == 0 {
                break;
            }
            let class_selector = rest[..class_end].trim();
            if class_selector.contains(':') {
                if let Some(brace_index) = rest.find('{') {
                    let after_brace = &rest[brace_index + 1..];
                    if let Some(end_brace) = after_brace.find('}') {
                        rest = &after_brace[end_brace + 1..];
                        continue;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            let class_name = class_selector.to_string();
            if let Some(brace_index) = rest.find('{') {
                let after_brace = &rest[brace_index + 1..];
                if let Some(end_brace) = after_brace.find('}') {
                    let block = &after_brace[..end_brace];
                    let mut props = HashMap::new();
                    for declaration in block.split(';') {
                        let declaration = declaration.trim();
                        if declaration.is_empty() {
                            continue;
                        }
                        if let Some(colon_index) = declaration.find(':') {
                            let prop = declaration[..colon_index].trim();
                            let value = declaration[colon_index + 1..].trim();
                            props.insert(prop.to_string(), value.to_string());
                        }
                    }
                    rules.push(Rule { class_name, props });
                    rest = &after_brace[end_brace + 1..];
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        rules
    }

    /// Looks up a given property for a CSS class.
    fn get_style(&self, class: &str, property: &str) -> Option<String> {
        for rule in &self.rules {
            if rule.class_name == class {
                if let Some(val) = rule.props.get(property) {
                    return Some(val.clone());
                }
            }
        }
        None
    }

    /// Parses a color string (either "rgba(...)" or "#rrggbb") into an egui::Color32.
    fn parse_color(&self, color_str: &str) -> Option<egui::Color32> {
        let s = color_str.trim().to_lowercase();
        if s.starts_with("rgba(") {
            let inner = s.trim_start_matches("rgba(").trim_end_matches(")").trim();
            let values: Vec<&str> = inner.split(',').map(|v| v.trim()).collect();
            if values.len() == 4 {
                if let (Ok(r), Ok(g), Ok(b), Ok(a)) = (
                    values[0].parse::<u8>(),
                    values[1].parse::<u8>(),
                    values[2].parse::<u8>(),
                    values[3].parse::<f32>(),
                ) {
                    return Some(egui::Color32::from_rgba_unmultiplied(
                        r,
                        g,
                        b,
                        (a * 255.0) as u8,
                    ));
                }
            }
        } else if s.starts_with('#') {
            let hex = s.trim_start_matches('#');
            if hex.len() == 6 {
                if let (Ok(r), Ok(g), Ok(b)) = (
                    u8::from_str_radix(&hex[0..2], 16),
                    u8::from_str_radix(&hex[2..4], 16),
                    u8::from_str_radix(&hex[4..6], 16),
                ) {
                    return Some(egui::Color32::from_rgb(r, g, b));
                }
            }
        }
        None
    }

    /// Returns an order value for a section (used to sort sections).
    fn get_order(&self, section: &str) -> i32 {
        self.get_style(section, "order")
            .and_then(|s| s.parse::<i32>().ok())
            .unwrap_or(0)
    }

    /// Returns the layout property for a section.
    fn get_layout(&self, section: &str) -> Option<String> {
        self.get_style(section, "layout")
    }

    /// Applies basic style properties (background-color, color, padding, border-radius, etc.)
    /// to the given UI.
    fn apply_style(&self, ui: &mut egui::Ui, class: &str) {
        let style = ui.style_mut();
        let mut visuals = style.visuals.clone();
        let mut spacing = style.spacing.clone();

        if let Some(bg_color) = self.get_style(class, "background-color") {
            if let Some(color) = self.parse_color(&bg_color) {
                visuals.window_fill = color;
                visuals.panel_fill = color;
            }
        }
        if let Some(text_color) = self.get_style(class, "color") {
            if let Some(color) = self.parse_color(&text_color) {
                visuals.override_text_color = Some(color);
            }
        }
        if let Some(padding_str) = self.get_style(class, "padding") {
            let padding_clean = padding_str.trim().replace("px", "");
            if let Ok(padding) = padding_clean.parse::<f32>() {
                spacing.item_spacing = egui::Vec2::splat(padding);
                spacing.window_margin = egui::Margin::same(padding);
            }
        }
        if let Some(radius_str) = self.get_style(class, "border-radius") {
            let radius_clean = radius_str.trim().replace("px", "");
            if let Ok(radius) = radius_clean.parse::<f32>() {
                let rounding = egui::Rounding::same(radius);
                visuals.window_rounding = rounding;
                visuals.widgets.noninteractive.rounding = rounding;
                visuals.widgets.inactive.rounding = rounding;
                visuals.widgets.hovered.rounding = rounding;
                visuals.widgets.active.rounding = rounding;
            }
        }
        if let Some(font_size_str) = self.get_style(class, "font-size") {
            let size_clean = font_size_str.trim().replace("px", "");
            if let Ok(size) = size_clean.parse::<f32>() {
                if let Some(text_style) = style.text_styles.get_mut(&egui::TextStyle::Body) {
                    text_style.size = size;
                }
            }
        }
        style.visuals = visuals;
        style.spacing = spacing;
    }
}

/// A helper that temporarily overrides the UI style. It clones the current style,
/// lets you modify it, then runs your widget-drawing code and finally restores the original style.
fn with_custom_style<R>(
    ui: &mut egui::Ui,
    modify: impl FnOnce(&mut egui::Style),
    f: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    // Save the current style.
    let old_style = ui.style().clone();
    {
        let style = ui.style_mut();
        modify(style);
    }
    let result = f(ui);
    {
        let style = ui.style_mut();
        *style = (*old_style).clone();
    }
    result
}

/// The trait your application must implement.
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

/// A simple wrapper that will be used to run your application.
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

        let _config = app.get_config();
        let audio_controller = AudioController::new(_config)?;
        audio_controller.start_polling(_config);

        eframe::run_native("Application Launcher", native_options, Box::new(|cc| {
            let wrapper = EframeWrapper {
                app,
                audio_controller,
                current_volume: 0.0,
                editing: None,
                focused: false,
                icon_manager: IconManager::new(),
                theme,
            };

            let bg_color = wrapper.theme
                .get_style("main-window", "background-color")
                .and_then(|s| wrapper.theme.parse_color(&s))
                .unwrap_or(egui::Color32::BLACK);

            if let Some(gl) = cc.gl.as_ref() {
                let r = bg_color.r() as f32 / 255.0;
                let g = bg_color.g() as f32 / 255.0;
                let b = bg_color.b() as f32 / 255.0;
                let a = bg_color.a() as f32 / 255.0;
                unsafe {
                    gl.clear_color(r, g, b, a);
                }
            }
            cc.egui_ctx.request_repaint();
            Box::new(wrapper)
        }))?;
        Ok(())
    }
}

/// A helper to apply the "align" property from your CSS to wrap a section in an appropriate layout.
fn with_alignment<R>(
    ui: &mut egui::Ui,
    theme: &Theme,
    section: &str,
    f: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    if let Some(align) = theme.get_style(section, "align") {
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

/// The main wrapper that renders your UI.
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
    /// Render the search bar.
    fn render_search_bar(&mut self, ui: &mut egui::Ui) {
        with_alignment(ui, &self.theme, "search-bar", |ui| {
            // First apply the theme styling for "search-bar"
            self.theme.apply_style(ui, "search-bar");
            // Retrieve background color and rounding (with transparency support)
            let bg_color = self
                .theme
                .get_style("search-bar", "background-color")
                .and_then(|s| self.theme.parse_color(&s))
                .unwrap_or(egui::Color32::from_rgba_unmultiplied(0, 0, 0, 220));
            let rounding = if let Some(radius_str) = self.theme.get_style("search-bar", "border-radius") {
                if let Ok(radius) = radius_str.trim().replace("px", "").parse::<f32>() {
                    egui::Rounding::same(radius)
                } else {
                    egui::Rounding::same(0.0)
                }
            } else {
                egui::Rounding::same(0.0)
            };
            // Wrap the search bar in a frame with the theme-defined fill and rounding.
            egui::Frame::none()
                .fill(bg_color)
                .rounding(rounding)
                .show(ui, |ui| {
                    with_custom_style(ui, |style| {
                        if let Some(text_color) = self
                            .theme
                            .get_style("search-bar", "color")
                            .and_then(|s| self.theme.parse_color(&s))
                        {
                            style.visuals.override_text_color = Some(text_color);
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

    /// Render the volume slider.
    fn render_volume_slider(&mut self, ui: &mut egui::Ui) {
        with_alignment(ui, &self.theme, "volume-slider", |ui| {
            self.theme.apply_style(ui, "volume-slider");
            if let Some(text_color) = self
                .theme
                .get_style("volume-slider", "color")
                .and_then(|s| self.theme.parse_color(&s))
            {
                ui.style_mut().visuals.override_text_color = Some(text_color);
            }
            ui.horizontal(|ui| {
                if let Some(gap_str) = self.theme.get_style("volume-slider", "gap") {
                    if let Ok(gap) = gap_str.trim().replace("px", "").parse::<f32>() {
                        ui.spacing_mut().item_spacing.x = gap;
                    }
                }
                ui.label("Volume:");
                let vol = &mut self.current_volume;
                // Prepare slider visuals based on the theme.
                let slider_visuals = {
                    let mut visuals = ui.style().visuals.widgets.inactive.clone();
                    if let Some(bg_color) = self.theme.get_style("volume-slider", "background-color")
                        .and_then(|s| self.theme.parse_color(&s))
                    {
                        visuals.bg_fill = bg_color;
                    }
                    if let Some(radius_str) = self.theme.get_style("volume-slider", "border-radius") {
                        if let Ok(radius) = radius_str.trim().replace("px", "").parse::<f32>() {
                            visuals.rounding = egui::Rounding::same(radius);
                        }
                    }
                    visuals
                };
                with_custom_style(ui, |style| {
                    style.visuals.widgets.inactive = slider_visuals.clone();
                    style.visuals.widgets.hovered = slider_visuals.clone();
                    style.visuals.widgets.active = slider_visuals.clone();
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

    /// Render the application list.
    fn render_app_list(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        with_alignment(ui, &self.theme, "app-list", |ui| {
            self.theme.apply_style(ui, "app-list");
            egui::ScrollArea::vertical().show(ui, |ui| {
                for app_name in self.app.get_search_results() {
                    // Wrap each application item in a frame using the "app-item" style.
                    let item_bg_color = self.theme.get_style("app-item", "background-color")
                        .and_then(|s| self.theme.parse_color(&s))
                        .unwrap_or(egui::Color32::from_rgba_unmultiplied(50, 50, 50, 230));
                    let item_rounding = if let Some(radius_str) = self.theme.get_style("app-item", "border-radius") {
                        if let Ok(radius) = radius_str.trim().replace("px", "").parse::<f32>() {
                            egui::Rounding::same(radius)
                        } else {
                            egui::Rounding::same(0.0)
                        }
                    } else {
                        egui::Rounding::same(0.0)
                    };
                    egui::Frame::none()
                        .fill(item_bg_color)
                        .rounding(item_rounding)
                        .inner_margin(egui::Margin {
                            left: 6.0,
                            right: 6.0,
                            top: 4.0,
                            bottom: 4.0,
                        })
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
                                    if let Some(tex) = self.app.get_icon_path(&app_name)
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
                                    // Draw the settings icon.
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
                                    // Apply custom style for the app button.
                                    with_custom_style(ui, |style| {
                                        if let Some(bg_color) = self.theme.get_style("app-button", "background-color")
                                            .and_then(|s| self.theme.parse_color(&s))
                                        {
                                            style.visuals.widgets.inactive.bg_fill = bg_color;
                                            style.visuals.widgets.hovered.bg_fill = bg_color;
                                            style.visuals.widgets.active.bg_fill = bg_color;
                                        }
                                        if let Some(radius_str) = self.theme.get_style("app-button", "border-radius") {
                                            if let Ok(radius) = radius_str.trim().replace("px", "").parse::<f32>() {
                                                let rounding = egui::Rounding::same(radius);
                                                style.visuals.widgets.inactive.rounding = rounding;
                                                style.visuals.widgets.hovered.rounding = rounding;
                                                style.visuals.widgets.active.rounding = rounding;
                                            }
                                        }
                                        if let Some(text_color) = self.theme.get_style("app-button", "color")
                                            .and_then(|s| self.theme.parse_color(&s))
                                        {
                                            style.visuals.override_text_color = Some(text_color);
                                        }
                                    }, |ui| {
                                        if ui.add(egui::Button::new(&app_name).min_size(egui::Vec2::new(0.0, 15.0))).clicked() {
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

    /// Render the time display.
    fn render_time_display(&mut self, ui: &mut egui::Ui) {
        with_alignment(ui, &self.theme, "time-display", |ui| {
            self.theme.apply_style(ui, "time-display");
            ui.label(self.app.get_time());
        });
    }

    /// Render the power buttons.
    fn render_power_button(&mut self, ui: &mut egui::Ui) {
        with_alignment(ui, &self.theme, "power-button", |ui| {
            if let Some(bg_color) = self.theme.get_style("power-button", "background-color")
                .and_then(|s| self.theme.parse_color(&s))
            {
                with_custom_style(ui, |style| {
                    style.visuals.widgets.inactive.bg_fill = bg_color;
                    style.visuals.widgets.hovered.bg_fill = bg_color;
                    style.visuals.widgets.active.bg_fill = bg_color;
                    if let Some(text_color) = self.theme.get_style("power-button", "color")
                        .and_then(|s| self.theme.parse_color(&s))
                    {
                        style.visuals.override_text_color = Some(text_color);
                    }
                }, |ui| {
                    ui.horizontal(|ui| {
                        for &(l, c) in &[("Power", "P"), ("Restart", "R"), ("Logout", "L")] {
                            if ui.button(l).clicked() {
                                self.app.handle_input(c);
                            }
                        }
                    });
                });
            } else {
                ui.horizontal(|ui| {
                    for &(l, c) in &[("Power", "P"), ("Restart", "R"), ("Logout", "L")] {
                        if ui.button(l).clicked() {
                            self.app.handle_input(c);
                        }
                    }
                });
            }
        });
    }

    /// Dispatch rendering based on section.
    fn render_section(&mut self, ui: &mut egui::Ui, section: &str, ctx: &egui::Context) {
        match section {
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
        if FOCUS_REQUESTED.swap(false, std::sync::atomic::Ordering::Relaxed) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        self.app.update();
        if self.audio_controller.update_volume().is_ok() {
            self.current_volume = self.audio_controller.get_volume();
        }

        let bg_color = self.theme
            .get_style("main-window", "background-color")
            .and_then(|s| self.theme.parse_color(&s))
            .unwrap_or(egui::Color32::BLACK);
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(bg_color))
            .show(ctx, |ui| {
                self.theme.apply_style(ui, "main-window");

                let mut sections = vec![
                    ("search-bar", self.theme.get_order("search-bar"), self.theme.get_layout("search-bar")),
                    ("app-list", self.theme.get_order("app-list"), self.theme.get_layout("app-list")),
                    ("volume-slider", self.theme.get_order("volume-slider"), self.theme.get_layout("volume-slider")),
                    ("time-display", self.theme.get_order("time-display"), self.theme.get_layout("time-display")),
                    ("power-button", self.theme.get_order("power-button"), self.theme.get_layout("power-button")),
                ];
                sections.sort_by_key(|&(_, order, _)| order);

                let mut i = 0;
                while i < sections.len() {
                    if sections[i].2.as_deref() == Some("horizontal") {
                        let mut group = vec![sections[i].0];
                        i += 1;
                        while i < sections.len() && sections[i].2.as_deref() == Some("horizontal") {
                            group.push(sections[i].0);
                            i += 1;
                        }
                        ui.horizontal(|ui| {
                            for sec in group {
                                self.render_section(ui, sec, ctx);
                            }
                        });
                    } else {
                        self.render_section(ui, sections[i].0, ctx);
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
    FOCUS_REQUESTED.store(true, std::sync::atomic::Ordering::Relaxed);
}
