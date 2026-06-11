use std::{
    collections::HashMap,
    error::Error,
    fs::{read_to_string, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use eframe;
use serde::{Deserialize, Serialize};
use crate::app_launcher::resolve_icon_path;

/// Local wall-clock time — replaces `time::OffsetDateTime` with zero extra deps.
/// Populated via `libc::localtime_r`, which is always available on Linux
/// (libc is already a transitive dep via zbus → nix → libc).
pub struct LocalTime {
    pub year:  i32,
    pub month: u8,   // 1–12
    pub day:   u8,
    pub hour:  u8,
    pub min:   u8,
    pub sec:   u8,
}

impl LocalTime {
    pub fn now() -> Self {
        #[cfg(unix)]
        unsafe {
            let mut t: libc::time_t = 0;
            libc::time(&mut t);
            let mut tm: libc::tm = std::mem::zeroed();
            libc::localtime_r(&t, &mut tm);
            Self {
                year:  (tm.tm_year + 1900),
                month: (tm.tm_mon + 1) as u8,
                day:   tm.tm_mday as u8,
                hour:  tm.tm_hour as u8,
                min:   tm.tm_min as u8,
                sec:   tm.tm_sec as u8,
            }
        }
        #[cfg(not(unix))]
        Self { year: 2024, month: 1, day: 1, hour: 0, min: 0, sec: 0 }
    }
}

const DEFAULT_THEME: &str = r#"
/* ═══════════════════════════════════════════════════════
   Tusk Launcher — Default Theme
   Define your palette here; reference it with var(--name).
   Hover states use standard :selector:hover { } blocks.
   ═══════════════════════════════════════════════════════ */

:root {
    --bg-base:     rgba(12,  12,  18,  0.96);
    --bg-raised:   rgba(36,  36,  52,  1);
    --bg-hover:    rgba(52,  52,  74,  1);
    --accent:      rgba(110, 90,  220, 1);
    --accent-hi:   rgba(135, 115, 245, 1);
    --text:        rgba(218, 216, 232, 1);
    --text-bright: rgba(235, 233, 250, 1);
    --text-dim:    rgba(120, 118, 140, 1);
    --green:       rgba(72,  210, 140, 1);
    --transparent: rgba(0,   0,   0,   0);
}

/* Main Window
 * Layout (px):
 *   search-bar  top:10  h:26  → ends:36
 *   app-list    top:40  h:130 → ends:170
 *   tray-icon   top:174 h:18  → ends:192
 *   time/vol    top:196 h:16  → ends:212
 *   power       top:216 h:20  → ends:236  */
.main-window {
    background-color: var(--bg-base);
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
    background-color: var(--bg-raised);
    border-radius: 8px;
    color: var(--text);
    padding: 0px;
    font-size: 12px;
}
.search-bar:hover {
    background-color: rgba(48, 48, 68, 1);
}

/* App List Container */
.app-list {
    position: absolute;
    left: 12px;
    top: 40px;
    width: 196px;
    height: 130px;
    background-color: var(--transparent);
    padding: 0px;
    border-radius: 0px;
}

/* App Button */
.app-button {
    background-color: var(--bg-raised);
    color: var(--text);
    border-radius: 6px;
    padding: 0px;
    font-size: 12px;
    order: 2;
}
.app-button:hover {
    background-color: var(--bg-hover);
    color: var(--text-bright);
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
    color: var(--text-dim);
    font-size: 14px;
    offset-x: 10px;
    offset-y: -2px;
    order: 0;
}
.settings-button:hover {
    color: var(--accent-hi);
}

/* System Tray Strip */
.tray-icon {
    position: absolute;
    left: 12px;
    top: 174px;
    width: 196px;
    height: 18px;
    background-color: var(--transparent);
    color: var(--text);
    indicator-color: var(--green);
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
    background-color: var(--transparent);
    color: var(--text-dim);
    text-align: center;
}

/* Volume Slider */
.volume-slider {
    position: absolute;
    left: 12px;
    top: 196px;
    width: 196px;
    height: 16px;
    background-color: var(--bg-raised);
    color: var(--text);
    border-radius: 6px;
    gap: 5px;
}
.volume-slider:hover {
    background-color: var(--bg-hover);
}

/* Power / Restart / Logout Buttons */
.power-button {
    position: absolute;
    left: 12px;
    top: 216px;
    width: 196px;
    height: 20px;
    background-color: var(--bg-raised);
    color: var(--text);
    border-radius: 6px;
    padding: 0px;
}
.power-button:hover {
    background-color: var(--bg-hover);
    color: var(--text-bright);
}

/* Edit / Save / Cancel (env-vars popup) */
.edit-button {
    background-color: var(--accent);
    color: var(--text);
    border-radius: 6px;
    padding: 0px;
    font-size: 12px;
}
.edit-button:hover {
    background-color: var(--accent-hi);
    color: white;
}

/* Env-Vars Input Field */
.env-input {
    background-color: var(--bg-raised);
    color: var(--text);
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
        let icon_cache_dir = crate::paths::config_home().join("tusk-launcher/icons");
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
            logout_commands: vec![
                "loginctl terminate-session $XDG_SESSION_ID".into(),
                "hyprctl dispatch exit".into(), "swaymsg exit".into(),
                "gnome-session-quit --logout --no-prompt".into(),
                "qdbus org.kde.ksmserver /KSMServer logout 0 0 0".into(),
            ],
            enable_icons: true,
            icon_cache_dir,
            show_settings_button: true,
            enable_system_tray: false,
        }
    }
}

pub fn format_datetime(t: &LocalTime, config: &Config) -> String {
    let date_str = match config.time_order {
        TimeOrder::MdyHms => format!("{:02}/{:02}/{}", t.month, t.day, t.year),
        TimeOrder::YmdHms => format!("{}/{:02}/{:02}", t.year, t.month, t.day),
        TimeOrder::DmyHms => format!("{:02}/{:02}/{}", t.day, t.month, t.year),
    };
    let time_str = format_time_fields(t.hour, t.min, t.sec, &config.time_format);
    format!("{} {}", time_str, date_str)
}

fn format_time_fields(hour: u8, min: u8, sec: u8, fmt: &str) -> String {
    fmt
        .replace("%I", &format!("{:02}", if hour % 12 == 0 { 12 } else { hour % 12 }))
        .replace("%H", &format!("{:02}", hour))
        .replace("%M", &format!("{:02}", min))
        .replace("%S", &format!("{:02}", sec))
        .replace("%p", if hour < 12 { "AM" } else { "PM" })
        .replace("%P", if hour < 12 { "am" } else { "pm" })
}

// ============================================================================
// Theme
// ============================================================================

/// Strip `/* ... */` block comments from CSS source.
fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut chars = css.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            while let Some(c) = chars.next() {
                if c == '*' && chars.peek() == Some(&'/') { chars.next(); break; }
            }
        } else { out.push(c); }
    }
    out
}

/// Normalise property names — accept common aliases so old themes written
/// with `x`/`y`, `text-color`, `hover-*` prefixes still work.
fn normalize_prop(key: &str) -> &str {
    match key {
        "x"                      => "left",
        "y"                      => "top",
        "x-offset"               => "offset-x",
        "y-offset"               => "offset-y",
        "text-color"             => "color",
        "hover-text-color"       => "color-hover",
        "hover-background-color" => "background-color-hover",
        other                    => other,
    }
}

/// Strip surrounding quotes / `url(...)` wrapper from a raw CSS value.
fn clean_value(s: &str) -> String {
    let s = s.trim();
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        return s[1..s.len()-1].to_string();
    }
    if let Some(inner) = s.strip_prefix("url(").and_then(|t| t.strip_suffix(')')) {
        return inner.trim().trim_matches(|c| c == '"' || c == '\'').to_string();
    }
    s.to_string()
}

/// Replace every `var(--name)` in `val` with the corresponding entry from `vars`.
/// Handles up to 8 levels of chaining (e.g. `--a: var(--b); --b: red`).
fn resolve_vars(val: &str, vars: &HashMap<String, String>) -> String {
    if !val.contains("var(") { return val.to_string(); }
    let mut result = val.to_string();
    for _ in 0..8 {
        if !result.contains("var(") { break; }
        let mut next = String::with_capacity(result.len());
        let mut rest = result.as_str();
        while let Some(start) = rest.find("var(--") {
            next.push_str(&rest[..start]);
            let after = start + 4; // skip "var("
            match rest[after..].find(')') {
                Some(end) => {
                    let name = &rest[after..after + end];
                    match vars.get(name) {
                        Some(v) => next.push_str(v),
                        None    => next.push_str(&rest[start..after + end + 1]),
                    }
                    rest = &rest[after + end + 1..];
                }
                None => { next.push_str(&rest[start..]); rest = ""; }
            }
        }
        next.push_str(rest);
        if next == result { break; }
        result = next;
    }
    result
}

/// HSL (all components 0..1) → (r, g, b) bytes.
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    fn hue(p: f32, q: f32, mut t: f32) -> f32 {
        if t < 0.0 { t += 1.0; } else if t > 1.0 { t -= 1.0; }
        if t < 1.0/6.0 { return p + (q-p)*6.0*t; }
        if t < 0.5     { return q; }
        if t < 2.0/3.0 { return p + (q-p)*(2.0/3.0-t)*6.0; }
        p
    }
    if s == 0.0 { let v = (l*255.0).round() as u8; return (v, v, v); }
    let q = if l < 0.5 { l*(1.0+s) } else { l+s-l*s };
    let p = 2.0*l - q;
    (
        (hue(p, q, h+1.0/3.0)*255.0).round() as u8,
        (hue(p, q, h        )*255.0).round() as u8,
        (hue(p, q, h-1.0/3.0)*255.0).round() as u8,
    )
}

/// CSS named colours (UI-relevant subset).
fn css_named_color(s: &str) -> Option<eframe::egui::Color32> {
    use eframe::egui::Color32;
    Some(match s {
        "black"                   => Color32::BLACK,
        "white"                   => Color32::WHITE,
        "transparent"             => Color32::TRANSPARENT,
        "red"                     => Color32::from_rgb(255,   0,   0),
        "green"                   => Color32::from_rgb(  0, 128,   0),
        "lime"                    => Color32::from_rgb(  0, 255,   0),
        "blue"                    => Color32::from_rgb(  0,   0, 255),
        "yellow"                  => Color32::from_rgb(255, 255,   0),
        "cyan"  | "aqua"          => Color32::from_rgb(  0, 255, 255),
        "magenta" | "fuchsia"     => Color32::from_rgb(255,   0, 255),
        "orange"                  => Color32::from_rgb(255, 165,   0),
        "purple"                  => Color32::from_rgb(128,   0, 128),
        "pink"                    => Color32::from_rgb(255, 192, 203),
        "gray"  | "grey"          => Color32::from_rgb(128, 128, 128),
        "darkgray"  | "darkgrey"  => Color32::from_rgb(169, 169, 169),
        "lightgray" | "lightgrey" => Color32::from_rgb(211, 211, 211),
        "silver"                  => Color32::from_rgb(192, 192, 192),
        "navy"                    => Color32::from_rgb(  0,   0, 128),
        "teal"                    => Color32::from_rgb(  0, 128, 128),
        "maroon"                  => Color32::from_rgb(128,   0,   0),
        "olive"                   => Color32::from_rgb(128, 128,   0),
        _                         => return None,
    })
}

#[derive(Clone)]
pub struct Theme {
    styles: HashMap<String, HashMap<String, String>>,
}

impl Theme {
    pub fn load_or_create() -> Theme {
        match Self::try_load() {
            Ok(t)  => t,
            Err(e) => { eprintln!("Failed to load theme: {}", e); Self::parse_css(DEFAULT_THEME) }
        }
    }

    fn try_load() -> Result<Theme, Box<dyn Error>> {
        let path = crate::paths::place_config_file("tusk-launcher/theme.css")?;
        if !path.exists() {
            OpenOptions::new().write(true).create(true).open(&path)?.write_all(DEFAULT_THEME.as_bytes())?;
        }
        Ok(Self::parse_css(&read_to_string(&path)?))
    }

    fn parse_css(css: &str) -> Theme {
        let cleaned = strip_comments(css);

        // ── Pass 1: collect CSS custom properties (--name: value) ─────────────
        // Scoped globally; `:root {}` is the canonical location but any block works.
        let mut vars: HashMap<String, String> = HashMap::new();
        {
            let mut rest = cleaned.as_str();
            while let Some(open) = rest.find('{') {
                let inner = open + 1;
                match rest[inner..].find('}') {
                    None        => break,
                    Some(close) => {
                        for decl in rest[inner..inner+close].split(';') {
                            if let Some((k, v)) = decl.trim().split_once(':') {
                                let k = k.trim().to_lowercase();
                                if k.starts_with("--") { vars.insert(k, clean_value(v)); }
                            }
                        }
                        rest = &rest[inner + close + 1..];
                    }
                }
            }
        }

        // ── Pass 2: parse rule blocks into class-name → props map ─────────────
        let mut styles: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut rest = cleaned.as_str();
        while !rest.is_empty() {
            let Some(open) = rest.find('{') else { break };
            let selector = rest[..open].trim().to_lowercase();
            let inner = open + 1;
            let Some(close) = rest[inner..].find('}') else { break };
            let block = &rest[inner..inner + close];
            rest = &rest[inner + close + 1..];

            // Skip :root and @-rules (vars already handled above)
            if selector == ":root" || selector.starts_with('@') { continue; }

            // Strip leading '.' → ".app-button:hover" becomes "app-button:hover"
            let class = selector.trim_start_matches('.').trim().to_string();
            if class.is_empty() { continue; }

            let entry = styles.entry(class).or_default();
            for decl in block.split(';') {
                let decl = decl.trim();
                if decl.is_empty() { continue; }
                if let Some((k, v)) = decl.split_once(':') {
                    let k = k.trim().to_lowercase();
                    if k.is_empty() || k.starts_with("--") { continue; }
                    let v = resolve_vars(&clean_value(v), &vars);
                    entry.insert(normalize_prop(&k).to_string(), v);
                }
            }
        }

        Theme { styles }
    }

    fn get(&self, class: &str, prop: &str) -> Option<String> {
        self.styles.get(class)?.get(&prop.to_lowercase()).cloned()
    }

    fn parse_color(&self, s: &str) -> Option<eframe::egui::Color32> {
        use eframe::egui::Color32;
        let s = s.trim().to_lowercase();
        if s == "transparent" { return Some(Color32::TRANSPARENT); }

        // rgb() / rgba()
        if s.starts_with("rgba(") || s.starts_with("rgb(") {
            let is_rgba = s.starts_with("rgba(");
            let inner   = s.strip_prefix(if is_rgba {"rgba("} else {"rgb("})?.strip_suffix(')')?;
            let p: Vec<&str> = inner.split(',').map(str::trim).collect();
            if p.len() < 3 { return None; }
            let r: u8  = p[0].parse().ok()?;
            let g: u8  = p[1].parse().ok()?;
            let b: u8  = p[2].parse().ok()?;
            let a: f32 = if is_rgba && p.len() == 4 { p[3].parse().ok()? } else { 1.0 };
            return Some(Color32::from_rgba_unmultiplied(r, g, b, (a*255.0) as u8));
        }

        // hsl() / hsla()
        if s.starts_with("hsla(") || s.starts_with("hsl(") {
            let is_hsla = s.starts_with("hsla(");
            let inner   = s.strip_prefix(if is_hsla {"hsla("} else {"hsl("})?.strip_suffix(')')?;
            let p: Vec<&str> = inner.split(',').map(str::trim).collect();
            if p.len() < 3 { return None; }
            let h = p[0].trim_end_matches("deg").parse::<f32>().ok()? / 360.0;
            let s = p[1].trim_end_matches('%').parse::<f32>().ok()? / 100.0;
            let l = p[2].trim_end_matches('%').parse::<f32>().ok()? / 100.0;
            let a: f32 = if is_hsla && p.len() == 4 { p[3].parse().ok()? } else { 1.0 };
            let (r, g, b) = hsl_to_rgb(h, s, l);
            return Some(Color32::from_rgba_unmultiplied(r, g, b, (a*255.0) as u8));
        }

        // #hex (3, 4, 6, 8 digit)
        if s.starts_with('#') {
            let h = s.trim_start_matches('#');
            return match h.len() {
                3 => Some(Color32::from_rgb(
                    u8::from_str_radix(&h[0..1].repeat(2), 16).ok()?,
                    u8::from_str_radix(&h[1..2].repeat(2), 16).ok()?,
                    u8::from_str_radix(&h[2..3].repeat(2), 16).ok()?,
                )),
                4 => Some(Color32::from_rgba_unmultiplied(
                    u8::from_str_radix(&h[0..1].repeat(2), 16).ok()?,
                    u8::from_str_radix(&h[1..2].repeat(2), 16).ok()?,
                    u8::from_str_radix(&h[2..3].repeat(2), 16).ok()?,
                    u8::from_str_radix(&h[3..4].repeat(2), 16).ok()?,
                )),
                6 => Some(Color32::from_rgb(
                    u8::from_str_radix(&h[0..2], 16).ok()?,
                    u8::from_str_radix(&h[2..4], 16).ok()?,
                    u8::from_str_radix(&h[4..6], 16).ok()?,
                )),
                8 => Some(Color32::from_rgba_unmultiplied(
                    u8::from_str_radix(&h[0..2], 16).ok()?,
                    u8::from_str_radix(&h[2..4], 16).ok()?,
                    u8::from_str_radix(&h[4..6], 16).ok()?,
                    u8::from_str_radix(&h[6..8], 16).ok()?,
                )),
                _ => None,
            };
        }

        css_named_color(&s)
    }

    fn get_px(&self, class: &str, prop: &str) -> Option<f32> {
        self.get(class, prop)?.trim_end_matches("px").parse().ok()
    }

    fn get_order(&self, sec: &str) -> i32 {
        self.get(sec, "order").and_then(|s| s.parse().ok()).unwrap_or(0)
    }

    fn get_position(&self, class: &str) -> Option<(f32, f32)> {
        // normalize_prop maps x→left and y→top at parse time.
        Some((self.get_px(class, "left")?, self.get_px(class, "top")?))
    }

    pub fn get_config(&self) -> Config {
        let mut config = Config::default();
        if let Some(props) = self.styles.get("config") {
            macro_rules! set {
                ($key:expr, $field:ident, $typ:ty) => {
                    if let Some(val) = props.get($key) {
                        if let Ok(parsed) = val.parse::<$typ>() { config.$field = parsed; }
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
            for (key, field) in [
                ("power-commands",   &mut config.power_commands),
                ("restart-commands", &mut config.restart_commands),
                ("logout-commands",  &mut config.logout_commands),
            ] {
                if let Some(val) = props.get(key) {
                    *field = val.split(',').map(|s| s.trim().to_string()).collect();
                }
            }
        }
        config
    }

    fn get_frame_props(&self, class: &str, default: eframe::egui::Color32)
        -> (eframe::egui::Color32, Option<eframe::egui::Color32>, eframe::egui::CornerRadius)
    {
        let base  = self.get(class, "background-color")
                        .and_then(|s| self.parse_color(&s)).unwrap_or(default);
        // :hover pseudo-class block takes precedence over the legacy -hover suffix.
        let hover = self.get(&format!("{}:hover", class), "background-color")
                        .or_else(|| self.get(class, "background-color-hover"))
                        .and_then(|s| self.parse_color(&s));
        let round = self.get(class, "border-radius")
                        .and_then(|s| s.replace("px", "").trim().parse::<f32>().ok())
                        .map(|v| eframe::egui::CornerRadius::same(v as u8))
                        .unwrap_or_default();
        (base, hover, round)
    }

    pub fn apply_style(&self, ui: &mut eframe::egui::Ui, class: &str) {
        let style = ui.style_mut();
        if let Some(bg) = self.get(class, "background-color").and_then(|s| self.parse_color(&s)) {
            if bg != eframe::egui::Color32::TRANSPARENT { style.visuals.panel_fill = bg; }
        }
        if let Some(tc) = self.get(class, "color").and_then(|s| self.parse_color(&s)) {
            style.visuals.override_text_color = Some(tc);
        }
        if let Some(pad) = self.get_px(class, "padding") {
            style.spacing.item_spacing  = eframe::egui::vec2(pad, pad);
            style.spacing.window_margin = eframe::egui::Margin::symmetric(pad as i8, pad as i8);
        }
        if let Some(rad) = self.get_px(class, "border-radius") {
            let r = eframe::egui::CornerRadius::same(rad as u8);
            for w in [&mut style.visuals.widgets.noninteractive, &mut style.visuals.widgets.inactive,
                      &mut style.visuals.widgets.hovered,        &mut style.visuals.widgets.active] {
                w.corner_radius = r;
            }
        }
        if let Some(sz) = self.get_px(class, "font-size") {
            if let Some(text) = style.text_styles.get_mut(&eframe::egui::TextStyle::Body) { text.size = sz; }
        }
    }

    pub fn apply_widget_style(&self, style: &mut eframe::egui::Style, class: &str) {
        if let Some(bg) = self.get(class, "background-color").and_then(|s| self.parse_color(&s)) {
            let hover = self.get(&format!("{}:hover", class), "background-color")
                            .or_else(|| self.get(class, "background-color-hover"))
                            .and_then(|s| self.parse_color(&s))
                            .unwrap_or(bg);
            set_widget_bg(style, bg, hover);
        }
        if let Some(tc) = self.get(class, "color").and_then(|s| self.parse_color(&s)) {
            style.visuals.override_text_color = Some(tc);
        }
    }

    fn get_text_color(&self, class: &str, hovered: bool) -> Option<eframe::egui::Color32> {
        if hovered {
            // :hover pseudo-class block first, then legacy color-hover suffix.
            self.get(&format!("{}:hover", class), "color")
                .or_else(|| self.get(class, "color-hover"))
                .or_else(|| self.get(class, "color"))
                .and_then(|s| self.parse_color(&s))
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

/// Truncate `text` to fit within `max_w` pixels using the given font, appending `…`.
fn truncate_text(ui: &eframe::egui::Ui, text: &str, font_id: &eframe::egui::FontId, max_w: f32) -> String {
    let measure = |s: &str| -> f32 {
        ui.painter().layout_no_wrap(s.to_owned(), font_id.clone(), eframe::egui::Color32::WHITE).size().x
    };
    if measure(text) <= max_w { return text.to_owned(); }
    let ew = measure("…");
    let limit = (max_w - ew).max(0.0);
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi + 1) / 2;
        let s: String = chars[..mid].iter().collect();
        if measure(&s) <= limit { lo = mid; } else { hi = mid - 1; }
    }
    format!("{}…", chars[..lo].iter().collect::<String>())
}

fn custom_button_width(
    ui: &mut eframe::egui::Ui,
    label: &str,
    class: &str,
    theme: &Theme,
    min_width: Option<f32>,
) -> eframe::egui::Response {
    custom_button_scroll(ui, label, class, theme, min_width, None)
}

/// Render a themed button.  When `scroll_offset` is `Some(x)`, the full label
/// is drawn clipped and shifted left by `x` pixels (marquee scroll on hover).
/// When `None`, long text is truncated with `…`.
fn custom_button_scroll(
    ui: &mut eframe::egui::Ui,
    label: &str,
    class: &str,
    theme: &Theme,
    min_width: Option<f32>,
    scroll_offset: Option<f32>,
) -> eframe::egui::Response {
    let font_id   = ui.style().text_styles.get(&eframe::egui::TextStyle::Button).cloned().unwrap_or_default();
    let pad       = ui.spacing().button_padding;
    let full_size = ui.painter().layout_no_wrap(label.to_owned(), font_id.clone(), eframe::egui::Color32::WHITE).size();
    let w         = min_width.unwrap_or(full_size.x + pad.x * 2.0);
    let h         = full_size.y + pad.y * 2.0;
    let (rect, resp) = ui.allocate_exact_size(eframe::egui::vec2(w, h), eframe::egui::Sense::click_and_drag());

    if ui.is_rect_visible(rect) {
        let (base, hover_opt, round) = theme.get_frame_props(class, ui.style().visuals.widgets.inactive.bg_fill);
        let normal_tc = theme.get(class, "color").and_then(|s| theme.parse_color(&s)).unwrap_or(eframe::egui::Color32::WHITE);
        let hover_tc  = theme.get(&format!("{}:hover", class), "color")
                            .or_else(|| theme.get(class, "color-hover"))
                            .and_then(|s| theme.parse_color(&s))
                            .unwrap_or(normal_tc);
        let bg = if resp.hovered() { hover_opt.unwrap_or(base) } else { base };
        let tc = if resp.hovered() { hover_tc } else { normal_tc };

        let avail_text_w = (w - pad.x * 2.0).max(0.0);

        // Background rect: text-content width rather than full row width.
        // When scrolling we cap it to avail_text_w (the visible window).
        let bg_text_w = match scroll_offset {
            Some(_) => avail_text_w,
            None    => {
                let display = truncate_text(ui, label, &font_id, avail_text_w);
                ui.painter().layout_no_wrap(display, font_id.clone(), eframe::egui::Color32::WHITE).size().x
            }
        };
        let bg_rect = eframe::egui::Rect::from_min_size(
            rect.min,
            eframe::egui::vec2((bg_text_w + pad.x * 2.0).min(w), h),
        );
        ui.painter().rect_filled(bg_rect, round, bg);

        match scroll_offset {
            Some(offset) => {
                let clip = eframe::egui::Rect::from_min_size(
                    eframe::egui::pos2(rect.min.x + pad.x, rect.min.y),
                    eframe::egui::vec2(avail_text_w, h),
                );
                ui.painter().with_clip_rect(clip).text(
                    eframe::egui::pos2(clip.min.x - offset, rect.center().y),
                    eframe::egui::Align2::LEFT_CENTER,
                    label, font_id, tc,
                );
            }
            None => {
                let display = truncate_text(ui, label, &font_id, avail_text_w);
                ui.painter().text(
                    eframe::egui::pos2(rect.min.x + pad.x, rect.center().y),
                    eframe::egui::Align2::LEFT_CENTER,
                    &display, font_id, tc,
                );
            }
        }
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

/// Build a ViewportId for a tray menu popup.
fn tray_menu_vp_id(icon_id: &str) -> eframe::egui::ViewportId {
    eframe::egui::ViewportId::from_hash_of(format!("tray_menu_{icon_id}"))
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
// LayoutCache
// ============================================================================

#[derive(Clone, Copy)]
enum ElemKind { Settings, Icon, App }

struct BgImage { path: String, size_mode: String, opacity: f32 }

struct SectionInfo { name: &'static str, pos: Option<(f32, f32)>, size: Option<eframe::egui::Vec2> }

struct LayoutCache {
    win_size:             eframe::egui::Vec2,
    win_bg:               eframe::egui::Color32,
    bg_image:             Option<BgImage>,
    sections:             Vec<SectionInfo>,
    elem_order:           Vec<ElemKind>,
    settings_w:           f32,
    settings_h:           f32,
    settings_ox:          f32,
    settings_oy:          f32,
    icon_w:               f32,
    icon_h:               f32,
    vol_gap:              Option<f32>,
    env_w:                f32,
    env_h:                f32,
    tray_w:               f32,
    tray_h:               f32,
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
            .and_then(|s| theme.parse_color(&s)).unwrap_or(egui::Color32::BLACK);

        let bg_image = theme.get("main-window", "background-image")
            .filter(|s| !s.is_empty())
            .and_then(|img| resolve_icon_path("main-window", &img, config))
            .map(|path| BgImage {
                size_mode: theme.get("main-window", "background-size").unwrap_or_else(|| "stretch".into()),
                opacity:   theme.get("main-window", "background-opacity").and_then(|s| s.parse().ok()).unwrap_or(1.0),
                path,
            });

        let mut raw: Vec<(&'static str, i32)> = vec![
            ("search-bar", theme.get_order("search-bar")),
            ("app-list",   theme.get_order("app-list")),
        ];
        if config.enable_audio_control { raw.push(("volume-slider", theme.get_order("volume-slider"))); }
        if config.show_time            { raw.push(("time-display",   theme.get_order("time-display"))); }
        if config.enable_power_options { raw.push(("power-button",   theme.get_order("power-button"))); }
        if config.enable_system_tray   { raw.push(("tray-icon",      theme.get_order("tray-icon"))); }
        raw.sort_by_key(|(_, o)| *o);

        let sections = raw.into_iter().map(|(name, _)| SectionInfo {
            pos:  theme.get_position(name),
            size: if matches!(name, "search-bar" | "app-list") {
                theme.get_px(name, "width").zip(theme.get_px(name, "height")).map(|(w, h)| egui::vec2(w, h))
            } else { None },
            name,
        }).collect();

        let mut elems: Vec<(i32, ElemKind)> = vec![
            (theme.get("settings-button", "order").and_then(|s| s.parse().ok()).unwrap_or(0), ElemKind::Settings),
            (theme.get("app-icon",        "order").and_then(|s| s.parse().ok()).unwrap_or(1), ElemKind::Icon),
            (theme.get("app-button",      "order").and_then(|s| s.parse().ok()).unwrap_or(2), ElemKind::App),
        ];
        elems.sort_by_key(|(o, _)| *o);

        let tray_indicator_color = theme.get("tray-icon", "indicator-color")
            .and_then(|s| theme.parse_color(&s))
            .unwrap_or(egui::Color32::from_rgb(94, 206, 135));

        let win_w = theme.get_px("main-window", "width").unwrap_or(220.0);

        LayoutCache {
            win_size,
            win_bg,
            bg_image,
            sections,
            elem_order:  elems.into_iter().map(|(_, k)| k).collect(),
            settings_w:  theme.get_px("settings-button", "width").unwrap_or(22.0),
            settings_h:  theme.get_px("settings-button", "height").unwrap_or(22.0),
            settings_ox: theme.get_px("settings-button", "offset-x").unwrap_or(0.0),
            settings_oy: theme.get_px("settings-button", "offset-y").unwrap_or(0.0),
            icon_w:      theme.get_px("app-icon", "width").unwrap_or(22.0),
            icon_h:      theme.get_px("app-icon", "height").unwrap_or(22.0),
            vol_gap:     theme.get_px("volume-slider", "gap"),
            env_w:       theme.get_px("env-input", "width").unwrap_or(300.0),
            env_h:       theme.get_px("env-input", "height").unwrap_or(150.0),
            tray_w:      theme.get_px("tray-icon", "width").unwrap_or(win_w - 24.0),
            tray_h:      theme.get_px("tray-icon", "height").unwrap_or(18.0),
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

        let audio    = crate::system::AudioController::new(&cfg)?;
        audio.start_polling(&cfg);
        let sni_host = crate::sni::SniHost::new(&cfg);

        eframe::run_native(
            "Application Launcher",
            eframe::NativeOptions {
                viewport,
                renderer: eframe::Renderer::Wgpu,
                ..Default::default()
            },
            Box::new(move |cc| {
                if let Some(s) = theme.get("env-input", "scaling").and_then(|s| s.parse::<f32>().ok()) {
                    cc.egui_ctx.set_pixels_per_point(s);
                }
                cc.egui_ctx.request_repaint();
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
                    // Key: icon.id (or "{id}_attn"). Value: (icon_rev, TextureHandle).
                    // Re-uploaded when icon_rev differs from stored rev.
                    tray_textures: HashMap::new(),
                    tray_name_cache: HashMap::new(),
                    tray_menu_open: None,
                    tray_menu_fetched: None,
                    scroll_offsets: HashMap::new(),
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
    icon_manager:     crate::app_launcher::IconManager,
    layout:           LayoutCache,
    cached_time:      String,
    last_time_update: Instant,
    theme:            Arc<Theme>,
    config:           Config,
    sni_host:         Option<crate::sni::SniHost>,
    /// (icon_rev, handle) — re-uploaded when rev changes.
    tray_textures:    HashMap<String, (u32, eframe::egui::TextureHandle)>,
    tray_name_cache:  HashMap<String, Option<String>>,
    tray_menu_open:    Option<String>,
    tray_menu_fetched: Option<String>,
    /// Per-app scroll offset for marquee text on hover (pixels from left).
    scroll_offsets:   HashMap<String, f32>,
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
                    let r = ui.add(eframe::egui::TextEdit::singleline(&mut query).hint_text("Search...").frame(eframe::egui::Frame::NONE));
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
                let vis = { let mut s = ui.style().visuals.widgets.inactive.clone(); s.bg_fill = base; s.corner_radius = round; s };
                with_custom_style(ui, |s| {
                    s.visuals.widgets.inactive        = vis.clone();
                    s.visuals.widgets.hovered.bg_fill = hover.unwrap_or(base);
                    s.visuals.widgets.hovered.weak_bg_fill = hover.unwrap_or(base);
                    s.visuals.widgets.active          = vis;
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
        let filtered: Vec<String> = if query.trim().is_empty() && !self.config.enable_recent_apps {
            Vec::new()
        } else {
            self.app.get_search_results().into_iter().take(self.config.max_search_results).collect()
        };

        ui.vertical(|ui| {
            for app_name in filtered {
                let _row_id = ui.id().with(&app_name);
                ui.horizontal(|ui| {
                    for &kind in &self.layout.elem_order {
                        match kind {
                            ElemKind::Settings if self.config.show_settings_button => {
                                let (w, h)   = (self.layout.settings_w, self.layout.settings_h);
                                let (ox, oy) = (self.layout.settings_ox, self.layout.settings_oy);
                                // Interact on the ALLOCATED rect only — translating the interact
                                // rect outside its allocation triggers egui's debug red box.
                                let (base_rect, resp) = ui.allocate_exact_size(
                                    eframe::egui::vec2(w, h),
                                    eframe::egui::Sense::click(),
                                );
                                // Offset is applied only to the PAINT position, not the layout rect.
                                let paint_center = base_rect.center() + eframe::egui::vec2(ox, oy);
                                // Don't call apply_style here — mutating panel_fill mid-row corrupts
                                // the clip state for the icon cell that follows.
                                let color = self.theme.get_text_color("settings-button", resp.hovered())
                                    .unwrap_or(eframe::egui::Color32::from_rgb(64, 64, 64));
                                let font = eframe::egui::TextStyle::Button.resolve(ui.style());
                                ui.painter().text(paint_center, eframe::egui::Align2::CENTER_CENTER, "⚙", font, color);
                                if resp.clicked() {
                                    self.editing_windows.insert(app_name.clone(), self.app.get_formatted_launch_options(&app_name));
                                }
                            }
                            ElemKind::Icon if self.config.enable_icons => {
                                // Always allocate icon space so every row has the same width,
                                // regardless of whether this particular app has an icon.
                                let (rect, _) = ui.allocate_exact_size(
                                    eframe::egui::vec2(self.layout.icon_w, self.layout.icon_h),
                                    eframe::egui::Sense::hover(),
                                );
                                if let Some(icon_path) = self.app.get_icon_path(&app_name) {
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
                                let btn_w = ui.available_width();
                                let font_id = ui.style().text_styles
                                    .get(&eframe::egui::TextStyle::Button).cloned().unwrap_or_default();
                                let pad = ui.spacing().button_padding;
                                let avail_text_w = (btn_w - pad.x * 2.0).max(0.0);
                                let full_text_w = ui.painter().layout_no_wrap(
                                    app_name.clone(), font_id, eframe::egui::Color32::WHITE,
                                ).size().x;
                                // Marquee on hover when text overflows; truncate with … otherwise.
                                let scroll_offset = if full_text_w > avail_text_w {
                                    let hover_rect = eframe::egui::Rect::from_min_size(
                                        ui.cursor().min, eframe::egui::vec2(btn_w, 22.0),
                                    );
                                    if ui.rect_contains_pointer(hover_rect) {
                                        let max_scroll = full_text_w - avail_text_w + 20.0;
                                        let off = self.scroll_offsets.entry(app_name.clone()).or_insert(-20.0);
                                        *off = (*off + 1.2).min(max_scroll);
                                        if *off >= max_scroll { *off = -20.0; } // loop
                                        ctx.request_repaint();
                                        Some(off.max(0.0))
                                    } else {
                                        self.scroll_offsets.remove(&app_name);
                                        None
                                    }
                                } else {
                                    self.scroll_offsets.remove(&app_name);
                                    None
                                };
                                let resp = custom_button_scroll(ui, &app_name, "app-button",
                                    &self.theme, Some(btn_w), scroll_offset);
                                if resp.clicked()           { self.app.launch_app(&app_name); }
                                if resp.secondary_clicked() {
                                    self.editing_windows.insert(app_name.clone(),
                                        self.app.get_formatted_launch_options(&app_name));
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
            ui.label(&self.cached_time);
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

        const ICON_SZ: f32 = 16.0;
        const GAP:     f32 = 3.0;
        let (tray_w, tray_h) = (self.layout.tray_w, self.layout.tray_h);
        let icon_size = egui::vec2(ICON_SZ, ICON_SZ);

        let (bg, _, round) = self.theme.get_frame_props("tray-icon", egui::Color32::TRANSPARENT);
        let strip_origin = ui.cursor().min;
        if bg != egui::Color32::TRANSPARENT {
            ui.painter().rect_filled(
                egui::Rect::from_min_size(strip_origin, egui::vec2(tray_w, tray_h)), round, bg,
            );
        }
        let (strip_rect, _) = ui.allocate_exact_size(egui::vec2(tray_w, tray_h), egui::Sense::hover());

        let icons: Vec<crate::sni::TrayIcon> = self.sni_host
            .as_ref()
            .and_then(|h| h.items.try_lock().ok())
            .map(|g| g.iter().filter(|i| i.status != crate::sni::TrayStatus::Passive).cloned().collect())
            .unwrap_or_default();

        if icons.is_empty() {
            let dot_r  = 3.0_f32;
            let center = egui::pos2(strip_rect.min.x + GAP + dot_r, strip_rect.center().y);
            ui.painter().circle_filled(center, dot_r, self.layout.tray_indicator_color);
            return;
        }

        let cy  = strip_rect.center().y;
        let mut x = strip_rect.min.x + GAP;

        for icon in &icons {
            let icon_rect = egui::Rect::from_min_size(egui::pos2(x, cy - ICON_SZ * 0.5), icon_size);
            x += ICON_SZ + GAP;

            let use_attn = icon.status == crate::sni::TrayStatus::NeedsAttention
                && (!icon.attention_icon_rgba.is_empty() || icon.attention_icon_name.is_some());

            let (tex_rgba, tex_w, tex_h, tex_name) = if use_attn {
                (&icon.attention_icon_rgba, icon.attention_icon_w, icon.attention_icon_h, &icon.attention_icon_name)
            } else {
                (&icon.icon_rgba, icon.icon_w, icon.icon_h, &icon.icon_name)
            };
            let tex_key = if use_attn { format!("{}_attn", icon.id) } else { icon.id.clone() };

            // Re-upload texture only when pixel data has changed (tracked via icon_rev).
            // NOTE: icon_rgba is already RGBA — argb_to_rgba() was called once in sni.rs.
            //       Do NOT convert again here.
            if tex_w > 0 && tex_h > 0 && !tex_rgba.is_empty() {
                let needs_upload = self.tray_textures.get(&tex_key)
                    .map(|(rev, _)| *rev != icon.icon_rev)
                    .unwrap_or(true);
                if needs_upload {
                    let img    = egui::ColorImage::from_rgba_unmultiplied([tex_w as usize, tex_h as usize], tex_rgba);
                    let handle = ctx.load_texture(&tex_key, img, egui::TextureOptions::LINEAR);
                    self.tray_textures.insert(tex_key.clone(), (icon.icon_rev, handle));
                }
            }

            if ui.is_rect_visible(icon_rect) {
                if let Some((_, tex)) = self.tray_textures.get(&tex_key) {
                    ui.painter().image(
                        tex.id(), icon_rect,
                        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                } else if let Some(name) = tex_name.as_deref().filter(|s| !s.is_empty()) {
                    let cache_key = format!("{}|{}", name, icon.icon_theme_path.as_deref().unwrap_or(""));
                    let resolved  = self.tray_name_cache
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

            let resp = ui.interact(icon_rect, ui.id().with(&icon.id), egui::Sense::click())
                .on_hover_text(&icon.tooltip_title);

            if resp.hovered() || self.tray_menu_open.as_deref() == Some(&icon.id) {
                ui.painter().rect_stroke(
                    icon_rect, 2.0,
                    egui::Stroke::new(1.0, egui::Color32::from_white_alpha(100)),
                    egui::StrokeKind::Middle,
                );
            }

            if resp.clicked() {
                if let Some(host) = &self.sni_host {
                    if icon.item_is_menu {
                        let pos = resp.interact_rect.center();
                        host.context_menu(&icon.bus_name, &icon.obj_path, pos.x as i32, pos.y as i32);
                    } else {
                        host.activate(&icon.bus_name, &icon.obj_path);
                    }
                }
                if let Some(old_id) = self.tray_menu_open.take() {
                    ctx.send_viewport_cmd_to(tray_menu_vp_id(&old_id), egui::ViewportCommand::Close);
                }
            }

            if resp.hovered() {
                let scroll = ui.input(|i| i.smooth_scroll_delta);
                if let Some(host) = &self.sni_host {
                    if scroll.y.abs() > 0.5 { host.scroll(&icon.bus_name, &icon.obj_path, scroll.y as i32, "vertical"); }
                    if scroll.x.abs() > 0.5 { host.scroll(&icon.bus_name, &icon.obj_path, scroll.x as i32, "horizontal"); }
                }
            }

            if resp.secondary_clicked() {
                if self.tray_menu_open.as_deref() == Some(&icon.id) {
                    ctx.send_viewport_cmd_to(tray_menu_vp_id(&icon.id), egui::ViewportCommand::Close);
                    self.tray_menu_open = None;
                } else {
                    if let Some(old_id) = self.tray_menu_open.take() {
                        ctx.send_viewport_cmd_to(tray_menu_vp_id(&old_id), egui::ViewportCommand::Close);
                    }
                    self.tray_menu_open    = Some(icon.id.clone());
                    self.tray_menu_fetched = None;
                    if let (Some(host), Some(menu_path)) = (&self.sni_host, &icon.menu_path) {
                        host.menu_about_to_show(&icon.bus_name, menu_path);
                    }
                }
            }

            if self.tray_menu_open.as_deref() == Some(&icon.id) {
                if self.tray_menu_fetched.as_deref() != Some(&icon.id) {
                    if let (Some(host), Some(menu_path)) = (&self.sni_host, &icon.menu_path) {
                        host.fetch_menu(&icon.bus_name, menu_path, &icon.id);
                    }
                    self.tray_menu_fetched = Some(icon.id.clone());
                }

                if icon.menu_path.is_some() {
                    let menu_items   = icon.menu_items.clone();
                    let menu_loaded  = icon.menu_loaded;
                    let icon_id      = icon.id.clone();
                    let bus_name     = icon.bus_name.clone();
                    let menu_path    = icon.menu_path.clone();
                    let indicator    = self.layout.tray_indicator_color;
                    let win_bg       = self.layout.win_bg;
                    let tooltip      = icon.tooltip_title.clone();
                    let action_key   = format!("tray_menu_action_{icon_id}");
                    let theme_menu   = Arc::clone(&self.theme);

                    if !menu_loaded { ctx.request_repaint(); }

                    let item_count = menu_items.iter().filter(|i| !i.is_separator).count();
                    let win_h      = (item_count as f32 * 28.0 + 32.0).clamp(60.0, 400.0);
                    let vp_id      = tray_menu_vp_id(&icon_id);
                    let viewport   = egui::ViewportBuilder::default()
                        .with_title(if tooltip.is_empty() { "Menu".into() } else { tooltip })
                        .with_inner_size([180.0_f32, win_h])
                        .with_resizable(false).with_transparent(true).with_always_on_top();

                    ctx.show_viewport_immediate(vp_id, viewport, move |ctx, _| {
                        let action_key = format!("tray_menu_action_{icon_id}");
                        #[allow(deprecated)]
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

                    let ak_id = egui::Id::new(&action_key);
                    if let Some(item_id) = ctx.data_mut(|d| d.get_temp::<i32>(ak_id)) {
                        if item_id >= 0 {
                            if let (Some(host), Some(mp)) = (&self.sni_host, &menu_path) {
                                host.menu_event(&bus_name, mp, item_id);
                            }
                        }
                        self.tray_menu_open = None;
                        ctx.data_mut(|d| d.remove::<i32>(ak_id));
                        ctx.send_viewport_cmd_to(vp_id, egui::ViewportCommand::Close);
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

// ============================================================================
// Tray icon name resolution
// ============================================================================

fn resolve_tray_icon_name(name: &str, app_theme_path: Option<&str>, config: &Config) -> Option<String> {
    if name.is_empty() { return None; }

    if name.starts_with('/') {
        if std::path::Path::new(name).exists() { return Some(name.to_string()); }
        for ext in &["png", "svg", "xpm"] {
            let p = format!("{name}.{ext}");
            if std::path::Path::new(&p).exists() { return Some(p); }
        }
    }

    const EXTS:   &[&str] = &["png", "svg", "xpm"];
    const SIZES:  &[&str] = &["256x256", "128x128", "64x64", "48x48", "32x32", "24x24", "22x22", "16x16", "scalable"];
    const CATS:   &[&str] = &["apps", "status", "devices", "actions", "categories", "emblems", "mimetypes", "places"];
    const THEMES: &[&str] = &[
        "hicolor", "Papirus", "Papirus-Dark", "Papirus-Light",
        "Adwaita", "breeze", "breeze-dark", "gnome", "locolor",
        "oxygen", "Tango", "elementary", "Humanity",
    ];

    // Also try suffix-stripped variants (e.g. "audio-volume-medium-panel" → "audio-volume-medium").
    let stripped = name.strip_suffix("-panel")
        .or_else(|| name.strip_suffix("-symbolic"))
        .or_else(|| name.strip_suffix("-rtl"))
        .or_else(|| name.strip_suffix("-ltr"));
    let candidates: Vec<&str> = std::iter::once(name).chain(stripped).collect();

    let mut base_dirs: Vec<std::path::PathBuf> = Vec::new();
    if let Some(p) = app_theme_path { base_dirs.push(p.into()); }
    if let Some(home) = std::env::var_os("HOME") {
        let h = std::path::Path::new(&home);
        base_dirs.push(h.join(".local/share/icons"));
        base_dirs.push(h.join(".icons"));
    }
    base_dirs.push("/usr/share/icons".into());
    base_dirs.push("/usr/local/share/icons".into());

    for candidate in &candidates {
        for base in &base_dirs {
            for theme in THEMES {
                for size in SIZES {
                    for cat in CATS {
                        for ext in EXTS {
                            let p = base.join(theme).join(size).join(cat).join(format!("{candidate}.{ext}"));
                            if p.exists() { return Some(p.to_string_lossy().into_owned()); }
                        }
                    }
                    for ext in EXTS {
                        let p = base.join(theme).join(size).join(format!("{candidate}.{ext}"));
                        if p.exists() { return Some(p.to_string_lossy().into_owned()); }
                    }
                }
            }
            for ext in EXTS {
                let p = base.join(format!("{candidate}.{ext}"));
                if p.exists() { return Some(p.to_string_lossy().into_owned()); }
            }
        }
        for ext in EXTS {
            let p = format!("/usr/share/pixmaps/{candidate}.{ext}");
            if std::path::Path::new(&p).exists() { return Some(p); }
        }
    }
    resolve_icon_path(name, name, config)
}

// ============================================================================
// DBusMenu rendering
// ============================================================================

/// Colors/styles parsed once per render_menu_items call, shared across all items.
struct MenuStyle {
    bg_normal:  eframe::egui::Color32,
    bg_hover:   eframe::egui::Color32,
    tc_normal:  eframe::egui::Color32,
    tc_disabled: eframe::egui::Color32,
    rounding:   eframe::egui::CornerRadius,
    font_id:    eframe::egui::FontId,
}

impl MenuStyle {
    fn from_theme(theme: &Theme, ui: &eframe::egui::Ui) -> Self {
        let bg_normal = theme.get("app-button", "background-color")
            .and_then(|s| theme.parse_color(&s))
            .unwrap_or(eframe::egui::Color32::from_rgb(122, 162, 247));
        let bg_hover = theme.get("app-button", "background-color-hover")
            .and_then(|s| theme.parse_color(&s)).unwrap_or(bg_normal);
        let tc_normal = theme.get("app-button", "color")
            .and_then(|s| theme.parse_color(&s)).unwrap_or(eframe::egui::Color32::WHITE);
        let tc_disabled = eframe::egui::Color32::from_rgba_unmultiplied(
            tc_normal.r(), tc_normal.g(), tc_normal.b(), 100,
        );
        let rounding = theme.get("app-button", "border-radius")
            .and_then(|s| s.replace("px", "").parse::<f32>().ok())
            .map(|v| eframe::egui::CornerRadius::same(v as u8))
            .unwrap_or_default();
        let font_id = ui.style().text_styles.get(&eframe::egui::TextStyle::Button).cloned().unwrap_or_default();
        MenuStyle { bg_normal, bg_hover, tc_normal, tc_disabled, rounding, font_id }
    }
}

fn render_menu_items(
    ui:        &mut eframe::egui::Ui,
    items:     &[crate::sni::MenuItem],
    indicator: eframe::egui::Color32,
    theme:     &Theme,
) -> Option<i32> {
    use eframe::egui;
    let style   = MenuStyle::from_theme(theme, ui);
    let mut clicked = None;

    for item in items {
        if item.is_separator { ui.separator(); continue; }
        if item.label.is_empty() { continue; }

        let avail_w = ui.available_width();

        if item.children.is_empty() {
            let galley = ui.painter().layout_no_wrap(item.label.clone(), style.font_id.clone(), egui::Color32::WHITE);
            let h      = galley.size().y + ui.spacing().button_padding.y * 2.0;
            let (rect, response) = ui.allocate_exact_size(egui::vec2(avail_w, h), egui::Sense::click());

            if ui.is_rect_visible(rect) {
                let hovered = response.hovered() && item.enabled;
                ui.painter().rect_filled(rect, style.rounding, if hovered { style.bg_hover } else { style.bg_normal });
                ui.painter().text(
                    egui::pos2(rect.min.x + ui.spacing().button_padding.x, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    &item.label, style.font_id.clone(),
                    if item.enabled { style.tc_normal } else { style.tc_disabled },
                );
            }
            if response.clicked() && item.enabled { clicked = Some(item.id); }
        } else {
            let open_key = egui::Id::new(("tray_submenu", &item.label, item.id));
            let is_open: bool = ui.ctx().data(|d| d.get_temp(open_key).unwrap_or(false));
            let header = format!("{} {}", if is_open { "▼" } else { "▶" }, item.label);
            let galley = ui.painter().layout_no_wrap(header.clone(), style.font_id.clone(), egui::Color32::WHITE);
            let h      = galley.size().y + ui.spacing().button_padding.y * 2.0;
            let (rect, response) = ui.allocate_exact_size(egui::vec2(avail_w, h), egui::Sense::click());

            if ui.is_rect_visible(rect) {
                ui.painter().rect_filled(rect, style.rounding, if response.hovered() { style.bg_hover } else { style.bg_normal });
                ui.painter().text(
                    egui::pos2(rect.min.x + ui.spacing().button_padding.x, rect.center().y),
                    egui::Align2::LEFT_CENTER, &header, style.font_id.clone(), style.tc_normal,
                );
            }
            if response.clicked() { ui.ctx().data_mut(|d| d.insert_temp(open_key, !is_open)); }

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

// ============================================================================
// eframe::App
// ============================================================================

impl eframe::App for EframeWrapper {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.app.update();

        if self.config.enable_audio_control {
            self.current_volume = self.audio_controller.get_volume();
        }

        if self.config.show_time && self.last_time_update.elapsed() >= Duration::from_secs(1) {
            self.cached_time      = self.app.get_time();
            self.last_time_update = Instant::now();
        }

        let (esc, enter) = ctx.input(|i| (
            i.key_pressed(eframe::egui::Key::Escape),
            i.key_pressed(eframe::egui::Key::Enter),
        ));

        let (w, h) = (self.layout.win_size.x, self.layout.win_size.y);
        let bg     = self.layout.win_bg;
        let rect   = eframe::egui::Rect::from_min_size(eframe::egui::pos2(0.0, 0.0), eframe::egui::vec2(w, h));

        eframe::egui::Area::new("main".into()).fixed_pos(eframe::egui::pos2(0.0, 0.0)).show(&ctx, |ui| {
            ui.set_min_size(eframe::egui::vec2(w, h));
            ui.set_max_size(eframe::egui::vec2(w, h));

            if let Some(ref bgi) = self.layout.bg_image {
                if let Some(tex) = self.icon_manager.get_texture(&ctx, &bgi.path) {
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
                        _ => (rect, eframe::egui::Rect::from_min_max(eframe::egui::Pos2::ZERO, eframe::egui::Pos2::new(1.0, 1.0))),
                    };
                    let tint = eframe::egui::Color32::from_white_alpha((bgi.opacity * 255.0) as u8);
                    ui.painter().image(tex.id(), draw_rect, uv, tint);
                } else {
                    ui.painter().rect_filled(rect, 0.0, bg);
                }
            } else {
                ui.painter().rect_filled(rect, 0.0, bg);
            }

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
                area.show(&ctx, |ui| {
                    if let Some(sz) = size { ui.set_min_size(sz); ui.set_max_size(sz); }
                    self.render_section(ui, name, &ctx);
                });
            }
        });

        // Editing windows (env-vars popup)
        let mut to_remove = Vec::new();

        for (app_name, opts) in self.editing_windows.iter() {
            let (win_bg, env_w, env_h) = (self.layout.win_bg, self.layout.env_w, self.layout.env_h);
            let app_clone   = app_name.clone();
            let opts_clone  = opts.clone();
            let theme_clone = Arc::clone(&self.theme);
            let vp_id       = eframe::egui::ViewportId::from_hash_of(format!("env_{app_name}"));
            let viewport    = eframe::egui::ViewportBuilder::default()
                .with_title(app_name.clone())
                .with_inner_size([env_w, env_h])
                .with_resizable(false).with_transparent(true).with_always_on_top();

            let mem_key    = format!("env_opts_{app_name}");
            let action_key = format!("env_action_{app_name}");

            let current_opts = ctx.data_mut(|d| {
                d.get_persisted::<String>(eframe::egui::Id::new(&mem_key))
                    .unwrap_or_else(|| opts_clone.clone())
            });

            if current_opts != opts_clone {
                let stored_app_key = format!("env_app_{app_name}");
                let stored_app = ctx.data_mut(|d| d.get_persisted::<String>(eframe::egui::Id::new(&stored_app_key)));
                if stored_app.as_ref() != Some(&app_clone) {
                    ctx.data_mut(|d| {
                        d.insert_persisted(eframe::egui::Id::new(&mem_key),        opts_clone.clone());
                        d.insert_persisted(eframe::egui::Id::new(&stored_app_key), app_clone.clone());
                    });
                }
            }

            ctx.show_viewport_immediate(vp_id, viewport, move |ctx, _| {
                let mem_key    = format!("env_opts_{app_clone}");
                let action_key = format!("env_action_{app_clone}");
                let mut opts = ctx.data_mut(|d| {
                    d.get_persisted::<String>(eframe::egui::Id::new(&mem_key))
                        .unwrap_or_else(|| opts_clone.clone())
                });
                #[allow(deprecated)]
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

            if let Some(action) = ctx.data_mut(|d| d.get_temp::<String>(eframe::egui::Id::new(&action_key))) {
                if action == "save" {
                    let final_opts = ctx.data_mut(|d| {
                        d.get_persisted::<String>(eframe::egui::Id::new(&mem_key)).unwrap_or_else(|| opts.clone())
                    });
                    self.app.handle_input(&format!("LAUNCH_OPTIONS:{}:{}", app_name, final_opts));
                }
                to_remove.push(app_name.clone());
                ctx.data_mut(|d| {
                    d.remove::<String>(eframe::egui::Id::new(&mem_key));
                    d.remove::<String>(eframe::egui::Id::new(&action_key));
                    d.remove::<String>(eframe::egui::Id::new(&format!("env_app_{app_name}")));
                });
                ctx.send_viewport_cmd_to(vp_id, eframe::egui::ViewportCommand::Close);
            }
        }
        for app_name in to_remove { self.editing_windows.remove(&app_name); }

        if esc   && self.editing_windows.is_empty() { self.app.handle_input("ESC"); }
        if enter && self.editing_windows.is_empty() { self.app.handle_input("ENTER"); }
        if self.app.should_quit() { ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Close); }
    }
}

pub fn load_theme() -> Arc<Theme> { Arc::new(Theme::load_or_create()) }
