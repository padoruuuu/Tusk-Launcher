use std::sync::Mutex;
use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use serde::{Serialize, Deserialize};
use once_cell::sync::Lazy;

static RECENT_APPS_FILE: Lazy<PathBuf> = Lazy::new(|| PathBuf::from("recent_apps.toml"));

#[derive(Serialize, Deserialize, Clone)]
pub struct RecentAppsCache {
    pub recent_apps: VecDeque<String>,
}

fn save_cache<T: Serialize>(file: &PathBuf, cache: &T) -> Result<(), Box<dyn std::error::Error>> {
    let toml_string = toml::to_string_pretty(cache)?;
    fs::write(file, toml_string)?;
    Ok(())
}

pub static RECENT_APPS_CACHE: Lazy<Mutex<RecentAppsCache>> = Lazy::new(|| {
    if RECENT_APPS_FILE.exists() {
        let data = fs::read_to_string(&*RECENT_APPS_FILE).expect("Failed to read recent apps file");
        let cache: RecentAppsCache = toml::from_str(&data).expect("Failed to deserialize recent apps data");
        Mutex::new(cache)
    } else {
        Mutex::new(RecentAppsCache { recent_apps: VecDeque::new() })
    }
});

pub fn update_cache(app_name: &str, enable_recent_apps: bool) -> Result<(), Box<dyn std::error::Error>> {
    if !enable_recent_apps {
        return Ok(());
    }
    let mut cache = RECENT_APPS_CACHE.lock().map_err(|e| format!("Lock error: {:?}", e))?;
    cache.recent_apps.retain(|x| x != app_name);
    cache.recent_apps.push_front(app_name.to_string());
    if cache.recent_apps.len() > 10 {
        cache.recent_apps.pop_back();
    }
    let cache_data = cache.clone();
    drop(cache);
    save_cache(&RECENT_APPS_FILE, &cache_data)?;
    Ok(())
}