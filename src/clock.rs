use chrono::prelude::*;
use std::time::SystemTime;

pub fn get_current_time() -> String {
    let datetime: DateTime<Local> = SystemTime::now().into();
    datetime.format("%I:%M %p %m/%d/%Y").to_string()
}
