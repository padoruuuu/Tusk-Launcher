use std::sync::Mutex;
use std::collections::{VecDeque, HashMap};
use std::fs;
use std::path::PathBuf;
use serde::{Serialize, Deserialize};
use once_cell::sync::Lazy;
use crate::app_launcher::AppLaunchOptions;

static CACHE_FILE: Lazy<PathBuf> = Lazy::new(|| PathBuf::from("app_cache.toml"));

#[derive(Serialize, Deserialize, Clone)]
pub struct AppCache {
    pub recent_apps: VecDeque<String>,
    pub launch_options: HashMap<String, AppLaunchOptions>,
}

impl Default for AppCache {
    fn default() -> Self {
        Self {
            recent_apps: VecDeque::new(),
            launch_options: HashMap::new(),
        }
    }
}

fn save_cache(cache: &AppCache) -> Result<(), Box<dyn std::error::Error>> {
    let toml_string = toml::to_string_pretty(cache)?;
    fs::write(&*CACHE_FILE, toml_string)?;
    Ok(())
}

pub static APP_CACHE: Lazy<Mutex<AppCache>> = Lazy::new(|| {
    if CACHE_FILE.exists() {
        let data = fs::read_to_string(&*CACHE_FILE).expect("Failed to read cache file");
        let cache: AppCache = toml::from_str(&data).expect("Failed to deserialize cache data");
        Mutex::new(cache)
    } else {
        Mutex::new(AppCache::default())
    }
});

pub fn update_recent_apps(app_name: &str, enable_recent_apps: bool) -> Result<(), Box<dyn std::error::Error>> {
    if !enable_recent_apps {
        return Ok(());
    }
    let mut cache = APP_CACHE.lock().map_err(|e| format!("Lock error: {:?}", e))?;
    cache.recent_apps.retain(|x| x != app_name);
    cache.recent_apps.push_front(app_name.to_string());
    if cache.recent_apps.len() > 10 {
        cache.recent_apps.pop_back();
    }
    let cache_data = cache.clone();
    drop(cache);
    save_cache(&cache_data)?;
    Ok(())
}

pub fn update_launch_options(app_name: &str, options: AppLaunchOptions) -> Result<(), Box<dyn std::error::Error>> {
    let mut cache = APP_CACHE.lock().map_err(|e| format!("Lock error: {:?}", e))?;
    cache.launch_options.insert(app_name.to_string(), options);
    let cache_data = cache.clone();
    drop(cache);
    save_cache(&cache_data)?;
    Ok(())
}

pub fn get_launch_options() -> HashMap<String, AppLaunchOptions> {
    APP_CACHE.lock()
        .map(|cache| cache.launch_options.clone())
        .unwrap_or_default()
}