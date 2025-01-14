// app_launcher.rs
use std::{
    collections::HashMap,
    fs,
    process::Command,
    path::PathBuf,
};
use xdg::BaseDirectories;
use rayon::prelude::*;
use serde::{Serialize, Deserialize};
use crate::cache::{update_recent_apps, get_launch_options, update_launch_options};
use crate::gui::AppInterface;
use crate::config::{Config, load_config, get_current_time_in_timezone};

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct AppLaunchOptions {
    pub custom_command: Option<String>,
    pub working_directory: Option<String>,
    pub environment_vars: HashMap<String, String>,
}

fn get_desktop_entries() -> Vec<(String, String)> {
    BaseDirectories::new()
        .map(|xdg| xdg.get_data_dirs())
        .unwrap_or_default()
        .par_iter()
        .flat_map(|dir| {
            let apps_dir = dir.join("applications");
            fs::read_dir(&apps_dir)
                .ok()
                .into_iter()
                .flat_map(|entries| entries
                    .filter_map(|entry| entry.ok())
                    .filter(|entry| entry.path().extension().map_or(false, |ext| ext == "desktop"))
                    .filter_map(|entry| parse_desktop_entry(&entry.path())))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn parse_desktop_entry(path: &PathBuf) -> Option<(String, String)> {
    let content = fs::read_to_string(path).ok()?;
    let mut name = None;
    let mut exec = None;

    for line in content.lines() {
        match line.split_once('=') {
            Some(("Name", value)) => name = Some(value.trim().to_string()),
            Some(("Exec", value)) => {
                exec = Some(value.trim().to_string());
                if name.is_some() { break; }
            }
            _ => continue,
        }
    }

    name.zip(exec).map(|(name, exec)| {
        let exec = ["%f", "%u", "%U", "%F", "%i", "%c", "%k"]
            .iter()
            .fold(exec, |acc, &placeholder| acc.replace(placeholder, ""))
            .trim()
            .to_string();
        (name, exec)
    })
}

fn search_applications(query: &str, applications: &[(String, String)], max_results: usize) -> Vec<(String, String)> {
    let query = query.to_lowercase();
    applications.iter()
        .filter(|(name, _)| name.to_lowercase().contains(&query))
        .take(max_results)
        .cloned()
        .collect()
}

fn launch_app(app_name: &str, exec_cmd: &str, options: &Option<AppLaunchOptions>, enable_recent_apps: bool) -> Result<(), Box<dyn std::error::Error>> {
    if enable_recent_apps {
        update_recent_apps(app_name, true)?;
    }

    let home_dir = dirs::home_dir().ok_or("No home directory")?;
    let (cmd, dir) = if let Some(opts) = options {
        (
            opts.custom_command.as_deref().unwrap_or(exec_cmd),
            opts.working_directory.as_deref().unwrap_or_else(|| home_dir.to_str().unwrap_or(""))
        )
    } else {
        (exec_cmd, home_dir.to_str().unwrap_or(""))
    };

    let mut command = Command::new("sh");
    command.arg("-c").arg(cmd).current_dir(dir);

    if let Some(opts) = options {
        for (key, value) in &opts.environment_vars {
            command.env(key, value);
        }
    }

    command.spawn()?;
    Ok(())
}

pub struct AppLauncher {
    query: String,
    applications: Vec<(String, String)>,
    results: Vec<(String, String)>,
    quit: bool,
    config: Config,
    launch_options: HashMap<String, AppLaunchOptions>,
}

impl Default for AppLauncher {
    fn default() -> Self {
        let config = load_config();
        let applications = get_desktop_entries();
        let launch_options = get_launch_options();
        
        // Get recent apps from cache if enabled
        let results = if config.enable_recent_apps {
            use crate::cache::APP_CACHE;
            APP_CACHE.lock()
                .ok()
                .map(|cache| cache.recent_apps.iter()
                    .filter_map(|app| applications.iter()
                        .find(|(name, _)| name == app)
                        .cloned())
                    .take(config.max_search_results)
                    .collect())
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
                    let (app_name, options_str) = (parts[1], parts[2]);
                    let options = parse_launch_options_input(options_str);
                    self.launch_options.insert(app_name.to_string(), options.clone());
                    let _ = update_launch_options(app_name, options);
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
                self.results = search_applications(&self.query, &self.applications, self.config.max_search_results);
            }
        }
    }

    fn should_quit(&self) -> bool { self.quit }
    fn get_query(&self) -> String { self.query.clone() }
    fn get_search_results(&self) -> Vec<String> { self.results.iter().map(|(name, _)| name.clone()).collect() }
    fn get_time(&self) -> String { get_current_time_in_timezone(&self.config) }
    fn get_config(&self) -> &Config { &self.config }
    
    fn launch_app(&mut self, app_name: &str) {
        if let Some((_, exec_cmd)) = self.results.iter().find(|(name, _)| name == app_name) {
            let options = self.launch_options.get(app_name).cloned();
            if launch_app(app_name, exec_cmd, &options, self.config.enable_recent_apps).is_ok() {
                self.quit = true;
            }
        }
    }

    fn start_launch_options_edit(&mut self, app_name: &str) -> String {
        self.get_formatted_launch_options(app_name)
    }

    fn get_launch_options(&self, app_name: &str) -> Option<&AppLaunchOptions> {
        self.launch_options.get(app_name)
    }
}

impl AppLauncher {
    fn launch_first_result(&mut self) {
        if let Some((app_name, exec_cmd)) = self.results.first() {
            let options = self.launch_options.get(app_name).cloned();
            if launch_app(app_name, exec_cmd, &options, self.config.enable_recent_apps).is_ok() {
                self.quit = true;
            }
        }
    }

    fn get_formatted_launch_options(&self, app_name: &str) -> String {
        self.launch_options.get(app_name)
            .map(|opts| {
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
                result
            })
            .unwrap_or_default()
    }
}

fn parse_launch_options_input(input: &str) -> AppLaunchOptions {
    let mut parts = input.split_whitespace().peekable();
    let mut options = AppLaunchOptions::default();
    
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
                let mut cmd = vec![part.to_string()];
                cmd.extend(parts.map(ToString::to_string));
                options.custom_command = Some(cmd.join(" "));
                break;
            }
        }
    }
    
    options
}