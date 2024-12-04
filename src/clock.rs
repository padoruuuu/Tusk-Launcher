use chrono::prelude::*;
use std::time::SystemTime;
use crate::config::{Config, format_datetime};

// This function retrieves the current time using the configured format
pub fn get_current_time(config: &Config) -> String {
    let datetime: DateTime<Local> = SystemTime::now().into();
    format_datetime(&datetime, config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, TimeOrder};

    #[test]
    fn test_get_current_time() {
        // Test default configuration
        let config_default = Config::default();
        let time_str_default = get_current_time(&config_default);
        println!("Default time format: {}", time_str_default);

        // Test custom time format and order
        let mut config_custom = Config::default();
        config_custom.time_format = "%H:%M:%S".to_string();
        config_custom.time_order = TimeOrder::YmdHms;
        let time_str_custom = get_current_time(&config_custom);
        println!("Custom time format: {}", time_str_custom);
    }
}