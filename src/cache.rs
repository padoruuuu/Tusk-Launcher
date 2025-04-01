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
use resvg::{tiny_skia::Pixmap, usvg};

use crate::{app_launcher::AppLaunchOptions, gui::Config};
use xdg::BaseDirectories;

static CACHE_FILE: Lazy<PathBuf> = Lazy::new(|| {
    let xdg = BaseDirectories::new()
        .map(|bd| bd.get_config_home().to_owned())
        .unwrap_or_else(|_| PathBuf::from("."));
    let mut path = xdg.join("tusk-launcher");
    fs::create_dir_all(&path).expect("Failed to create config directory");
    path.push("app_cache.toml");
    path
});

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct AppEntry {
    pub launch_options: Option<AppLaunchOptions>,
    pub icon_directory: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AppCache {
    /// Maps app names to their entries, ordered by recency
    pub apps: Vec<(String, AppEntry)>,
}

impl Default for AppCache {
    fn default() -> Self {
        Self {
            apps: Vec::new(),
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

fn save_cache(cache: &AppCache) -> Result<(), Box<dyn std::error::Error>> {
    fs::write(&*CACHE_FILE, toml::to_string_pretty(cache)?)?;
    Ok(())
}

pub static APP_CACHE: Lazy<Mutex<AppCache>> = Lazy::new(|| {
    Mutex::new(
        if CACHE_FILE.exists() {
            // Try to load the new format
            match toml::from_str::<AppCache>(&fs::read_to_string(&*CACHE_FILE).expect("Failed to read cache file")) {
                Ok(cache) => cache,
                Err(_) => {
                    // If it fails, try to load the old format and convert
                    #[derive(Deserialize)]
                    struct OldAppCache {
                        recent_apps: VecDeque<String>,
                        launch_options: HashMap<String, AppLaunchOptions>,
                        icon_directories: HashMap<String, String>,
                    }

                    match toml::from_str::<OldAppCache>(&fs::read_to_string(&*CACHE_FILE).expect("Failed to read cache file")) {
                        Ok(old_cache) => {
                            // Convert old format to new format
                            let mut new_cache = AppCache::default();
                            
                            // Add entries for all apps that have either launch options or icon directories
                            let mut all_apps: Vec<String> = old_cache.launch_options.keys().cloned().collect();
                            all_apps.extend(old_cache.icon_directories.keys().cloned());
                            all_apps.sort();
                            all_apps.dedup();
                            
                            // First add apps that are in the recent_apps list, in order
                            for app_name in old_cache.recent_apps.iter() {
                                if !new_cache.apps.iter().any(|(name, _)| name == app_name) {
                                    let entry = AppEntry {
                                        launch_options: old_cache.launch_options.get(app_name).cloned(),
                                        icon_directory: old_cache.icon_directories.get(app_name).cloned(),
                                    };
                                    new_cache.apps.push((app_name.clone(), entry));
                                }
                            }
                            
                            // Then add any remaining apps
                            for app_name in all_apps {
                                if !new_cache.apps.iter().any(|(name, _)| name == &app_name) {
                                    let entry = AppEntry {
                                        launch_options: old_cache.launch_options.get(&app_name).cloned(),
                                        icon_directory: old_cache.icon_directories.get(&app_name).cloned(),
                                    };
                                    new_cache.apps.push((app_name, entry));
                                }
                            }
                            
                            // Save the converted cache immediately
                            let _ = save_cache(&new_cache);
                            new_cache
                        },
                        Err(_) => AppCache::default(),
                    }
                }
            }
        } else {
            AppCache::default()
        },
    )
});

/// Get a list of recent app names in order from most to least recent
pub fn get_recent_apps() -> Vec<String> {
    APP_CACHE
        .lock()
        .map(|c| c.apps.iter().map(|(name, _)| name.clone()).collect())
        .unwrap_or_default()
}

pub fn update_recent_apps(
    app_name: &str,
    enable_recent_apps: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if !enable_recent_apps {
        return Ok(());
    }

    let mut cache = APP_CACHE.lock().map_err(|e| format!("Lock error: {:?}", e))?;
    
    // Remove existing entry if present
    let existing_entry = cache.apps.iter()
        .position(|(name, _)| name == app_name)
        .map(|pos| cache.apps.remove(pos).1);
    
    // Create or reuse app entry
    let entry = existing_entry.unwrap_or_default();
    
    // Add to the front (most recent)
    cache.apps.insert(0, (app_name.to_owned(), entry));
    
    // Trim if needed
    if cache.apps.len() > 10 {
        cache.apps.truncate(10);
    }
    
    save_cache(&cache)?;
    Ok(())
}

pub fn update_launch_options(
    app_name: &str,
    options: AppLaunchOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut cache = APP_CACHE.lock().map_err(|e| format!("Lock error: {:?}", e))?;
    
    // Find existing entry or position to insert new entry
    let pos = cache.apps.iter().position(|(name, _)| name == app_name);
    
    if let Some(pos) = pos {
        // Update existing entry
        cache.apps[pos].1.launch_options = Some(options);
    } else {
        // Create new entry
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

/// Resolves an icon path by first checking the cached directory path stored in app_cache.toml.
/// If not cached, searches using XDG directories and updates the app cache with the directory.
pub fn resolve_icon_path(icon_name: &str, config: &Config) -> Option<String> {
    if icon_name.is_empty() || !config.enable_icons {
        return None;
    }
    let icon_path = Path::new(icon_name);
    if icon_path.is_absolute() {
        return Some(icon_name.to_owned());
    }

    // Check for an already cached directory in the app cache
    {
        let cache = APP_CACHE.lock().ok()?;
        for (_, entry) in &cache.apps {
            if let Some(dir_path) = &entry.icon_directory {
                let extensions = ["png", "svg", "xpm"];
                for ext in &extensions {
                    let full_path = Path::new(dir_path).join(format!("{}.{}", icon_name, ext));
                    if full_path.exists() {
                        return full_path.to_str().map(String::from);
                    }
                }
            }
        }
    }

    // Get XDG base directories
    let xdg = match BaseDirectories::new() {
        Ok(xdg) => xdg,
        Err(_) => return None,
    };

    // Build search paths using XDG directories
    let mut search_paths = Vec::new();
    search_paths.push(xdg.get_data_home().join("flatpak/exports/share/icons"));
    for data_dir in xdg.get_data_dirs() {
        search_paths.push(data_dir.join("icons"));
    }
    search_paths.push(PathBuf::from("/var/lib/flatpak/exports/share/icons"));
    
    // Use XDG data dirs for pixmaps too if possible
    let pixmaps_paths: Vec<PathBuf> = xdg
        .get_data_dirs()
        .into_iter()
        .map(|dir| dir.join("pixmaps"))
        .collect();
    search_paths.extend(pixmaps_paths);
    
    // Fallback to standard pixmaps location if not found in XDG dirs
    search_paths.push(PathBuf::from("/usr/share/pixmaps"));

    let icon_themes = ["hicolor", "Adwaita", "gnome", "breeze", "oxygen"];
    let icon_sizes = [
        "512x512", "256x256", "128x128", "64x64", "48x48", "32x32", "24x24", "16x16", "scalable",
    ];
    let categories = ["apps", "devices", "places", "mimetypes", "status", "actions"];
    let extensions = ["png", "svg", "xpm"];

    let check_icon = |base: &Path, theme: &str, size: &str, category: &str, icon: &str| -> Option<(PathBuf, PathBuf)> {
        let dir = base.join(theme).join(size).join(category);
        for ext in &extensions {
            let p = dir.join(format!("{}.{}", icon, ext));
            if p.exists() { 
                return Some((p, dir.clone()));
            }
        }
        None
    };

    // Search for the icon in the provided search paths
    let found = search_paths.iter().find_map(|base| {
        icon_themes.iter().find_map(|theme| {
            icon_sizes.iter().find_map(|size| {
                categories.iter().find_map(|cat| check_icon(base, theme, size, cat, icon_name))
            })
        })
    });

    if let Some((found_path, dir_path)) = found {
        // Cache the directory where the icon was found
        if let Ok(mut cache) = APP_CACHE.lock() {
            if let Some(dir_str) = dir_path.to_str() {
                // First, try to find if there's an existing app with this exact name
                let app_pos = cache.apps.iter().position(|(name, _)| name == icon_name);
                
                if let Some(pos) = app_pos {
                    // Update existing entry
                    cache.apps[pos].1.icon_directory = Some(dir_str.to_owned());
                } else {
                    // Look for app name variants
                    // Common pattern is "org.app.Name" vs "Name"
                    let stripped_name = if icon_name.contains('.') {
                        icon_name.split('.').last().unwrap_or(icon_name)
                    } else {
                        icon_name
                    };
                    
                    // Try to find an app with a matching name (either full or base name)
                    let related_app_pos = cache.apps.iter().position(|(name, _)| {
                        if name == icon_name {
                            return true;
                        }
                        
                        if name.contains('.') && name.split('.').last().unwrap_or("") == stripped_name {
                            return true;
                        }
                        
                        if icon_name.contains('.') && icon_name.split('.').last().unwrap_or("") == name {
                            return true;
                        }
                        
                        false
                    });
                    
                    if let Some(pos) = related_app_pos {
                        // Update existing related entry
                        cache.apps[pos].1.icon_directory = Some(dir_str.to_owned());
                    } else {
                        // Create new entry if no related app found
                        let mut entry = AppEntry::default();
                        entry.icon_directory = Some(dir_str.to_owned());
                        cache.apps.push((icon_name.to_owned(), entry));
                    }
                }
                
                let _ = save_cache(&cache);
            }
        }
        return found_path.to_str().map(String::from);
    }

    // Fallback to searching directly in pixmaps directories
    for pixmap_dir in search_paths.iter().filter(|p| p.to_str().map_or(false, |s| s.contains("pixmaps"))) {
        for ext in &extensions {
            let p = pixmap_dir.join(format!("{}.{}", icon_name, ext));
            if p.exists() {
                // Cache the pixmaps directory
                if let Ok(mut cache) = APP_CACHE.lock() {
                    if let Some(dir_str) = pixmap_dir.to_str() {
                        // First, try to find if there's an existing app with this exact name
                        let app_pos = cache.apps.iter().position(|(name, _)| name == icon_name);
                        
                        if let Some(pos) = app_pos {
                            // Update existing entry
                            cache.apps[pos].1.icon_directory = Some(dir_str.to_owned());
                        } else {
                            // Look for app name variants
                            // Common pattern is "org.app.Name" vs "Name"
                            let stripped_name = if icon_name.contains('.') {
                                icon_name.split('.').last().unwrap_or(icon_name)
                            } else {
                                icon_name
                            };
                            
                            // Try to find an app with a matching name (either full or base name)
                            let related_app_pos = cache.apps.iter().position(|(name, _)| {
                                if name == icon_name {
                                    return true;
                                }
                                
                                if name.contains('.') && name.split('.').last().unwrap_or("") == stripped_name {
                                    return true;
                                }
                                
                                if icon_name.contains('.') && icon_name.split('.').last().unwrap_or("") == name {
                                    return true;
                                }
                                
                                false
                            });
                            
                            if let Some(pos) = related_app_pos {
                                // Update existing related entry
                                cache.apps[pos].1.icon_directory = Some(dir_str.to_owned());
                            } else {
                                // Create new entry if no related app found
                                let mut entry = AppEntry::default();
                                entry.icon_directory = Some(dir_str.to_owned());
                                cache.apps.push((icon_name.to_owned(), entry));
                            }
                        }
                        
                        let _ = save_cache(&cache);
                    }
                }
                return p.to_str().map(String::from);
            }
        }
    }
    
    None
}