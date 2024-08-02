use std::collections::VecDeque;
use std::path::PathBuf;
use once_cell::sync::Lazy;
use serde::{Serialize, Deserialize};
use std::fs;
use bincode::{serialize, deserialize};

static RECENT_APPS_FILE: Lazy<PathBuf> = Lazy::new(|| PathBuf::from("recent_apps.bin"));

#[derive(Serialize, Deserialize, Clone)]
pub struct RecentAppsCache {
    recent_apps: VecDeque<String>,
}

impl RecentAppsCache {
    pub fn new() -> Self {
        if RECENT_APPS_FILE.exists() {
            let data = fs::read(&*RECENT_APPS_FILE).expect("Failed to read recent apps file");
            let recent_apps: VecDeque<String> = deserialize(&data).expect("Failed to deserialize recent apps data");
            Self { recent_apps }
        } else {
            Self { recent_apps: VecDeque::new() }
        }
    }

    pub fn get_recent_apps(&self) -> VecDeque<String> {
        self.recent_apps.clone()
    }

    pub fn add_recent_app(&mut self, app_name: &str) {
        self.recent_apps.retain(|x| x != app_name);
        self.recent_apps.push_front(app_name.to_string());
        if self.recent_apps.len() > 10 {
            self.recent_apps.pop_back();
        }
        self.save_cache();
    }

    fn save_cache(&self) {
        let data = serialize(self).expect("Failed to serialize recent apps data");
        fs::write(&*RECENT_APPS_FILE, data).expect("Failed to write recent apps file");
    }
}