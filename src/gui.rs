use std::{
    collections::HashMap,
    error::Error,
    fs::{read_to_string, create_dir_all, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use time::OffsetDateTime;
use eframe;
use serde::{Deserialize, Serialize};
use xdg;
use crate::app_launcher::resolve_icon_path;

const DEFAULT_THEME: &str = r#"
/* ═══════════════════════════════════════════════════════
   Tusk Launcher — Default Theme
   Palette:
     bg-base    rgba(12,  12,  18,  0.96)  near-black
     bg-surface rgba(24,  24,  36,  1)     card surface
     bg-raised  rgba(36,  36,  52,  1)     elevated / inputs
     bg-hover   rgba(52,  52,  74,  1)     hover state
     accent     rgba(110, 90,  220, 1)     soft violet
     accent-hi  rgba(135, 115, 245, 1)     accent hover
     text       rgba(218, 216, 232, 1)     primary text
     text-dim   rgba(120, 118, 140, 1)     secondary text
     green      rgba(72,  210, 140, 1)     status indicator
   ═══════════════════════════════════════════════════════ */

/* Main Window
 * Layout (px):
 *   search-bar  top:10  h:26  → ends:36
 *   app-list    top:40  h:130 → ends:170
 *   tray-icon   top:174 h:18  → ends:192
 *   time/vol    top:196 h:16  → ends:212
 *   power       top:216 h:20  → ends:236  */
.main-window {
    background-color: rgba(12, 12, 18, 0.96);
    width: 220px;
    height: 242px;
    background-image: url("");
    background-size: stretch;
    background-opacity: 1.0;
}

/* Search Bar */
.search-bar {
    position: absolute;
    left: 12px;
    top: 10px;
    width: 196px;
    height: 26px;
    background-color: rgba(36, 36, 52, 1);
    background-color-hover: rgba(48, 48, 68, 1);
    border-radius: 8px;
    color: rgba(218, 216, 232, 1);
    color-hover: rgba(218, 216, 232, 1);
    padding: 0px;
    font-size: 12px;
}

/* App List Container */
.app-list {
    position: absolute;
    left: 12px;
    top: 40px;
    width: 196px;
    height: 130px;
    background-color: rgba(0, 0, 0, 0);
    padding: 0px;
    border-radius: 0px;
}

/* App Button */
.app-button {
    background-color: rgba(36, 36, 52, 1);
    background-color-hover: rgba(52, 52, 74, 1);
    color: rgba(218, 216, 232, 1);
    color-hover: rgba(235, 233, 250, 1);
    border-radius: 6px;
    padding: 0px;
    font-size: 12px;
    order: 2;
}

/* App Icon */
.app-icon {
    width: 16px;
    height: 16px;
    order: 1;
}

/* Settings Gear */
.settings-button {
    width: 20px;
    height: 20px;
    color: rgba(120, 118, 140, 1);
    color-hover: rgba(135, 115, 245, 1);
    font-size: 14px;
    offset-x: 10px;
    offset-y: -2px;
    order: 0;
}

/* System Tray Strip */
.tray-icon {
    position: absolute;
    left: 12px;
    top: 174px;
    width: 196px;
    height: 18px;
    background-color: rgba(0, 0, 0, 0);
    color: rgba(218, 216, 232, 1);
    indicator-color: rgba(72, 210, 140, 1);
    font-size: 10px;
    border-radius: 0px;
    text-align: left;
}

/* Clock */
.time-display {
    position: absolute;
    left: 12px;
    top: 196px;
    width: 196px;
    height: 16px;
    background-color: rgba(0, 0, 0, 0);
    color: rgba(120, 118, 140, 1);
    color-hover: rgba(120, 118, 140, 1);
    text-align: center;
}

/* Volume Slider */
.volume-slider {
    position: absolute;
    left: 12px;
    top: 196px;
    width: 196px;
    height: 16px;
    background-color: rgba(36, 36, 52, 1);
    background-color-hover: rgba(52, 52, 74, 1);
    color: rgba(218, 216, 232, 1);
    color-hover: rgba(218, 216, 232, 1);
    border-radius: 6px;
    gap: 5px;
}

/* Power / Restart / Logout Buttons */
.power-button {
    position: absolute;
    left: 12px;
    top: 216px;
    width: 196px;
    height: 20px;
    background-color: rgba(36, 36, 52, 1);
    background-color-hover: rgba(52, 52, 74, 1);
    color: rgba(218, 216, 232, 1);
    color-hover: rgba(235, 233, 250, 1);
    border-radius: 6px;
    padding: 0px;
}

/* Edit / Save / Cancel (env-vars popup) */
.edit-button {
    background-color: rgba(110, 90, 220, 1);
    background-color-hover: rgba(135, 115, 245, 1);
    color: rgba(218, 216, 232, 1);
    color-hover: rgba(255, 255, 255, 1);
    border-radius: 6px;
    padding: 0px;
    font-size: 12px;
}

/* Env-Vars Input Field */
.env-input {
    background-color: rgba(36, 36, 52, 1);
    color: rgba(218, 216, 232, 1);
    color-hover: rgba(218, 216, 232, 1);
    padding: 0px;
    font-size: 12px;
    border-radius: 6px;
    width: 200px;
    height: 60px;
    scaling: 1.0;
}

/* Configuration */
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
    enable-system-tray: true;
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
    pub enable_system_tray: bool,
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
            enable_system_tray: false,
        }
    }
}

pub fn format_datetime(datetime: &OffsetDateTime, config: &Config) -> String {
    use time::macros::format_description;

    let date_str = match config.time_order {
        TimeOrder::MdyHms => {
            let format = format_description!("[month]/[day]/[year]");
            datetime.format(&format).unwrap_or_default()
        }
        TimeOrder::YmdHms => {
            let format = format_description!("[year]/[month]/[day]");
            datetime.format(&format).unwrap_or_default()
        }
        TimeOrder::DmyHms => {
            let format = format_description!("[day]/[month]/[year]");
            datetime.format(&format).unwrap_or_default()
        }
    };

    let time_str = format_time_with_chrono_format(datetime, &config.time_format);
    format!("{} {}", time_str, date_str)
}

fn format_time_with_chrono_format(dt: &OffsetDateTime, format_str: &str) -> String {
    format_str
        .replace("%I", &format!("{:02}", dt.hour() % 12))
        .replace("%H", &format!("{:02}", dt.hour()))
        .replace("%M", &format!("{:02}", dt.minute()))
        .replace("%S", &format!("{:02}", dt.second()))
        .replace("%p", if dt.hour() < 12 { "AM" } else { "PM" })
        .replace("%P", if dt.hour() < 12 { "am" } else { "pm" })
}

// ============================================================================
// Theme
// ============================================================================

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

        let mut content = read_to_string(&path)?;

        // Migration: overwrite with the current default theme whenever the on-disk file
        // matches any known old default (detected by sentinel values unique to each old version).
        let is_old_default =
            // v1: 200px wide, 0px radius, Nord blue accent
            (content.contains("width: 200px;") && content.contains("rgba(122, 162, 247, 1)"))
            // v2: new palette but old power-button hover (violet accent on power)
            || (content.contains("rgba(110, 90, 220, 1)") && content.contains(".power-button"));
        if is_old_default {
            content = DEFAULT_THEME.to_string();
            if let Ok(mut f) = OpenOptions::new().write(true).truncate(true).open(&path) {
                let _ = f.write_all(content.as_bytes());
            }
        } else {
            // Targeted migration: replace opaque tray-icon background with transparent.
            let old_tray_bg = "background-color: rgba(46, 52, 64, 1);";
            if content.contains(".tray-icon") && content.contains(old_tray_bg) {
                if let Some(tray_pos) = content.find(".tray-icon") {
                    if let Some(rel) = content[tray_pos..].find(old_tray_bg) {
                        let abs = tray_pos + rel;
                        content.replace_range(abs..abs + old_tray_bg.len(),
                            "background-color: rgba(0, 0, 0, 0);");
                        if let Ok(mut f) = OpenOptions::new().write(true).truncate(true).open(&path) {
                            let _ = f.write_all(content.as_bytes());
                        }
                    }
                }
            }
        }

        Ok(Self::parse_css(&content))
    }

    fn parse_css(css: &str) -> Theme {
        let mut styles = HashMap::new();

        // Remove comments
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

                            if (val.starts_with('"') && val.ends_with('"')) ||
                               (val.starts_with('\'') && val.ends_with('\'')) {
                                val = val[1..val.len()-1].to_string();
                            }

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

            set!("enable-recent-apps",        enable_recent_apps,        bool);
            set!("max-search-results",         max_search_results,        usize);
            set!("enable-power-options",       enable_power_options,      bool);
            set!("show-time",                  show_time,                 bool);
            set!("enable-audio-control",       enable_audio_control,      bool);
            set!("max-volume",                 max_volume,                f32);
            set!("volume-update-interval-ms",  volume_update_interval_ms, u64);
            set!("enable-icons",               enable_icons,              bool);
            set!("show-settings-button",       show_settings_button,      bool);
            set!("enable-system-tray",         enable_system_tray,        bool);

            if let Some(val) = props.get("time-format") { config.time_format = val.clone(); }
            if let Some(val) = props.get("time-order") {
                config.time_order = match val.as_str() {
                    "YmdHms" => TimeOrder::YmdHms,
                    "DmyHms" => TimeOrder::DmyHms,
                    _        => TimeOrder::MdyHms,
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
        let base  = self.get(class, "background-color").and_then(|s| self.parse_color(&s)).unwrap_or(default);
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
            // Don't set panel_fill when transparent — egui would paint a solid
            // background rect for the container even if we skip our own rect_filled.
            if bg != eframe::egui::Color32::TRANSPARENT {
                style.visuals.panel_fill = bg;
            }
        }
        if let Some(tc) = self.get(class, "color").and_then(|s| self.parse_color(&s)) {
            style.visuals.override_text_color = Some(tc);
        }
        if let Some(pad) = self.get_px(class, "padding") {
            style.spacing.item_spacing       = eframe::egui::vec2(pad, pad);
            style.spacing.window_margin      = eframe::egui::Margin::symmetric(pad as i8, pad as i8);
        }
        if let Some(rad) = self.get_px(class, "border-radius") {
            let r = eframe::egui::CornerRadius::same(rad as u8);
            for w in [&mut style.visuals.widgets.noninteractive, &mut style.visuals.widgets.inactive,
                      &mut style.visuals.widgets.hovered,        &mut style.visuals.widgets.active] {
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
    w.inactive.bg_fill = base;  w.hovered.bg_fill = hover;  w.active.bg_fill = base;
    w.inactive.weak_bg_fill = base; w.hovered.weak_bg_fill = hover; w.active.weak_bg_fill = base;
    w.inactive.bg_stroke = eframe::egui::Stroke::new(0.0, t);
    w.hovered.bg_stroke  = eframe::egui::Stroke::new(0.0, t);
    w.active.bg_stroke   = eframe::egui::Stroke::new(0.0, t);
    w.hovered.expansion = 0.0;
    w.active.expansion  = 0.0;
}

fn custom_button(ui: &mut eframe::egui::Ui, label: &str, class: &str, theme: &Theme) -> eframe::egui::Response {
    custom_button_width(ui, label, class, theme, None)
}

fn custom_button_width(ui: &mut eframe::egui::Ui, label: &str, class: &str, theme: &Theme, min_width: Option<f32>) -> eframe::egui::Response {
    let font_id = ui.style().text_styles.get(&eframe::egui::TextStyle::Button).cloned().unwrap_or_default();
    let galley  = ui.painter().layout_no_wrap(label.to_owned(), font_id.clone(), eframe::egui::Color32::WHITE);
    let text_size = galley.size() + ui.spacing().button_padding * 2.0;
    let w = match min_width {
        Some(mw) => text_size.x.max(mw),
        None     => text_size.x,
    };
    let size = eframe::egui::vec2(w, text_size.y);
    // Allocate with click+hover sense directly — splitting into allocate(hover) + interact(click)
    // causes egui to consume the hover event in the discarded first response, breaking hover.
    let (rect, resp) = ui.allocate_exact_size(size, eframe::egui::Sense::click_and_drag());

    if ui.is_rect_visible(rect) {
        let (base, hover_opt, round) = theme.get_frame_props(class, ui.style().visuals.widgets.inactive.bg_fill);
        let normal_tc = theme.get(class, "color").and_then(|s| theme.parse_color(&s)).unwrap_or(eframe::egui::Color32::WHITE);
        let hover_tc  = theme.get(class, "color-hover").and_then(|s| theme.parse_color(&s)).unwrap_or(normal_tc);

        let bg = if resp.hovered() { hover_opt.unwrap_or(base) } else { base };
        let tc = if resp.hovered() { hover_tc } else { normal_tc };

        ui.painter().rect_filled(rect, round, bg);
        ui.painter().text(rect.center(), eframe::egui::Align2::CENTER_CENTER, label, font_id, tc);
    }
    resp
}

fn with_custom_style<R>(
    ui: &mut eframe::egui::Ui,
    f: impl FnOnce(&mut eframe::egui::Style),
    g: impl FnOnce(&mut eframe::egui::Ui) -> R,
) -> R {
    let old = ui.style().clone();
    f(ui.style_mut());
    let res = g(ui);
    *ui.style_mut() = (*old).clone();
    res
}

fn with_alignment<R>(
    ui: &mut eframe::egui::Ui,
    theme: &Theme,
    sec: &str,
    f: impl FnOnce(&mut eframe::egui::Ui) -> R,
) -> R {
    if let Some(align) = theme.get(sec, "text-align") {
        let layout = match align.as_str() {
            "center" => eframe::egui::Layout::centered_and_justified(eframe::egui::Direction::LeftToRight),
            "left"   => eframe::egui::Layout::left_to_right(eframe::egui::Align::Min),
            "right"  => eframe::egui::Layout::right_to_left(eframe::egui::Align::Max),
            _        => eframe::egui::Layout::default(),
        };
        ui.with_layout(layout, f).inner
    } else {
        f(ui)
    }
}

// ============================================================================
// AppInterface
// ============================================================================

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

// ============================================================================
// LayoutCache – computed once from Theme + Config, never recomputed.
//
// Eliminates dozens of HashMap lookups per frame:
//   • Window size / background color / bg image path
//   • Sorted section list with pre-fetched positions and size constraints
//   • App-row element order (avoids Vec alloc + sort per visible app per frame)
//   • Settings button / icon geometry (avoids 4+ lookups per visible app per frame)
//   • Env popup dimensions (avoids 3 lookups per open popup per frame)
// ============================================================================

/// Which sub-element to render inside an app row.
#[derive(Clone, Copy)]
enum ElemKind { Settings, Icon, App }

/// Pre-resolved background image data.
struct BgImage {
    path:      String,
    size_mode: String,
    opacity:   f32,
}

/// A section with its position and optional size constraints pre-fetched.
struct SectionInfo {
    name: &'static str,
    pos:  Option<(f32, f32)>,
    size: Option<eframe::egui::Vec2>,
}

struct LayoutCache {
    win_size:    eframe::egui::Vec2,
    win_bg:      eframe::egui::Color32,
    /// Background image, fully resolved (path + size mode + opacity).
    /// `None` means draw a plain colour rect – no per-frame APP_CACHE lock needed.
    bg_image:    Option<BgImage>,
    /// Sections in their final render order, positions and size constraints baked in.
    sections:    Vec<SectionInfo>,
    /// App-row element order (settings / icon / button), sorted once.
    elem_order:  Vec<ElemKind>,
    settings_w:  f32,
    settings_h:  f32,
    settings_ox: f32,
    settings_oy: f32,
    icon_w:      f32,
    icon_h:      f32,
    vol_gap:     Option<f32>,
    env_w:       f32,
    env_h:       f32,
    /// Tray strip dimensions, pre-fetched so render_tray_icon has no theme lookups.
    tray_w:      f32,
    tray_h:      f32,
    /// Colour of the live-status dot in the tray-icon widget. Pre-parsed once.
    tray_indicator_color: eframe::egui::Color32,
}

impl LayoutCache {
    fn build(theme: &Theme, config: &Config) -> Self {
        use eframe::egui;

        let win_size = egui::vec2(
            theme.get_px("main-window", "width").unwrap_or(300.0),
            theme.get_px("main-window", "height").unwrap_or(200.0),
        );
        let win_bg = theme.get("main-window", "background-color")
            .and_then(|s| theme.parse_color(&s))
            .unwrap_or(egui::Color32::BLACK);

        // Resolve background image path once so we never lock APP_CACHE per frame.
        let bg_image = theme.get("main-window", "background-image")
            .filter(|s| !s.is_empty())
            .and_then(|img| resolve_icon_path("main-window", &img, config))
            .map(|path| BgImage {
                size_mode: theme.get("main-window", "background-size")
                    .unwrap_or_else(|| "stretch".to_string()),
                opacity: theme.get("main-window", "background-opacity")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1.0),
                path,
            });

        // Build and sort sections list once.
        let mut raw: Vec<(&'static str, i32)> = vec![
            ("search-bar", theme.get_order("search-bar")),
            ("app-list",   theme.get_order("app-list")),
        ];
        if config.enable_audio_control { raw.push(("volume-slider", theme.get_order("volume-slider"))); }
        if config.show_time            { raw.push(("time-display",   theme.get_order("time-display"))); }
        if config.enable_power_options { raw.push(("power-button",   theme.get_order("power-button"))); }
        if config.enable_system_tray   { raw.push(("tray-icon",      theme.get_order("tray-icon"))); }
        raw.sort_by_key(|(_, o)| *o);

        let sections = raw.into_iter().map(|(name, _)| {
            let pos  = theme.get_position(name);
            let size = if matches!(name, "search-bar" | "app-list") {
                theme.get_px(name, "width")
                    .zip(theme.get_px(name, "height"))
                    .map(|(w, h)| egui::vec2(w, h))
            } else {
                None
            };
            SectionInfo { name, pos, size }
        }).collect();

        // Pre-sort element ordering for app rows (previously rebuilt per-app per-frame).
        let mut elems: Vec<(i32, ElemKind)> = vec![
            (theme.get("settings-button", "order").and_then(|s| s.parse().ok()).unwrap_or(0), ElemKind::Settings),
            (theme.get("app-icon",        "order").and_then(|s| s.parse().ok()).unwrap_or(1), ElemKind::Icon),
            (theme.get("app-button",      "order").and_then(|s| s.parse().ok()).unwrap_or(2), ElemKind::App),
        ];
        elems.sort_by_key(|(o, _)| *o);
        let elem_order = elems.into_iter().map(|(_, k)| k).collect();

        // Pre-parse the tray status-dot colour once; fall back to a pleasant green.
        let tray_indicator_color = theme.get("tray-icon", "indicator-color")
            .and_then(|s| theme.parse_color(&s))
            .unwrap_or(egui::Color32::from_rgb(94, 206, 135));

        // Window width minus margins; matches the CSS default.
        let win_w = theme.get_px("main-window", "width").unwrap_or(220.0);
        let tray_w = theme.get_px("tray-icon", "width").unwrap_or(win_w - 24.0);
        let tray_h = theme.get_px("tray-icon", "height").unwrap_or(18.0);

        LayoutCache {
            win_size,
            win_bg,
            bg_image,
            sections,
            elem_order,
            settings_w:  theme.get_px("settings-button", "width").unwrap_or(22.0),
            settings_h:  theme.get_px("settings-button", "height").unwrap_or(22.0),
            settings_ox: theme.get_px("settings-button", "offset-x").unwrap_or(0.0),
            settings_oy: theme.get_px("settings-button", "offset-y").unwrap_or(0.0),
            icon_w:      theme.get_px("app-icon", "width").unwrap_or(22.0),
            icon_h:      theme.get_px("app-icon", "height").unwrap_or(22.0),
            vol_gap:     theme.get_px("volume-slider", "gap"),
            env_w:       theme.get_px("env-input", "width").unwrap_or(300.0),
            env_h:       theme.get_px("env-input", "height").unwrap_or(150.0),
            tray_w,
            tray_h,
            tray_indicator_color,
        }
    }
}

// ============================================================================
// EframeGui / EframeWrapper
// ============================================================================

pub struct EframeGui;

impl EframeGui {
    pub fn run(app: Box<dyn AppInterface>) -> Result<(), Box<dyn Error>> {
        let theme  = Arc::new(Theme::load_or_create());
        let cfg    = theme.get_config();
        let layout = LayoutCache::build(&theme, &cfg);
        let (w, h) = (layout.win_size.x, layout.win_size.y);

        let viewport = eframe::egui::ViewportBuilder::default()
            .with_inner_size([w, h])
            .with_always_on_top()
            .with_decorations(false)
            .with_resizable(false)
            .with_active(true)
            .with_transparent(true);

        let audio = crate::system::AudioController::new(&cfg)?;
        audio.start_polling(&cfg);

        // Start the SNI watcher so apps register their tray icons with us.
        let sni_host = crate::sni::SniHost::new(&cfg);

        eframe::run_native(
            "Application Launcher",
            eframe::NativeOptions { viewport, ..Default::default() },
            Box::new(move |cc| {
                if let Some(s) = theme.get("env-input", "scaling").and_then(|s| s.parse::<f32>().ok()) {
                    cc.egui_ctx.set_pixels_per_point(s);
                }
                cc.egui_ctx.request_repaint();

                // Prime time cache immediately so first frame shows real value.
                let cached_time = app.get_time();

                    Ok(Box::new(EframeWrapper {
                        app,
                        audio_controller: audio,
                        current_volume: 0.0,
                        editing_windows: HashMap::new(),
                        focused: false,
                        icon_manager: crate::app_launcher::IconManager::new(),
                        layout,
                        cached_time,
                        last_time_update: Instant::now(),
                        theme,
                        config: cfg,
                        sni_host,
                        tray_textures: HashMap::new(),
                        tray_name_cache: HashMap::new(),
                        tray_menu_open: None,
                        tray_menu_fetched: None,
                    }))
            }),
        )?;
        Ok(())
    }
}

struct EframeWrapper {
    app:              Box<dyn AppInterface>,
    audio_controller: crate::system::AudioController,
    current_volume:   f32,
    editing_windows:  HashMap<String, String>,
    focused:          bool,
    /// Unified icon cache for both app icons and tray pixmaps.
    icon_manager:     crate::app_launcher::IconManager,
    layout:           LayoutCache,
    /// Clock string, refreshed at most once per second instead of every frame.
    cached_time:      String,
    last_time_update: Instant,
    /// Arc so clones in viewport closures are a pointer copy, not a deep clone.
    theme:            Arc<Theme>,
    config:           Config,
    /// SNI host handle; `None` when `enable_system_tray` is false.
    sni_host:         Option<crate::sni::SniHost>,
    /// Cached egui textures for SNI ARGB32 pixmaps, keyed by "{id}" or "{id}_attn".
    /// File-path-based tray icons go through icon_manager instead.
    tray_textures:    HashMap<String, eframe::egui::TextureHandle>,
    /// Cache of resolved file paths for SNI icon names (avoids per-frame filesystem walk).
    /// Maps icon-name → resolved path (None = not found, skip re-searching).
    tray_name_cache:  HashMap<String, Option<String>>,
    /// Which tray icon's menu is open, and whether we've fetched it yet.
    tray_menu_open:    Option<String>,
    tray_menu_fetched: Option<String>,
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
                if let Some(gap) = self.layout.vol_gap { ui.spacing_mut().item_spacing.x = gap; }
                ui.label("Volume:");
                let (base, hover, round) = self.theme.get_frame_props("volume-slider", ui.style().visuals.widgets.inactive.bg_fill);
                let vis = {
                    let mut s = ui.style().visuals.widgets.inactive.clone();
                    s.bg_fill = base; s.corner_radius = round; s
                };
                with_custom_style(ui, |s| {
                    s.visuals.widgets.inactive = vis.clone();
                    s.visuals.widgets.hovered.bg_fill       = hover.unwrap_or(base);
                    s.visuals.widgets.hovered.weak_bg_fill  = hover.unwrap_or(base);
                    s.visuals.widgets.active = vis;
                    let t = eframe::egui::Color32::TRANSPARENT;
                    s.visuals.widgets.inactive.bg_stroke = eframe::egui::Stroke::new(0.0, t);
                    s.visuals.widgets.hovered.bg_stroke  = eframe::egui::Stroke::new(0.0, t);
                    s.visuals.widgets.active.bg_stroke   = eframe::egui::Stroke::new(0.0, t);
                    s.visuals.widgets.hovered.expansion  = 0.0;
                    s.visuals.widgets.active.expansion   = 0.0;
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
        let query    = self.app.get_query();
        let filtered: Vec<String> = if query.trim().is_empty() {
            if self.config.enable_recent_apps {
                self.app.get_search_results().into_iter().take(self.config.max_search_results).collect()
            } else {
                Vec::new()
            }
        } else {
            self.app.get_search_results().into_iter().take(self.config.max_search_results).collect()
        };

        ui.vertical(|ui| {
            for app_name in filtered {
                let row_id = ui.id().with(&app_name);
                ui.horizontal(|ui| {
                    // Use pre-sorted element order from LayoutCache –
                    // previously a Vec was allocated and sorted for every visible app every frame.
                    for &kind in &self.layout.elem_order {
                        match kind {
                            ElemKind::Settings if self.config.show_settings_button => {
                                let (w, h)   = (self.layout.settings_w, self.layout.settings_h);
                                let (ox, oy) = (self.layout.settings_ox, self.layout.settings_oy);
                                let (base_rect, _) = ui.allocate_exact_size(eframe::egui::vec2(w, h), eframe::egui::Sense::hover());
                                let rect = base_rect.translate(eframe::egui::vec2(ox, oy));
                                let resp = ui.interact(rect, row_id.with("settings-button"), eframe::egui::Sense::click());
                                self.theme.apply_style(ui, "settings-button");
                                let color = self.theme.get_text_color("settings-button", resp.hovered())
                                    .unwrap_or(eframe::egui::Color32::from_rgb(64, 64, 64));
                                let font = eframe::egui::TextStyle::Button.resolve(ui.style());
                                ui.painter().text(rect.center(), eframe::egui::Align2::CENTER_CENTER, "⚙", font, color);
                                if resp.clicked() {
                                    self.editing_windows.insert(app_name.clone(), self.app.get_formatted_launch_options(&app_name));
                                }
                            }
                            ElemKind::Icon if self.config.enable_icons => {
                                if let Some(icon_path) = self.app.get_icon_path(&app_name) {
                                    let (rect, _) = ui.allocate_exact_size(
                                        eframe::egui::vec2(self.layout.icon_w, self.layout.icon_h),
                                        eframe::egui::Sense::hover(),
                                    );
                                    if let Some(tex) = self.icon_manager.get_texture(ctx, &icon_path) {
                                        ui.painter().image(
                                            tex.id(), rect,
                                            eframe::egui::Rect::from_min_max(eframe::egui::Pos2::ZERO, eframe::egui::Pos2::new(1.0, 1.0)),
                                            eframe::egui::Color32::WHITE,
                                        );
                                    }
                                }
                            }
                            ElemKind::App => {
                                // Sense::click_and_drag is set inside custom_button_width — no second interact needed.
                                let resp = custom_button_width(ui, &app_name, "app-button", &self.theme, None);
                                if resp.clicked()           { self.app.launch_app(&app_name); }
                                if resp.secondary_clicked() {
                                    self.editing_windows.insert(app_name.clone(), self.app.get_formatted_launch_options(&app_name));
                                }
                            }
                            _ => {}
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
            ui.label(&self.cached_time); // throttled to 1 Hz, not every frame
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

    fn render_tray_icon(&mut self, ui: &mut eframe::egui::Ui, ctx: &eframe::egui::Context) {
        use eframe::egui;

        self.theme.apply_style(ui, "tray-icon");

        // Use pre-cached dimensions from LayoutCache — no theme lookups per frame.
        let tray_w = self.layout.tray_w;
        let tray_h = self.layout.tray_h;

        const ICON_SZ: f32 = 16.0;
        const GAP:     f32 = 3.0;
        let icon_size = egui::vec2(ICON_SZ, ICON_SZ);

        // Draw strip background (skip when transparent to avoid egui filling the area).
        let (bg, _, round) = self.theme.get_frame_props("tray-icon", egui::Color32::TRANSPARENT);
        let strip_origin = ui.cursor().min;
        if bg != egui::Color32::TRANSPARENT {
            ui.painter().rect_filled(
                egui::Rect::from_min_size(strip_origin, egui::vec2(tray_w, tray_h)),
                round, bg,
            );
        }

        let (strip_rect, _) = ui.allocate_exact_size(egui::vec2(tray_w, tray_h), egui::Sense::hover());

        // Clone only the visible icons from the mutex rather than the full list.
        // try_lock avoids blocking the render thread if the SNI thread holds the lock.
        let icons: Vec<crate::sni::TrayIcon> = self
            .sni_host
            .as_ref()
            .and_then(|h| h.items.try_lock().ok())
            .map(|g| g.iter().filter(|i| i.status != crate::sni::TrayStatus::Passive).cloned().collect())
            .unwrap_or_default();

        let visible: Vec<&crate::sni::TrayIcon> = icons.iter().collect();

        if visible.is_empty() {
            let dot_r = 3.0_f32;
            let center = egui::pos2(strip_rect.min.x + GAP + dot_r, strip_rect.center().y);
            ui.painter().circle_filled(center, dot_r, self.layout.tray_indicator_color);
            return;
        }

        // Pack icons left-to-right; each is vertically centred in the strip.
        let cy  = strip_rect.center().y;
        let mut x = strip_rect.min.x + GAP;

        for icon in &visible {
            let icon_rect = egui::Rect::from_min_size(
                egui::pos2(x, cy - ICON_SZ * 0.5),
                icon_size,
            );
            x += ICON_SZ + GAP;

            // ── Texture upload ──────────────────────────────────────────────
            let use_attention = icon.status == crate::sni::TrayStatus::NeedsAttention
                && (!icon.attention_icon_rgba.is_empty() || icon.attention_icon_name.is_some());

            let (tex_rgba, tex_w, tex_h, tex_name) = if use_attention {
                (&icon.attention_icon_rgba, icon.attention_icon_w, icon.attention_icon_h, &icon.attention_icon_name)
            } else {
                (&icon.icon_rgba, icon.icon_w, icon.icon_h, &icon.icon_name)
            };
            let tex_key = if use_attention { format!("{}_attn", icon.id) } else { icon.id.clone() };

            if tex_w > 0 && tex_h > 0 && !tex_rgba.is_empty()
                && !self.tray_textures.contains_key(&tex_key)
            {
                // SNI pixmaps are ARGB32 (network byte order / big-endian u32 per pixel).
                // egui expects RGBA, so reorder: [A,R,G,B] → [R,G,B,A].
                let mut rgba = vec![0u8; tex_rgba.len()];
                for (src, dst) in tex_rgba.chunks_exact(4).zip(rgba.chunks_exact_mut(4)) {
                    dst[0] = src[1]; // R
                    dst[1] = src[2]; // G
                    dst[2] = src[3]; // B
                    dst[3] = src[0]; // A
                }
                let img    = egui::ColorImage::from_rgba_unmultiplied([tex_w as usize, tex_h as usize], &rgba);
                let handle = ctx.load_texture(&tex_key, img, egui::TextureOptions::LINEAR);
                self.tray_textures.insert(tex_key.clone(), handle);
            }

            // ── Draw icon ───────────────────────────────────────────────────
            if ui.is_rect_visible(icon_rect) {
                let texture = self.tray_textures.get(&tex_key);
                if let Some(tex) = texture {
                    ui.painter().image(
                        tex.id(), icon_rect,
                        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                } else if let Some(name) = tex_name.as_deref().filter(|s: &&str| !s.is_empty()) {
                    // Cache the resolved path so we don't re-walk the filesystem every frame.
                    let cache_key = format!("{}|{}", name, icon.icon_theme_path.as_deref().unwrap_or(""));
                    let resolved = self.tray_name_cache
                        .entry(cache_key)
                        .or_insert_with(|| resolve_tray_icon_name(name, icon.icon_theme_path.as_deref(), &self.config))
                        .as_deref();
                    if let Some(path) = resolved {
                        if let Some(tex) = self.icon_manager.get_texture(ctx, path) {
                            ui.painter().image(
                                tex.id(), icon_rect,
                                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                                egui::Color32::WHITE,
                            );
                        } else {
                            ui.painter().circle_filled(icon_rect.center(), ICON_SZ * 0.4, self.layout.tray_indicator_color);
                        }
                    } else {
                        ui.painter().circle_filled(icon_rect.center(), ICON_SZ * 0.4, self.layout.tray_indicator_color);
                    }
                } else {
                    ui.painter().circle_filled(icon_rect.center(), ICON_SZ * 0.4, self.layout.tray_indicator_color);
                }
            }

            // ── Interact ────────────────────────────────────────────────────
            let resp = ui.interact(icon_rect, ui.id().with(&icon.id), egui::Sense::click());
            let resp = resp.on_hover_text(&icon.tooltip_title);

            // Hover highlight / open-menu outline.
            if resp.hovered() || self.tray_menu_open.as_deref() == Some(&icon.id) {
                ui.painter().rect_stroke(
                    icon_rect, 2.0,
                    egui::Stroke::new(1.0, egui::Color32::from_white_alpha(100)),
                    egui::StrokeKind::Middle,
                );
            }

            // Left click → Activate (or ContextMenu when ItemIsMenu == true).
            if resp.clicked() {
                if let Some(host) = &self.sni_host {
                    if icon.item_is_menu {
                        let pos = resp.interact_rect.center();
                        host.context_menu(&icon.bus_name, &icon.obj_path, pos.x as i32, pos.y as i32);
                    } else {
                        host.activate(&icon.bus_name, &icon.obj_path);
                    }
                }
                if self.tray_menu_open.is_some() {
                    let old_id = self.tray_menu_open.take().unwrap();
                    let vp_id  = egui::ViewportId::from_hash_of(format!("tray_menu_{}", old_id));
                    ctx.send_viewport_cmd_to(vp_id, egui::ViewportCommand::Close);
                }
            }

            // Scroll wheel → forward to item.
            if resp.hovered() {
                let scroll = ui.input(|i| i.smooth_scroll_delta);
                if scroll.y.abs() > 0.5 {
                    if let Some(host) = &self.sni_host {
                        host.scroll(&icon.bus_name, &icon.obj_path, scroll.y as i32, "vertical");
                    }
                }
                if scroll.x.abs() > 0.5 {
                    if let Some(host) = &self.sni_host {
                        host.scroll(&icon.bus_name, &icon.obj_path, scroll.x as i32, "horizontal");
                    }
                }
            }

            // Right click → open / close DBusMenu viewport.
            if resp.secondary_clicked() {
                if self.tray_menu_open.as_deref() == Some(&icon.id) {
                    let vp_id = egui::ViewportId::from_hash_of(format!("tray_menu_{}", icon.id));
                    ctx.send_viewport_cmd_to(vp_id, egui::ViewportCommand::Close);
                    self.tray_menu_open = None;
                } else {
                    if let Some(old_id) = self.tray_menu_open.take() {
                        let vp_id = egui::ViewportId::from_hash_of(format!("tray_menu_{}", old_id));
                        ctx.send_viewport_cmd_to(vp_id, egui::ViewportCommand::Close);
                    }
                    self.tray_menu_open    = Some(icon.id.clone());
                    self.tray_menu_fetched = None;
                    if let (Some(host), Some(menu_path)) = (&self.sni_host, &icon.menu_path) {
                        host.menu_about_to_show(&icon.bus_name, menu_path);
                    }
                }
            }

            // Menu viewport — only rendered for the currently-open icon.
            if self.tray_menu_open.as_deref() == Some(&icon.id) {
                if self.tray_menu_fetched.as_deref() != Some(&icon.id) {
                    if let (Some(host), Some(menu_path)) = (&self.sni_host, &icon.menu_path) {
                        host.fetch_menu(&icon.bus_name, menu_path, &icon.id);
                    }
                    self.tray_menu_fetched = Some(icon.id.clone());
                }

                if icon.menu_path.is_some() {
                    let menu_items  = icon.menu_items.clone();
                    let menu_loaded = icon.menu_loaded;
                    let icon_id     = icon.id.clone();
                    let bus_name    = icon.bus_name.clone();
                    let menu_path   = icon.menu_path.clone();
                    let indicator   = self.layout.tray_indicator_color;
                    let win_bg      = self.layout.win_bg;
                    let tooltip     = icon.tooltip_title.clone();
                    let action_key  = format!("tray_menu_action_{}", icon_id);
                    let theme_menu  = Arc::clone(&self.theme);

                    if !menu_loaded { ctx.request_repaint(); }

                    let item_count = menu_items.iter().filter(|i| !i.is_separator).count();
                    let win_h      = (item_count as f32 * 28.0 + 32.0).clamp(60.0, 400.0);

                    let viewport_id = egui::ViewportId::from_hash_of(format!("tray_menu_{}", icon_id));
                    let viewport    = egui::ViewportBuilder::default()
                        .with_title(if tooltip.is_empty() { "Menu".to_string() } else { tooltip })
                        .with_inner_size([180.0_f32, win_h])
                        .with_resizable(false)
                        .with_transparent(true)
                        .with_always_on_top();

                    ctx.show_viewport_immediate(viewport_id, viewport, move |ctx, _| {
                        let action_key = format!("tray_menu_action_{}", icon_id);
                        egui::CentralPanel::default()
                            .frame(egui::Frame::NONE.fill(win_bg))
                            .show(ctx, |ui| {
                                ui.add_space(4.0);
                                if !menu_loaded {
                                    ui.add_enabled(false, egui::Label::new("Loading…"));
                                } else if menu_items.is_empty() {
                                    ui.add_enabled(false, egui::Label::new("No menu items"));
                                } else {
                                    let clicked = render_menu_items(ui, &menu_items, indicator, &theme_menu);
                                    if let Some(item_id) = clicked {
                                        ctx.data_mut(|d| d.insert_temp(egui::Id::new(&action_key), item_id));
                                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                    }
                                }
                                if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                                    ctx.data_mut(|d| d.insert_temp(egui::Id::new(&action_key), -1i32));
                                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                }
                            });
                    });

                    if let Some(item_id) = ctx.data_mut(|d| d.get_temp::<i32>(egui::Id::new(&action_key))) {
                        if item_id >= 0 {
                            if let (Some(host), Some(mp)) = (&self.sni_host, &menu_path) {
                                host.menu_event(&bus_name, mp, item_id);
                            }
                        }
                        self.tray_menu_open = None;
                        ctx.data_mut(|d| d.remove::<i32>(egui::Id::new(&action_key)));
                        ctx.send_viewport_cmd_to(viewport_id, egui::ViewportCommand::Close);
                    }
                }
            }
        }
    }

    fn render_section(&mut self, ui: &mut eframe::egui::Ui, sec: &str, ctx: &eframe::egui::Context) {
        match sec {
            "search-bar"    => self.render_search_bar(ui),
            "volume-slider" => self.render_volume_slider(ui),
            "app-list"      => self.render_app_list(ui, ctx),
            "time-display"  => self.render_time_display(ui),
            "power-button"  => self.render_power_button(ui),
            "tray-icon"     => self.render_tray_icon(ui, ctx),
            _               => {}
        }
    }
}

/// Resolve a freedesktop icon name to an absolute file path by searching the
/// full XDG icon theme hierarchy.
fn resolve_tray_icon_name(
    name:           &str,
    app_theme_path: Option<&str>,
    config:         &Config,
) -> Option<String> {
    if name.is_empty() { return None; }

    // If the name looks like an absolute path that already exists, use it directly.
    if name.starts_with('/') {
        if std::path::Path::new(name).exists() { return Some(name.to_string()); }
        for ext in &["png", "svg", "xpm"] {
            let p = format!("{name}.{ext}");
            if std::path::Path::new(&p).exists() { return Some(p); }
        }
    }

    let exts   = ["png", "svg", "xpm"];
    let sizes   = ["256x256", "128x128", "64x64", "48x48", "32x32", "24x24", "22x22", "16x16", "scalable"];
    let cats    = ["apps", "status", "devices", "actions", "categories", "emblems", "mimetypes", "places"];
    let themes  = ["hicolor", "Papirus", "Papirus-Dark", "Papirus-Light",
                   "Adwaita", "breeze", "breeze-dark", "gnome", "locolor",
                   "oxygen", "Tango", "elementary", "Humanity"];

    // Try the exact name plus common suffix-stripped variants.
    // e.g. "audio-volume-medium-panel" → also try "audio-volume-medium".
    let stripped = name.strip_suffix("-panel")
        .or_else(|| name.strip_suffix("-symbolic"))
        .or_else(|| name.strip_suffix("-rtl"))
        .or_else(|| name.strip_suffix("-ltr"));
    let candidates: Vec<&str> = std::iter::once(name)
        .chain(stripped.into_iter())
        .collect();

    // Build the list of base dirs to search, in priority order.
    let mut base_dirs: Vec<std::path::PathBuf> = Vec::new();
    if let Some(p) = app_theme_path { base_dirs.push(std::path::PathBuf::from(p)); }
    if let Some(home) = std::env::var_os("HOME") {
        let home = std::path::Path::new(&home);
        base_dirs.push(home.join(".local/share/icons"));
        base_dirs.push(home.join(".icons"));
    }
    base_dirs.push(std::path::PathBuf::from("/usr/share/icons"));
    base_dirs.push(std::path::PathBuf::from("/usr/local/share/icons"));

    for candidate in &candidates {
        for base in &base_dirs {
            for theme in &themes {
                for size in &sizes {
                    for cat in &cats {
                        for ext in &exts {
                            let p = base.join(theme).join(size).join(cat).join(format!("{candidate}.{ext}"));
                            if p.exists() { return Some(p.to_string_lossy().into_owned()); }
                        }
                    }
                    // Also try without category sub-dir.
                    for ext in &exts {
                        let p = base.join(theme).join(size).join(format!("{candidate}.{ext}"));
                        if p.exists() { return Some(p.to_string_lossy().into_owned()); }
                    }
                }
            }
            // Flat root of base dir.
            for ext in &exts {
                let p = base.join(format!("{candidate}.{ext}"));
                if p.exists() { return Some(p.to_string_lossy().into_owned()); }
            }
        }
        // Pixmaps fallback.
        for ext in &exts {
            let p = format!("/usr/share/pixmaps/{candidate}.{ext}");
            if std::path::Path::new(&p).exists() { return Some(p); }
        }
    }

    // App-launcher helper (handles icon DB / .desktop cross-reference).
    crate::app_launcher::resolve_icon_path(name, name, config)
}

/// Recursively render DBusMenu items; returns the clicked item id if any.
fn render_menu_items(
    ui:        &mut eframe::egui::Ui,
    items:     &[crate::sni::MenuItem],
    indicator: eframe::egui::Color32,
    theme:     &Theme,
) -> Option<i32> {
    use eframe::egui;
    let mut clicked = None;

    // Pre-fetch app-button colors once for all items in this call.
    let bg_normal = theme.get("app-button", "background-color")
        .and_then(|s| theme.parse_color(&s))
        .unwrap_or(egui::Color32::from_rgb(122, 162, 247));
    let bg_hover = theme.get("app-button", "background-color-hover")
        .and_then(|s| theme.parse_color(&s))
        .unwrap_or(bg_normal);
    let tc_normal = theme.get("app-button", "color")
        .and_then(|s| theme.parse_color(&s))
        .unwrap_or(egui::Color32::WHITE);
    // Disabled text: same color but semi-transparent.
    let tc_disabled = egui::Color32::from_rgba_unmultiplied(
        tc_normal.r(), tc_normal.g(), tc_normal.b(), 100,
    );
    let rounding = theme.get("app-button", "border-radius")
        .and_then(|s| s.replace("px", "").parse::<f32>().ok())
        .map(|v| egui::CornerRadius::same(v as u8))
        .unwrap_or_default();
    let font_id = ui.style().text_styles
        .get(&egui::TextStyle::Button).cloned().unwrap_or_default();

    for item in items {
        if item.is_separator {
            ui.separator();
            continue;
        }
        if item.label.is_empty() { continue; }

        let avail_w = ui.available_width();

        if item.children.is_empty() {
            // ── Leaf item (enabled or disabled) ──────────────────────────────
            let galley = ui.painter().layout_no_wrap(
                item.label.clone(), font_id.clone(), egui::Color32::WHITE,
            );
            let h    = galley.size().y + ui.spacing().button_padding.y * 2.0;
            let size = egui::vec2(avail_w, h);
            let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());

            if ui.is_rect_visible(rect) {
                let hovered = response.hovered() && item.enabled;
                let bg = if hovered { bg_hover } else { bg_normal };
                let tc = if item.enabled { tc_normal } else { tc_disabled };

                ui.painter().rect_filled(rect, rounding, bg);
                ui.painter().text(
                    egui::pos2(rect.min.x + ui.spacing().button_padding.x, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    &item.label, font_id.clone(), tc,
                );
            }
            if response.clicked() && item.enabled {
                clicked = Some(item.id);
            }

        } else {
            // ── Submenu with custom hand-drawn collapsible header ─────────────
            // CollapsingHeader ignores bg_fill overrides, so we draw it ourselves.
            let open_key = egui::Id::new(("tray_submenu", &item.label, item.id));
            let is_open: bool = ui.ctx().data(|d| d.get_temp(open_key).unwrap_or(false));

            let arrow = if is_open { "▼ " } else { "▶ " };
            let header_label = format!("{}{}", arrow, item.label);
            let galley = ui.painter().layout_no_wrap(
                header_label.clone(), font_id.clone(), egui::Color32::WHITE,
            );
            let h    = galley.size().y + ui.spacing().button_padding.y * 2.0;
            let size = egui::vec2(avail_w, h);
            let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());

            if ui.is_rect_visible(rect) {
                let bg = if response.hovered() { bg_hover } else { bg_normal };
                ui.painter().rect_filled(rect, rounding, bg);
                ui.painter().text(
                    egui::pos2(rect.min.x + ui.spacing().button_padding.x, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    &header_label, font_id.clone(), tc_normal,
                );
            }
            if response.clicked() {
                ui.ctx().data_mut(|d| d.insert_temp(open_key, !is_open));
            }

            if is_open {
                ui.indent(open_key, |ui| {
                    if let Some(id) = render_menu_items(ui, &item.children, indicator, theme) {
                        clicked = Some(id);
                    }
                });
            }
        }
    }

    clicked
}

impl eframe::App for EframeWrapper {
    fn update(&mut self, ctx: &eframe::egui::Context, _frame: &mut eframe::Frame) {
        self.app.update();

        // Read volume from the Arc<Mutex<f32>> kept by the polling thread.
        // The old code called update_volume() here, which spawned a `wpctl` subprocess every frame.
        if self.config.enable_audio_control {
            self.current_volume = self.audio_controller.get_volume();
        }

        // Refresh the clock string at most once per second – the display only changes by the minute.
        if self.config.show_time && self.last_time_update.elapsed() >= Duration::from_secs(1) {
            self.cached_time     = self.app.get_time();
            self.last_time_update = Instant::now();
        }

        // Read all keyboard state in one input closure instead of two.
        let (esc, enter) = ctx.input(|i| (
            i.key_pressed(eframe::egui::Key::Escape),
            i.key_pressed(eframe::egui::Key::Enter),
        ));

        // ----------------------------------------------------------------
        // Main window
        // ----------------------------------------------------------------
        let (w, h) = (self.layout.win_size.x, self.layout.win_size.y);
        let bg     = self.layout.win_bg;
        let rect   = eframe::egui::Rect::from_min_size(eframe::egui::pos2(0.0, 0.0), eframe::egui::vec2(w, h));

        eframe::egui::Area::new("main".into()).fixed_pos(eframe::egui::pos2(0.0, 0.0)).show(ctx, |ui| {
            ui.set_min_size(eframe::egui::vec2(w, h));
            ui.set_max_size(eframe::egui::vec2(w, h));

            // Draw background. BgImage.path is already resolved – no per-frame APP_CACHE lock.
            if let Some(ref bgi) = self.layout.bg_image {
                if let Some(tex) = self.icon_manager.get_texture(ctx, &bgi.path) {
                    let img_size = tex.size_vec2();
                    let (draw_rect, uv) = match bgi.size_mode.as_str() {
                        "fit" => {
                            let scale    = (rect.width() / img_size.x).min(rect.height() / img_size.y);
                            let new_size = img_size * scale;
                            let offset   = (rect.size() - new_size) * 0.5;
                            (eframe::egui::Rect::from_min_size(rect.min + offset, new_size),
                             eframe::egui::Rect::from_min_max(eframe::egui::Pos2::ZERO, eframe::egui::Pos2::new(1.0, 1.0)))
                        }
                        "fill" => {
                            let scale    = (rect.width() / img_size.x).max(rect.height() / img_size.y);
                            let new_size = img_size * scale;
                            let offset   = (new_size - rect.size()) * 0.5;
                            let uv_min   = eframe::egui::Pos2::new(offset.x / new_size.x, offset.y / new_size.y);
                            let uv_max   = eframe::egui::Pos2::new(1.0 - offset.x / new_size.x, 1.0 - offset.y / new_size.y);
                            (rect, eframe::egui::Rect::from_min_max(uv_min, uv_max))
                        }
                        _ => (rect, eframe::egui::Rect::from_min_max(
                            eframe::egui::Pos2::ZERO, eframe::egui::Pos2::new(1.0, 1.0))),
                    };
                    let tint = eframe::egui::Color32::from_white_alpha((bgi.opacity * 255.0) as u8);
                    ui.painter().image(tex.id(), draw_rect, uv, tint);
                } else {
                    ui.painter().rect_filled(rect, 0.0, bg);
                }
            } else {
                ui.painter().rect_filled(rect, 0.0, bg);
            }

            // Copy section data to plain locals before the loop so the borrow on
            // self.layout ends before the closure that calls self.render_section.
            let sections: Vec<(&'static str, Option<(f32, f32)>, Option<eframe::egui::Vec2>)> =
                self.layout.sections.iter().map(|s| (s.name, s.pos, s.size)).collect();

            for (name, pos, size) in sections {
                let area = if let Some((x, y)) = pos {
                    eframe::egui::Area::new(name.to_owned().into())
                        .order(eframe::egui::Order::Foreground)
                        .fixed_pos(eframe::egui::pos2(x, y))
                } else {
                    eframe::egui::Area::new(name.to_owned().into())
                        .order(eframe::egui::Order::Foreground)
                };

                area.show(ctx, |ui| {
                    if let Some(sz) = size {
                        ui.set_min_size(sz);
                        ui.set_max_size(sz);
                    }
                    self.render_section(ui, name, ctx);
                });
            }
        });

        // ----------------------------------------------------------------
        // Editing windows (env-vars popup)
        // ----------------------------------------------------------------
        let mut to_remove = Vec::new();

        for (app_name, opts) in self.editing_windows.iter() {
            // Use cached popup dimensions – no per-frame theme lookups.
            let win_bg  = self.layout.win_bg;
            let env_w   = self.layout.env_w;
            let env_h   = self.layout.env_h;

            let app_clone   = app_name.clone();
            let opts_clone  = opts.clone();
            let theme_clone = Arc::clone(&self.theme);

            let viewport_id = eframe::egui::ViewportId::from_hash_of(format!("env_{}", app_name));
            let viewport = eframe::egui::ViewportBuilder::default()
                .with_title(app_name.clone())
                .with_inner_size([env_w, env_h])
                .with_resizable(false)
                .with_transparent(true)
                .with_always_on_top();

            let mem_key    = format!("env_opts_{}", app_name);
            let action_key = format!("env_action_{}", app_name);

            let current_opts = ctx.data_mut(|d| {
                d.get_persisted::<String>(eframe::egui::Id::new(&mem_key))
                    .unwrap_or_else(|| opts_clone.clone())
            });

            if current_opts != opts_clone {
                let stored_app_key = format!("env_app_{}", app_name);
                let stored_app     = ctx.data_mut(|d| d.get_persisted::<String>(eframe::egui::Id::new(&stored_app_key)));
                if stored_app.as_ref() != Some(&app_clone) {
                    ctx.data_mut(|d| {
                        d.insert_persisted(eframe::egui::Id::new(&mem_key),        opts_clone.clone());
                        d.insert_persisted(eframe::egui::Id::new(&stored_app_key), app_clone.clone());
                    });
                }
            }

            ctx.show_viewport_immediate(viewport_id, viewport, move |ctx, _| {
                let mem_key    = format!("env_opts_{}", app_clone);
                let action_key = format!("env_action_{}", app_clone);

                let mut opts = ctx.data_mut(|d| {
                    d.get_persisted::<String>(eframe::egui::Id::new(&mem_key))
                        .unwrap_or_else(|| opts_clone.clone())
                });

                eframe::egui::CentralPanel::default()
                    .frame(eframe::egui::Frame::NONE.fill(win_bg))
                    .show(ctx, |ui| {
                        ui.vertical(|ui| {
                            ui.label(&app_clone);
                            ui.add_space(4.0);
                            with_alignment(ui, &theme_clone, "env-input", |ui| {
                                theme_clone.apply_style(ui, "env-input");
                                ui.add(eframe::egui::TextEdit::singleline(&mut opts)
                                    .hint_text("Enter env variables...")
                                    .desired_width(f32::INFINITY));
                            });
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                if custom_button(ui, "Save",   "edit-button", &theme_clone).clicked() {
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

                ctx.data_mut(|d| d.insert_persisted(eframe::egui::Id::new(&mem_key), opts));
            });

            // Unified save/cancel teardown – previously duplicated in two separate branches.
            if let Some(action) = ctx.data_mut(|d| d.get_temp::<String>(eframe::egui::Id::new(&action_key))) {
                if action == "save" {
                    let final_opts = ctx.data_mut(|d| {
                        d.get_persisted::<String>(eframe::egui::Id::new(&mem_key))
                            .unwrap_or_else(|| opts.clone())
                    });
                    self.app.handle_input(&format!("LAUNCH_OPTIONS:{}:{}", app_name, final_opts));
                }
                // Both save and cancel share the same cleanup path.
                to_remove.push(app_name.clone());
                ctx.data_mut(|d| {
                    d.remove::<String>(eframe::egui::Id::new(&mem_key));
                    d.remove::<String>(eframe::egui::Id::new(&action_key));
                    d.remove::<String>(eframe::egui::Id::new(&format!("env_app_{}", app_name)));
                });
                ctx.send_viewport_cmd_to(viewport_id, eframe::egui::ViewportCommand::Close);
            }
        }

        for app_name in to_remove { self.editing_windows.remove(&app_name); }

        // Keyboard events were read at the top of update() in a single input closure.
        if esc   && self.editing_windows.is_empty() { self.app.handle_input("ESC"); }
        if enter && self.editing_windows.is_empty() { self.app.handle_input("ENTER"); }
        if self.app.should_quit() { ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Close); }
    }
}

pub fn load_theme() -> Arc<Theme> { Arc::new(Theme::load_or_create()) }