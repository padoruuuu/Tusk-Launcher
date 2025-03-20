use std::{
    collections::{HashMap, VecDeque},
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time,
};

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

use eframe::egui;
use image;
use resvg::{
    tiny_skia::Pixmap,
    usvg,
};

use crate::{app_launcher::AppLaunchOptions, gui::Config};
use xdg::BaseDirectories;

static CACHE_FILE: Lazy<PathBuf> = Lazy::new(|| {
    let config_home = BaseDirectories::new()
        .map(|bd| bd.get_config_home().to_owned())
        .unwrap_or_else(|_| PathBuf::from("."));
    let mut path = config_home.join("tusk-launcher");
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
        // Reload if texture is missing or file has been modified.
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
            let opt = usvg::Options::default();
            let tree = usvg::Tree::from_data(&data, &opt)?;
            let size = tree.size().to_int_size();
            let mut pixmap = Pixmap::new(size.width(), size.height())
                .ok_or("Failed to create pixmap")?;
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

fn save_cache(cache: &AppCache) -> Result<(), Box<dyn std::error::Error>> {
    fs::write(&*CACHE_FILE, toml::to_string_pretty(cache)?)?;
    Ok(())
}

pub static APP_CACHE: Lazy<Mutex<AppCache>> = Lazy::new(|| {
    Mutex::new(if CACHE_FILE.exists() {
        toml::from_str(&fs::read_to_string(&*CACHE_FILE).expect("Failed to read cache file"))
            .expect("Failed to deserialize cache data")
    } else {
        AppCache::default()
    })
});

pub fn update_recent_apps(app_name: &str, enable_recent_apps: bool) -> Result<(), Box<dyn std::error::Error>> {
    if !enable_recent_apps {
        return Ok(());
    }
    let mut cache = APP_CACHE.lock().map_err(|e| format!("Lock error: {:?}", e))?;
    cache.recent_apps.retain(|x| x != app_name);
    cache.recent_apps.push_front(app_name.to_owned());
    if cache.recent_apps.len() > 10 {
        cache.recent_apps.pop_back();
    }
    save_cache(&cache.clone())?;
    Ok(())
}

pub fn update_launch_options(app_name: &str, options: AppLaunchOptions) -> Result<(), Box<dyn std::error::Error>> {
    let mut cache = APP_CACHE.lock().map_err(|e| format!("Lock error: {:?}", e))?;
    cache.launch_options.insert(app_name.to_owned(), options);
    save_cache(&cache.clone())?;
    Ok(())
}

pub fn get_launch_options() -> HashMap<String, AppLaunchOptions> {
    APP_CACHE
        .lock()
        .map(|c| c.launch_options.clone())
        .unwrap_or_default()
}

pub fn resolve_icon_path(icon_name: &str, config: &Config) -> Option<String> {
    if icon_name.is_empty() || !config.enable_icons {
        return None;
    }
    let icon_path = Path::new(icon_name);
    if icon_path.is_absolute() {
        return Some(icon_name.to_owned());
    }

    let cached_base = config.icon_cache_dir.join(icon_name);
    let extensions = ["png", "svg", "xpm"];
    if let Some(p) = extensions.iter().find_map(|ext| {
        let p = cached_base.with_extension(ext);
        if p.exists() { Some(p) } else { None }
    }) {
        return p.to_str().map(String::from);
    }

    let icon_sizes = ["512x512", "256x256", "128x128", "64x64", "48x48", "32x32", "24x24", "16x16", "scalable"];
    let categories = ["apps", "devices", "places", "mimetypes", "status", "actions"];
    let user_flatpak_base = BaseDirectories::new()
        .map(|bd| bd.get_data_home().to_owned())
        .unwrap_or_else(|_| PathBuf::from(".local/share"))
        .join("flatpak/exports/share/icons");
    let system_flatpak_base = PathBuf::from("/var/lib/flatpak/exports/share/icons");
    let search_paths = vec![
        user_flatpak_base,
        system_flatpak_base,
        PathBuf::from("/usr/share/icons"),
        PathBuf::from("/usr/local/share/icons"),
        BaseDirectories::new()
            .map(|bd| bd.get_data_home().to_owned())
            .unwrap_or_else(|_| PathBuf::from(".local/share"))
            .join("icons"),
        PathBuf::from("/usr/share/pixmaps"),
    ];
    let icon_themes = ["hicolor", "Adwaita", "gnome", "breeze", "oxygen"];

    let check_icon = |base: &Path, theme: &str, size: &str, category: &str, icon: &str| -> Option<PathBuf> {
        let dir = base.join(theme).join(size).join(category);
        extensions.iter().find_map(|ext| {
            let p = dir.join(format!("{}.{}", icon, ext));
            if p.exists() { Some(p) } else { None }
        })
    };

    for base in search_paths {
        if let Some(found) = icon_themes.iter().find_map(|theme| {
            icon_sizes.iter().find_map(|size| {
                categories.iter().find_map(|cat| check_icon(&base, theme, size, cat, icon_name))
            })
        }) {
            fs::create_dir_all(&config.icon_cache_dir).ok()?;
            let ext = found.extension().and_then(|s| s.to_str()).unwrap_or("");
            let cached = config.icon_cache_dir.join(icon_name).with_extension(ext);
            if fs::copy(&found, &cached).is_ok() {
                return cached.to_str().map(String::from);
            }
        }
    }

    let pixmaps = Path::new("/usr/share/pixmaps");
    if let Some(found) = extensions.iter().find_map(|ext| {
        let p = pixmaps.join(icon_name).with_extension(ext);
        if p.exists() { Some(p) } else { None }
    }) {
        fs::create_dir_all(&config.icon_cache_dir).ok()?;
        let ext = found.extension().and_then(|s| s.to_str()).unwrap_or("");
        let cached = config.icon_cache_dir.join(icon_name).with_extension(ext);
        if fs::copy(&found, &cached).is_ok() {
            return cached.to_str().map(String::from);
        }
    }
    None
}
