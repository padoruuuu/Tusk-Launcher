use chrono::prelude::*;
use chrono_tz::Tz;
use std::time::SystemTime;
use crate::config::Config; // Assuming config.rs defines the Config struct

// This function retrieves the current time based on the configured timezone
pub fn get_current_time(config: &Config) -> String {
    let now_utc: DateTime<Utc> = SystemTime::now().into();

    // Check if the config contains a valid timezone
    if let Some(tz_str) = &config.timezone {
        if let Ok(timezone) = tz_str.parse::<Tz>() {
            let now_in_tz = now_utc.with_timezone(&timezone);
            return now_in_tz.format("%I:%M %p %m/%d/%Y").to_string();
        } else {
            eprintln!("Invalid timezone in config: {}", tz_str);
        }
    }

    // Fallback to system local time if the timezone is invalid or not provided
    let datetime: DateTime<Local> = SystemTime::now().into();
    datetime.format("%I:%M %p %m/%d/%Y").to_string()
}

// Ensure the function is used, for example, by adding it in main or another relevant location
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn test_get_current_time() {
        let config = Config::default();
        let time_str = get_current_time(&config);
        println!("Current time: {}", time_str); // Example usage
    }
}
