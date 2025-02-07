use std::sync::Mutex;
use std::collections::{VecDeque, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use serde::{Serialize, Deserialize};
use once_cell::sync::Lazy;
use crate::app_launcher::AppLaunchOptions;
use crate::config::Config;

static CACHE_FILE: Lazy<PathBuf> = Lazy::new(|| {
let mut path = dirs::config_dir()
.unwrap_or_else(|| PathBuf::from("."))
.join("tusk-launcher");
fs::create_dir_all(&path).expect("Failed to create config directory");
path.push("app_cache.toml");
path
});

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

pub fn resolve_icon_path(icon_name: &str, config: &Config) -> Option<String> {
if icon_name.is_empty() || !config.enable_icons {
return None;
}

let icon_path = Path::new(icon_name);
if icon_path.is_absolute() {
    return Some(icon_name.to_string());
}

let cached_base = config.icon_cache_dir.join(icon_name);
let extensions = ["png", "svg", "xpm"];

for ext in &extensions {
    let cached_path = cached_base.with_extension(ext);
    if cached_path.exists() {
        return cached_path.to_str().map(|s| s.to_string());
    }
}

let icon_sizes = ["512x512", "256x256", "128x128", "64x64", "48x48", "32x32", "24x24", "16x16", "scalable"];
let categories = ["apps", "devices", "places", "mimetypes", "status", "actions"];

let user_flatpak_base = dirs::data_local_dir()
    .unwrap_or_else(|| PathBuf::from(".local/share"))
    .join("flatpak/exports/share/icons");
let system_flatpak_base = PathBuf::from("/var/lib/flatpak/exports/share/icons");

let system_icon_dirs = vec![
    PathBuf::from("/usr/share/icons"),
    PathBuf::from("/usr/local/share/icons"),
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from(".local/share"))
        .join("icons"),
    PathBuf::from("/usr/share/pixmaps"),
];

let icon_themes = ["hicolor", "Adwaita", "gnome", "breeze", "oxygen"];

let check_icon = |base_dir: &Path, theme: &str, size: &str, category: &str, icon: &str| -> Option<PathBuf> {
    let icon_dir = base_dir.join(theme).join(size).join(category);
    for ext in &extensions {
        let icon_path = icon_dir.join(format!("{}.{}", icon, ext));
        if icon_path.exists() {
            return Some(icon_path);
        }
    }
    None
};

let mut search_paths = Vec::new();
search_paths.push(user_flatpak_base);
search_paths.push(system_flatpak_base);
search_paths.extend(system_icon_dirs);

for base_dir in search_paths {
    for theme in &icon_themes {
        for size in &icon_sizes {
            for category in &categories {
                if let Some(path) = check_icon(&base_dir, theme, size, category, icon_name) {
                    fs::create_dir_all(&config.icon_cache_dir).ok()?;
                    let cached_path = config.icon_cache_dir.join(icon_name)
                        .with_extension(path.extension().unwrap_or_default());
                    if fs::copy(&path, &cached_path).is_ok() {
                        return cached_path.to_str().map(|s| s.to_string());
                    }
                }
            }
        }
    }
}

let pixmaps = Path::new("/usr/share/pixmaps");
for ext in &extensions {
    let path = pixmaps.join(icon_name).with_extension(ext);
    if path.exists() {
        fs::create_dir_all(&config.icon_cache_dir).ok()?;
        let cached_path = config.icon_cache_dir.join(icon_name).with_extension(ext);
        if fs::copy(&path, &cached_path).is_ok() {
            return cached_path.to_str().map(|s| s.to_string());
        }
    }
}

None

}