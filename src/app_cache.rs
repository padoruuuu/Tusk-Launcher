use std::{
    collections::{HashMap, HashSet},
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

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct AppLaunchOptions {
    pub custom_command: Option<String>,
    pub working_directory: Option<String>,
    pub environment_vars: HashMap<String, String>,
}

impl std::fmt::Display for AppLaunchOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let custom = self.custom_command.as_deref().unwrap_or("");
        let working = self.working_directory.as_deref().unwrap_or("");
        let env_str = self
            .environment_vars
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join(",");
        write!(f, "{}|{}|{}", custom, working, env_str)
    }
}

impl FromStr for AppLaunchOptions {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.splitn(3, '|').collect();
        if parts.len() != 3 {
            return Err("Invalid format for AppLaunchOptions".into());
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
        Ok(AppLaunchOptions { custom_command: custom, working_directory: working, environment_vars })
    }
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct AppEntry {
    pub launch_options: Option<AppLaunchOptions>,
    pub icon_directory: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct AppCache {
    pub apps: Vec<(String, AppEntry)>,
}

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
        Self { icon_textures: HashMap::new() }
    }

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
            self.icon_textures.get(icon_path).and_then(|cache| cache.texture.clone())
        }
    }

    fn load_image(path: &str) -> Result<egui::ColorImage, Box<dyn std::error::Error>> {
        if path.to_lowercase().ends_with(".svg") {
            let data = fs::read(path)?;
            let tree = usvg::Tree::from_data(&data, &usvg::Options::default())?;
            let size = tree.size().to_int_size();
            let mut pixmap = Pixmap::new(size.width(), size.height()).ok_or("Failed to create pixmap")?;
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

static CACHE_FILE: Lazy<PathBuf> = Lazy::new(|| {
    let xdg = BaseDirectories::new()
        .map(|bd| bd.get_config_home().to_owned())
        .unwrap_or_else(|_| PathBuf::from("."));
    let mut path = xdg.join("tusk-launcher");
    fs::create_dir_all(&path).expect("Failed to create config directory");
    path.push("app_cache.txt");
    path
});

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\t', "\\t").replace('\n', "\\n")
}

fn unescape(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('\\') => result.push('\\'),
                Some('t') => result.push('\t'),
                Some('n') => result.push('\n'),
                Some(other) => {
                    result.push('\\');
                    result.push(other);
                }
                None => result.push('\\'),
            }
        } else {
            result.push(c);
        }
    }
    result
}

fn serialize_cache(cache: &AppCache) -> String {
    let mut s = String::from("APP_CACHE_V1\n");
    for (app_name, entry) in &cache.apps {
        let name_escaped = escape(app_name);
        let launch_options_escaped = entry.launch_options.as_ref().map(|opts| escape(&opts.to_string())).unwrap_or_default();
        let icon_escaped = entry.icon_directory.as_ref().map(|s| escape(s)).unwrap_or_default();
        s.push_str(&format!("{}\t{}\t{}\n", name_escaped, launch_options_escaped, icon_escaped));
    }
    s
}

fn deserialize_cache(s: &str) -> Result<AppCache, Box<dyn std::error::Error>> {
    let mut lines = s.lines();
    if lines.next().ok_or("Empty cache file")? != "APP_CACHE_V1" {
        return Err("Unsupported cache file version".into());
    }
    let mut cache = AppCache::default();
    for line in lines.filter(|l| !l.trim().is_empty()) {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() != 3 {
            continue;
        }
        let app_name = unescape(parts[0]);
        let launch_options = if parts[1].is_empty() { None } else { Some(parts[1].parse()?) };
        let icon_directory = if parts[2].is_empty() { None } else { Some(unescape(parts[2])) };
        cache.apps.push((app_name, AppEntry { launch_options, icon_directory }));
    }
    Ok(cache)
}

fn save_cache(cache: &AppCache) -> Result<(), Box<dyn std::error::Error>> {
    fs::write(&*CACHE_FILE, serialize_cache(cache))?;
    Ok(())
}

pub static APP_CACHE: Lazy<Mutex<AppCache>> = Lazy::new(|| {
    let cache = if CACHE_FILE.exists() {
        fs::read_to_string(&*CACHE_FILE)
            .ok()
            .and_then(|s| deserialize_cache(&s).ok())
            .unwrap_or_default()
    } else {
        AppCache::default()
    };
    Mutex::new(cache)
});

pub fn update_recent_apps(app_name: &str, enable_recent_apps: bool) -> Result<(), Box<dyn std::error::Error>> {
    if !enable_recent_apps {
        return Ok(());
    }
    let mut cache = APP_CACHE.lock().map_err(|e| format!("Lock error: {:?}", e))?;
    let existing_entry = cache.apps.iter().position(|(name, _)| name == app_name).map(|pos| cache.apps.remove(pos).1);
    let entry = existing_entry.unwrap_or_default();
    cache.apps.insert(0, (app_name.to_owned(), entry));
    save_cache(&cache)?;
    Ok(())
}

pub fn update_launch_options(app_name: &str, options: AppLaunchOptions) -> Result<(), Box<dyn std::error::Error>> {
    let mut cache = APP_CACHE.lock().map_err(|e| format!("Lock error: {:?}", e))?;
    if let Some(pos) = cache.apps.iter().position(|(name, _)| name == app_name) {
        cache.apps[pos].1.launch_options = Some(options);
    } else {
        let mut entry = AppEntry::default();
        entry.launch_options = Some(options);
        cache.apps.push((app_name.to_owned(), entry));
    }
    save_cache(&cache)?;
    Ok(())
}

pub fn get_launch_options() -> HashMap<String, AppLaunchOptions> {
    APP_CACHE.lock().map(|c| {
        c.apps.iter()
         .filter_map(|(name, entry)| entry.launch_options.clone().map(|opts| (name.clone(), opts)))
         .collect()
    }).unwrap_or_default()
}

fn update_cache_with_icon(app_name: &str, icon_path: &str) -> Option<String> {
    if let Ok(mut cache) = APP_CACHE.lock() {
        if let Some(pos) = cache.apps.iter().position(|(key, _)| key == app_name) {
            cache.apps[pos].1.icon_directory = Some(icon_path.to_string());
        } else {
            let mut entry = AppEntry::default();
            entry.icon_directory = Some(icon_path.to_string());
            cache.apps.push((app_name.to_owned(), entry));
        }
        let _ = save_cache(&cache);
    }
    Some(icon_path.to_string())
}

pub fn resolve_icon_path(app_name: &str, icon_name: &str, config: &crate::gui::Config) -> Option<String> {
    if icon_name.is_empty() || !config.enable_icons {
        return None;
    }

    if icon_name.starts_with("steam_icon:") {
        let appid = icon_name.trim_start_matches("steam_icon:");
        let search_paths = get_icon_search_paths();
        
        let steam_patterns = [
            format!("{}_header.jpg", appid),
            format!("{}_library_600x900.jpg", appid),
            format!("{}_library.png", appid),
            format!("{}_icon.png", appid),
            format!("{}.png", appid),
            format!("{}.jpg", appid),
            format!("{}.ico", appid),
        ];
        
        for path in &search_paths {
            for pattern in &steam_patterns {
                let full_path = path.join(pattern);
                if full_path.exists() {
                    return update_cache_with_icon(app_name, full_path.to_str().unwrap());
                }
            }
        }
        
        return resolve_icon_path(app_name, appid, config);
    }

    if Path::new(icon_name).is_dir() {
        if let Ok(entries) = fs::read_dir(icon_name) {
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                if let Some(ext) = path.extension().and_then(|ext| ext.to_str()).map(|s| s.to_owned()) {
                    if ["png", "jpg", "jpeg", "svg", "ico"].contains(&ext.to_lowercase().as_str()) {
                        return update_cache_with_icon(app_name, path.to_str().unwrap());
                    }
                }
            }
        }
    }

    if let Ok(cache) = APP_CACHE.lock() {
        if let Some((_, entry)) = cache.apps.iter().find(|(key, _)| key == app_name) {
            if let Some(ref cached_icon) = entry.icon_directory {
                if Path::new(cached_icon).exists() {
                    return Some(cached_icon.clone());
                }
            }
        }
    }

    if Path::new(icon_name).exists() {
        return update_cache_with_icon(app_name, icon_name);
    }

    let search_paths = get_icon_search_paths();
    let steam_patterns = [
        format!("{}_header.jpg", icon_name),
        format!("{}_library_600x900.jpg", icon_name),
        format!("{}_library.png", icon_name),
        format!("{}_icon.png", icon_name),
        format!("{}.png", icon_name),
        format!("{}.jpg", icon_name),
        format!("{}.ico", icon_name),
    ];
    
    for path in &search_paths {
        for pattern in &steam_patterns {
            let full_path = path.join(pattern);
            if full_path.exists() {
                return full_path.to_str().and_then(|p| update_cache_with_icon(app_name, p));
            }
        }
    }

    let icon_themes = ["hicolor", "Adwaita", "gnome", "breeze", "oxygen"];
    let icon_sizes = ["512x512", "256x256", "128x128", "64x64", "48x48", "32x32", "24x24", "16x16", "scalable"];
    let categories = ["apps", "devices", "places", "mimetypes", "status", "actions"];
    let extensions = ["png", "svg", "xpm", "jpg", "jpeg", "ico"];
    
    if let Some(path) = search_paths.iter().find_map(|base| {
        icon_themes.iter().find_map(|theme| {
            icon_sizes.iter().find_map(|size| {
                categories.iter().find_map(|cat| {
                    let dir = base.join(theme).join(size).join(cat);
                    for ext in &extensions {
                        let p = dir.join(format!("{}.{}", icon_name, ext));
                        if p.exists() {
                            return p.to_str().map(String::from);
                        }
                    }
                    None
                })
            })
        })
    }) {
        return update_cache_with_icon(app_name, &path);
    }

    None
}

fn get_icon_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(xdg) = BaseDirectories::new() {
        paths.push(xdg.get_data_home().join("icons"));
        paths.push(xdg.get_data_home().join("flatpak/exports/share/icons"));
        for data_dir in xdg.get_data_dirs() {
            paths.push(data_dir.join("icons"));
            paths.push(data_dir.join("pixmaps"));
        }
    }
    paths.push(PathBuf::from("/usr/share/pixmaps"));
    paths.push(PathBuf::from("/var/lib/flatpak/exports/share/icons"));
    
    if let Ok(home) = std::env::var("HOME") {
        let steam_paths = [
            ".local/share/icons/hicolor"
        ];
        
        for steam_path in steam_paths {
            let full_path = PathBuf::from(&home).join(steam_path);
            if full_path.exists() {
                paths.push(full_path);
            }
        }
    }
    
    paths
}

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
        let mut app_dirs = Vec::new();
        for dir in xdg.get_data_dirs() {
            app_dirs.push(dir.join("applications"));
        }
        app_dirs.push(xdg.get_data_home().join("applications"));
        app_dirs.push(xdg.get_data_home().join("flatpak/exports/share/applications"));
        for dir in app_dirs {
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

fn get_steam_entries() -> Vec<(String, String, String)> {
    let mut entries = Vec::new();
    let mut seen_appids = HashSet::new();
    let home = std::env::var("HOME").unwrap_or_default();
    let steam_path = PathBuf::from(&home).join(".local/share/Steam");
    if !steam_path.exists() {
        return entries;
    }
    let mut library_paths = vec![steam_path.clone()];
    let library_vdf = steam_path.join("steamapps/libraryfolders.vdf");
    if library_vdf.exists() {
        if let Ok(content) = fs::read_to_string(&library_vdf) {
            for line in content.lines() {
                if line.contains("\"path\"") {
                    let parts: Vec<&str> = line.split('"').filter(|s| !s.trim().is_empty()).collect();
                    if parts.len() >= 2 {
                        let lib_path = PathBuf::from(parts[1]);
                        if lib_path.exists() && !library_paths.contains(&lib_path) {
                            library_paths.push(lib_path);
                        }
                    }
                }
            }
        }
    }
    for lib in library_paths {
        let steamapps = lib.join("steamapps");
        if steamapps.exists() && steamapps.is_dir() {
            if let Ok(entries_iter) = fs::read_dir(&steamapps) {
                for entry in entries_iter.filter_map(Result::ok) {
                    let path = entry.path();
                    if path.is_file() && path.file_name().unwrap_or_default().to_string_lossy().starts_with("appmanifest_") {
                        if let Ok(content) = fs::read_to_string(&path) {
                            let mut appid = None;
                            let mut name = None;
                            let mut installdir = None;
                            for line in content.lines() {
                                let trimmed = line.trim();
                                if trimmed.starts_with("\"appid\"") {
                                    let parts: Vec<&str> = trimmed.split('"').collect();
                                    if parts.len() >= 4 {
                                        appid = Some(parts[3].to_string());
                                    }
                                }
                                if trimmed.starts_with("\"name\"") {
                                    let parts: Vec<&str> = trimmed.split('"').collect();
                                    if parts.len() >= 4 {
                                        name = Some(parts[3].to_string());
                                    }
                                }
                                if trimmed.starts_with("\"installdir\"") {
                                    let parts: Vec<&str> = trimmed.split('"').collect();
                                    if parts.len() >= 4 {
                                        installdir = Some(parts[3].to_string());
                                    }
                                }
                            }
                            if let (Some(appid_val), Some(name_val), Some(installdir_val)) = (appid, name, installdir) {
                                if seen_appids.contains(&appid_val) {
                                    continue;
                                }
                                seen_appids.insert(appid_val.clone());
                                let exec_cmd = format!("steam steam://rungameid/{}", appid_val);
                                let icon_dir = steamapps.join("common").join(&installdir_val);
                                let mut icon_path = String::new();
                                if icon_dir.exists() {
                                    icon_path = icon_dir.to_string_lossy().to_string();
                                    if let Ok(mut entries) = fs::read_dir(&icon_dir) {
                                        let has_icon = entries.any(|e| {
                                            e.ok().and_then(|entry| {
                                                entry.path().extension().and_then(|ext| ext.to_str().map(|s| s.to_owned()))
                                            }).map_or(false, |ext| {
                                                ["png", "jpg", "jpeg", "svg", "ico"].contains(&ext.to_lowercase().as_str())
                                            })
                                        });
                                        if !has_icon {
                                            icon_path.clear();
                                        }
                                    }
                                }
                                if icon_path.is_empty() {
                                    icon_path = format!("steam_icon:{}", appid_val);
                                }
                                entries.push((name_val, exec_cmd, icon_path));
                            }
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
        (command, opts.working_directory.as_deref().unwrap_or_else(|| home_dir.to_str().unwrap_or("")))
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
    command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    command.spawn()?;
    Ok(())
}

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
    if !command_parts.is_empty() {
        options.custom_command = Some(command_parts.join(" "));
    }
    options
}

pub struct AppLauncher {
    query: String,
    applications: Vec<(String, String, String)>,
    results: Vec<(String, String, String)>,
    quit: bool,
    config: crate::gui::Config,
    launch_options: HashMap<String, AppLaunchOptions>,
}

impl Default for AppLauncher {
    fn default() -> Self {
        let config = crate::gui::Config::default();
        let mut apps = get_desktop_entries();
        apps.extend(get_steam_entries());
        let mut unique_apps: HashMap<String, (String, String)> = HashMap::new();
        for (name, cmd, icon) in apps {
            unique_apps.entry(name).or_insert((cmd, icon));
        }
        let applications = unique_apps.into_iter().map(|(name, (cmd, icon))| (name, cmd, icon)).collect::<Vec<_>>();
        let launch_options = get_launch_options();
        let results = if config.enable_recent_apps {
            APP_CACHE
                .lock()
                .ok()
                .map(|cache| {
                    cache
                        .apps
                        .iter()
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

impl crate::gui::AppInterface for AppLauncher {
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
            "P" if self.config.enable_power_options => crate::power::power_off(&self.config),
            "R" if self.config.enable_power_options => crate::power::restart(&self.config),
            "L" if self.config.enable_power_options => crate::power::logout(&self.config),
            _ => {
                self.query = input.to_string();
                self.results = if self.config.enable_recent_apps && self.query.trim().is_empty() {
                    self.get_recent_apps()
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
        crate::clock::get_current_time(&self.config)
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
            if let Some(dir) = &opts.working_directory {
                result.push_str(&format!("-w {} ", dir));
            }
            if let Some(cmd) = &opts.custom_command {
                result.push_str(cmd);
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

    fn get_recent_apps(&self) -> Vec<(String, String, String)> {
        APP_CACHE
            .lock()
            .ok()
            .map(|cache| {
                cache
                    .apps
                    .iter()
                    .map(|(name, _)| name.clone())
                    .filter_map(|app_name| self.applications.iter().find(|(name, _, _)| name == &app_name).cloned())
                    .take(self.config.max_search_results)
                    .collect()
            })
            .unwrap_or_default()
    }
}