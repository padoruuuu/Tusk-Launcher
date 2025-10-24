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
        write!(f, "{}|{}|{}", 
            self.custom_command.as_deref().unwrap_or(""),
            self.working_directory.as_deref().unwrap_or(""),
            self.environment_vars.iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>().join(",")
        )
    }
}

impl FromStr for AppLaunchOptions {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.splitn(3, '|').collect();
        if parts.len() != 3 { return Err("Invalid format".into()); }
        
        Ok(AppLaunchOptions {
            custom_command: if parts[0].is_empty() { None } else { Some(parts[0].to_string()) },
            working_directory: if parts[1].is_empty() { None } else { Some(parts[1].to_string()) },
            environment_vars: if parts[2].is_empty() { HashMap::new() } else {
                parts[2].split(',').filter_map(|entry| 
                    entry.split_once('=').map(|(k, v)| (k.to_string(), v.to_string()))
                ).collect()
            }
        })
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
        let tree = usvg::Tree::from_data(&fs::read(path)?, &usvg::Options::default())?;
        let size = tree.size().to_int_size();
        let mut pixmap = Pixmap::new(size.width(), size.height()).ok_or("Failed to create pixmap")?;
        resvg::render(&tree, usvg::Transform::default(), &mut pixmap.as_mut());
        Ok(egui::ColorImage::from_rgba_unmultiplied([size.width() as usize, size.height() as usize], pixmap.data()))
    } else {
        let img = image::open(path)?.into_rgba8();
        Ok(egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], &img))
    }
}

    fn create_placeholder() -> egui::ColorImage {
        egui::ColorImage::from_rgba_unmultiplied([16, 16], &[127u8; 16 * 16 * 4])
    }
}

static CACHE_FILE: Lazy<PathBuf> = Lazy::new(|| {
    let xdg = BaseDirectories::new();
    let config_home = xdg.get_config_home()
        .unwrap_or_else(|| PathBuf::from("."));
    let mut path = config_home.join("tusk-launcher");
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
        s.push_str(&format!("{}\t{}\t{}\n",
            escape(app_name),
            entry.launch_options.as_ref().map(|opts| escape(&opts.to_string())).unwrap_or_default(),
            entry.icon_directory.as_ref().map(|s| escape(s)).unwrap_or_default()
        ));
    }
    s
}

fn deserialize_cache(s: &str) -> Result<AppCache, Box<dyn std::error::Error>> {
    let mut lines = s.lines();
    if lines.next() != Some("APP_CACHE_V1") { return Err("Unsupported cache version".into()); }
    
    Ok(AppCache {
        apps: lines.filter(|l| !l.trim().is_empty())
            .filter_map(|line| {
                let parts: Vec<&str> = line.split('\t').collect();
                if parts.len() == 3 {
                    Some((
                        unescape(parts[0]),
                        AppEntry {
                            launch_options: if parts[1].is_empty() { None } else { parts[1].parse().ok() },
                            icon_directory: if parts[2].is_empty() { None } else { Some(unescape(parts[2])) }
                        }
                    ))
                } else { None }
            }).collect()
    })
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
    let mut cache = match APP_CACHE.lock() {
        Ok(guard) => guard,
        Err(_) => return None,
    };

    let entry = if let Some(pos) = cache.apps.iter().position(|(key, _)| key == app_name) {
        &mut cache.apps[pos].1
    } else {
        cache.apps.push((app_name.to_owned(), AppEntry::default()));
        &mut cache.apps.last_mut().unwrap().1
    };

    entry.icon_directory = Some(icon_path.to_string());
    let _ = save_cache(&cache);
    Some(icon_path.to_string())
}

pub fn resolve_icon_path(app_name: &str, icon_name: &str, config: &crate::gui::Config) -> Option<String> {
    if icon_name.is_empty() || !config.enable_icons { return None; }

    // Check cache first
    if let Ok(cache) = APP_CACHE.lock() {
        if let Some((_, entry)) = cache.apps.iter().find(|(key, _)| key == app_name) {
            if let Some(ref cached_icon) = entry.icon_directory {
                if Path::new(cached_icon).exists() {
                    return Some(cached_icon.clone());
                }
            }
        }
    }

    // Direct path check
    if Path::new(icon_name).exists() {
        return update_cache_with_icon(app_name, icon_name);
    }

    // Steam icon handling
    if icon_name.starts_with("steam_icon:") {
        let appid = icon_name.trim_start_matches("steam_icon:");
        return find_steam_icon(appid, app_name)
            .or_else(|| resolve_icon_path(app_name, appid, config));
    }

    // Directory search
    if Path::new(icon_name).is_dir() {
        if let Some(icon_file) = find_icon_in_directory(icon_name) {
            return update_cache_with_icon(app_name, &icon_file);
        }
    }

    // System icon search
    find_system_icon(icon_name).and_then(|path| update_cache_with_icon(app_name, &path))
}

fn find_steam_icon(appid: &str, app_name: &str) -> Option<String> {
    let patterns = [
        format!("{}_header.jpg", appid), format!("{}_library_600x900.jpg", appid),
        format!("{}_library.png", appid), format!("{}_icon.png", appid),
        format!("{}.png", appid), format!("{}.jpg", appid), format!("{}.ico", appid)
    ];
    
    get_icon_search_paths().iter()
        .flat_map(|path| patterns.iter().map(move |pattern| path.join(pattern)))
        .find(|path| path.exists())
        .and_then(|path| path.to_str().map(|s| s.to_owned()))
        .and_then(|path| update_cache_with_icon(app_name, &path))
}

fn find_icon_in_directory(dir: &str) -> Option<String> {
    fs::read_dir(dir).ok()?.filter_map(Result::ok)
        .find(|entry| {
            entry.path().extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ["png", "jpg", "jpeg", "svg", "ico"].contains(&ext.to_lowercase().as_str()))
                .unwrap_or(false)
        })
        .and_then(|entry| entry.path().to_str().map(String::from))
}

fn find_system_icon(icon_name: &str) -> Option<String> {
    let search_paths = get_icon_search_paths();
    let themes = ["hicolor", "Adwaita", "gnome", "breeze", "oxygen"];
    let sizes = ["512x512", "256x256", "128x128", "64x64", "48x48", "32x32", "24x24", "16x16", "scalable"];
    let categories = ["apps", "devices", "places", "mimetypes", "status", "actions"];
    let extensions = ["png", "svg", "xpm", "jpg", "jpeg", "ico"];
    
    search_paths.iter()
        .flat_map(|base| themes.iter().map(move |theme| base.join(theme)))
        .flat_map(|theme_path| sizes.iter().map(move |size| theme_path.join(size)))
        .flat_map(|size_path| categories.iter().map(move |cat| size_path.join(cat)))
        .flat_map(|cat_path| extensions.iter().map(move |ext| cat_path.join(format!("{}.{}", icon_name, ext))))
        .find(|path| path.exists())
        .and_then(|path| path.to_str().map(String::from))
}

fn get_icon_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let xdg = BaseDirectories::new();
    
    if let Some(data_home) = xdg.get_data_home() {
        paths.push(data_home.join("icons"));
        paths.push(data_home.join("flatpak/exports/share/icons"));
    }
    
    for data_dir in xdg.get_data_dirs() {
        paths.push(data_dir.join("icons"));
        paths.push(data_dir.join("pixmaps"));
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
    let mut fields = (None, None, None, None); // name, exec, icon, wm_class
    
    for line in content.lines() {
        if let Some((key, value)) = line.split_once('=') {
            match (key.trim(), &mut fields) {
                ("Name", (name @ None, _, _, _)) => *name = Some(value.trim().to_string()),
                ("Exec", (_, exec @ None, _, _)) => *exec = Some(value.trim().to_string()),
                ("Icon", (_, _, icon @ None, _)) => *icon = Some(value.trim().to_string()),
                ("StartupWMClass", (_, _, _, wm_class @ None)) => *wm_class = Some(value.trim().to_string()),
                _ => {}
            }
        }
    }
    
    let (name, exec, icon, wm_class) = fields;
    let mut exec = exec?;
    
    // Clean exec command
    for placeholder in ["%f", "%F", "%u", "%U", "%c", "%k", "@@"] {
        exec = exec.replace(placeholder, "");
    }
    if let Some(icon_val) = &icon {
        exec = exec.replace("%i", &format!("--icon {}", icon_val));
    }
    if let Some(class) = wm_class {
        if !exec.contains("flatpak run") {
            exec.push_str(&format!(" --class {}", class));
        }
    }
    
    Some((name?, exec.trim().to_string(), icon.unwrap_or_default()))
}

fn get_desktop_entries() -> Vec<(String, String, String)> {
    let xdg = BaseDirectories::new();
    
    let mut app_dirs = xdg.get_data_dirs().into_iter().map(|d| d.join("applications")).collect::<Vec<_>>();
    
    if let Some(data_home) = xdg.get_data_home() {
        app_dirs.push(data_home.join("applications"));
        app_dirs.push(data_home.join("flatpak/exports/share/applications"));
    }
    
    app_dirs.into_iter()
        .filter_map(|dir| fs::read_dir(dir).ok())
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().map_or(false, |ext| ext == "desktop"))
        .filter_map(|entry| parse_desktop_entry(&entry.path()))
        .collect()
}

fn get_steam_entries() -> Vec<(String, String, String)> {
    let home = std::env::var("HOME").unwrap_or_default();
    let steam_path = PathBuf::from(&home).join(".local/share/Steam");
    if !steam_path.exists() { return Vec::new(); }
    
    let library_paths = get_steam_library_paths(&steam_path);
    let mut seen_appids = HashSet::new();
    
    library_paths.into_iter()
        .map(|lib| lib.join("steamapps"))
        .filter(|path| path.exists())
        .filter_map(|steamapps| fs::read_dir(steamapps).ok())
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("appmanifest_"))
        .filter_map(|entry| parse_steam_manifest(&entry.path(), &mut seen_appids))
        .collect()
}

fn get_steam_library_paths(steam_path: &PathBuf) -> Vec<PathBuf> {
    let mut paths = vec![steam_path.clone()];
    let library_vdf = steam_path.join("steamapps/libraryfolders.vdf");
    
    if let Ok(content) = fs::read_to_string(&library_vdf) {
        for line in content.lines() {
            if line.contains("\"path\"") {
                if let Some(path_str) = line.split('"').nth(3) {
                    let lib_path = PathBuf::from(path_str);
                    if lib_path.exists() && !paths.contains(&lib_path) {
                        paths.push(lib_path);
                    }
                }
            }
        }
    }
    paths
}

fn parse_steam_manifest(path: &PathBuf, seen_appids: &mut HashSet<String>) -> Option<(String, String, String)> {
    let content = fs::read_to_string(path).ok()?;
    let mut fields = (None, None, None); // appid, name, installdir
    
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(value) = extract_quoted_value(trimmed, "appid") {
            fields.0 = Some(value);
        } else if let Some(value) = extract_quoted_value(trimmed, "name") {
            fields.1 = Some(value);
        } else if let Some(value) = extract_quoted_value(trimmed, "installdir") {
            fields.2 = Some(value);
        }
    }
    
    let (appid, name, installdir) = fields;
    let (appid, name, installdir) = (appid?, name?, installdir?);
    
    if !seen_appids.insert(appid.clone()) { return None; }
    
    let icon_path = determine_steam_icon_path(path, &appid, &installdir);
    Some((name, format!("steam steam://rungameid/{}", appid), icon_path))
}

fn extract_quoted_value(line: &str, key: &str) -> Option<String> {
    if line.starts_with(&format!("\"{}\"", key)) {
        line.split('"').nth(3).map(String::from)
    } else { None }
}

fn determine_steam_icon_path(manifest_path: &PathBuf, appid: &str, installdir: &str) -> String {
    let steamapps = manifest_path.parent().unwrap();
    let icon_dir = steamapps.join("common").join(installdir);
    
    if icon_dir.exists() && has_icon_files(&icon_dir) {
        icon_dir.to_string_lossy().to_string()
    } else {
        format!("steam_icon:{}", appid)
    }
}

fn has_icon_files(dir: &PathBuf) -> bool {
    fs::read_dir(dir).ok()
        .map(|entries| entries.filter_map(Result::ok).any(|entry| {
            entry.path().extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ["png", "jpg", "jpeg", "svg", "ico"].contains(&ext.to_lowercase().as_str()))
                .unwrap_or(false)
        }))
        .unwrap_or(false)
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