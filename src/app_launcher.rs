use crate::desktop_entry::{get_desktop_entries, parse_desktop_entry};
use crate::recent_apps::RecentAppsCache;
use crate::utils::launch_app;

use std::collections::VecDeque;
use rayon::prelude::*;

#[derive(Clone)]
pub struct AppLauncher {
    applications: Vec<(String, String)>,
    recent_apps_cache: RecentAppsCache,
}

impl AppLauncher {
    pub fn new() -> Self {
        let applications = get_desktop_entries()
            .par_iter()
            .filter_map(|path| parse_desktop_entry(path))
            .collect();

        let recent_apps_cache = RecentAppsCache::new();

        Self {
            applications,
            recent_apps_cache,
        }
    }

    pub fn search_applications(&self, query: &str) -> Vec<(String, String)> {
        self.applications
            .iter()
            .filter(|(name, _)| name.to_lowercase().contains(&query.to_lowercase()))
            .cloned()
            .take(5)
            .collect()
    }

    pub fn get_recent_apps(&self) -> VecDeque<String> {
        self.recent_apps_cache.get_recent_apps()
    }

    pub fn launch_app(&mut self, app_name: &str, exec_cmd: &str) -> Result<(), Box<dyn std::error::Error>> {
        launch_app(app_name, exec_cmd)?;
        self.recent_apps_cache.add_recent_app(app_name);
        Ok(())
    }

    pub fn get_applications(&self) -> &Vec<(String, String)> {
        &self.applications
    }
}