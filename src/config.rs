use std::fs;
use std::path::PathBuf;
use serde::{Serialize, Deserialize};
use once_cell::sync::Lazy;
use chrono::{DateTime, Local};

static CONFIG_FILE: Lazy<PathBuf> = Lazy::new(|| {
    let mut path = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("tusk-launcher");
    fs::create_dir_all(&path).expect("Failed to create config directory");
    path.push("config.toml");
    path
});

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
}

#[derive(Serialize, Deserialize, Clone)]
pub enum TimeOrder {
    MdyHms,  // Month/Day/Year Hours:Minutes:Seconds
    YmdHms,  // Year/Month/Day Hours:Minutes:Seconds
    DmyHms,  // Day/Month/Year Hours:Minutes:Seconds
}

impl Default for Config {
    fn default() -> Self {
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
        }
    }
}

pub fn load_config() -> Config {
    if CONFIG_FILE.exists() {
        let content = fs::read_to_string(&*CONFIG_FILE).expect("Failed to read config file");
        toml::from_str(&content).unwrap_or_else(|_| {
            eprintln!("Failed to parse config file. Using default configuration.");
            Config::default()
        })
    } else {
        let config = Config::default();
        save_config(&config).expect("Failed to save default config");
        config
    }
}

pub fn save_config(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    let content = toml::to_string_pretty(config)?;
    fs::write(&*CONFIG_FILE, content)?;
    Ok(())
}

pub fn get_current_time_in_timezone(config: &Config) -> String {
    let datetime: DateTime<Local> = Local::now();
    format_datetime(&datetime, config)
}

pub fn format_datetime(datetime: &DateTime<Local>, config: &Config) -> String {
    let date_part = match config.time_order {
        TimeOrder::MdyHms => datetime.format("%m/%d/%Y").to_string(),
        TimeOrder::YmdHms => datetime.format("%Y/%m/%d").to_string(),
        TimeOrder::DmyHms => datetime.format("%d/%m/%Y").to_string(),
    };

    let time_part = datetime.format(&config.time_format).to_string();

    format!("{} {}", time_part, date_part)
}