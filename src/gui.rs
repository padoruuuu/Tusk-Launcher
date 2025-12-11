use std::{
    collections::HashMap,
    error::Error,
    fs::{read_to_string, create_dir_all, OpenOptions},
    io::Write,
    path::PathBuf,
};
use time::OffsetDateTime;
use eframe;
use serde::{Deserialize, Serialize};
use xdg;
use crate::app_launcher::resolve_icon_path;

const DEFAULT_THEME: &str = r#"
/* Main Window Styles */
.main-window {
    background-color: rgba(0, 0, 0, 0.9);
    width: 200px;
    height: 200px;
    background-image: url("");
    background-size: stretch; /* Options: stretch, fit, fill */
    background-opacity: 1.0;
}

/* Search Bar Styles */
.search-bar {
    position: absolute;
    left: 20px;
    top: 10px;
    width: 150px;
    height: 25px;
    background-color: rgba(59, 66, 82, 1);
    background-color-hover: rgba(76, 86, 106, 1);
    border-radius: 0px;
    color: rgba(236, 239, 244, 1);
    color-hover: rgba(236, 239, 244, 1);
    padding: 0px;
    font-size: 12px;
}

/* App List Container */
.app-list {
    position: absolute;
    left: 2px;
    top: 35px;
    width: 109px;
    height: 108px;
    background-color: rgba(46, 52, 64, 1);
    padding: 0px;
    border-radius: 0px;
}

/* App Button Styles */
.app-button {
    background-color: rgba(122, 162, 247, 1);
    background-color-hover: rgba(102, 138, 196, 1);
    color: rgba(236, 239, 244, 1);
    color-hover: rgba(236, 239, 244, 1);
    border-radius: 0px;
    padding: 0px;
    font-size: 12px;
    order: 2;
}

/* App Icon Styles */
.app-icon {
    width: 18px;
    height: 18px;
    order: 1;
}

/* Settings Button Styles */
.settings-button {
    width: 22px;
    height: 22px;
    color: rgba(122, 162, 247, 1);
    color-hover: rgba(102, 138, 196, 0.5);
    font-size: 16px;
    offset-x: 10px;
    offset-y: -3px;
    order: 0;
}

/* Time Display Styles */
.time-display {
    position: absolute;
    left: 30px;
    top: 160px;
    width: 200px;
    height: 50px;
    background-color: rgba(46, 52, 64, 1);
    color: rgba(236, 239, 244, 1);
    color-hover: rgba(236, 239, 244, 1);
    text-align: center;
}

/* Volume Slider Styles */
.volume-slider {
    position: absolute;
    left: 40px;
    top: 155px;
    width: 200px;
    height: 50px;
    background-color: rgba(46, 52, 64, 1);
    background-color-hover: rgba(67, 76, 94, 1);
    color: rgba(236, 239, 244, 1);
    color-hover: rgba(236, 239, 244, 1);
    border-radius: 4px;
    gap: 5px;
}

/* Power Button Styles */
.power-button {
    position: absolute;
    left: 20px;
    top: 190px;
    width: 65px;
    height: 15px;
    background-color: rgba(122, 162, 247, 1);
    background-color-hover: rgba(102, 138, 196, 1);
    color: rgba(236, 239, 244, 1);
    color-hover: rgba(236, 239, 244, 1);
    border-radius: 0px;
    padding: 0px;
}

/* Edit/Environment Button Styles */
.edit-button {
    background-color: rgba(122, 162, 247, 1);
    background-color-hover: rgba(102, 138, 196, 1);
    color: rgba(236, 239, 244, 1);
    color-hover: rgba(236, 239, 244, 1);
    border-radius: 0px;
    padding: 0px;
    font-size: 12px;
}

/* Environment Variables Input */
.env-input {
    background-color: rgba(59, 66, 82, 1);
    color: rgba(236, 239, 244, 1);
    color-hover: rgba(236, 239, 244, 1);
    padding: 0px;
    font-size: 12px;
    border-radius: 0px;
    width: 150px;
    height: 50px;
    scaling: 1.0;
}

/* Configuration Settings */
.config {
    enable-recent-apps: true;
    max-search-results: 5;
    enable-power-options: true;
    show-time: true;
    time-format: "%I:%M %p";
    time-order: MdyHms; /* Options: MdyHms, YmdHms, DmyHms */
    enable-audio-control: false;
    max-volume: 1.5;
    volume-update-interval-ms: 500;
    power-commands: "systemctl poweroff, loginctl poweroff, poweroff, halt";
    restart-commands: "systemctl reboot, loginctl reboot, reboot";
    logout-commands: "loginctl terminate-session $XDG_SESSION_ID, hyprctl dispatch exit, swaymsg exit, gnome-session-quit --logout --no-prompt, qdbus org.kde.ksmserver /KSMServer logout 0 0 0";
    enable-icons: true;
    show-settings-button: true;
}
"#;

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
    pub show_settings_button: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub enum TimeOrder { MdyHms, YmdHms, DmyHms, }

impl Default for Config {
    fn default() -> Self {
        let icon_cache_dir = xdg::BaseDirectories::new()
            .get_config_home()
            .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string())).join(".config"))
            .join("tusk-launcher/icons");
        
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
            power_commands: vec!["systemctl poweroff".into(), "loginctl poweroff".into(), "poweroff".into(), "halt".into()],
            restart_commands: vec!["systemctl reboot".into(), "loginctl reboot".into(), "reboot".into()],
            logout_commands: vec!["loginctl terminate-session $XDG_SESSION_ID".into(), "hyprctl dispatch exit".into(), "swaymsg exit".into(), "gnome-session-quit --logout --no-prompt".into(), "qdbus org.kde.ksmserver /KSMServer logout 0 0 0".into()],
            enable_icons: true,
            icon_cache_dir,
            show_settings_button: true,
        }
    }
}

pub fn format_datetime(datetime: &OffsetDateTime, config: &Config) -> String {
    use time::macros::format_description;
    
    let date_str = match config.time_order {
        TimeOrder::MdyHms => {
            let format = format_description!("[month]/[day]/[year]");
            datetime.format(&format).unwrap_or_default()
        },
        TimeOrder::YmdHms => {
            let format = format_description!("[year]/[month]/[day]");
            datetime.format(&format).unwrap_or_default()
        },
        TimeOrder::DmyHms => {
            let format = format_description!("[day]/[month]/[year]");
            datetime.format(&format).unwrap_or_default()
        },
    };
    
    let time_str = format_time_with_chrono_format(datetime, &config.time_format);
    format!("{} {}", time_str, date_str)
}

fn format_time_with_chrono_format(dt: &OffsetDateTime, format_str: &str) -> String {
    let result = format_str
        .replace("%I", &format!("{:02}", dt.hour() % 12))
        .replace("%H", &format!("{:02}", dt.hour()))
        .replace("%M", &format!("{:02}", dt.minute()))
        .replace("%S", &format!("{:02}", dt.second()))
        .replace("%p", if dt.hour() < 12 { "AM" } else { "PM" })
        .replace("%P", if dt.hour() < 12 { "am" } else { "pm" });
    result
}

// Simplified CSS storage - much cleaner than before
pub struct Theme {
    styles: HashMap<String, HashMap<String, String>>,
}

impl Clone for Theme {
    fn clone(&self) -> Self {
        Theme { styles: self.styles.clone() }
    }
}

impl Theme {
    pub fn load_or_create() -> Theme {
        match Self::try_load() {
            Ok(theme) => theme,
            Err(e) => {
                eprintln!("Failed to load theme: {}", e);
                Self::parse_css(DEFAULT_THEME)
            }
        }
    }

    fn try_load() -> Result<Theme, Box<dyn Error>> {
        let dirs = xdg::BaseDirectories::new();
        let path = dirs.place_config_file("tusk-launcher/theme.css")?;
        
        if let Some(parent) = path.parent() {
            create_dir_all(parent)?;
        }
        
        if !path.exists() {
            let mut file = OpenOptions::new().write(true).create(true).open(&path)?;
            file.write_all(DEFAULT_THEME.as_bytes())?;
        }
        
        let content = read_to_string(&path)?;
        Ok(Self::parse_css(&content))
    }

    fn parse_css(css: &str) -> Theme {
        let mut styles = HashMap::new();
        
        // Remove comments efficiently
        let mut cleaned = String::with_capacity(css.len());
        let mut chars = css.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '/' && chars.peek() == Some(&'*') {
                chars.next();
                while let Some(c) = chars.next() {
                    if c == '*' && chars.peek() == Some(&'/') {
                        chars.next();
                        break;
                    }
                }
            } else {
                cleaned.push(c);
            }
        }
        
        // Parse rules
        let mut rest = cleaned.as_str();
        while let Some(dot_pos) = rest.find('.') {
            rest = &rest[dot_pos + 1..];
            let class_end = rest.find(|c: char| c.is_whitespace() || c == '{').unwrap_or(rest.len());
            if class_end == 0 { break; }
            
            let class_name = rest[..class_end].trim().to_string();
            
            if let Some(open_brace) = rest.find('{') {
                rest = &rest[open_brace + 1..];
                if let Some(close_brace) = rest.find('}') {
                    let block = &rest[..close_brace];
                    let mut props = HashMap::new();
                    
                    for decl in block.split(';') {
                        let decl = decl.trim();
                        if decl.is_empty() { continue; }
                        
                        if let Some((key, val)) = decl.split_once(':') {
                            let key = key.trim().to_lowercase();
                            let mut val = val.trim().to_string();
                            
                            // Clean quotes
                            if (val.starts_with('"') && val.ends_with('"')) || 
                               (val.starts_with('\'') && val.ends_with('\'')) {
                                val = val[1..val.len()-1].to_string();
                            }
                            
                            // Handle url()
                            if val.starts_with("url(") && val.ends_with(')') {
                                val = val[4..val.len()-1].trim().trim_matches(|c| c == '"' || c == '\'').to_string();
                            }
                            
                            props.insert(key, val);
                        }
                    }
                    
                    styles.insert(class_name, props);
                    rest = &rest[close_brace + 1..];
                } else { break; }
            } else { break; }
        }
        
        Theme { styles }
    }

    fn get(&self, class: &str, prop: &str) -> Option<String> {
        self.styles.get(class)?.get(&prop.to_lowercase()).cloned()
    }

    fn get_combined(&self, classes: &[&str], prop: &str) -> Option<String> {
        classes.iter().find_map(|&c| self.get(c, prop))
    }

    fn parse_color(&self, s: &str) -> Option<eframe::egui::Color32> {
        let s = s.trim().to_lowercase();
        if s == "transparent" { return Some(eframe::egui::Color32::TRANSPARENT); }
        
        if s.starts_with("rgba(") || s.starts_with("rgb(") {
            let is_rgba = s.starts_with("rgba(");
            let prefix = if is_rgba { "rgba(" } else { "rgb(" };
            let inner = s.strip_prefix(prefix)?.strip_suffix(')')?.trim();
            let parts: Vec<&str> = inner.split(',').map(|p| p.trim()).collect();
            
            let (r, g, b, a) = match (is_rgba, parts.len()) {
                (true, 4) | (false, 3) => {
                    let r = parts[0].parse().ok()?;
                    let g = parts[1].parse().ok()?;
                    let b = parts[2].parse().ok()?;
                    let a = if is_rgba { parts[3].parse::<f32>().ok()? } else { 1.0 };
                    (r, g, b, a)
                }
                _ => return None,
            };
            
            return Some(eframe::egui::Color32::from_rgba_unmultiplied(r, g, b, (a * 255.0) as u8));
        }
        
        if s.starts_with('#') {
            let hex = s.trim_start_matches('#');
            match hex.len() {
                3 => {
                    let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()?;
                    let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()?;
                    let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()?;
                    Some(eframe::egui::Color32::from_rgb(r, g, b))
                }
                6 => {
                    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                    Some(eframe::egui::Color32::from_rgb(r, g, b))
                }
                8 => {
                    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                    let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
                    Some(eframe::egui::Color32::from_rgba_unmultiplied(r, g, b, a))
                }
                _ => None,
            }
        } else {
            None
        }
    }

    fn get_px(&self, class: &str, prop: &str) -> Option<f32> {
        self.get(class, prop)?.trim_end_matches("px").parse().ok()
    }

    fn get_order(&self, sec: &str) -> i32 {
        self.get(sec, "order").and_then(|s| s.parse().ok()).unwrap_or(0)
    }

    fn get_position(&self, class: &str) -> Option<(f32, f32)> {
        Some((self.get_px(class, "left")?, self.get_px(class, "top")?))
    }

    pub fn get_config(&self) -> Config {
        let mut config = Config::default();
        
        if let Some(props) = self.styles.get("config") {
            macro_rules! set {
                ($key:expr, $field:ident, $typ:ty) => {
                    if let Some(val) = props.get($key) {
                        if let Ok(parsed) = val.parse::<$typ>() {
                            config.$field = parsed;
                        }
                    }
                };
            }
            
            set!("enable-recent-apps", enable_recent_apps, bool);
            set!("max-search-results", max_search_results, usize);
            set!("enable-power-options", enable_power_options, bool);
            set!("show-time", show_time, bool);
            set!("enable-audio-control", enable_audio_control, bool);
            set!("max-volume", max_volume, f32);
            set!("volume-update-interval-ms", volume_update_interval_ms, u64);
            set!("enable-icons", enable_icons, bool);
            set!("show-settings-button", show_settings_button, bool);
            
            if let Some(val) = props.get("time-format") { config.time_format = val.clone(); }
            if let Some(val) = props.get("time-order") {
                config.time_order = match val.as_str() {
                    "YmdHms" => TimeOrder::YmdHms,
                    "DmyHms" => TimeOrder::DmyHms,
                    _ => TimeOrder::MdyHms,
                };
            }
            
            if let Some(val) = props.get("power-commands") {
                config.power_commands = val.split(',').map(|s| s.trim().to_string()).collect();
            }
            if let Some(val) = props.get("restart-commands") {
                config.restart_commands = val.split(',').map(|s| s.trim().to_string()).collect();
            }
            if let Some(val) = props.get("logout-commands") {
                config.logout_commands = val.split(',').map(|s| s.trim().to_string()).collect();
            }
        }
        
        config
    }

    fn get_frame_props(&self, class: &str, default: eframe::egui::Color32) 
        -> (eframe::egui::Color32, Option<eframe::egui::Color32>, eframe::egui::CornerRadius)
    {
        let base = self.get(class, "background-color").and_then(|s| self.parse_color(&s)).unwrap_or(default);
        let hover = self.get(class, "background-color-hover").and_then(|s| self.parse_color(&s));
        let round = self.get(class, "border-radius")
            .and_then(|s| s.replace("px", "").parse::<f32>().ok())
            .map(|v| eframe::egui::CornerRadius::same(v as u8))
            .unwrap_or_default();
        (base, hover, round)
    }

    pub fn apply_style(&self, ui: &mut eframe::egui::Ui, class: &str) {
        let style = ui.style_mut();
        if let Some(bg) = self.get(class, "background-color").and_then(|s| self.parse_color(&s)) {
            style.visuals.panel_fill = bg;
        }
        if let Some(tc) = self.get(class, "color").and_then(|s| self.parse_color(&s)) {
            style.visuals.override_text_color = Some(tc);
        }
        if let Some(pad) = self.get_px(class, "padding") {
            style.spacing.item_spacing = eframe::egui::vec2(pad, pad);
            style.spacing.window_margin = eframe::egui::Margin::symmetric(pad as i8, pad as i8);
        }
        if let Some(rad) = self.get_px(class, "border-radius") {
            let r = eframe::egui::CornerRadius::same(rad as u8);
            for w in [&mut style.visuals.widgets.noninteractive, &mut style.visuals.widgets.inactive,
                      &mut style.visuals.widgets.hovered, &mut style.visuals.widgets.active] {
                w.corner_radius = r;
            }
        }
        if let Some(sz) = self.get_px(class, "font-size") {
            if let Some(text) = style.text_styles.get_mut(&eframe::egui::TextStyle::Body) {
                text.size = sz;
            }
        }
    }

    pub fn apply_widget_style(&self, style: &mut eframe::egui::Style, class: &str) {
        if let Some(bg) = self.get(class, "background-color").and_then(|s| self.parse_color(&s)) {
            let hover = self.get(class, "background-color-hover").and_then(|s| self.parse_color(&s)).unwrap_or(bg);
            set_widget_bg(style, bg, hover);
        }
        if let Some(tc) = self.get(class, "color").and_then(|s| self.parse_color(&s)) {
            style.visuals.override_text_color = Some(tc);
        }
    }

    pub fn apply_combined_widget_style(&self, style: &mut eframe::egui::Style, classes: &[&str]) {
        let base = self.get_combined(classes, "background-color")
            .and_then(|s| self.parse_color(&s))
            .unwrap_or(eframe::egui::Color32::TRANSPARENT);
        let hover = self.get_combined(classes, "background-color-hover")
            .and_then(|s| self.parse_color(&s))
            .unwrap_or(base);
        set_widget_bg(style, base, hover);
        if let Some(tc) = self.get_combined(classes, "color").and_then(|s| self.parse_color(&s)) {
            style.visuals.override_text_color = Some(tc);
        }
    }

    fn get_text_color(&self, class: &str, hovered: bool) -> Option<eframe::egui::Color32> {
        if hovered {
            self.get(class, "color-hover").and_then(|s| self.parse_color(&s))
                .or_else(|| self.get(class, "color").and_then(|s| self.parse_color(&s)))
        } else {
            self.get(class, "color").and_then(|s| self.parse_color(&s))
        }
    }
}

fn set_widget_bg(style: &mut eframe::egui::Style, base: eframe::egui::Color32, hover: eframe::egui::Color32) {
    let t = eframe::egui::Color32::TRANSPARENT;
    let w = &mut style.visuals.widgets;
    w.inactive.bg_fill = base; w.hovered.bg_fill = hover; w.active.bg_fill = base;
    w.inactive.weak_bg_fill = base; w.hovered.weak_bg_fill = hover; w.active.weak_bg_fill = base;
    w.inactive.bg_stroke = eframe::egui::Stroke::new(0.0, t);
    w.hovered.bg_stroke = eframe::egui::Stroke::new(0.0, t);
    w.active.bg_stroke = eframe::egui::Stroke::new(0.0, t);
    w.hovered.expansion = 0.0; w.active.expansion = 0.0;
}

fn custom_button(ui: &mut eframe::egui::Ui, label: &str, class: &str, theme: &Theme) -> eframe::egui::Response {
    let font_id = ui.style().text_styles.get(&eframe::egui::TextStyle::Button).cloned().unwrap_or_default();
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font_id.clone(), eframe::egui::Color32::WHITE);
    let size = galley.size() + ui.spacing().button_padding * 2.0;
    let (rect, _) = ui.allocate_exact_size(size, eframe::egui::Sense::hover());
    let resp = ui.interact(rect, ui.id().with(label), eframe::egui::Sense::click());
    
    if ui.is_rect_visible(rect) {
        let (base, hover_opt, round) = theme.get_frame_props(class, ui.style().visuals.widgets.inactive.bg_fill);
        let normal_tc = theme.get(class, "color").and_then(|s| theme.parse_color(&s)).unwrap_or(eframe::egui::Color32::WHITE);
        let hover_tc = theme.get(class, "color-hover").and_then(|s| theme.parse_color(&s)).unwrap_or(normal_tc);
        
        let bg = if resp.hovered() { hover_opt.unwrap_or(base) } else { base };
        let tc = if resp.hovered() { hover_tc } else { normal_tc };
        
        ui.painter().rect_filled(rect, round, bg);
        ui.painter().text(rect.center(), eframe::egui::Align2::CENTER_CENTER, label, font_id, tc);
    }
    resp
}

fn with_custom_style<R>(ui: &mut eframe::egui::Ui, f: impl FnOnce(&mut eframe::egui::Style), g: impl FnOnce(&mut eframe::egui::Ui) -> R) -> R {
    let old = ui.style().clone();
    f(ui.style_mut());
    let res = g(ui);
    *ui.style_mut() = (*old).clone();
    res
}

fn with_alignment<R>(ui: &mut eframe::egui::Ui, theme: &Theme, sec: &str, f: impl FnOnce(&mut eframe::egui::Ui) -> R) -> R {
    if let Some(align) = theme.get(sec, "text-align") {
        let layout = match align.as_str() {
            "center" => eframe::egui::Layout::centered_and_justified(eframe::egui::Direction::LeftToRight),
            "left" => eframe::egui::Layout::left_to_right(eframe::egui::Align::Min),
            "right" => eframe::egui::Layout::right_to_left(eframe::egui::Align::Max),
            _ => eframe::egui::Layout::default(),
        };
        ui.with_layout(layout, f).inner
    } else { f(ui) }
}

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
        let theme = Theme::load_or_create();
        let cfg = theme.get_config();
        let (w, h) = (theme.get_px("main-window", "width").unwrap_or(300.0),
                      theme.get_px("main-window", "height").unwrap_or(200.0));
        
        let viewport = eframe::egui::ViewportBuilder::default()
            .with_inner_size([w, h])
            .with_always_on_top()
            .with_decorations(false)
            .with_resizable(false)
            .with_active(true)
            .with_transparent(true);
        
        let audio = crate::audio::AudioController::new(&cfg)?;
        audio.start_polling(&cfg);
        
        eframe::run_native("Application Launcher", eframe::NativeOptions { viewport, ..Default::default() },
            Box::new(move |cc| {
                if let Some(s) = theme.get("env-input", "scaling").and_then(|s| s.parse::<f32>().ok()) {
                    cc.egui_ctx.set_pixels_per_point(s);
                }
                cc.egui_ctx.request_repaint();
                Ok(Box::new(EframeWrapper {
                    app, audio_controller: audio, current_volume: 0.0,
                    editing_windows: HashMap::new(), focused: false,
                    icon_manager: crate::app_launcher::IconManager::new(),
                    theme, config: cfg,
                }))
            }))?;
        Ok(())
    }
}

struct EframeWrapper {
    app: Box<dyn AppInterface>,
    audio_controller: crate::audio::AudioController,
    current_volume: f32,
    editing_windows: HashMap<String, String>,
    focused: bool,
    icon_manager: crate::app_launcher::IconManager,
    theme: Theme,
    config: Config,
}

impl EframeWrapper {
    fn render_search_bar(&mut self, ui: &mut eframe::egui::Ui) {
        with_alignment(ui, &self.theme, "search-bar", |ui| {
            self.theme.apply_style(ui, "search-bar");
            let (base, hover, round) = self.theme.get_frame_props("search-bar", ui.visuals().panel_fill);
            let rect = ui.available_rect_before_wrap();
            let resp = ui.interact(rect, ui.id().with("search-bar"), eframe::egui::Sense::hover());
            let fill = if resp.hovered() { hover.unwrap_or(base) } else { base };
            eframe::egui::Frame::NONE.fill(fill).corner_radius(round).show(ui, |ui| {
                with_custom_style(ui, |s| {
                    if let Some(tc) = self.theme.get_text_color("search-bar", resp.hovered()) {
                        s.visuals.override_text_color = Some(tc);
                    }
                }, |ui| {
                    let mut query = self.app.get_query();
                    let r = ui.add(eframe::egui::TextEdit::singleline(&mut query).hint_text("Search...").frame(false));
                    if !self.focused { r.request_focus(); self.focused = true; }
                    if r.changed() && !query.starts_with("LAUNCH_OPTIONS:") { self.app.handle_input(&query); }
                })
            });
        });
    }

    fn render_volume_slider(&mut self, ui: &mut eframe::egui::Ui) {
        with_alignment(ui, &self.theme, "volume-slider", |ui| {
            self.theme.apply_style(ui, "volume-slider");
            ui.horizontal(|ui| {
                if let Some(gap) = self.theme.get_px("volume-slider", "gap") { ui.spacing_mut().item_spacing.x = gap; }
                ui.label("Volume:");
                let (base, hover, round) = self.theme.get_frame_props("volume-slider", ui.style().visuals.widgets.inactive.bg_fill);
                let vis = { let mut s = ui.style().visuals.widgets.inactive.clone(); s.bg_fill = base; s.corner_radius = round; s };
                with_custom_style(ui, |s| {
                    s.visuals.widgets.inactive = vis.clone();
                    s.visuals.widgets.hovered.bg_fill = hover.unwrap_or(base);
                    s.visuals.widgets.hovered.weak_bg_fill = hover.unwrap_or(base);
                    s.visuals.widgets.active = vis;
                    let t = eframe::egui::Color32::TRANSPARENT;
                    s.visuals.widgets.inactive.bg_stroke = eframe::egui::Stroke::new(0.0, t);
                    s.visuals.widgets.hovered.bg_stroke = eframe::egui::Stroke::new(0.0, t);
                    s.visuals.widgets.active.bg_stroke = eframe::egui::Stroke::new(0.0, t);
                    s.visuals.widgets.hovered.expansion = 0.0;
                    s.visuals.widgets.active.expansion = 0.0;
                }, |ui| {
                    let slider = eframe::egui::Slider::new(&mut self.current_volume, 0.0..=self.config.max_volume)
                        .custom_formatter(|n, _| format!("{:.0}%", n * 100.0))
                        .custom_parser(|s| s.trim().trim_end_matches('%').parse::<f64>().ok().map(|n| n / 100.0));
                    if ui.add(slider).changed() { let _ = self.audio_controller.set_volume(self.current_volume); }
                });
            });
        });
    }

    fn render_app_list(&mut self, ui: &mut eframe::egui::Ui, ctx: &eframe::egui::Context) {
        self.theme.apply_style(ui, "app-list");
        let query = self.app.get_query();
        let filtered = if query.trim().is_empty() {
            if self.config.enable_recent_apps { 
                self.app.get_search_results().into_iter().take(self.config.max_search_results).collect() 
            } else { Vec::new() }
        } else { 
            self.app.get_search_results().into_iter().take(self.config.max_search_results).collect() 
        };
        
        ui.vertical(|ui| {
            for app_name in filtered {
                let row_id = ui.id().with(&app_name);
                ui.horizontal(|ui| {
                    let mut elems = vec![
                        (self.theme.get("settings-button", "order").and_then(|s| s.parse().ok()).unwrap_or(0), "settings"),
                        (self.theme.get("app-icon", "order").and_then(|s| s.parse().ok()).unwrap_or(1), "icon"),
                        (self.theme.get("app-button", "order").and_then(|s| s.parse().ok()).unwrap_or(2), "app"),
                    ];
                    elems.sort_by_key(|(o, _)| *o);
                    
                    for (_, elem) in elems {
                        match elem {
                            "settings" if self.config.show_settings_button => {
                                let w = self.theme.get_px("settings-button", "width").unwrap_or(22.0);
                                let h = self.theme.get_px("settings-button", "height").unwrap_or(22.0);
                                let x_off = self.theme.get_px("settings-button", "offset-x").unwrap_or(0.0);
                                let y_off = self.theme.get_px("settings-button", "offset-y").unwrap_or(0.0);
                                let (base_rect, _) = ui.allocate_exact_size(eframe::egui::vec2(w, h), eframe::egui::Sense::hover());
                                let rect = base_rect.translate(eframe::egui::vec2(x_off, y_off));
                                let resp = ui.interact(rect, row_id.with("settings-button"), eframe::egui::Sense::click());
                                self.theme.apply_style(ui, "settings-button");
                                let color = self.theme.get_text_color("settings-button", resp.hovered())
                                    .unwrap_or(eframe::egui::Color32::from_rgb(64, 64, 64));
                                let font = eframe::egui::TextStyle::Button.resolve(ui.style());
                                ui.painter().text(rect.center(), eframe::egui::Align2::CENTER_CENTER, "⚙", font, color);
                                if resp.clicked() {
                                    self.editing_windows.insert(app_name.clone(), self.app.get_formatted_launch_options(&app_name));
                                }
                            },
                            "icon" if self.config.enable_icons => {
                                if let Some(icon_path) = self.app.get_icon_path(&app_name) {
                                    let w = self.theme.get_px("app-icon", "width").unwrap_or(22.0);
                                    let h = self.theme.get_px("app-icon", "height").unwrap_or(22.0);
                                    let (rect, _) = ui.allocate_exact_size(eframe::egui::vec2(w, h), eframe::egui::Sense::hover());
                                    if let Some(tex) = self.icon_manager.get_texture(ctx, &icon_path) {
                                        ui.painter().image(tex.id(), rect, 
                                            eframe::egui::Rect::from_min_max(eframe::egui::Pos2::ZERO, eframe::egui::Pos2::new(1.0, 1.0)),
                                            eframe::egui::Color32::WHITE);
                                    }
                                }
                            },
                            "app" => {
                                with_custom_style(ui, |s| { self.theme.apply_combined_widget_style(s, &["app-button"]); }, |ui| {
                                    let font_id = ui.style().text_styles.get(&eframe::egui::TextStyle::Button).cloned().unwrap_or_default();
                                    let galley = ui.painter().layout_no_wrap(app_name.clone(), font_id.clone(), eframe::egui::Color32::WHITE);
                                    let size = galley.size() + ui.spacing().button_padding * 2.0;
                                    let (rect, _) = ui.allocate_exact_size(size, eframe::egui::Sense::hover());
                                    let resp = ui.interact(rect, ui.id().with(&app_name), eframe::egui::Sense::click().union(eframe::egui::Sense::click_and_drag()));
                                    
                                    if ui.is_rect_visible(rect) {
                                        let (base, hover_opt, round) = self.theme.get_frame_props("app-button", ui.style().visuals.widgets.inactive.bg_fill);
                                        let normal_tc = self.theme.get("app-button", "color").and_then(|s| self.theme.parse_color(&s)).unwrap_or(eframe::egui::Color32::WHITE);
                                        let hover_tc = self.theme.get("app-button", "color-hover").and_then(|s| self.theme.parse_color(&s)).unwrap_or(normal_tc);
                                        let bg = if resp.hovered() { hover_opt.unwrap_or(base) } else { base };
                                        let tc = if resp.hovered() { hover_tc } else { normal_tc };
                                        
                                        ui.painter().rect_filled(rect, round, bg);
                                        ui.painter().text(rect.center(), eframe::egui::Align2::CENTER_CENTER, &app_name, font_id, tc);
                                    }
                                    
                                    if resp.clicked() { self.app.launch_app(&app_name); }
                                    if resp.secondary_clicked() {
                                        self.editing_windows.insert(app_name.clone(), self.app.get_formatted_launch_options(&app_name));
                                    }
                                });
                            },
                            _ => {},
                        }
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
            with_custom_style(ui, |s| { self.theme.apply_widget_style(s, "power-button"); }, |ui| {
                ui.horizontal(|ui| {
                    for &(lbl, cmd) in &[("Power", "P"), ("Restart", "R"), ("Logout", "L")] {
                        if custom_button(ui, lbl, "power-button", &self.theme).clicked() {
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
        if self.config.enable_audio_control && self.audio_controller.update_volume().is_ok() {
            self.current_volume = self.audio_controller.get_volume();
        }
        
        let w = self.theme.get_px("main-window", "width").unwrap_or(300.0);
        let h = self.theme.get_px("main-window", "height").unwrap_or(200.0);
        let bg = self.theme.get("main-window", "background-color").and_then(|s| self.theme.parse_color(&s)).unwrap_or(eframe::egui::Color32::BLACK);
        
        eframe::egui::Area::new("main".into()).fixed_pos(eframe::egui::pos2(0.0, 0.0)).show(ctx, |ui| {
            ui.set_min_size(eframe::egui::vec2(w, h));
            ui.set_max_size(eframe::egui::vec2(w, h));
            let rect = eframe::egui::Rect::from_min_size(eframe::egui::pos2(0.0, 0.0), eframe::egui::vec2(w, h));
            
            if let Some(img) = self.theme.get("main-window", "background-image").filter(|s| !s.is_empty()) {
                if let Some(path) = resolve_icon_path("main-window", &img, &self.config) {
                    if let Some(tex) = self.icon_manager.get_texture(ctx, &path) {
                        let size_mode = self.theme.get("main-window", "background-size").unwrap_or("stretch".to_string());
                        let img_size = tex.size_vec2();
                        let (draw_rect, uv) = match size_mode.as_str() {
                            "fit" => {
                                let scale = (rect.width() / img_size.x).min(rect.height() / img_size.y);
                                let new_size = img_size * scale;
                                let offset = (rect.size() - new_size) * 0.5;
                                (eframe::egui::Rect::from_min_size(rect.min + offset, new_size),
                                 eframe::egui::Rect::from_min_max(eframe::egui::Pos2::ZERO, eframe::egui::Pos2::new(1.0, 1.0)))
                            },
                            "fill" => {
                                let scale = (rect.width() / img_size.x).max(rect.height() / img_size.y);
                                let new_size = img_size * scale;
                                let offset = (new_size - rect.size()) * 0.5;
                                let uv_min = eframe::egui::Pos2::new(offset.x / new_size.x, offset.y / new_size.y);
                                let uv_max = eframe::egui::Pos2::new(1.0 - offset.x / new_size.x, 1.0 - offset.y / new_size.y);
                                (rect, eframe::egui::Rect::from_min_max(uv_min, uv_max))
                            },
                            _ => (rect, eframe::egui::Rect::from_min_max(eframe::egui::Pos2::ZERO, eframe::egui::Pos2::new(1.0, 1.0))),
                        };
                        let opacity = self.theme.get("main-window", "background-opacity").and_then(|s| s.parse::<f32>().ok()).unwrap_or(1.0);
                        let tint = eframe::egui::Color32::from_white_alpha((opacity * 255.0) as u8);
                        ui.painter().image(tex.id(), draw_rect, uv, tint);
                    } else {
                        ui.painter().rect_filled(rect, 0.0, bg);
                    }
                } else {
                    ui.painter().rect_filled(rect, 0.0, bg);
                }
            } else {
                ui.painter().rect_filled(rect, 0.0, bg);
            }
            
            let mut secs = vec!["search-bar", "app-list"];
            if self.config.enable_audio_control { secs.push("volume-slider"); }
            if self.config.show_time { secs.push("time-display"); }
            if self.config.enable_power_options { secs.push("power-button"); }
            secs.sort_by_key(|s| self.theme.get_order(s));
            
            for sec in secs {
                let area = if let Some((x, y)) = self.theme.get_position(sec) {
                    eframe::egui::Area::new(sec.to_owned().into()).order(eframe::egui::Order::Foreground).fixed_pos(eframe::egui::pos2(x, y))
                } else {
                    eframe::egui::Area::new(sec.to_owned().into()).order(eframe::egui::Order::Foreground)
                };
                
                area.show(ctx, |ui| {
                    if matches!(sec, "search-bar" | "env-input" | "app-list") {
                        if let (Some(w), Some(h)) = (self.theme.get_px(sec, "width"), self.theme.get_px(sec, "height")) {
                            ui.set_min_size(eframe::egui::vec2(w, h));
                            ui.set_max_size(eframe::egui::vec2(w, h));
                        }
                    }
                    self.render_section(ui, sec, ctx);
                });
            }
        });
        
        let esc = ctx.input(|i| i.key_pressed(eframe::egui::Key::Escape));
        let mut to_remove = Vec::new();
        
        for (app_name, opts) in self.editing_windows.iter() {
            let env_bg = self.theme.get("env-input", "background-color").and_then(|s| self.theme.parse_color(&s)).unwrap_or(eframe::egui::Color32::from_rgba_unmultiplied(59, 66, 82, 255));
            let w = self.theme.get_px("env-input", "width").unwrap_or(300.0);
            let h = self.theme.get_px("env-input", "height").unwrap_or(150.0);
            
            let app_clone = app_name.clone();
            let opts_clone = opts.clone();
            let theme_clone = self.theme.clone();
            
            let viewport_id = eframe::egui::ViewportId::from_hash_of(format!("env_{}", app_name));
            let viewport = eframe::egui::ViewportBuilder::default()
                .with_title(format!("Environment Variables - {}", app_name))
                .with_inner_size([w, h])
                .with_resizable(false)
                .with_always_on_top();
            
            let mem_key = format!("env_opts_{}", app_name);
            let current_opts = ctx.data_mut(|d| d.get_persisted::<String>(eframe::egui::Id::new(&mem_key)).unwrap_or_else(|| opts_clone.clone()));
            
            if current_opts != opts_clone {
                let stored_app_key = format!("env_app_{}", app_name);
                let stored_app = ctx.data_mut(|d| d.get_persisted::<String>(eframe::egui::Id::new(&stored_app_key)));
                
                if stored_app.as_ref() != Some(&app_clone) {
                    ctx.data_mut(|d| {
                        d.insert_persisted(eframe::egui::Id::new(&mem_key), opts_clone.clone());
                        d.insert_persisted(eframe::egui::Id::new(&stored_app_key), app_clone.clone());
                    });
                }
            }
            
            let action_key = format!("env_action_{}", app_name);
            
            ctx.show_viewport_immediate(viewport_id, viewport, move |ctx, _| {
                let mem_key = format!("env_opts_{}", app_clone);
                let action_key = format!("env_action_{}", app_clone);
                
                let mut opts = ctx.data_mut(|d| d.get_persisted::<String>(eframe::egui::Id::new(&mem_key)).unwrap_or_else(|| opts_clone.clone()));
                
                eframe::egui::CentralPanel::default().frame(eframe::egui::Frame::NONE.fill(env_bg)).show(ctx, |ui| {
                    ui.vertical(|ui| {
                        ui.label(format!("Environment Variables for {}", app_clone));
                        ui.add_space(4.0);
                        with_alignment(ui, &theme_clone, "env-input", |ui| {
                            theme_clone.apply_style(ui, "env-input");
                            ui.add(eframe::egui::TextEdit::singleline(&mut opts).hint_text("Enter env variables...").desired_width(f32::INFINITY));
                        });
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            if custom_button(ui, "Save", "edit-button", &theme_clone).clicked() {
                                ctx.data_mut(|d| d.insert_temp(eframe::egui::Id::new(&action_key), "save".to_string()));
                            }
                            if custom_button(ui, "Cancel", "edit-button", &theme_clone).clicked() {
                                ctx.data_mut(|d| d.insert_temp(eframe::egui::Id::new(&action_key), "cancel".to_string()));
                            }
                        });
                    });
                    
                    if ctx.input(|i| i.key_pressed(eframe::egui::Key::Escape)) {
                        ctx.data_mut(|d| d.insert_temp(eframe::egui::Id::new(&action_key), "cancel".to_string()));
                    }
                });
                
                ctx.data_mut(|d| d.insert_persisted(eframe::egui::Id::new(&mem_key), opts.clone()));
            });
            
            if let Some(action) = ctx.data_mut(|d| d.get_temp::<String>(eframe::egui::Id::new(&action_key))) {
                if action == "save" {
                    let final_opts = ctx.data_mut(|d| d.get_persisted::<String>(eframe::egui::Id::new(&mem_key)).unwrap_or_else(|| opts.clone()));
                    self.app.handle_input(&format!("LAUNCH_OPTIONS:{}:{}", app_name, final_opts));
                    to_remove.push(app_name.clone());
                    ctx.data_mut(|d| {
                        d.remove::<String>(eframe::egui::Id::new(&mem_key));
                        d.remove::<String>(eframe::egui::Id::new(&action_key));
                        d.remove::<String>(eframe::egui::Id::new(&format!("env_app_{}", app_name)));
                    });
                    ctx.send_viewport_cmd_to(viewport_id, eframe::egui::ViewportCommand::Close);
                } else if action == "cancel" {
                    to_remove.push(app_name.clone());
                    ctx.data_mut(|d| {
                        d.remove::<String>(eframe::egui::Id::new(&mem_key));
                        d.remove::<String>(eframe::egui::Id::new(&action_key));
                        d.remove::<String>(eframe::egui::Id::new(&format!("env_app_{}", app_name)));
                    });
                    ctx.send_viewport_cmd_to(viewport_id, eframe::egui::ViewportCommand::Close);
                }
            }
        }
        
        for app_name in to_remove { self.editing_windows.remove(&app_name); }
        
        if esc && self.editing_windows.is_empty() { self.app.handle_input("ESC"); }
        if ctx.input(|i| i.key_pressed(eframe::egui::Key::Enter)) && self.editing_windows.is_empty() {
            self.app.handle_input("ENTER");
        }
        if self.app.should_quit() { ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Close); }
    }
}

pub fn load_theme() -> Theme { Theme::load_or_create() }