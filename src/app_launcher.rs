use std::{
    collections::{HashSet, HashMap},
    fs,
    process::Command,
    path::PathBuf,
};
use xdg::BaseDirectories;
use rayon::prelude::*;
use serde::{Serialize, Deserialize};
use once_cell::sync::Lazy;
use crate::cache::{update_cache, RECENT_APPS_CACHE};
use crate::gui::AppInterface;
use crate::config::{Config, load_config, get_current_time_in_timezone};

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct AppLaunchOptions {
    pub custom_command: Option<String>,
    pub working_directory: Option<String>,
    pub environment_vars: HashMap<String, String>,
}

static LAUNCH_OPTIONS_FILE: Lazy<PathBuf> = Lazy::new(|| PathBuf::from("launch_options.json"));

fn save_launch_options(options: &HashMap<String, AppLaunchOptions>) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_string_pretty(options)?;
    fs::write(&*LAUNCH_OPTIONS_FILE, json)?;
    Ok(())
}

fn load_launch_options() -> HashMap<String, AppLaunchOptions> {
    if LAUNCH_OPTIONS_FILE.exists() {
        if let Ok(json) = fs::read_to_string(&*LAUNCH_OPTIONS_FILE) {
            if let Ok(options) = serde_json::from_str(&json) {
                return options;
            }
        }
    }
    HashMap::new()
}

fn get_desktop_entries() -> Vec<PathBuf> {
    let xdg_dirs = BaseDirectories::new().unwrap();
    let data_dirs = xdg_dirs.get_data_dirs();

    data_dirs.par_iter()
        .flat_map(|dir| {
            let desktop_files = dir.join("applications");
            fs::read_dir(&desktop_files).ok()
                .into_iter()
                .flat_map(|entries| entries.filter_map(Result::ok))
                .map(|entry| entry.path())
                .filter(|path| path.extension().map_or(false, |ext| ext == "desktop"))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn parse_desktop_entry(path: &PathBuf) -> Option<(String, String)> {
    let content = fs::read_to_string(path).ok()?;
    let mut name = None;
    let mut exec = None;
    for line in content.lines() {
        if line.starts_with("Name=") {
            name = Some(line[5..].trim().to_string());
        } else if line.starts_with("Exec=") {
            exec = Some(line[5..].trim().to_string());
        }
        if name.is_some() && exec.is_some() {
            break;
        }
    }
    name.zip(exec).map(|(name, exec)| {
        let placeholders = ["%f", "%u", "%U", "%F", "%i", "%c", "%k"];
        let cleaned_exec = placeholders.iter().fold(exec, |acc, &placeholder| 
            acc.replace(placeholder, "")
        ).trim().to_string();
        (name, cleaned_exec)
    })
}

fn search_applications(query: &str, applications: &[(String, String)], max_results: usize) -> Vec<(String, String)> {
    let query = query.to_lowercase();
    let mut unique_results = HashSet::new();
    
    applications.iter()
        .filter(|(name, _)| name.to_lowercase().contains(&query))
        .filter_map(|(name, exec)| {
            if unique_results.insert(name.clone()) {
                Some((name.clone(), exec.clone()))
            } else {
                None
            }
        })
        .take(max_results)
        .collect()
}

fn launch_app(app_name: &str, default_exec_cmd: &str, launch_options: &Option<AppLaunchOptions>, enable_recent_apps: bool) -> Result<(), Box<dyn std::error::Error>> {
    update_cache(app_name, enable_recent_apps)?;

    let home_dir = dirs::home_dir().ok_or("Failed to find home directory")?;
    
    // Determine the final command and working directory
    let (exec_cmd, working_dir) = if let Some(options) = launch_options {
        (
            options.custom_command.clone().unwrap_or_else(|| default_exec_cmd.to_string()),
            options.working_directory.clone().unwrap_or_else(|| home_dir.to_string_lossy().into_owned())
        )
    } else {
        (default_exec_cmd.to_string(), home_dir.to_string_lossy().into_owned())
    };

    // Prepare the command
    let mut cmd = Command::new("sh");
    cmd.arg("-c")
       .arg(&exec_cmd)
       .current_dir(working_dir);

    // Add any environment variables if specified
    if let Some(options) = launch_options {
        for (key, value) in &options.environment_vars {
            cmd.env(key, value);
        }
    }

    // Spawn the command
    cmd.spawn()?;
    
    Ok(())
}

pub struct AppLauncher {
    query: String,
    applications: Vec<(String, String)>,
    search_results: Vec<(String, String)>,
    is_quit: bool,
    config: Config,
    launch_options: HashMap<String, AppLaunchOptions>,
}

impl Default for AppLauncher {
    fn default() -> Self {
        let config = load_config();
        let applications: Vec<(String, String)> = get_desktop_entries()
            .par_iter()
            .filter_map(|path| parse_desktop_entry(path))
            .collect();

        let launch_options = load_launch_options();

        let search_results = if config.enable_recent_apps {
            let recent_apps_cache = RECENT_APPS_CACHE.lock().expect("Failed to acquire read lock");
            recent_apps_cache.recent_apps.iter()
                .filter_map(|app_name| {
                    applications.iter()
                        .find(|(name, _)| name == app_name)
                        .cloned()
                })
                .take(config.max_search_results)
                .collect()
        } else {
            Vec::new()
        };

        Self {
            query: String::new(),
            search_results,
            applications,
            is_quit: false,
            config,
            launch_options,
        }
    }
}

impl AppInterface for AppLauncher {
    fn update(&mut self) {
        if self.is_quit {
            std::process::exit(0);
        }
    }

    fn handle_input(&mut self, input: &str) {
        if input.starts_with("LAUNCH_OPTIONS:") {
            // Special handling for launch options
            let parts: Vec<&str> = input.split(':').collect();
            if parts.len() >= 3 {
                let app_name = parts[1];
                let options_str = parts[2];
                
                let options = parse_launch_options_input(options_str);
                self.launch_options.insert(app_name.to_string(), options);
                save_launch_options(&self.launch_options).unwrap_or_else(|e| eprintln!("Failed to save launch options: {}", e));
                
                // Reset query to prevent launch options from appearing in search
                self.query = String::new();
                return;
            }
        }

        // Rest of the existing handle_input logic
        match input {
            "ESC" => self.is_quit = true,
            "ENTER" => self.launch_first_result(),
            "P" if self.config.enable_power_options => crate::power::power_off(&self.config),
            "R" if self.config.enable_power_options => crate::power::restart(&self.config),
            "L" if self.config.enable_power_options => crate::power::logout(&self.config),

            _ => {
                self.query = input.to_string();
                self.search_results = search_applications(&self.query, &self.applications, self.config.max_search_results);
            }
        }
    }

    fn should_quit(&self) -> bool {
        self.is_quit
    }

    fn get_query(&self) -> String {
        self.query.clone()
    }

    fn get_search_results(&self) -> Vec<String> {
        self.search_results.iter().map(|(name, _)| name.clone()).collect()
    }

    fn get_time(&self) -> String {
        get_current_time_in_timezone(&self.config)
    }

    fn launch_app(&mut self, app_name: &str) {
        if let Some((_, exec_cmd)) = self.search_results.iter().find(|(name, _)| name == app_name) {
            let launch_options = self.launch_options.get(app_name).cloned();
            if let Err(err) = launch_app(app_name, exec_cmd, &launch_options, self.config.enable_recent_apps) {
                eprintln!("Failed to launch app: {}", err);
            } else {
                self.is_quit = true;
            }
        }
    }

    fn get_config(&self) -> &Config {
        &self.config
    }

    fn start_launch_options_edit(&mut self, app_name: &str) -> String {
        // Return formatted existing options if they exist
        self.get_formatted_launch_options(app_name)
    }

    fn get_launch_options(&self, app_name: &str) -> Option<&AppLaunchOptions> {
        // Return launch options if they exist
        self.launch_options.get(app_name)
    }
}

impl AppLauncher {
    fn launch_first_result(&mut self) {
        if let Some((app_name, exec_cmd)) = self.search_results.first() {
            let launch_options = self.launch_options.get(app_name).cloned();
            if let Err(err) = launch_app(app_name, exec_cmd, &launch_options, self.config.enable_recent_apps) {
                eprintln!("Failed to launch app: {}", err);
            } else {
                self.is_quit = true;
            }
        }
    }

    fn get_formatted_launch_options(&self, app_name: &str) -> String {
        // Convert launch options to a formatted string for display/editing
        if let Some(options) = self.launch_options.get(app_name) {
            let mut formatted = String::new();
            // Add environment variables first
            for (key, value) in &options.environment_vars {
                formatted.push_str(&format!("-e {}={} ", key, value));
            }
            // Add custom command and working directory if present
            if let Some(custom_command) = &options.custom_command {
                formatted.push_str(custom_command);
            }
            if let Some(working_dir) = &options.working_directory {
                formatted.push_str(&format!(" -w {}", working_dir));
            }
            formatted
        } else {
            String::new()
        }
    }
}

// Helper function to parse launch options input
fn parse_launch_options_input(input: &str) -> AppLaunchOptions {
    let mut parts = input.split_whitespace();
    let mut custom_command = None;
    let mut working_directory = None;
    let mut environment_vars = HashMap::new();
    
    while let Some(part) = parts.next() {
        if part.starts_with("-e") {
            if let Some((key, value)) = part[3..].split_once('=') {
                environment_vars.insert(key.to_string(), value.to_string());
            }
        } else if part == "-w" {
            if let Some(dir) = parts.next() {
                working_directory = Some(dir.to_string());
            }
        } else {
            // Collect the current part and all remaining parts as the custom command
            let mut full_command = vec![part.to_string()];
            full_command.extend(parts.map(|s| s.to_string()));
            custom_command = Some(full_command.join(" "));
            break;
        }
    }
    
    AppLaunchOptions {
        custom_command,
        working_directory,
        environment_vars,
    }
}