use std::{
    collections::HashMap,
    fs,
    path::Path,
    process::{Command, Stdio},
    str::FromStr,
};

use xdg::BaseDirectories;
use serde::{Serialize, Deserialize};

use crate::{
    cache::{update_recent_apps, get_launch_options, update_launch_options, resolve_icon_path, APP_CACHE},
    gui::{AppInterface, Config},
    clock::get_current_time,
    power,
};

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
                .collect::<Vec<_>>()
                .join(",")
        )
    }
}

impl FromStr for AppLaunchOptions {
    type Err = String;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.splitn(3, '|').collect();
        if parts.len() != 3 {
            return Err("Invalid format".to_string());
        }
        
        let environment_vars = if parts[2].is_empty() {
            HashMap::new()
        } else {
            parts[2].split(',')
                .filter_map(|e| e.split_once('='))
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        
        Ok(AppLaunchOptions {
            custom_command: if parts[0].is_empty() { None } else { Some(parts[0].to_string()) },
            working_directory: if parts[1].is_empty() { None } else { Some(parts[1].to_string()) },
            environment_vars,
        })
    }
}

type AppEntry = (String, String, String);

fn parse_desktop_entry(path: &Path) -> Option<AppEntry> {
    let content = fs::read_to_string(path).ok()?;
    let mut name = None;
    let mut exec = None;
    let mut icon = None;
    let mut wm_class = None;
    
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
    
    // Clean exec command
    for ph in ["%f", "%F", "%u", "%U", "%c", "%k", "@@"] {
        exec = exec.replace(ph, "");
    }
    exec = exec.replace("%i", &format!("--icon {}", icon)).trim().to_string();
    
    // Add WM class for non-flatpak apps
    if let Some(class) = wm_class {
        if !exec.contains("flatpak run") {
            exec.push_str(&format!(" --class {}", class));
        }
    }
    
    Some((name, exec, icon))
}

fn get_desktop_entries() -> Vec<AppEntry> {
    let Ok(xdg) = BaseDirectories::new() else { return Vec::new() };
    let mut entries = Vec::new();
    
    let process_dir = |dir: &Path, entries: &mut Vec<AppEntry>| {
        if let Ok(read_dir) = fs::read_dir(dir) {
            entries.extend(
                read_dir
                    .filter_map(Result::ok)
                    .filter(|e| e.path().extension().map_or(false, |ext| ext == "desktop"))
                    .filter_map(|e| parse_desktop_entry(&e.path()))
            );
        }
    };
    
    // System directories
    for dir in xdg.get_data_dirs() {
        process_dir(&dir.join("applications"), &mut entries);
    }
    
    // User directories
    let user_dirs = [
        xdg.get_data_home().join("applications"),
        xdg.get_data_home().join("flatpak/exports/share/applications"),
        xdg.get_data_home().join("applications/steam"),
    ];
    
    for dir in user_dirs {
        process_dir(&dir, &mut entries);
    }
    
    entries
}

fn search_applications(query: &str, apps: &[AppEntry], max_results: usize) -> Vec<AppEntry> {
    let query = query.to_lowercase();
    apps.iter()
        .filter(|(name, _, _)| name.to_lowercase().contains(&query))
        .take(max_results)
        .cloned()
        .collect()
}

fn launch_app(app_name: &str, exec_cmd: &str, options: &Option<AppLaunchOptions>, enable_recent_apps: bool) -> Result<(), Box<dyn std::error::Error>> {
    if enable_recent_apps {
        update_recent_apps(app_name, true)?;
    }
    
    let home_dir = std::env::var("HOME").map_err(|_| "No home directory")?;
    
    let (cmd, dir) = if let Some(opts) = options {
        let command = match &opts.custom_command {
            Some(custom) if custom.trim() == "%command%" => exec_cmd.to_string(),
            Some(custom) if custom.contains("%command%") => custom.replace("%command%", exec_cmd),
            Some(custom) => format!("{} {}", custom, exec_cmd),
            None => exec_cmd.to_string(),
        };
        (command, opts.working_directory.as_deref().unwrap_or(&home_dir))
    } else {
        (exec_cmd.to_string(), &home_dir)
    };
    
    let mut command = Command::new("sh");
    command
        .args(["-c", &cmd])
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    
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
    applications: Vec<AppEntry>,
    results: Vec<AppEntry>,
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
            APP_CACHE.lock().ok()
                .map(|cache| {
                    cache.apps.iter()
                        .filter_map(|(name, _)| 
                            applications.iter().find(|(app_name, _, _)| app_name == name).cloned()
                        )
                        .take(config.max_search_results)
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        
        Self { query: String::new(), results, applications, quit: false, config, launch_options }
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
                    APP_CACHE.lock().ok()
                        .map(|cache| {
                            cache.apps.iter()
                                .filter_map(|(name, _)| 
                                    self.applications.iter().find(|(app_name, _, _)| app_name == name).cloned()
                                )
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

    fn should_quit(&self) -> bool { self.quit }
    fn get_query(&self) -> String { self.query.clone() }
    fn get_search_results(&self) -> Vec<String> {
        self.results.iter().map(|(name, _, _)| name.clone()).collect()
    }
    fn get_time(&self) -> String { get_current_time(&self.config) }

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
                
                result.trim().to_string()
            })
            .unwrap_or_default()
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
    
    if !command_parts.is_empty() {
        options.custom_command = Some(command_parts.join(" "));
    }
    
    options
}
