use chrono::Local;
use crate::gui::{Config, format_datetime};

pub fn get_current_time(config: &Config) -> String {
    format_datetime(&Local::now(), config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gui::{Config, TimeOrder, format_datetime};

    #[test]
    fn test_get_current_time() {
        let default = Config::default();
        assert!(!get_current_time(&default).is_empty());

        let mut custom = Config::default();
        custom.time_format = "%H:%M:%S".into();
        custom.time_order = TimeOrder::YmdHms;
        assert!(!get_current_time(&custom).is_empty());
    }
}
