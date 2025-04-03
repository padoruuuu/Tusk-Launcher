// merged.rs
//
// This merged file lovingly combines the functionalities from both cache.rs and app_launcher.rs.
// It preserves full functionality, features, and compatibility with the rest of the project.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    str::FromStr,
    sync::Mutex,
    time,
};

use once_cell::sync::Lazy;
use eframe::egui;
use image;
use resvg::{tiny_skia::Pixmap, usvg};
use xdg::BaseDirectories;
use serde::{Serialize, Deserialize};

//
// --------------- AppLaunchOptions & Its Implementations ---------------
//

// Define AppLaunchOptions used throughout the project.
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct AppLaunchOptions {
    pub custom_command: Option<String>,
    pub working_directory: Option<String>,
    pub environment_vars: HashMap<String, String>,
}

// Implement Display for AppLaunchOptions so it can be converted to a string for caching.
impl std::fmt::Display for AppLaunchOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
         let custom = self.custom_command.as_deref().unwrap_or("");
         let working = self.working_directory.as_deref().unwrap_or("");
         let env_str = self.environment_vars.iter()
             .map(|(k, v)| format!("{}={}", k, v))
             .collect::<Vec<_>>()
             .join(",");
         write!(f, "{}|{}|{}", custom, working, env_str)
    }
}

// Implement FromStr for AppLaunchOptions to parse from the cached string.
impl FromStr for AppLaunchOptions {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
         let parts: Vec<&str> = s.splitn(3, '|').collect();
         if parts.len() != 3 {
             return Err("Invalid format for AppLaunchOptions".to_string());
         }
         let custom = if parts[0].is_empty() { None } else { Some(parts[0].to_string()) };
         let working = if parts[1].is_empty() { None } else { Some(parts[1].to_string()) };
         let mut environment_vars = HashMap::new();
         if !parts[2].is_empty() {
             for entry in parts[2].split(',') {
                 if let Some((k, v)) = entry.split_once('=') {
                     environment_vars.insert(k.to_string(), v.to_string());
                 }
             }
         }
         Ok(AppLaunchOptions {
              custom_command: custom,
              working_directory: working,
              environment_vars,
         })
    }
}

//
// --------------- Cache Functionality ---------------
//

// Structures for application cache.
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct AppEntry {
    pub launch_options: Option<AppLaunchOptions>,
    /// Holds the full resolved icon path (or empty if not set)
    pub icon_directory: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct AppCache {
    /// Each entry is keyed by the application name.
    pub apps: Vec<(String, AppEntry)>,
}

// Structure for holding a cached icon texture.
#[derive(Default)]
pub struct IconCache {
    texture: Option<egui::TextureHandle>,
    last_modified: Option<time::SystemTime>,
}

pub struct IconManager {
    icon_textures: HashMap<String, IconCache>,
}

impl IconManager {
    pub fn new() -> Self {
        Self {
            icon_textures: HashMap::new(),
        }
    }

    /// Loads a texture from the provided icon path. It reloads the texture if the file has been modified.
    pub fn get_texture(
        &mut self,
        ctx: &egui::Context,
        icon_path: &str,
    ) -> Option<egui::TextureHandle> {
        let reload = self.icon_textures.get(icon_path).map_or(true, |cache| {
            fs::metadata(icon_path)
                .and_then(|m| m.modified().map(|mod_time| {
                    cache.last_modified.map_or(true, |lm| lm != mod_time)
                }))
                .unwrap_or(true)
        });
        if reload {
            let img = Self::load_image(icon_path).unwrap_or_else(|_| Self::create_placeholder());
            let tex = ctx.load_texture(icon_path, img, Default::default());
            self.icon_textures.insert(
                icon_path.to_owned(),
                IconCache {
                    texture: Some(tex.clone()),
                    last_modified: fs::metadata(icon_path).and_then(|m| m.modified()).ok(),
                },
            );
            Some(tex)
        } else {
            self.icon_textures
                .get(icon_path)
                .and_then(|cache| cache.texture.clone())
        }
    }

    fn load_image(path: &str) -> Result<egui::ColorImage, Box<dyn std::error::Error>> {
        if path.to_lowercase().ends_with(".svg") {
            let data = fs::read(path)?;
            let tree = usvg::Tree::from_data(&data, &usvg::Options::default())?;
            let size = tree.size().to_int_size();
            let mut pixmap =
                Pixmap::new(size.width(), size.height()).ok_or("Failed to create pixmap")?;
            resvg::render(&tree, usvg::Transform::default(), &mut pixmap.as_mut());
            Ok(egui::ColorImage::from_rgba_unmultiplied(
                [size.width() as usize, size.height() as usize],
                pixmap.data(),
            ))
        } else {
            let img = image::open(path)?.into_rgba8();
            Ok(egui::ColorImage::from_rgba_unmultiplied(
                [img.width() as usize, img.height() as usize],
                &img,
            ))
        }
    }

    fn create_placeholder() -> egui::ColorImage {
        egui::ColorImage::from_rgba_unmultiplied([16, 16], &[127u8; 16 * 16 * 4])
    }
}

// A static cache file path determined by XDG BaseDirectories.
static CACHE_FILE: Lazy<PathBuf> = Lazy::new(|| {
    let xdg = BaseDirectories::new()
        .map(|bd| bd.get_config_home().to_owned())
        .unwrap_or_else(|_| PathBuf::from("."));
    let mut path = xdg.join("tusk-launcher");
    fs::create_dir_all(&path).expect("Failed to create config directory");
    // Using our custom .txt file format
    path.push("app_cache.txt");
    path
});

// Helpers for escaping/unescaping strings in our custom text format.
fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
     .replace('\t', "\\t")
     .replace('\n', "\\n")
}

fn unescape(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                match next {
                    '\\' => result.push('\\'),
                    't' => result.push('\t'),
                    'n' => result.push('\n'),
                    other => {
                        result.push('\\');
                        result.push(other);
                    }
                }
            } else {
                result.push('\\');
            }
        } else {
            result.push(c);
        }
    }
    result
}

// Serializes the cache using our custom text format.
fn serialize_cache(cache: &AppCache) -> String {
    let mut s = String::from("APP_CACHE_V1\n");
    for (app_name, entry) in &cache.apps {
        let name_escaped = escape(app_name);
        let launch_options_escaped = entry.launch_options
            .as_ref()
            .map(|opts| escape(&opts.to_string()))
            .unwrap_or_default();
        let icon_escaped = entry.icon_directory
            .as_ref()
            .map(|s| escape(s))
            .unwrap_or_default();
        s.push_str(&format!("{}\t{}\t{}\n", name_escaped, launch_options_escaped, icon_escaped));
    }
    s
}

fn deserialize_cache(s: &str) -> Result<AppCache, Box<dyn std::error::Error>> {
    let mut lines = s.lines();
    let header = lines.next().ok_or("Empty cache file")?;
    if header != "APP_CACHE_V1" {
        return Err("Unsupported cache file version".into());
    }
    let mut cache = AppCache::default();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() != 3 {
            continue;
        }
        let app_name = unescape(parts[0]);
        let launch_options = if parts[1].is_empty() {
            None
        } else {
            Some(parts[1].parse()?)
        };
        let icon_directory = if parts[2].is_empty() {
            None
        } else {
            Some(unescape(parts[2]))
        };
        cache.apps.push((app_name, AppEntry { launch_options, icon_directory }));
    }
    Ok(cache)
}

fn save_cache(cache: &AppCache) -> Result<(), Box<dyn std::error::Error>> {
    let data = serialize_cache(cache);
    fs::write(&*CACHE_FILE, data)?;
    Ok(())
}

// Global application cache.
pub static APP_CACHE: Lazy<Mutex<AppCache>> = Lazy::new(|| {
    let cache = if CACHE_FILE.exists() {
        match fs::read_to_string(&*CACHE_FILE)
            .ok()
            .and_then(|s| deserialize_cache(&s).ok()) {
                Some(c) => c,
                None => AppCache::default(),
            }
    } else {
        AppCache::default()
    };
    Mutex::new(cache)
});

// Updates the recent apps list if enabled.
pub fn update_recent_apps(
    app_name: &str,
    enable_recent_apps: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if !enable_recent_apps {
        return Ok(());
    }
    let mut cache = APP_CACHE.lock().map_err(|e| format!("Lock error: {:?}", e))?;
    let existing_entry = cache.apps.iter()
        .position(|(name, _)| name == app_name)
        .map(|pos| cache.apps.remove(pos).1);
    let entry = existing_entry.unwrap_or_default();
    // Insert at the beginning to mark as most recent.
    cache.apps.insert(0, (app_name.to_owned(), entry));
    // Removed cache truncation so that all cached apps persist.
    save_cache(&cache)?;
    Ok(())
}

// Updates launch options for an app.
pub fn update_launch_options(
    app_name: &str,
    options: AppLaunchOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut cache = APP_CACHE.lock().map_err(|e| format!("Lock error: {:?}", e))?;
    let pos = cache.apps.iter().position(|(name, _)| name == app_name);
    if let Some(pos) = pos {
        cache.apps[pos].1.launch_options = Some(options);
    } else {
        let mut entry = AppEntry::default();
        entry.launch_options = Some(options);
        cache.apps.push((app_name.to_owned(), entry));
    }
    save_cache(&cache)?;
    Ok(())
}

// Retrieves launch options for all apps.
pub fn get_launch_options() -> HashMap<String, AppLaunchOptions> {
    APP_CACHE
        .lock()
        .map(|c| {
            c.apps.iter()
                .filter_map(|(name, entry)| {
                    entry.launch_options.clone().map(|options| (name.clone(), options))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Resolves an icon path for an application by searching XDG directories for the full icon file.
/// If found, updates the same app entry with the full path so that subsequent lookups need not re-search.
pub fn resolve_icon_path(app_name: &str, icon_name: &str, config: &crate::gui::Config) -> Option<String> {
    if icon_name.is_empty() || !config.enable_icons {
        return None;
    }
    {
        let cache = APP_CACHE.lock().ok()?;
        if let Some((_, entry)) = cache.apps.iter().find(|(key, _)| key == app_name) {
            if let Some(ref cached_icon) = entry.icon_directory {
                if Path::new(cached_icon).exists() {
                    return Some(cached_icon.clone());
                }
            }
        }
    }
    let xdg = BaseDirectories::new().ok()?;
    let mut search_paths = Vec::new();
    search_paths.push(xdg.get_data_home().join("flatpak/exports/share/icons"));
    for data_dir in xdg.get_data_dirs() {
        search_paths.push(data_dir.join("icons"));
    }
    search_paths.push(PathBuf::from("/var/lib/flatpak/exports/share/icons"));
    let mut pixmaps_paths: Vec<PathBuf> = xdg.get_data_dirs().into_iter().map(|dir| dir.join("pixmaps")).collect();
    search_paths.append(&mut pixmaps_paths);
    search_paths.push(PathBuf::from("/usr/share/pixmaps"));

    let icon_themes = ["hicolor", "Adwaita", "gnome", "breeze", "oxygen"];
    let icon_sizes = [
        "512x512", "256x256", "128x128", "64x64", "48x48", "32x32", "24x24", "16x16", "scalable",
    ];
    let categories = ["apps", "devices", "places", "mimetypes", "status", "actions"];
    let extensions = ["png", "svg", "xpm"];

    let check_icon = |base: &Path, theme: &str, size: &str, category: &str, icon: &str| -> Option<PathBuf> {
        let dir = base.join(theme).join(size).join(category);
        for ext in &extensions {
            let p = dir.join(format!("{}.{}", icon, ext));
            if p.exists() {
                return Some(p);
            }
        }
        None
    };

    let found = search_paths.iter().find_map(|base| {
        icon_themes.iter().find_map(|theme| {
            icon_sizes.iter().find_map(|size| {
                categories.iter().find_map(|cat| check_icon(base, theme, size, cat, icon_name))
            })
        })
    });

    if let Some(found_path) = found {
        if let Ok(mut cache) = APP_CACHE.lock() {
            if let Some(pos) = cache.apps.iter().position(|(key, _)| key == app_name) {
                cache.apps[pos].1.icon_directory = found_path.to_str().map(String::from);
            } else {
                let mut entry = AppEntry::default();
                entry.icon_directory = found_path.to_str().map(String::from);
                cache.apps.push((app_name.to_owned(), entry));
            }
            let _ = save_cache(&cache);
        }
        return found_path.to_str().map(String::from);
    }
    None
}

//
// --------------- App Launcher Functionality ---------------
//

// Note: This section assumes that modules like `gui`, `clock`, and `power` are defined elsewhere in your project.
use crate::gui::{AppInterface, Config};
use crate::clock::get_current_time;
use crate::power;

fn parse_desktop_entry(path: &PathBuf) -> Option<(String, String, String)> {
    let content = fs::read_to_string(path).ok()?;
    let (mut name, mut exec, mut icon, mut wm_class) = (None, None, None, None);
    for line in content.lines() {
        if let Some((k, v)) = line.split_once('=') {
            match k.trim() {
                "Name" if name.is_none() => name = Some(v.trim().to_string()),
                "Exec" if exec.is_none() => exec = Some(v.trim().to_string()),
                "Icon" if icon.is_none() => icon = Some(v.trim().to_string()),
                "StartupWMClass" if wm_class.is_none() => wm_class = Some(v.trim().to_string()),
                _ => {}
            }
        }
    }
    let name = name?;
    let mut exec = exec?;
    let icon = icon.unwrap_or_default();
    for ph in ["%f", "%F", "%u", "%U", "%c", "%k", "@@"] {
        exec = exec.replace(ph, "");
    }
    exec = exec.replace("%i", &format!("--icon {}", icon)).trim().to_string();
    if let Some(class) = wm_class {
        if !exec.contains("flatpak run") {
            exec.push_str(&format!(" --class {}", class));
        }
    }
    Some((name, exec, icon))
}

fn get_desktop_entries() -> Vec<(String, String, String)> {
    let mut entries = Vec::new();
    if let Ok(xdg) = BaseDirectories::new() {
        // Scan system data directories.
        for dir in xdg.get_data_dirs() {
            let apps_dir = dir.join("applications");
            if let Ok(read_dir) = fs::read_dir(&apps_dir) {
                for entry in read_dir.filter_map(Result::ok) {
                    if entry.path().extension().map_or(false, |ext| ext == "desktop") {
                        if let Some(e) = parse_desktop_entry(&entry.path()) {
                            entries.push(e);
                        }
                    }
                }
            }
        }
        // Add extra directories from the user's data home.
        let extra_dirs = vec![
            xdg.get_data_home().join("applications"),
            xdg.get_data_home().join("flatpak/exports/share/applications"),
            xdg.get_data_home().join("applications/steam"),
        ];
        for dir in extra_dirs {
            if let Ok(read_dir) = fs::read_dir(&dir) {
                for entry in read_dir.filter_map(Result::ok) {
                    if entry.path().extension().map_or(false, |ext| ext == "desktop") {
                        if let Some(e) = parse_desktop_entry(&entry.path()) {
                            entries.push(e);
                        }
                    }
                }
            }
        }
    }
    entries
}

fn search_applications(query: &str, apps: &[(String, String, String)], max_results: usize) -> Vec<(String, String, String)> {
    let query = query.to_lowercase();
    apps.iter()
        .filter(|(name, _, _)| name.to_lowercase().contains(&query))
        .take(max_results)
        .cloned()
        .collect()
}

fn launch_app(
    app_name: &str,
    exec_cmd: &str,
    options: &Option<AppLaunchOptions>,
    enable_recent_apps: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if enable_recent_apps {
        update_recent_apps(app_name, true)?;
    }
    let home_dir = std::env::var("HOME").map(PathBuf::from).map_err(|_| "No home directory")?;
    
    let (cmd, dir) = if let Some(opts) = options {
        let command = if let Some(custom_cmd) = &opts.custom_command {
            if custom_cmd.trim() == "%command%" {
                exec_cmd.to_string()
            } else if custom_cmd.contains("%command%") {
                custom_cmd.replace("%command%", exec_cmd)
            } else {
                format!("{} {}", custom_cmd, exec_cmd)
            }
        } else {
            exec_cmd.to_string()
        };
        (
            command,
            opts.working_directory.as_deref().unwrap_or_else(|| home_dir.to_str().unwrap_or("")),
        )
    } else {
        (exec_cmd.to_string(), home_dir.to_str().unwrap_or(""))
    };
    
    let mut command = Command::new("sh");
    command.arg("-c").arg(&cmd).current_dir(dir);
    
    if let Some(opts) = options {
        for (key, value) in &opts.environment_vars {
            command.env(key, value);
        }
    }
    
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    
    command.spawn()?;
    Ok(())
}

// Parses a user-input string into launch options.
fn parse_launch_options_input(input: &str, _original_command: Option<String>) -> AppLaunchOptions {
    let mut parts = input.split_whitespace().peekable();
    let mut options = AppLaunchOptions::default();
    let mut command_parts = Vec::new();
    
    while let Some(part) = parts.next() {
        match part {
            "-e" => {
                if let Some(env_var) = parts.next() {
                    if let Some((key, value)) = env_var.split_once('=') {
                        options.environment_vars.insert(key.to_string(), value.to_string());
                    }
                }
            }
            "-w" => {
                if let Some(dir) = parts.next() {
                    options.working_directory = Some(dir.to_string());
                }
            }
            _ => {
                command_parts.push(part.to_string());
                command_parts.extend(parts.map(|s| s.to_string()));
                break;
            }
        }
    }
    
    let input_command = if !command_parts.is_empty() {
        command_parts.join(" ")
    } else {
        String::new()
    };
    
    if !input_command.is_empty() {
        options.custom_command = Some(input_command);
    }
    
    options
}

//
// --------------- AppLauncher Struct & Implementation ---------------
//

pub struct AppLauncher {
    query: String,
    applications: Vec<(String, String, String)>,
    results: Vec<(String, String, String)>,
    quit: bool,
    config: Config,
    launch_options: HashMap<String, AppLaunchOptions>,
}

impl Default for AppLauncher {
    fn default() -> Self {
        let config = Config::default();
        let applications = get_desktop_entries();
        let launch_options = get_launch_options();
        let results = if config.enable_recent_apps {
            use std::sync::Mutex;
            APP_CACHE
                .lock()
                .ok()
                .map(|cache| {
                    cache.apps.iter()
                        .map(|(name, _)| name.clone())
                        .filter_map(|app_name| applications.iter().find(|(name, _, _)| name == &app_name).cloned())
                        .take(config.max_search_results)
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        Self {
            query: String::new(),
            results,
            applications,
            quit: false,
            config,
            launch_options,
        }
    }
}

impl AppInterface for AppLauncher {
    fn update(&mut self) {
        if self.quit {
            std::process::exit(0);
        }
    }

    fn handle_input(&mut self, input: &str) {
        match input {
            s if s.starts_with("LAUNCH_OPTIONS:") => {
                let parts: Vec<&str> = s.split(':').collect();
                if parts.len() >= 3 {
                    let (app_name, opts_str) = (parts[1], parts[2]);
                    let orig_cmd = self.get_app_command(app_name);
                    let opts = parse_launch_options_input(opts_str, orig_cmd);
                    self.launch_options.insert(app_name.to_string(), opts.clone());
                    let _ = update_launch_options(app_name, opts);
                    self.query.clear();
                }
            }
            "ESC" => self.quit = true,
            "ENTER" => self.launch_first_result(),
            "P" if self.config.enable_power_options => power::power_off(&self.config),
            "R" if self.config.enable_power_options => power::restart(&self.config),
            "L" if self.config.enable_power_options => power::logout(&self.config),
            _ => {
                self.query = input.to_string();
                self.results = if self.config.enable_recent_apps && self.query.trim().is_empty() {
                    APP_CACHE
                        .lock()
                        .ok()
                        .map(|cache| {
                            cache.apps.iter()
                                .map(|(name, _)| name.clone())
                                .filter_map(|app_name| self.applications.iter().find(|(name, _, _)| name == &app_name).cloned())
                                .take(self.config.max_search_results)
                                .collect()
                        })
                        .unwrap_or_default()
                } else {
                    search_applications(&self.query, &self.applications, self.config.max_search_results)
                };
            }
        }
    }

    fn should_quit(&self) -> bool {
        self.quit
    }

    fn get_query(&self) -> String {
        self.query.clone()
    }

    fn get_search_results(&self) -> Vec<String> {
        self.results.iter().map(|(name, _, _)| name.clone()).collect()
    }

    fn get_time(&self) -> String {
        get_current_time(&self.config)
    }

    fn launch_app(&mut self, app_name: &str) {
        if let Some((_, exec_cmd, _)) = self.results.iter().find(|(name, _, _)| name == app_name) {
            let options = self.launch_options.get(app_name).cloned();
            if launch_app(app_name, exec_cmd, &options, self.config.enable_recent_apps).is_ok() {
                self.quit = true;
            }
        }
    }

    fn get_icon_path(&self, app_name: &str) -> Option<String> {
        self.results
            .iter()
            .find(|(name, _, _)| name == app_name)
            .and_then(|(name, _, icon)| resolve_icon_path(name, icon, &self.config))
    }

    fn get_formatted_launch_options(&self, app_name: &str) -> String {
        if let Some(opts) = self.launch_options.get(app_name) {
            let mut result = String::new();
            for (key, value) in &opts.environment_vars {
                result.push_str(&format!("-e {}={} ", key, value));
            }
            if let Some(cmd) = &opts.custom_command {
                result.push_str(cmd);
            }
            if let Some(dir) = &opts.working_directory {
                result.push_str(&format!(" -w {}", dir));
            }
            result.trim().to_string()
        } else {
            String::new()
        }
    }
}

impl AppLauncher {
    fn launch_first_result(&mut self) {
        if let Some((app_name, exec_cmd, _)) = self.results.first() {
            let options = self.launch_options.get(app_name).cloned();
            if launch_app(app_name, exec_cmd, &options, self.config.enable_recent_apps).is_ok() {
                self.quit = true;
            }
        }
    }

    fn get_app_command(&self, app_name: &str) -> Option<String> {
        self.applications
            .iter()
            .find(|(name, _, _)| name == app_name)
            .map(|(_, cmd, _)| cmd.clone())
    }
}
