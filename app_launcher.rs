use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    str::FromStr,
    sync::{Arc, LazyLock, Mutex},
    thread,
    time,
};
use eframe::egui;

use serde::{Serialize, Deserialize};

// ============================================================================
// Public cache data structures (unchanged public API)
// ============================================================================

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct AppLaunchOptions {
    pub custom_command:    Option<String>,
    pub working_directory: Option<String>,
    pub environment_vars:  HashMap<String, String>,
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
            custom_command:    if parts[0].is_empty() { None } else { Some(parts[0].to_string()) },
            working_directory: if parts[1].is_empty() { None } else { Some(parts[1].to_string()) },
            environment_vars:  if parts[2].is_empty() {
                HashMap::new()
            } else {
                parts[2].split(',')
                    .filter_map(|e| e.split_once('='))
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect()
            },
        })
    }
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct AppEntry {
    pub launch_options:   Option<AppLaunchOptions>,
    pub icon_path:        Option<String>,
    pub exec_command:     Option<String>,
    pub terminal_command: Option<String>,
    pub last_used:        Option<u64>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct AppCache {
    pub apps: Vec<(String, AppEntry)>,
}

#[derive(Default)]
struct IconCache {
    texture:       Option<egui::TextureHandle>,
    last_modified: Option<time::SystemTime>,
}

pub struct IconManager {
    icon_textures: HashMap<String, IconCache>,
}

// ============================================================================
// Runtime app representation
//
// Replaces the bare (String, String, String) tuple with a struct that
// pre-computes the lowercase name, avoiding per-keystroke allocations in search.
// ============================================================================

struct App {
    name:       String,
    name_lower: String,   // computed once, used for every search
    exec:       String,
    icon:       String,
}

impl App {
    fn new(name: String, exec: String, icon: String) -> Self {
        let name_lower = name.to_lowercase();
        App { name, name_lower, exec, icon }
    }
}

const ICON_EXTS: &[&str] = &["png", "svg", "jpg", "jpeg", "ico"];

// ============================================================================
// Cache management
// ============================================================================

static CACHE_FILE: LazyLock<PathBuf> = LazyLock::new(|| {
    let path = crate::paths::config_home().join("tusk-launcher");
    fs::create_dir_all(&path).ok();
    path.join("app_cache.txt")
});

pub static APP_CACHE: LazyLock<Mutex<AppCache>> = LazyLock::new(|| {
    let cache = CACHE_FILE.exists()
        .then(|| fs::read_to_string(&*CACHE_FILE).ok())
        .flatten()
        .and_then(|s: String| deserialize_cache(&s).ok())
        .unwrap_or_default();
    Mutex::new(cache)
});

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\t', "\\t").replace('\n', "\\n")
}

fn unescape(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars  = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('\\') => result.push('\\'),
                Some('t')  => result.push('\t'),
                Some('n')  => result.push('\n'),
                Some(other) => { result.push('\\'); result.push(other); }
                None       => result.push('\\'),
            }
        } else {
            result.push(c);
        }
    }
    result
}

fn serialize_cache(cache: &AppCache) -> String {
    let mut s = String::from("APP_CACHE_V4\n");
    for (app_name, entry) in &cache.apps {
        s.push_str(&format!("{}\t{}\t{}\t{}\t{}\t{}\n",
            escape(app_name),
            entry.launch_options.as_ref().map(|o| escape(&o.to_string())).unwrap_or_default(),
            entry.icon_path.as_ref().map(|s| escape(s)).unwrap_or_default(),
            entry.exec_command.as_ref().map(|s| escape(s)).unwrap_or_default(),
            entry.terminal_command.as_ref().map(|s| escape(s)).unwrap_or_default(),
            entry.last_used.map(|t| t.to_string()).unwrap_or_default(),
        ));
    }
    s
}

fn deserialize_cache(s: &str) -> Result<AppCache, Box<dyn std::error::Error>> {
    let mut lines   = s.lines();
    let version     = lines.next();
    let is_v4 = version == Some("APP_CACHE_V4");
    let is_v3 = version == Some("APP_CACHE_V3");
    let is_v2 = version == Some("APP_CACHE_V2");
    let is_v1 = version == Some("APP_CACHE_V1");

    if !is_v1 && !is_v2 && !is_v3 && !is_v4 {
        return Err("Unsupported cache version".into());
    }

    Ok(AppCache {
        apps: lines
            .filter(|l| !l.trim().is_empty())
            .filter_map(|line| {
                let parts: Vec<&str> = line.split('\t').collect();
                if is_v4 && parts.len() == 6 {
                    Some((unescape(parts[0]), AppEntry {
                        launch_options:   (!parts[1].is_empty()).then(|| parts[1].parse().ok()).flatten(),
                        icon_path:        (!parts[2].is_empty()).then(|| unescape(parts[2])),
                        exec_command:     (!parts[3].is_empty()).then(|| unescape(parts[3])),
                        terminal_command: (!parts[4].is_empty()).then(|| unescape(parts[4])),
                        last_used:        (!parts[5].is_empty()).then(|| parts[5].parse().ok()).flatten(),
                    }))
                } else if is_v3 && parts.len() == 5 {
                    Some((unescape(parts[0]), AppEntry {
                        launch_options:   (!parts[1].is_empty()).then(|| parts[1].parse().ok()).flatten(),
                        icon_path:        (!parts[2].is_empty()).then(|| unescape(parts[2])),
                        exec_command:     (!parts[3].is_empty()).then(|| unescape(parts[3])),
                        terminal_command: (!parts[4].is_empty()).then(|| unescape(parts[4])),
                        last_used:        None,
                    }))
                } else if is_v2 && parts.len() == 4 {
                    Some((unescape(parts[0]), AppEntry {
                        launch_options:   (!parts[1].is_empty()).then(|| parts[1].parse().ok()).flatten(),
                        icon_path:        (!parts[2].is_empty()).then(|| unescape(parts[2])),
                        exec_command:     None,
                        terminal_command: (!parts[3].is_empty()).then(|| unescape(parts[3])),
                        last_used:        None,
                    }))
                } else if is_v1 && parts.len() == 3 {
                    Some((unescape(parts[0]), AppEntry {
                        launch_options:   (!parts[1].is_empty()).then(|| parts[1].parse().ok()).flatten(),
                        icon_path:        (!parts[2].is_empty()).then(|| unescape(parts[2])),
                        exec_command:     None,
                        terminal_command: None,
                        last_used:        None,
                    }))
                } else {
                    None
                }
            })
            .collect(),
    })
}

fn save_cache(cache: &AppCache) -> Result<(), Box<dyn std::error::Error>> {
    fs::write(&*CACHE_FILE, serialize_cache(cache))?;
    Ok(())
}

fn get_or_create_entry<'a>(cache: &'a mut AppCache, app_name: &str) -> &'a mut AppEntry {
    match cache.apps.iter().position(|(name, _)| name == app_name) {
        Some(idx) => &mut cache.apps[idx].1,
        None => {
            cache.apps.insert(0, (app_name.to_owned(), AppEntry::default()));
            &mut cache.apps[0].1
        }
    }
}

pub fn update_recent_apps(app_name: &str, enable_recent_apps: bool) -> Result<(), Box<dyn std::error::Error>> {
    if !enable_recent_apps { return Ok(()); }

    let mut cache    = APP_CACHE.lock().map_err(|e| format!("Lock error: {:?}", e))?;
    let timestamp    = time::SystemTime::now()
        .duration_since(time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs());

    if let Some(pos) = cache.apps.iter().position(|(name, _)| name == app_name) {
        let mut entry = cache.apps.remove(pos);
        entry.1.last_used = timestamp;
        cache.apps.insert(0, entry);
        save_cache(&cache)
    } else {
        Ok(())
    }
}

pub fn update_launch_options(app_name: &str, options: AppLaunchOptions) -> Result<(), Box<dyn std::error::Error>> {
    let mut cache = APP_CACHE.lock().map_err(|e| format!("Lock error: {:?}", e))?;
    get_or_create_entry(&mut cache, app_name).launch_options = Some(options);
    save_cache(&cache)
}

pub fn get_launch_options() -> HashMap<String, AppLaunchOptions> {
    APP_CACHE.lock()
        .ok()
        .map(|c| c.apps.iter()
            .filter_map(|(name, entry): &(String, AppEntry)| entry.launch_options.clone().map(|opts| (name.clone(), opts)))
            .collect()
        )
        .unwrap_or_default()
}

fn cache_app_metadata(app_name: &str, exec_cmd: &str, icon_path: &str) {
    if let Ok(mut cache) = APP_CACHE.lock() {
        let entry = get_or_create_entry(&mut cache, app_name);
        if entry.exec_command.is_none()                        { entry.exec_command     = Some(exec_cmd.to_string()); }
        if entry.icon_path.is_none() && !icon_path.is_empty() {
            // Canonicalize absolute paths so we never persist a symlink into the cache.
            // Symlinks appear with flatpak/snap icon exports, LibreOffice, and anything
            // installed outside /usr/share; storing the symlink rather than its real
            // target means a package update that moves the target silently breaks every
            // cached icon for that app. fs::canonicalize follows all symlinks and
            // resolves `..` segments; if it fails (path doesn't exist yet, or a
            // relative/bare name like "discord"), we fall back to the original string.
            let resolved = if icon_path.starts_with('/') {
                fs::canonicalize(icon_path)
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| icon_path.to_string())
            } else {
                icon_path.to_string()
            };
            entry.icon_path = Some(resolved);
        }
        if entry.terminal_command.is_none() {
            if let Some(term_cmd) = extract_terminal_command(exec_cmd) {
                entry.terminal_command = Some(term_cmd);
            }
        }
        let _ = save_cache(&cache);
    }
}

fn get_cached_data(app_name: &str) -> Option<(Option<String>, Option<String>, Option<String>)> {
    APP_CACHE.lock()
        .ok()
        .and_then(|cache| {
            cache.apps.iter()
                .find(|(name, _)| name == app_name)
                .map(|(_, entry)| (
                    entry.icon_path.clone(),
                    entry.exec_command.clone(),
                    entry.terminal_command.clone(),
                ))
        })
}

/// Returns cached apps that have a known exec command. These are shown immediately
/// while the background filesystem scan is in progress.
fn get_all_cached_apps() -> Vec<App> {
    APP_CACHE.lock()
        .ok()
        .map(|cache| {
            cache.apps.iter()
                .filter_map(|(name, entry): &(String, AppEntry)| {
                    let exec = entry.exec_command.as_ref()?;
                    let icon = entry.icon_path.as_deref().unwrap_or("").to_string();
                    Some(App::new(name.clone(), exec.clone(), icon))
                })
                .collect()
        })
        .unwrap_or_default()
}

// ============================================================================
// Icon management
// ============================================================================

impl IconManager {
    pub fn new() -> Self {
        Self { icon_textures: HashMap::new() }
    }

    pub fn get_texture(&mut self, ctx: &egui::Context, icon_path: &str) -> Option<egui::TextureHandle> {
        let needs_reload = self.icon_textures.get(icon_path)
            .map_or(true, |cache| {
                fs::metadata(icon_path)
                    .and_then(|m| m.modified())
                    .ok()
                    .map_or(true, |mod_time| cache.last_modified.map_or(true, |lm| lm != mod_time))
            });

        if needs_reload {
            let img = Self::load_image(icon_path).unwrap_or_else(|_| Self::create_placeholder());
            let tex = ctx.load_texture(icon_path, img, Default::default());
            self.icon_textures.insert(icon_path.to_owned(), IconCache {
                texture:       Some(tex.clone()),
                last_modified: fs::metadata(icon_path).and_then(|m| m.modified()).ok(),
            });
            Some(tex)
        } else {
            self.icon_textures.get(icon_path).and_then(|c| c.texture.clone())
        }
    }

    fn load_image(path: &str) -> Result<egui::ColorImage, Box<dyn std::error::Error>> {
        let lower = path.to_lowercase();
        if lower.ends_with(".svg") {
            let (rgba, w, h) = crate::svg::rasterize(&fs::read(path)?, 32)?;
            return Ok(egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba));
        }
        if lower.ends_with(".png") { return load_png(path); }
        if lower.ends_with(".jpg") || lower.ends_with(".jpeg") { return load_jpeg(path); }
        if lower.ends_with(".ico") {
            if let Ok(img) = load_ico(path) { return Ok(img); }
            if let Ok(img) = load_png(path) { return Ok(img); }
        }
        Err(format!("unsupported image format: {path}").into())
    }

    fn create_placeholder() -> egui::ColorImage {
        egui::ColorImage::from_rgba_unmultiplied([16, 16], &[127u8; 16 * 16 * 4])
    }
}

// ── per-format image decoders (replace the `image` crate) ───────────────────

fn load_png(path: &str) -> Result<egui::ColorImage, Box<dyn std::error::Error>> {
    // png 0.18: Decoder requires BufRead + Seek; output_buffer_size() returns Option<usize>
    let file    = std::io::BufReader::new(std::fs::File::open(path)?);
    let decoder = png::Decoder::new(file);
    let mut reader = decoder.read_info()?;
    let mut buf = vec![0u8; reader.output_buffer_size().unwrap_or(0)];
    let info = reader.next_frame(&mut buf)?;
    let (w, h) = (info.width as usize, info.height as usize);
    let src = &buf[..info.buffer_size()];
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba          => src.to_vec(),
        png::ColorType::Rgb           => src.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha=> src.chunks(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Grayscale     => src.iter().flat_map(|&v|  [v,    v,    v,    255 ]).collect(),
        png::ColorType::Indexed       => return Err("indexed PNG not supported".into()),
    };
    Ok(egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba))
}

fn load_jpeg(path: &str) -> Result<egui::ColorImage, Box<dyn std::error::Error>> {
    use image::GenericImageView;
    let img = image::open(path)?;
    let (w, h) = img.dimensions();
    let rgba = img.into_rgba8().into_raw();
    Ok(egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba))
}

fn load_ico(path: &str) -> Result<egui::ColorImage, Box<dyn std::error::Error>> {
    let file  = std::fs::File::open(path)?;
    let icon  = ico::IconDir::read(file)?;
    // Pick the largest entry.
    let entry = icon.entries().iter().max_by_key(|e| e.width() as u32 * e.height() as u32)
        .ok_or("empty ICO")?;
    let img   = entry.decode()?;
    let (w, h) = (img.width() as usize, img.height() as usize);
    Ok(egui::ColorImage::from_rgba_unmultiplied([w, h], img.rgba_data()))
}

pub fn resolve_icon_path(app_name: &str, icon_name: &str, config: &crate::gui::Config) -> Option<String> {
    if icon_name.is_empty() || !config.enable_icons { return None; }

    if let Some((cached_icon, _, _)) = get_cached_data(app_name) {
        if let Some(ref icon) = cached_icon {
            let p = Path::new(icon);
            if p.exists() {
                // Heal pre-existing symlink entries written before this fix:
                // resolve to the real path and re-save so the cache stays clean.
                if p.is_symlink() {
                    if let Ok(real) = fs::canonicalize(p) {
                        let real_str = real.to_string_lossy().into_owned();
                        if let Ok(mut cache) = APP_CACHE.lock() {
                            if let Some(entry) = cache.apps.iter_mut()
                                .find(|(n, _)| n == app_name).map(|(_, e)| e)
                            {
                                entry.icon_path = Some(real_str.clone());
                            }
                            let _ = save_cache(&cache);
                        }
                        return Some(real_str);
                    }
                    // Canonicalize failed — symlink is broken; fall through to fresh search.
                } else {
                    return Some(icon.clone());
                }
            }
            // Path doesn't exist (stale cache entry); fall through to fresh search.
        }
    }

    if Path::new(icon_name).exists() { return Some(icon_name.to_string()); }

    if let Some(appid) = icon_name.strip_prefix("steam_icon:") {
        return find_steam_icon(appid)
            .or_else(|| resolve_icon_path(app_name, appid, config));
    }

    if Path::new(icon_name).is_dir() {
        if let Some(icon_file) = find_icon_in_directory(icon_name) { return Some(icon_file); }
    }

    find_system_icon(icon_name)
}

fn find_steam_icon(appid: &str) -> Option<String> {
    let patterns = [
        format!("{}_header.jpg", appid),
        format!("{}_library_600x900.jpg", appid),
        format!("{}_library.png", appid),
        format!("{}_icon.png", appid),
        format!("{}.png", appid),
        format!("{}.jpg", appid),
        format!("{}.ico", appid),
    ];
    get_icon_search_paths().iter()
        .flat_map(|path| patterns.iter().map(move |pat| path.join(pat)))
        .find(|path| path.exists())
        .and_then(|path| path.to_str().map(String::from))
}

fn find_icon_in_directory(dir: &str) -> Option<String> {
    fs::read_dir(dir).ok()?
        .filter_map(Result::ok)
        .find(|entry| {
            entry.path().extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ICON_EXTS.contains(&ext.to_lowercase().as_str()))
                .unwrap_or(false)
        })
        .and_then(|entry| entry.path().to_str().map(String::from))
}

fn find_system_icon(icon_name: &str) -> Option<String> {
    const THEMES:     &[&str] = &["hicolor", "Adwaita", "gnome", "breeze", "oxygen"];
    const SIZES:      &[&str] = &["512x512", "256x256", "128x128", "64x64", "48x48", "32x32", "24x24", "16x16", "scalable"];
    const CATEGORIES: &[&str] = &["apps", "devices", "places", "mimetypes", "status", "actions"];

    let base_paths = get_icon_search_paths();

    // Pass 1: structured icon-theme layout  (base/theme/size/category/name.ext)
    let themed = base_paths.iter()
        .flat_map(|base| THEMES.iter().map(move |theme| base.join(theme)))
        .flat_map(|tp| SIZES.iter().map(move |sz| tp.join(sz)))
        .flat_map(|sp| CATEGORIES.iter().map(move |cat| sp.join(cat)))
        .flat_map(|cp| ICON_EXTS.iter().map(move |ext| cp.join(format!("{}.{}", icon_name, ext))))
        .find(|p| p.exists());

    if let Some(p) = themed { return p.to_str().map(String::from); }

    // Pass 2: flat directories — pixmaps layout is name.ext directly in the folder,
    // NOT theme/size/category/name.ext. Many distros ship icons here.
    let data_home = crate::paths::data_home();
    let flat_dirs: Vec<PathBuf> = [
        PathBuf::from("/usr/share/pixmaps"),
        PathBuf::from("/usr/local/share/pixmaps"),
    ].into_iter()
        .chain(crate::paths::data_dirs().into_iter().map(|d| d.join("pixmaps")))
        .chain(std::iter::once(data_home.join("icons")))
        .collect();

    flat_dirs.iter()
        .flat_map(|dir| ICON_EXTS.iter().map(move |ext| dir.join(format!("{}.{}", icon_name, ext))))
        .find(|p| p.exists())
        .and_then(|p| p.to_str().map(String::from))
}

fn get_icon_search_paths() -> Vec<PathBuf> {
    let data_home = crate::paths::data_home();
    let mut paths = vec![
        data_home.join("icons"),
        data_home.join("flatpak/exports/share/icons"),
    ];
    paths.extend(crate::paths::data_dirs().into_iter().flat_map(|dir| {
        [dir.join("icons"), dir.join("pixmaps")]
    }));
    paths.push(PathBuf::from("/usr/share/pixmaps"));
    paths.push(PathBuf::from("/var/lib/flatpak/exports/share/icons"));
    let hicolor = data_home.join("icons/hicolor");
    if hicolor.exists() { paths.push(hicolor); }
    paths
}

// ============================================================================
// Desktop entry parsing
// ============================================================================

fn parse_desktop_entry(path: &Path) -> Option<(String, String, String)> {
    let content  = fs::read_to_string(path).ok()?;
    let mut name     = None;
    let mut exec     = None;
    let mut icon     = None;
    let mut wm_class = None;

    for line in content.lines() {
        if let Some((key, value)) = line.split_once('=') {
            let value = value.trim().to_string();
            match key.trim() {
                "Name"           if name.is_none()     => name     = Some(value),
                "Exec"           if exec.is_none()     => exec     = Some(value),
                "Icon"           if icon.is_none()     => icon     = Some(value),
                "StartupWMClass" if wm_class.is_none() => wm_class = Some(value),
                _ => {}
            }
        }
    }

    let mut exec = exec?;

    // Strip all field codes per FreeDesktop Desktop Entry Specification §7.
    // File/URL codes are removed — we launch without file arguments.
    // Deprecated codes (%d %D %n %N %v %m) are removed per spec.
    for placeholder in ["%f", "%F", "%u", "%U", "%c", "%k",
                         "%d", "%D", "%n", "%N", "%v", "%m", "@@"] {
        exec = exec.replace(placeholder, "");
    }
    // %i expands to "--icon <name>" per spec; remove entirely if no icon.
    if let Some(icon_val) = &icon {
        exec = exec.replace("%i", &format!("--icon {}", icon_val));
    } else {
        exec = exec.replace("%i", "");
    }
    // StartupWMClass is a window-manager hint for taskbar matching.
    // It must NOT be passed as --class to the executable — apps like
    // Blender and EasyEffects do not accept that flag and exit silently.
    let _ = wm_class; // suppress unused-variable warning

    Some((name?, exec.trim().to_string(), icon.unwrap_or_default()))
}

fn get_desktop_entries() -> Vec<(String, String, String)> {
    let data_home = crate::paths::data_home();
    let mut app_dirs: Vec<PathBuf> = crate::paths::data_dirs().into_iter()
        .map(|d| d.join("applications"))
        .collect();
    app_dirs.push(data_home.join("applications"));
    app_dirs.push(data_home.join("flatpak/exports/share/applications"));
    app_dirs.into_iter()
        .filter_map(|dir| fs::read_dir(dir).ok())
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().map_or(false, |ext| ext == "desktop"))
        .filter_map(|entry| parse_desktop_entry(&entry.path()))
        .collect()
}

// ============================================================================
// Steam integration
// ============================================================================

fn get_steam_entries() -> Vec<(String, String, String)> {
    let home = match std::env::var("HOME") {
        Ok(home) => home,
        Err(_)   => return Vec::new(),
    };
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
    if let Ok(content) = fs::read_to_string(steam_path.join("steamapps/libraryfolders.vdf")) {
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
    let content    = fs::read_to_string(path).ok()?;
    let mut appid  = None;
    let mut name   = None;
    let mut installdir = None;

    for line in content.lines() {
        let trimmed = line.trim();
        if appid.is_none()      { appid      = extract_quoted_value(trimmed, "appid"); }
        if name.is_none()       { name       = extract_quoted_value(trimmed, "name"); }
        if installdir.is_none() { installdir = extract_quoted_value(trimmed, "installdir"); }
    }

    let appid      = appid?;
    let name       = name?;
    let installdir = installdir?;

    if !seen_appids.insert(appid.clone()) { return None; }

    let icon_path = determine_steam_icon_path(path, &appid, &installdir);
    Some((name, format!("steam steam://rungameid/{}", appid), icon_path))
}

fn extract_quoted_value(line: &str, key: &str) -> Option<String> {
    line.starts_with(&format!("\"{}\"", key))
        .then(|| line.split('"').nth(3).map(String::from))
        .flatten()
}

fn determine_steam_icon_path(manifest_path: &PathBuf, appid: &str, installdir: &str) -> String {
    let icon_dir = manifest_path.parent().unwrap().join("common").join(installdir);
    if icon_dir.exists() && has_icon_files(&icon_dir) {
        icon_dir.to_string_lossy().to_string()
    } else {
        format!("steam_icon:{}", appid)
    }
}

fn has_icon_files(dir: &PathBuf) -> bool {
    fs::read_dir(dir)
        .ok()
        .map(|entries| entries.filter_map(Result::ok).any(|entry| {
            entry.path().extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ICON_EXTS.contains(&ext.to_lowercase().as_str()))
                .unwrap_or(false)
        }))
        .unwrap_or(false)
}

// ============================================================================
// Search helpers
// ============================================================================

/// Filter `apps` by `query_lower` (already lowercased by the caller) and
/// return up to `max` indices. Storing indices avoids cloning strings on
/// every keystroke.
fn search_apps(query_lower: &str, apps: &[App], max: usize) -> Vec<usize> {
    apps.iter()
        .enumerate()
        .filter(|(_, app)| app.name_lower.contains(query_lower))
        .take(max)
        .map(|(i, _)| i)
        .collect()
}

/// Return indices of the most-recently-used apps.
///
/// Old implementation was O(n × m): for each entry in APP_CACHE it did a
/// linear scan of `apps`. The new version builds a name→index HashMap first
/// for O(1) lookups, making the whole thing O(n + m).
fn get_recent_indices(apps: &[App], config: &crate::gui::Config) -> Vec<usize> {
    let name_to_idx: HashMap<&str, usize> = apps.iter()
        .enumerate()
        .map(|(i, app)| (app.name.as_str(), i))
        .collect();

    APP_CACHE.lock()
        .ok()
        .map(|cache| {
            cache.apps.iter()
                .filter_map(|(name, _): &(String, AppEntry)| name_to_idx.get(name.as_str()).copied())
                .take(config.max_search_results)
                .collect()
        })
        .unwrap_or_default()
}

// ============================================================================
// App launch
// ============================================================================

fn extract_terminal_command(exec_cmd: &str) -> Option<String> {
    let cleaned = exec_cmd.split_whitespace().next()?.trim_start_matches("env ");
    cleaned.split('/').last()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty() && !s.starts_with('-'))
        .map(String::from)
}

fn launch_app(
    app_name: &str,
    exec_cmd: &str,
    icon_path: &str,
    options: &Option<AppLaunchOptions>,
    enable_recent_apps: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    cache_app_metadata(app_name, exec_cmd, icon_path);
    if enable_recent_apps { update_recent_apps(app_name, true)?; }

    let home_dir = std::env::var("HOME")
        .map(PathBuf::from)
        .map_err(|_| "No home directory")?;

    let (cmd, dir) = match options {
        Some(opts) => {
            let command = match &opts.custom_command {
                Some(c) if c.trim() == "%command%"     => exec_cmd.to_string(),
                Some(c) if c.contains("%command%")     => c.replace("%command%", exec_cmd),
                Some(c) => {
                    let ct = c.trim();
                    if !ct.contains(' ') { c.to_string() } else { format!("{} {}", c, exec_cmd) }
                }
                None => exec_cmd.to_string(),
            };
            let dir = opts.working_directory.as_deref()
                .unwrap_or_else(|| home_dir.to_str().unwrap_or(""));
            (command, dir)
        }
        None => (exec_cmd.to_string(), home_dir.to_str().unwrap_or("")),
    };

    let try_launch = |command_str: &str| -> Result<(), std::io::Error> {
        let mut command = Command::new("sh");
        command.arg("-c").arg(command_str).current_dir(dir);
        if let Some(opts) = options {
            for (key, value) in &opts.environment_vars { command.env(key, value); }
        }
        let mut child = command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
        std::thread::sleep(std::time::Duration::from_millis(100));
        match child.try_wait() {
            Ok(Some(status)) if !status.success() =>
                Err(std::io::Error::new(std::io::ErrorKind::Other, "Process exited with error")),
            Ok(Some(_)) | Ok(None) => Ok(()),
            Err(e) => Err(e),
        }
    };

    try_launch(&cmd)
        .or_else(|_| {
            if let Some(opts) = options {
                if let Some(custom_cmd) = &opts.custom_command { return try_launch(custom_cmd); }
            }
            Err(std::io::Error::new(std::io::ErrorKind::Other, "Custom command fallback not available"))
        })
        .or_else(|_| {
            if let Some((_, _, Some(terminal_cmd))) = get_cached_data(app_name) {
                return try_launch(&terminal_cmd);
            }
            Err(std::io::Error::new(std::io::ErrorKind::Other, "Cached terminal command not available"))
        })
        .or_else(|_| {
            if let Some(terminal_cmd) = extract_terminal_command(exec_cmd) {
                return try_launch(&terminal_cmd);
            }
            Err(std::io::Error::new(std::io::ErrorKind::Other, "All launch attempts failed"))
        })
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
}

fn parse_launch_options_input(input: &str, _original_command: Option<String>) -> AppLaunchOptions {
    let mut parts         = input.split_whitespace().peekable();
    let mut options       = AppLaunchOptions::default();
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
                if let Some(dir) = parts.next() { options.working_directory = Some(dir.to_string()); }
            }
            _ => {
                command_parts.push(part.to_string());
                command_parts.extend(parts.map(|s| s.to_string()));
                break;
            }
        }
    }

    if !command_parts.is_empty() { options.custom_command = Some(command_parts.join(" ")); }
    options
}

// ============================================================================
// AppLauncher
// ============================================================================

pub struct AppLauncher {
    query:          String,
    /// All known apps. Starts with cache contents; background scan appends new ones.
    apps:           Vec<App>,
    /// Indices into `apps` – avoids cloning strings on every query change.
    results:        Vec<usize>,
    quit:           bool,
    config:         crate::gui::Config,
    launch_options: HashMap<String, AppLaunchOptions>,
    /// Receives fresh apps from the background filesystem scan.
    pending_scan:   Arc<Mutex<Option<Vec<App>>>>,
}

impl Default for AppLauncher {
    fn default() -> Self {
        let config = crate::gui::Config::default();

        // Show cached apps immediately so the launcher is usable at once.
        let apps = get_all_cached_apps();
        let launch_options = get_launch_options();

        let results = if config.enable_recent_apps {
            get_recent_indices(&apps, &config)
        } else {
            Vec::new()
        };

        // Scan the filesystem for fresh entries on a background thread.
        // The main thread never blocks waiting for this.
        let pending_scan: Arc<Mutex<Option<Vec<App>>>> = Arc::new(Mutex::new(None));
        {
            let pending_clone = Arc::clone(&pending_scan);
            thread::spawn(move || {
                let mut fresh: Vec<App> = get_desktop_entries()
                    .into_iter()
                    .chain(get_steam_entries())
                    .map(|(name, exec, icon)| App::new(name, exec, icon))
                    .collect();

                // Deduplicate by name while preserving discovery order.
                let mut seen = HashSet::new();
                fresh.retain(|app| seen.insert(app.name.clone()));

                if let Ok(mut guard) = pending_clone.lock() {
                    *guard = Some(fresh);
                }
            });
        }

        AppLauncher { query: String::new(), apps, results, quit: false, config, launch_options, pending_scan }
    }
}

impl AppLauncher {
    /// Called each frame. Merges any freshly scanned apps into `self.apps`
    /// without blocking (uses `try_lock` so it never stalls the UI).
    fn poll_pending_scan(&mut self) {
        // try_lock: never stall the render thread waiting for the scan thread.
        let fresh = match self.pending_scan.try_lock() {
            Ok(mut guard) => guard.take(),
            Err(_)        => return,
        };

        let Some(fresh) = fresh else { return };

        // Merge: only append entries not already in self.apps.
        // Collect into owned Strings so the borrow on self.apps ends before we push.
        let existing: HashSet<String> = self.apps.iter().map(|a| a.name.clone()).collect();
        let had_apps = !self.apps.is_empty();
        for app in fresh {
            if !existing.contains(&app.name) {
                self.apps.push(app);
            }
        }

        // If we didn't have any apps before (cold start with empty cache), also
        // write a fresh snapshot of the metadata so the cache is populated.
        // Refresh the result list to pick up newly added entries.
        if !had_apps || !self.query.is_empty() {
            let q = self.query.to_lowercase();
            self.results = if q.is_empty() && self.config.enable_recent_apps {
                get_recent_indices(&self.apps, &self.config)
            } else if !q.is_empty() {
                search_apps(&q, &self.apps, self.config.max_search_results)
            } else {
                Vec::new()
            };
        }
    }

    fn launch_first_result(&mut self) {
        if let Some(&idx) = self.results.first() {
            let app     = &self.apps[idx];
            let options = self.launch_options.get(&app.name).cloned();
            if launch_app(&app.name, &app.exec, &app.icon, &options, self.config.enable_recent_apps).is_ok() {
                self.quit = true;
            }
        }
    }

    fn get_app_command(&self, app_name: &str) -> Option<String> {
        self.apps.iter().find(|a| a.name == app_name).map(|a| a.exec.clone())
    }
}

impl crate::gui::AppInterface for AppLauncher {
    fn update(&mut self) {
        // Integrate any background-scanned apps without blocking.
        self.poll_pending_scan();

        if self.quit { std::process::exit(0); }
    }

    fn handle_input(&mut self, input: &str) {
        match input {
            s if s.starts_with("LAUNCH_OPTIONS:") => {
                let parts: Vec<&str> = s.splitn(3, ':').collect();
                if parts.len() >= 3 {
                    let (app_name, opts_str) = (parts[1], parts[2]);
                    let orig_cmd = self.get_app_command(app_name);
                    let opts     = parse_launch_options_input(opts_str, orig_cmd);
                    self.launch_options.insert(app_name.to_string(), opts.clone());
                    let _ = update_launch_options(app_name, opts);
                    self.query.clear();
                }
            }
            "ESC"   => self.quit = true,
            "ENTER" => self.launch_first_result(),
            "P" if self.config.enable_power_options => crate::system::power_off(&self.config),
            "R" if self.config.enable_power_options => crate::system::restart(&self.config),
            "L" if self.config.enable_power_options => crate::system::logout(&self.config),
            _ => {
                self.query   = input.to_string();
                // Pre-lowercase once per query change, not once per app per query change.
                let q_lower  = self.query.to_lowercase();
                self.results = if self.config.enable_recent_apps && q_lower.trim().is_empty() {
                    get_recent_indices(&self.apps, &self.config)
                } else {
                    search_apps(&q_lower, &self.apps, self.config.max_search_results)
                };
            }
        }
    }

    fn should_quit(&self) -> bool { self.quit }

    fn get_query(&self) -> String { self.query.clone() }

    fn get_search_results(&self) -> Vec<String> {
        self.results.iter()
            .filter_map(|&i| self.apps.get(i))
            .map(|a| a.name.clone())
            .collect()
    }

    fn get_time(&self) -> String {
        crate::system::get_current_time(&self.config)
    }

    fn launch_app(&mut self, app_name: &str) {
        // Find by name in the result set (small, typically ≤5 items).
        if let Some(&idx) = self.results.iter().find(|&&i| self.apps[i].name == app_name) {
            let app     = &self.apps[idx];
            let options = self.launch_options.get(&app.name).cloned();
            if launch_app(&app.name, &app.exec, &app.icon, &options, self.config.enable_recent_apps).is_ok() {
                self.quit = true;
            }
        }
    }

    fn get_icon_path(&self, app_name: &str) -> Option<String> {
        self.results.iter()
            .find(|&&i| self.apps[i].name == app_name)
            .and_then(|&i| resolve_icon_path(&self.apps[i].name, &self.apps[i].icon, &self.config))
    }

    fn get_formatted_launch_options(&self, app_name: &str) -> String {
        self.launch_options.get(app_name).map(|opts| {
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
        }).unwrap_or_default()
    }
}
