mod clock;
mod power;
mod cache;

use std::{
    fs,
    process::Command,
    sync::Mutex,
    path::PathBuf,
};
use xdg::BaseDirectories;
use eframe::egui;
use eframe::egui::{CentralPanel, Context, ScrollArea, TextEdit, CursorIcon, Layout, Align};
use rayon::prelude::*;
use once_cell::sync::Lazy;
use cache::{update_cache, RECENT_APPS_CACHE};

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
            name = Some(line[5..].to_string());
        } else if line.starts_with("Exec=") {
            exec = Some(line[5..].to_string());
        }
        if name.is_some() && exec.is_some() {
            break;
        }
    }
    name.zip(exec).map(|(name, exec)| {
        let cleaned_exec = exec.replace("%f", "")
            .replace("%u", "")
            .replace("%U", "")
            .replace("%F", "")
            .replace("%i", "")
            .replace("%c", "")
            .replace("%k", "")
            .trim()
            .to_string();
        (name, cleaned_exec)
    })
}

fn search_applications(query: &str, applications: &[(String, String)]) -> Vec<(String, String)> {
    applications.iter()
        .filter(|(name, _)| name.to_lowercase().contains(&query.to_lowercase()))
        .cloned()
        .take(5)
        .collect()
}

fn launch_app(app_name: &str, exec_cmd: &str) -> Result<(), Box<dyn std::error::Error>> {
    update_cache(app_name)?;

    let home_dir = dirs::home_dir().ok_or("Failed to find home directory")?;
    Command::new("sh")
        .arg("-c")
        .arg(exec_cmd)
        .current_dir(home_dir)
        .spawn()?;
    Ok(())
}

struct AppLauncher {
    query: String,
    applications: Lazy<Vec<(String, String)>>,
    search_results: Vec<(String, String)>,
    is_quit: bool,
    focus_set: bool,
}

impl Default for AppLauncher {
    fn default() -> Self {
        let applications: Lazy<Vec<(String, String)>> = Lazy::new(|| {
            get_desktop_entries()
                .par_iter()
                .filter_map(|path| parse_desktop_entry(path))
                .collect()
        });

        let recent_apps_cache = RECENT_APPS_CACHE.lock().expect("Failed to acquire read lock");

        Self {
            query: String::new(),
            search_results: recent_apps_cache.recent_apps.iter().filter_map(|app_name| {
                applications.iter().find(|(name, _)| name == app_name).cloned()
            }).take(5).collect(),
            applications,
            is_quit: false,
            focus_set: false,
        }
    }
}

impl eframe::App for AppLauncher {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint();

        if self.is_quit {
            std::process::exit(0);
        }

        ctx.input(|i| {
            if i.key_pressed(egui::Key::Escape) {
                self.is_quit = true;
            }
            if i.key_pressed(egui::Key::Enter) {
                if let Some((app_name, exec_cmd)) = self.search_results.first() {
                    if let Err(err) = launch_app(app_name, exec_cmd) {
                        eprintln!("Failed to launch app: {}", err);
                    } else {
                        self.is_quit = true;
                    }
                }
            }
        });

        CentralPanel::default().show(ctx, |ui| {
            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                let response = ui.add(TextEdit::singleline(&mut self.query).hint_text("Search..."));
                
                if !self.focus_set {
                    response.request_focus();
                    self.focus_set = true;
                }

                if response.changed() {
                    self.search_results = search_applications(&self.query, &self.applications);
                }

                ScrollArea::vertical().show(ui, |ui| {
                    for (app_name, exec_cmd) in &self.search_results {
                        if ui.button(app_name).clicked() {
                            if let Err(err) = launch_app(app_name, exec_cmd) {
                                eprintln!("Failed to launch app: {}", err);
                            } else {
                                self.is_quit = true;
                            }
                        }
                    }
                });
            });

            ui.add_space(ui.available_height() - 100.0);

            ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                ui.horizontal(|ui| {
                    if ui.button("Power").clicked() {
                        power::power_off();
                    }
                    if ui.button("Restart").clicked() {
                        power::restart();
                    }
                    if ui.button("Logout").clicked() {
                        power::logout();
                    }
                });

                ui.separator();

                ui.label(clock::get_current_time());
            });
        });

        ctx.output_mut(|o| o.cursor_icon = CursorIcon::Default);
    }
}

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        ..Default::default()
    };
    eframe::run_native(
        "Application Launcher",
        native_options,
        Box::new(|_cc| Box::new(AppLauncher::default())),
    )
}
