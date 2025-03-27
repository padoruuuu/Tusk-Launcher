use std::{
    collections::HashMap,
    error::Error,
    fs::{read_to_string, create_dir_all, OpenOptions},
    io::Write,
    path::PathBuf,
    process,
};

use chrono::{DateTime, Local};
use eframe;
use serde::{Deserialize, Serialize};
use xdg;

const DEFAULT_THEME: &str = r#"/* Centered Streamlined Compact Theme with Absolute Positioning */

/* Main Window */
.main-window {
    background-color: rgba(0, 0, 0, 0.9);
    width: 300px;
    height: 200px;
}

/* Search Bar */
.search-bar {
    x: 60px;
    y: 10px;
    width: 150px;
    height: 25px;
    background-color: rgba(59, 66, 82, 1);
    hover-background-color: rgba(76, 86, 106, 1);
    border-radius: 6px;
    text-color: rgba(236, 239, 244, 1);
    hover-text-color: rgba(236, 239, 244, 1);
    padding: 4px;
    font-size: 12px;
}

/* App List */
.app-list {
    x: 62px;
    y: 40px;
    width: 109px;
    height: 108px;
    background-color: rgba(46, 52, 64, 1);
    padding: 1px;
}


/* App Button (used with custom_button for launching apps) */
.app-button {
    background-color: rgba(122, 162, 247, 1);
    hover-background-color: rgba(102, 138, 196, 1);
    text-color: rgba(236, 239, 244, 1);
    hover-text-color: rgba(236, 239, 244, 1);
    border-radius: 4px;
    padding: 3px;
    font-size: 14px;
}

/* Time Display */
.time-display {
    x: 72px;
    y: 160px;
    width: 200px;
    height: 50px;
    background-color: rgba(46, 52, 64, 1);
    text-color: rgba(236, 239, 244, 1);
    hover-text-color: rgba(236, 239, 244, 1);
    text-align: center;
}

/* Volume Slider */
.volume-slider {
    x: 40px;
    y: 155px;
    width: 200px;
    height: 50px;
    background-color: rgba(46, 52, 64, 1);
    hover-background-color: rgba(67, 76, 94, 1);
    text-color: rgba(236, 239, 244, 1);
    hover-text-color: rgba(236, 239, 244, 1);
    border-radius: 4px;
}

/* Power Button (used with custom_button for power options) */
.power-button {
    x: 60px;
    y: 190px;
    width: 65px;
    height: 15px;
    background-color: rgba(122, 162, 247, 1);
    hover-background-color: rgba(102, 138, 196, 1);
    text-color: rgba(236, 239, 244, 1);
    hover-text-color: rgba(236, 239, 244, 1);
    border-radius: 4px;
}

/* Edit Button (used with custom_button for Save/Cancel in env window) */
.edit-button {
    background-color: rgba(122, 162, 247, 1);
    hover-background-color: rgba(102, 138, 196, 1);
    text-color: rgba(236, 239, 244, 1);
    hover-text-color: rgba(236, 239, 244, 1);
    border-radius: 4px;
    padding: 3px;
    font-size: 12px;
}

/* Environment Variable Input Window */
.env-input {
    background-color: rgba(59, 66, 82, 1);
    text-color: rgba(236, 239, 244, 1);
    hover-text-color: rgba(236, 239, 244, 1);
    padding: 6px;
    font-size: 12px;
    border-radius: 4px;
    width: 250px;
    height: 50px;
}

/* Settings Button */
.settings-button {
    width: 22px;
    height: 22px;
    hover-text-color: rgba(102, 138, 196, 0.5);
    text-color: rgba(122, 162, 247, 1);
    font-size: 16px;
    x-offset: -10px;
    y-offset: 0px;
}


.config {
    enable_recent_apps: true;
    max_search_results: 5;
    enable_power_options: true;
    show_time: true;
    time_format: "%I:%M %p";
    time_order: HdyHms;
    enable_audio_control: false;
    max_volume: 1.5;
    volume_update_interval_ms: 500;
    power_commands: systemctl poweroff, loginctl poweroff, poweroff, halt;
    restart_commands: systemctl reboot, loginctl reboot, reboot;
    logout_commands: loginctl terminate-session $XDG_SESSION_ID, hyprctl dispatch exit, swaymsg exit, gnome-session-quit --logout --no-prompt, qdbus org.kde.ksmserver /KSMServer logout 0 0 0;
    icon_cache_dir: "/home/zeakz/.config/tusk-launcher/icons";
}
"#;

/// Remove C-style comments (/* ... */) from the CSS.
fn remove_comments(css: &str) -> String {
    let mut out = String::new();
    let mut in_comment = false;
    let mut chars = css.chars().peekable();
    while let Some(c) = chars.next() {
        if in_comment {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                in_comment = false;
            }
        } else if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            in_comment = true;
        } else {
            out.push(c);
        }
    }
    out
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Config {
    pub enable_recent_apps: bool,
    pub max_search_results: usize,
    pub enable_power_options: bool,
    pub show_time: bool,
    pub time_format: String,
    pub time_order: TimeOrder,
    pub enable_audio_control: bool,
    pub max_volume: f32,
    pub volume_update_interval_ms: u64,
    pub power_commands: Vec<String>,
    pub restart_commands: Vec<String>,
    pub logout_commands: Vec<String>,
    pub enable_icons: bool,
    pub icon_cache_dir: PathBuf,
}

#[derive(Serialize, Deserialize, Clone)]
pub enum TimeOrder {
    MdyHms,
    YmdHms,
    DmyHms,
}

impl Default for Config {
    fn default() -> Self {
        let icon_cache_dir = xdg::BaseDirectories::new()
            .map(|bd| bd.get_config_home().join("tusk-launcher/icons"))
            .unwrap_or_else(|_| PathBuf::from(".").join("tusk-launcher/icons"));
        Self {
            enable_recent_apps: true,
            max_search_results: 5,
            enable_power_options: true,
            show_time: true,
            time_format: "%I:%M %p".to_string(),
            time_order: TimeOrder::MdyHms,
            enable_audio_control: true,
            max_volume: 1.5,
            volume_update_interval_ms: 500,
            power_commands: vec![
                "systemctl poweroff".to_string(),
                "loginctl poweroff".to_string(),
                "poweroff".to_string(),
                "halt".to_string(),
            ],
            restart_commands: vec![
                "systemctl reboot".to_string(),
                "loginctl reboot".to_string(),
                "reboot".to_string(),
            ],
            logout_commands: vec![
                "loginctl terminate-session $XDG_SESSION_ID".to_string(),
                "hyprctl dispatch exit".to_string(),
                "swaymsg exit".to_string(),
                "gnome-session-quit --logout --no-prompt".to_string(),
                "qdbus org.kde.ksmserver /KSMServer logout 0 0 0".to_string(),
            ],
            enable_icons: true,
            icon_cache_dir,
        }
    }
}

/// Formats a given datetime according to the configuration.
pub fn format_datetime(datetime: &DateTime<Local>, config: &Config) -> String {
    let date = match config.time_order {
        TimeOrder::MdyHms => datetime.format("%m/%d/%Y"),
        TimeOrder::YmdHms => datetime.format("%Y/%m/%d"),
        TimeOrder::DmyHms => datetime.format("%d/%m/%Y"),
    };
    let time = datetime.format(&config.time_format);
    format!("{} {}", time, date)
}

#[derive(Serialize, Deserialize, Clone)]
struct Rule {
    class_name: String,
    props: HashMap<String, String>,
}

/// The Theme struct holds all parsed CSS rules.
pub struct Theme {
    rules: Vec<Rule>,
}

impl Theme {
    fn load_or_create() -> Result<Self, Box<dyn Error>> {
        let dirs = xdg::BaseDirectories::new()?;
        let path = dirs.place_config_file("tusk-launcher/theme.css")?;
        if let Some(parent) = path.parent() {
            create_dir_all(parent)?;
        }
        if !path.exists() {
            OpenOptions::new().write(true).create(true).open(&path)?
                .write_all(DEFAULT_THEME.as_bytes())?;
        }
        let content = read_to_string(&path)?;
        let cleaned = remove_comments(content.trim());
        Ok(Self {
            rules: Self::parse_css_rules(cleaned.trim()),
        })
    }

    fn parse_css_rules(css: &str) -> Vec<Rule> {
        let mut rules = Vec::new();
        let mut rest = css;
        while let Some(dot) = rest.find('.') {
            rest = &rest[dot + 1..];
            let class_end = rest.find(|c: char| c.is_whitespace() || c == '{').unwrap_or(rest.len());
            if class_end == 0 { break; }
            let class_name = rest[..class_end].trim().to_string();
            if let Some(brace_pos) = rest.find('{') {
                let after_brace = &rest[brace_pos + 1..];
                if let Some(end_brace) = after_brace.find('}') {
                    let block = &after_brace[..end_brace];
                    let props = block.split(';')
                        .filter_map(|decl| {
                            let decl = decl.trim();
                            if decl.is_empty() { None }
                            else { decl.split_once(':').map(|(k, v)| (k.trim().to_string(), v.trim().to_string())) }
                        })
                        .collect();
                    rules.push(Rule { class_name, props });
                    rest = &after_brace[end_brace + 1..];
                } else { break; }
            } else { break; }
        }
        rules
    }

    fn get_style(&self, class: &str, prop: &str) -> Option<String> {
        self.rules.iter()
            .find(|r| r.class_name.trim().to_lowercase() == class.trim().to_lowercase())
            .and_then(|r| r.props.get(prop).cloned())
    }

    fn get_combined_style(&self, classes: &[&str], prop: &str) -> Option<String> {
        classes.iter().find_map(|&c| self.get_style(c, prop))
    }

    fn parse_color(&self, s: &str) -> Option<eframe::egui::Color32> {
        let s = s.trim().to_lowercase();
        if s == "transparent" {
            return Some(eframe::egui::Color32::TRANSPARENT);
        }
        if s.starts_with("rgba(") {
            let inner = s.strip_prefix("rgba(")?.strip_suffix(")")?.trim();
            let vals: Vec<&str> = inner.split(',').map(|v| v.trim()).collect();
            if vals.len() == 4 {
                let r = vals[0].parse().ok()?;
                let g = vals[1].parse().ok()?;
                let b = vals[2].parse().ok()?;
                let a = vals[3].parse::<f32>().ok()?;
                return Some(eframe::egui::Color32::from_rgba_unmultiplied(r, g, b, (a * 255.0) as u8));
            }
        } else if s.starts_with('#') && s.len() == 7 {
            let hex = &s[1..];
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            return Some(eframe::egui::Color32::from_rgb(r, g, b));
        }
        None
    }

    fn get_px_value(&self, class: &str, prop: &str) -> Option<f32> {
        self.get_style(class, prop)?.trim_end_matches("px").parse().ok()
    }

    fn get_order(&self, section: &str) -> i32 {
        self.get_style(section, "order").and_then(|s| s.parse().ok()).unwrap_or(0)
    }

    pub fn get_config(&self) -> Config {
        let mut config = Config::default();
        if let Some(rule) = self.rules.iter().find(|r| r.class_name.trim().to_lowercase() == "config") {
            if let Some(val) = rule.props.get("max_search_results") {
                if let Ok(n) = val.parse::<usize>() { config.max_search_results = n; }
            }
            if let Some(val) = rule.props.get("enable_power_options") {
                if let Ok(b) = val.parse::<bool>() { config.enable_power_options = b; }
            }
            if let Some(val) = rule.props.get("show_time") {
                if let Ok(b) = val.parse::<bool>() { config.show_time = b; }
            }
            if let Some(val) = rule.props.get("time_format") {
                config.time_format = val.clone();
            }
            if let Some(val) = rule.props.get("time_order") {
                config.time_order = match val.as_str() {
                    "YmdHms" => TimeOrder::YmdHms,
                    "DmyHms" => TimeOrder::DmyHms,
                    _ => TimeOrder::MdyHms,
                };
            }
            if let Some(val) = rule.props.get("enable_audio_control") {
                if let Ok(b) = val.parse::<bool>() { config.enable_audio_control = b; }
            }
            if let Some(val) = rule.props.get("max_volume") {
                if let Ok(v) = val.parse::<f32>() { config.max_volume = v; }
            }
            if let Some(val) = rule.props.get("volume_update_interval_ms") {
                if let Ok(n) = val.parse::<u64>() { config.volume_update_interval_ms = n; }
            }
            if let Some(val) = rule.props.get("power_commands") {
                config.power_commands = val.split(',').map(|s| s.trim().to_string()).collect();
            }
            if let Some(val) = rule.props.get("restart_commands") {
                config.restart_commands = val.split(',').map(|s| s.trim().to_string()).collect();
            }
            if let Some(val) = rule.props.get("logout_commands") {
                config.logout_commands = val.split(',').map(|s| s.trim().to_string()).collect();
            }
            if let Some(val) = rule.props.get("icon_cache_dir") {
                let trimmed = val.trim().trim_matches('"');
                if trimmed.is_empty() {
                    config.enable_icons = false;
                } else {
                    config.icon_cache_dir = PathBuf::from(trimmed);
                    config.enable_icons = true;
                }
            }
        }
        config
    }

    fn get_frame_properties(&self, class: &str, default_fill: eframe::egui::Color32)
        -> (eframe::egui::Color32, Option<eframe::egui::Color32>, eframe::egui::CornerRadius) {
        let base = self.get_style(class, "background-color")
            .and_then(|s| self.parse_color(&s))
            .unwrap_or(default_fill);
        let hover = self.get_style(class, "hover-background-color")
            .and_then(|s| self.parse_color(&s));
        let rounding = self.get_style(class, "border-radius")
            .and_then(|s| s.replace("px", "").parse::<f32>().ok())
            .map(|val| eframe::egui::CornerRadius::same(val as u8))
            .unwrap_or_default();
        (base, hover, rounding)
    }

    fn apply_style(&self, ui: &mut eframe::egui::Ui, class: &str) {
        let style = ui.style_mut();
        if let Some(bg) = self.get_style(class, "background-color")
            .and_then(|s| self.parse_color(&s)) {
            style.visuals.panel_fill = bg;
        }
        if let Some(col) = self.get_style(class, "text-color")
            .and_then(|s| self.parse_color(&s)) {
            style.visuals.override_text_color = Some(col);
        }
        if let Some(pad) = self.get_style(class, "padding")
            .and_then(|s| s.replace("px", "").parse::<f32>().ok()) {
            style.spacing.item_spacing = eframe::egui::vec2(pad, pad);
            style.spacing.window_margin = eframe::egui::Margin::symmetric(pad as i8, pad as i8);
        }
        if let Some(rad) = self.get_style(class, "border-radius")
            .and_then(|s| s.replace("px", "").parse::<f32>().ok()) {
            let r = eframe::egui::CornerRadius::same(rad as u8);
            for widget in [
                &mut style.visuals.widgets.noninteractive,
                &mut style.visuals.widgets.inactive,
                &mut style.visuals.widgets.hovered,
                &mut style.visuals.widgets.active,
            ]
            .iter_mut() {
                widget.corner_radius = r;
            }
        }
        if let Some(sz) = self.get_style(class, "font-size")
            .and_then(|s| s.replace("px", "").parse::<f32>().ok()) {
            if let Some(text) = ui.style_mut().text_styles.get_mut(&eframe::egui::TextStyle::Body) {
                text.size = sz;
            }
        }
    }

    fn apply_widget_style(&self, style: &mut eframe::egui::Style, class: &str) {
        if let Some(bg) = self.get_style(class, "background-color").and_then(|s| self.parse_color(&s)) {
            let hover = self.get_style(class, "hover-background-color")
                .and_then(|s| self.parse_color(&s))
                .unwrap_or(bg);
            set_widget_bg(style, bg, hover);
        }
        if let Some(tc) = self.get_style(class, "text-color").and_then(|s| self.parse_color(&s)) {
            style.visuals.override_text_color = Some(tc);
        }
    }

    fn apply_combined_widget_style(&self, style: &mut eframe::egui::Style, classes: &[&str]) {
        let default_fill = eframe::egui::Color32::TRANSPARENT;
        let base = self.get_combined_style(classes, "container-background-color")
            .or_else(|| self.get_combined_style(classes, "background-color"))
            .and_then(|s| self.parse_color(&s))
            .unwrap_or(default_fill);
        let hover = self.get_combined_style(classes, "hover-background-color")
            .and_then(|s| self.parse_color(&s))
            .unwrap_or(base);
        set_widget_bg(style, base, hover);
        if let Some(tc) = self.get_combined_style(classes, "text-color")
            .and_then(|s| self.parse_color(&s)) {
            style.visuals.override_text_color = Some(tc);
        }
    }

    fn get_text_color(&self, class: &str, hovered: bool) -> Option<eframe::egui::Color32> {
        if hovered {
            self.get_style(class, "hover-text-color")
                .and_then(|s| self.parse_color(&s))
                .or_else(|| self.get_style(class, "text-color").and_then(|s| self.parse_color(&s)))
        } else {
            self.get_style(class, "text-color").and_then(|s| self.parse_color(&s))
        }
    }
}

/// Helper to set widget background properties.
fn set_widget_bg(style: &mut eframe::egui::Style, base: eframe::egui::Color32, hover: eframe::egui::Color32) {
    let transparent = eframe::egui::Color32::TRANSPARENT;
    let widgets = &mut style.visuals.widgets;
    widgets.inactive.bg_fill = base;
    widgets.hovered.bg_fill = hover;
    widgets.hovered.weak_bg_fill = hover;
    widgets.active.bg_fill = base;
    widgets.inactive.weak_bg_fill = base;
    widgets.active.weak_bg_fill = base;
    widgets.inactive.bg_stroke = eframe::egui::Stroke::new(0.0, transparent);
    widgets.hovered.bg_stroke = eframe::egui::Stroke::new(0.0, transparent);
    widgets.active.bg_stroke = eframe::egui::Stroke::new(0.0, transparent);
    widgets.hovered.expansion = 0.0;
    widgets.active.expansion = 0.0;
}

fn custom_button(ui: &mut eframe::egui::Ui, label: &str, class: &str, theme: &Theme) -> eframe::egui::Response {
    let text_style = eframe::egui::TextStyle::Button;
    let font_id = ui.style().text_styles.get(&text_style).cloned().unwrap_or_default();
    let galley = ui.fonts(|fonts| {
        fonts.layout_no_wrap(label.to_owned(), font_id.clone(), eframe::egui::Color32::WHITE)
    });
    let desired_size = galley.size() + ui.spacing().button_padding * 2.0;
    let (rect, response) = ui.allocate_exact_size(desired_size, eframe::egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let (base_bg, hover_bg, rounding) = theme.get_frame_properties(class, ui.style().visuals.widgets.inactive.bg_fill);
        let normal_tc = theme.get_style(class, "text-color")
            .and_then(|s| theme.parse_color(&s))
            .unwrap_or(eframe::egui::Color32::WHITE);
        let hover_tc = theme.get_style(class, "hover-text-color")
            .and_then(|s| theme.parse_color(&s))
            .unwrap_or(normal_tc);
        let bg = if response.hovered() { hover_bg.unwrap_or(base_bg) } else { base_bg };
        ui.painter().rect_filled(rect, rounding, bg);
        let text_color = if response.hovered() { hover_tc } else { normal_tc };
        let center_align = eframe::egui::Align2([eframe::egui::Align::Center, eframe::egui::Align::Center]);
        ui.painter().galley(rect.center() - galley.size() * 0.5, galley, text_color);
        ui.painter().text(rect.center(), center_align, label, font_id, text_color);
    }
    response
}

fn with_custom_style<R>(
    ui: &mut eframe::egui::Ui,
    modify: impl FnOnce(&mut eframe::egui::Style),
    f: impl FnOnce(&mut eframe::egui::Ui) -> R,
) -> R {
    let old = (**ui.style()).clone();
    modify(ui.style_mut());
    let res = f(ui);
    *ui.style_mut() = old;
    res
}

fn with_alignment<R>(
    ui: &mut eframe::egui::Ui,
    theme: &Theme,
    section: &str,
    f: impl FnOnce(&mut eframe::egui::Ui) -> R,
) -> R {
    if let Some(pos) = theme.get_style(section, "position") {
        let layout = match pos.as_str() {
            "center" => eframe::egui::Layout::centered_and_justified(eframe::egui::Direction::LeftToRight),
            "left" => eframe::egui::Layout::left_to_right(eframe::egui::Align::Min),
            "right" => eframe::egui::Layout::right_to_left(eframe::egui::Align::Max),
            _ => eframe::egui::Layout::default(),
        };
        ui.with_layout(layout, f).inner
    } else if let Some(align) = theme.get_style(section, "align") {
        let layout = match align.as_str() {
            "center" => eframe::egui::Layout::left_to_right(eframe::egui::Align::Center),
            "right" => eframe::egui::Layout::right_to_left(eframe::egui::Align::Center),
            "left" => eframe::egui::Layout::left_to_right(eframe::egui::Align::Min),
            _ => eframe::egui::Layout::default(),
        };
        ui.with_layout(layout, f).inner
    } else {
        f(ui)
    }
}

// ===================
// Application Interface & GUI
// ===================

pub trait AppInterface {
    fn update(&mut self);
    fn handle_input(&mut self, input: &str);
    fn should_quit(&self) -> bool;
    fn get_query(&self) -> String;
    fn get_search_results(&self) -> Vec<String>;
    fn get_time(&self) -> String;
    fn launch_app(&mut self, app_name: &str);
    fn get_icon_path(&self, app_name: &str) -> Option<String>;
    fn get_formatted_launch_options(&self, app_name: &str) -> String;
}

pub struct EframeGui;

impl EframeGui {
    pub fn run(app: Box<dyn AppInterface>) -> Result<(), Box<dyn Error>> {
        let (width, height) = (300.0, 200.0);
        let viewport_builder = eframe::egui::ViewportBuilder::default()
            .with_inner_size([width, height])
            .with_always_on_top()
            .with_decorations(false)
            .with_resizable(false)
            .with_active(true)
            .with_transparent(true)
            .with_position(eframe::egui::pos2(0.0, 0.0));
        let native_options = eframe::NativeOptions {
            viewport: viewport_builder,
            ..Default::default()
        };

        let theme = Theme::load_or_create()?;
        let cfg = theme.get_config();
        let audio = crate::audio::AudioController::new(&cfg)?;
        audio.start_polling(&cfg);

        eframe::run_native(
            "Application Launcher",
            native_options,
            Box::new(move |cc| {
                if let Some(scaling_str) = theme.get_style("env-input", "scaling") {
                    if let Ok(scaling) = scaling_str.trim().parse::<f32>() {
                        cc.egui_ctx.set_pixels_per_point(scaling);
                    }
                }
                cc.egui_ctx.request_repaint();
                Ok(Box::new(EframeWrapper {
                    app,
                    audio_controller: audio,
                    current_volume: 0.0,
                    editing: None,
                    focused: false,
                    icon_manager: crate::cache::IconManager::new(),
                    theme,
                }))
            }),
        )?;
        Ok(())
    }
}

struct EframeWrapper {
    app: Box<dyn AppInterface>,
    audio_controller: crate::audio::AudioController,
    current_volume: f32,
    editing: Option<(String, String)>,
    focused: bool,
    icon_manager: crate::cache::IconManager,
    theme: Theme,
}

impl EframeWrapper {
    fn render_search_bar(&mut self, ui: &mut eframe::egui::Ui) {
        with_alignment(ui, &self.theme, "search-bar", |ui| {
            self.theme.apply_style(ui, "search-bar");
            let (base_bg, hover_bg, rounding) = self.theme.get_frame_properties("search-bar", ui.visuals().panel_fill);
            let rect = ui.available_rect_before_wrap();
            let resp = ui.interact(rect, ui.id().with("search-bar"), eframe::egui::Sense::hover());
            let fill = if resp.hovered() { hover_bg.unwrap_or(base_bg) } else { base_bg };
            eframe::egui::Frame::NONE.fill(fill)
                .corner_radius(rounding)
                .show(ui, |ui| {
                    with_custom_style(ui, |s| {
                        if let Some(tc) = self.theme.get_text_color("search-bar", resp.hovered()) {
                            s.visuals.override_text_color = Some(tc);
                        }
                    }, |ui| {
                        let mut query = self.app.get_query();
                        let resp = ui.add(eframe::egui::TextEdit::singleline(&mut query)
                            .hint_text("Search...")
                            .frame(false));
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

    fn render_volume_slider(&mut self, ui: &mut eframe::egui::Ui) {
        with_alignment(ui, &self.theme, "volume-slider", |ui| {
            self.theme.apply_style(ui, "volume-slider");
            ui.horizontal(|ui| {
                if let Some(gap) = self.theme.get_px_value("volume-slider", "gap") {
                    ui.spacing_mut().item_spacing.x = gap;
                }
                ui.label("Volume:");
                let (base, hover, rounding) = self.theme.get_frame_properties("volume-slider", ui.style().visuals.widgets.inactive.bg_fill);
                let slider_visuals = {
                    let mut s = ui.style().visuals.widgets.inactive.clone();
                    s.bg_fill = base;
                    s.corner_radius = rounding;
                    s
                };
                with_custom_style(ui, |s| {
                    s.visuals.widgets.inactive = slider_visuals.clone();
                    s.visuals.widgets.hovered.bg_fill = hover.unwrap_or(base);
                    s.visuals.widgets.hovered.weak_bg_fill = hover.unwrap_or(base);
                    s.visuals.widgets.active = slider_visuals.clone();
                    s.visuals.widgets.inactive.bg_stroke = eframe::egui::Stroke::new(0.0, eframe::egui::Color32::TRANSPARENT);
                    s.visuals.widgets.hovered.bg_stroke = eframe::egui::Stroke::new(0.0, eframe::egui::Color32::TRANSPARENT);
                    s.visuals.widgets.active.bg_stroke = eframe::egui::Stroke::new(0.0, eframe::egui::Color32::TRANSPARENT);
                    s.visuals.widgets.hovered.expansion = 0.0;
                    s.visuals.widgets.active.expansion = 0.0;
                }, |ui| {
                    let slider = eframe::egui::Slider::new(&mut self.current_volume, 0.0..=self.theme.get_config().max_volume)
                        .custom_formatter(|n, _| format!("{:.0}%", n * 100.0))
                        .custom_parser(|s| s.trim().trim_end_matches('%').parse::<f64>().ok().map(|n| n / 100.0));
                    if ui.add(slider).changed() {
                        let _ = self.audio_controller.set_volume(self.current_volume);
                    }
                });
            });
        });
    }

    fn render_app_list(&mut self, ui: &mut eframe::egui::Ui, ctx: &eframe::egui::Context) {
        self.theme.apply_style(ui, "app-list");
        let query = self.app.get_query();
        let cfg = self.theme.get_config();
        let filtered: Vec<String> = if query.trim().is_empty() {
            if cfg.enable_recent_apps {
                self.app.get_search_results().into_iter().take(cfg.max_search_results).collect()
            } else {
                Vec::new()
            }
        } else {
            self.app.get_search_results().into_iter().take(cfg.max_search_results).collect()
        };
        ui.vertical(|ui| {
            for app_name in filtered {
                ui.horizontal(|ui| {
                    let mut settings_clicked = false;
                    let btn_width = self.theme.get_px_value("settings-button", "width").unwrap_or(22.0);
                    let btn_height = self.theme.get_px_value("settings-button", "height").unwrap_or(22.0);
                    let (rect, icon_resp) = ui.allocate_exact_size(eframe::egui::vec2(btn_width, btn_height), eframe::egui::Sense::click());
                    if icon_resp.clicked() && self.editing.is_none() {
                        settings_clicked = true;
                    }
                    // Draw icon if available.
                    if let Some(icon_path) = self.app.get_icon_path(&app_name) {
                        if let Some(tex) = self.icon_manager.get_texture(ctx, &icon_path) {
                            ui.painter().image(
                                tex.id(),
                                rect,
                                eframe::egui::Rect::from_min_max(eframe::egui::Pos2::ZERO, eframe::egui::Pos2::new(1.0, 1.0)),
                                eframe::egui::Color32::WHITE,
                            );
                        }
                    }
                    // Apply settings button style before drawing gear.
                    self.theme.apply_style(ui, "settings-button");
                    let gear = "⚙";
                    let gear_color = self.theme.get_text_color("settings-button", icon_resp.hovered())
                        .unwrap_or(eframe::egui::Color32::from_rgb(64, 64, 64));
                    let gear_font = if let Some(font_size_str) = self.theme.get_style("settings-button", "font-size") {
                        let mut text = eframe::egui::TextStyle::Button.resolve(ui.style());
                        if let Ok(font_size) = font_size_str.replace("px", "").parse::<f32>() {
                            text.size = font_size;
                        }
                        text
                    } else {
                        eframe::egui::TextStyle::Button.resolve(ui.style())
                    };
                    let gear_size = ui.fonts(|f| {
                        f.layout_no_wrap(gear.to_owned(), gear_font.clone(), gear_color).size()
                    });
                    let x_offset = self.theme.get_px_value("settings-button", "x-offset").unwrap_or(0.0);
                    let y_offset = self.theme.get_px_value("settings-button", "y-offset").unwrap_or(0.0);
                    let gear_pos = eframe::egui::pos2(
                        rect.center().x - gear_size.x / 2.0 + x_offset,
                        rect.center().y - gear_size.y / 2.0 + y_offset,
                    );
                    let center_align = eframe::egui::Align2([eframe::egui::Align::Center, eframe::egui::Align::Center]);
                    ui.painter().text(gear_pos, center_align, gear, gear_font, gear_color);
                    
                    if settings_clicked {
                        let prepop = self.app.get_formatted_launch_options(&app_name);
                        self.editing = Some((app_name.clone(), prepop));
                    } else {
                        with_custom_style(ui, |s| {
                            self.theme.apply_combined_widget_style(s, &["app-button"]);
                        }, |ui| {
                            if custom_button(ui, &app_name, "app-button", &self.theme).clicked() {
                                self.app.launch_app(&app_name);
                            }
                        });
                    }
                });
                ui.add_space(4.0);
            }
        });
    }

    fn render_time_display(&mut self, ui: &mut eframe::egui::Ui) {
        with_alignment(ui, &self.theme, "time-display", |ui| {
            self.theme.apply_style(ui, "time-display");
            ui.label(self.app.get_time());
        });
    }

    fn render_power_button(&mut self, ui: &mut eframe::egui::Ui) {
        with_alignment(ui, &self.theme, "power-button", |ui| {
            with_custom_style(ui, |s| {
                self.theme.apply_widget_style(s, "power-button");
            }, |ui| {
                ui.horizontal(|ui| {
                    for &(label, cmd) in &[("Power", "P"), ("Restart", "R"), ("Logout", "L")] {
                        if custom_button(ui, label, "power-button", &self.theme).clicked() {
                            self.app.handle_input(cmd);
                        }
                    }
                });
            });
        });
    }

    fn render_section(&mut self, ui: &mut eframe::egui::Ui, sec: &str, ctx: &eframe::egui::Context) {
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
    fn update(&mut self, ctx: &eframe::egui::Context, _frame: &mut eframe::Frame) {
        self.app.update();
        let cfg = self.theme.get_config();
        if cfg.enable_audio_control {
            if self.audio_controller.update_volume().is_ok() {
                self.current_volume = self.audio_controller.get_volume();
            }
        }
        // Use background color from .main-window, or fallback to .env-input.
        let bg = self.theme.get_style("main-window", "background-color")
            .or_else(|| self.theme.get_style("env-input", "background-color"))
            .and_then(|s| self.theme.parse_color(&s))
            .unwrap_or(eframe::egui::Color32::BLACK);

        // Apply the main-window dimensions.
        let main_width = self.theme.get_px_value("main-window", "width").map(|w| if w < 0.0 {300.0} else {w}).unwrap_or(300.0);
        let main_height = self.theme.get_px_value("main-window", "height").map(|h| if h < 0.0 {200.0} else {h}).unwrap_or(200.0);
        eframe::egui::Area::new("main-window".into())
            .fixed_pos(eframe::egui::pos2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_min_size(eframe::egui::vec2(main_width, main_height));
                ui.set_max_size(eframe::egui::vec2(main_width, main_height));
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
                        let area = eframe::egui::Area::new(sec.to_owned().into())
                            .order(eframe::egui::Order::Foreground)
                            .fixed_pos(eframe::egui::pos2(x, y));
                        area.show(ctx, |ui| {
                            if sec == "search-bar" || sec == "env-input" {
                                if let (Some(w), Some(h)) = (self.theme.get_px_value(sec, "width"), self.theme.get_px_value(sec, "height")) {
                                    ui.set_min_size(eframe::egui::vec2(w, h));
                                    ui.set_max_size(eframe::egui::vec2(w, h));
                                }
                            }
                            self.render_section(ui, sec, ctx);
                        });
                    } else {
                        self.render_section(ui, sec, ctx);
                    }
                }
            });

        if let Some((ref mut app_name, ref mut opts)) = self.editing {
            // Do not auto-populate if the input becomes completely empty.
            let x = self.theme.get_px_value("env-input", "x")
                .unwrap_or((ctx.input(|i| i.screen_rect().width()) - 300.0) / 2.0);
            let y = self.theme.get_px_value("env-input", "y")
                .unwrap_or((ctx.input(|i| i.screen_rect().height()) - 200.0) / 2.0);
            let env_input_bg = self.theme.get_style("env-input", "background-color")
                .and_then(|s| self.theme.parse_color(&s))
                .unwrap_or(eframe::egui::Color32::TRANSPARENT);
            let mut save = false;
            let mut cancel = false;
            let area = eframe::egui::Area::new("env-input".to_string().into())
                .order(eframe::egui::Order::Foreground)
                .movable(true)
                .default_pos(eframe::egui::pos2(x, y));
            area.show(ctx, |ui| {
                if let (Some(w), Some(h)) = (
                    self.theme.get_px_value("env-input", "width"),
                    self.theme.get_px_value("env-input", "height")
                ) {
                    ui.set_min_size(eframe::egui::vec2(w, h));
                    ui.set_max_size(eframe::egui::vec2(w, h));
                }
                eframe::egui::Frame::NONE.fill(env_input_bg).show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.label(format!("Environment Variables for {}", app_name));
                        ui.add_space(4.0);
                        with_alignment(ui, &self.theme, "env-input", |ui| {
                            self.theme.apply_style(ui, "env-input");
                            ui.add(eframe::egui::TextEdit::singleline(opts)
                                .hint_text("Enter env variables...")
                                .frame(false));
                        });
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            if custom_button(ui, "Save", "edit-button", &self.theme).clicked() {
                                save = true;
                            }
                            if custom_button(ui, "Cancel", "edit-button", &self.theme).clicked() {
                                cancel = true;
                            }
                        });
                    });
                });
            });
            if save {
                let result = format!("LAUNCH_OPTIONS:{}:{}", app_name, opts);
                self.app.handle_input(&result);
                self.editing = None;
            } else if cancel {
                self.editing = None;
            }
        }

        let esc_pressed = ctx.input(|i| i.key_pressed(eframe::egui::Key::Escape));
        let enter_pressed = ctx.input(|i| i.key_pressed(eframe::egui::Key::Enter));
        if esc_pressed && self.editing.is_some() {
            self.editing = None;
        } else if esc_pressed {
            self.app.handle_input("ESC");
        }
        if enter_pressed && self.editing.is_none() {
            self.app.handle_input("ENTER");
        }

        if self.app.should_quit() {
            process::exit(0);
        }
    }
}

pub fn load_theme() -> Result<Theme, Box<dyn Error>> {
    Theme::load_or_create()
}
