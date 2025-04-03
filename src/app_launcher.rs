use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    str::FromStr,
};

use xdg::BaseDirectories;
use serde::{Serialize, Deserialize};

use crate::{
    cache::{update_recent_apps, get_launch_options, update_launch_options},
    gui::AppInterface,
};
use crate::clock::get_current_time;

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct AppLaunchOptions {
    pub custom_command: Option<String>,
    pub working_directory: Option<String>,
    pub environment_vars: HashMap<String, String>,
}

// Implement Display for AppLaunchOptions so we can convert to a string for caching.
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
            // Main user applications directory where many .desktop entries reside.
            xdg.get_data_home().join("applications"),
            // For flatpak applications.
            xdg.get_data_home().join("flatpak/exports/share/applications"),
            // Specific Steam directory (if it exists).
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
        let applications = get_desktop_entries();
        let launch_options = get_launch_options();
        let results = if config.enable_recent_apps {
            use crate::cache::APP_CACHE;
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
            "P" if self.config.enable_power_options => crate::power::power_off(&self.config),
            "R" if self.config.enable_power_options => crate::power::restart(&self.config),
            "L" if self.config.enable_power_options => crate::power::logout(&self.config),
            _ => {
                self.query = input.to_string();
                self.results = if self.config.enable_recent_apps && self.query.trim().is_empty() {
                    use crate::cache::APP_CACHE;
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
            .and_then(|(name, _, icon)| crate::cache::resolve_icon_path(name, icon, &self.config))
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
