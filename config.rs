use std::fs;
use std::path::PathBuf;
use serde::{Serialize, Deserialize};
use once_cell::sync::Lazy;
use chrono_tz::Tz;
use chrono::{DateTime, Utc};

static CONFIG_FILE: Lazy<PathBuf> = Lazy::new(|| PathBuf::from("conf.conf"));

#[derive(Serialize, Deserialize, Clone)]
pub struct Config {
    pub enable_recent_apps: bool,
    pub max_search_results: usize,
    pub enable_power_options: bool,
    pub show_time: bool,
    pub timezone: Option<String>,  // New field for timezone
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enable_recent_apps: true,
            max_search_results: 5,
            enable_power_options: true,
            show_time: true,
            timezone: Some("UTC".to_string()),  // Default to UTC
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

// Function to get current time in the configured timezone
pub fn get_current_time_in_timezone(config: &Config) -> String {
    let now_utc: DateTime<Utc> = Utc::now();

    // Check if the config contains a valid timezone
    if let Some(tz_str) = &config.timezone {
        if let Ok(timezone) = tz_str.parse::<Tz>() {
            let now_in_tz = now_utc.with_timezone(&timezone);
            return now_in_tz.format("%Y-%m-%d %H:%M:%S").to_string();
        } else {
            eprintln!("Invalid timezone in config: {}", tz_str);
        }
    }

    // Fallback to UTC if the timezone is invalid or not provided
    now_utc.format("%Y-%m-%d %H:%M:%S").to_string()
}
