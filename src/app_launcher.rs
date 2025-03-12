use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    process::{Command, Stdio},
};
use xdg::BaseDirectories;
use rayon::prelude::*;
use serde::{Serialize, Deserialize};
use crate::{
    config::Config,
    cache::{update_recent_apps, get_launch_options, update_launch_options, resolve_icon_path},
};
use crate::gui::AppInterface;
use crate::config::{load_config, get_current_time_in_timezone};

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct AppLaunchOptions {
    pub custom_command: Option<String>,
    pub working_directory: Option<String>,
    pub environment_vars: HashMap<String, String>,
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
    for ph in ["%f", "%u", "%U", "%F"] {
        exec = exec.replace(ph, "");
    }
    exec = exec.replace("%i", &format!("--icon {}", icon)).trim().to_string();
    if let Some(class) = wm_class {
        exec.push_str(&format!(" --class {}", class));
    }
    Some((name, exec, icon))
}

fn get_desktop_entries() -> Vec<(String, String, String)> {
    BaseDirectories::new()
        .map(|xdg| xdg.get_data_dirs())
        .unwrap_or_default()
        .par_iter()
        .flat_map(|dir| {
            let apps_dir = dir.join("applications");
            fs::read_dir(&apps_dir)
                .ok()
                .into_iter()
                .flat_map(|entries| {
                    entries.filter_map(Result::ok)
                        .filter(|e| e.path().extension().map_or(false, |ext| ext == "desktop"))
                        .filter_map(|e| parse_desktop_entry(&e.path()))
                })
                .collect::<Vec<_>>()
        })
        .collect()
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
        let command = match &opts.custom_command {
            Some(custom_cmd) if custom_cmd.contains("%command%") => {
                // Replace %command% with the original exec_cmd at launch time
                custom_cmd.replace("%command%", exec_cmd)
            },
            Some(custom_cmd) => {
                // No %command% placeholder, use as a prefix to the original command
                format!("{} {}", custom_cmd, exec_cmd)
            },
            None => {
                // No custom command defined, use original
                exec_cmd.to_string()
            }
        };
        
        (
            command,
            opts.working_directory.as_deref().unwrap_or_else(|| home_dir.to_str().unwrap_or("")),
        )
    } else {
        (exec_cmd.to_string(), home_dir.to_str().unwrap_or(""))
    };
    
    // Create a command that includes environment variables if present
    let mut command = Command::new("sh");
    command.arg("-c").arg(&cmd).current_dir(dir);
    
    // Add environment variables if specified
    if let Some(opts) = options {
        for (key, value) in &opts.environment_vars {
            command.env(key, value);
        }
    }
    
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(())
}

pub struct AppLauncher {
    query: String,
    applications: Vec<(String, String, String)>,
    results: Vec<(String, String, String)>,
    quit: bool,
    config: Config,
    launch_options: HashMap<String, AppLaunchOptions>,
}

impl Default for AppLauncher {
    fn default() -> Self {
        let config = load_config();
        let applications = get_desktop_entries();
        let launch_options = get_launch_options();
        let results = if config.enable_recent_apps {
            use crate::cache::APP_CACHE;
            APP_CACHE
                .lock()
                .ok()
                .map(|cache| {
                    cache.recent_apps.iter()
                        .filter_map(|app| applications.iter().find(|(name, _, _)| name == app).cloned())
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
                            cache.recent_apps.iter()
                                .filter_map(|app| self.applications.iter().find(|(name, _, _)| name == app).cloned())
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
        get_current_time_in_timezone(&self.config)
    }

    fn get_config(&self) -> &Config {
        &self.config
    }

    fn launch_app(&mut self, app_name: &str) {
        if let Some((_, exec_cmd, _)) = self.results.iter().find(|(name, _, _)| name == app_name) {
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

    fn get_icon_path(&self, app_name: &str) -> Option<String> {
        self.results
            .iter()
            .find(|(name, _, _)| name == app_name)
            .and_then(|(_, _, icon)| resolve_icon_path(icon, &self.config))
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

    fn get_formatted_launch_options(&self, app_name: &str) -> String {
        self.launch_options.get(app_name).map(|opts| {
            let mut result = String::new();
            for (k, v) in &opts.environment_vars {
                result.push_str(&format!("-e {}={} ", k, v));
            }
            if let Some(cmd) = &opts.custom_command {
                result.push_str(cmd);
            }
            if let Some(dir) = &opts.working_directory {
                result.push_str(&format!(" -w {}", dir));
            }
            result
        }).unwrap_or_default()
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
                // If we encounter something that's not a flag, we're starting the command part
                command_parts.push(part.to_string());
                command_parts.extend(parts.map(|s| s.to_string()));
                break;
            }
        }
    }

    // Join command parts with spaces
    let input_command = if !command_parts.is_empty() {
        command_parts.join(" ")
    } else {
        String::new()
    };

    // Set the custom command only if user provided one
    if !input_command.is_empty() {
        options.custom_command = Some(input_command);
    }

    options
}