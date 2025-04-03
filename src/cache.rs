use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time,
};

use once_cell::sync::Lazy;
use eframe::egui;
use image;
use resvg::{tiny_skia::Pixmap, usvg};

use crate::{app_launcher::AppLaunchOptions, gui::Config};
use xdg::BaseDirectories;
use serde::{Serialize, Deserialize};

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

// Using our custom text format for now.
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
pub fn resolve_icon_path(app_name: &str, icon_name: &str, config: &Config) -> Option<String> {
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
