use std::{fs, process::{Command, Stdio}, sync::RwLock, time::SystemTime};
use std::collections::VecDeque;
use std::path::PathBuf;
use xdg::BaseDirectories;
use chrono::prelude::*;
use gio::prelude::*;
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, Entry, ListBox, ListBoxRow, Label, Separator, Button, Box as GtkBox, Orientation, ScrolledWindow};
use once_cell::sync::Lazy;
use serde::{Serialize, Deserialize};
use bincode::{serialize, deserialize};
use glib::clone;
use dirs;

static RECENT_APPS_FILE: Lazy<PathBuf> = Lazy::new(|| PathBuf::from("recent_apps.bin"));

#[derive(Serialize, Deserialize)]
struct RecentAppsCache { recent_apps: VecDeque<String> }

fn save_cache<T: Serialize>(file: &PathBuf, cache: &T) -> Result<(), Box<dyn std::error::Error>> {
    let data = serialize(cache)?;
    fs::write(file, data)?;
    Ok(())
}

static RECENT_APPS_CACHE: Lazy<RwLock<RecentAppsCache>> = Lazy::new(|| {
    let recent_apps = if RECENT_APPS_FILE.exists() {
        let data = fs::read(&*RECENT_APPS_FILE).expect("Failed to read recent apps file");
        deserialize(&data).expect("Failed to deserialize recent apps data")
    } else { VecDeque::new() };
    RwLock::new(RecentAppsCache { recent_apps })
});

fn get_desktop_entries() -> Vec<String> {
    let xdg_dirs = BaseDirectories::new().unwrap();
    let data_dirs = xdg_dirs.get_data_dirs();

    let mut entries = Vec::new();
    for dir in data_dirs {
        let desktop_files = dir.join("applications");
        if let Ok(entries_iter) = fs::read_dir(desktop_files) {
            for entry in entries_iter {
                if let Ok(entry) = entry {
                    if let Some(path) = entry.path().to_str() {
                        if path.ends_with(".desktop") {
                            entries.push(path.to_string());
                        }
                    }
                }
            }
        }
    }
    entries
}

fn parse_desktop_entry(path: &str) -> Option<(String, String)> {
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
    if let (Some(name), Some(exec)) = (name, exec) {
        // Clean up the Exec command
        let cleaned_exec = exec.replace("%f", "")
                               .replace("%u", "")
                               .replace("%U", "")
                               .replace("%F", "")
                               .replace("%i", "")
                               .replace("%c", "")
                               .replace("%k", "")
                               .trim()
                               .to_string();
        Some((name, cleaned_exec))
    } else {
        None
    }
}

fn search_applications(query: &str, applications: &[(String, String)]) -> Vec<(String, String)> {
    applications
        .iter()
        .filter(|(name, _)| name.to_lowercase().contains(&query.to_lowercase()))
        .cloned()
        .collect()
}

fn create_row(app_name: &str) -> ListBoxRow {
    let row = ListBoxRow::new();
    let label = Label::new(Some(app_name));
    label.set_halign(gtk::Align::Start);
    row.add(&label);
    row
}

fn launch_app(app_name: &str, exec_cmd: &str, window: &ApplicationWindow) -> Result<(), Box<dyn std::error::Error>> {
    let mut cache = RECENT_APPS_CACHE.write().map_err(|e| format!("Lock error: {:?}", e))?;
    cache.recent_apps.retain(|x| x != app_name);
    cache.recent_apps.push_front(app_name.to_string());
    if cache.recent_apps.len() > 10 { cache.recent_apps.pop_back(); }
    save_cache(&RECENT_APPS_FILE, &*cache)?;

    let home_dir = dirs::home_dir().ok_or("Failed to find home directory")?;
    Command::new("sh")
        .arg("-c")
        .arg(exec_cmd)
        .current_dir(home_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    window.close();
    Ok(())
}

fn handle_activate(_entry: &Entry, list_box: &ListBox, window: &ApplicationWindow, applications: &[(String, String)]) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(row) = list_box.get_row_at_index(0) {
        if let Some(label) = row.get_child().and_then(|child| child.downcast::<Label>().ok()) {
            let app_name = label.get_text().to_string();
            if let Some((_, exec_cmd)) = applications.iter().find(|(name, _)| name == &app_name) {
                launch_app(&app_name, exec_cmd, window)?;
            }
        }
    }
    Ok(())
}

fn main() {
    let applications: Vec<(String, String)> = get_desktop_entries()
        .iter()
        .filter_map(|path| parse_desktop_entry(path))
        .collect();

    let application = Application::new(Some("com.example.GtkApplication"), Default::default()).expect("Failed to initialize GTK application");

    application.connect_activate(move |app| {
        let window = ApplicationWindow::new(app);
        window.set_title("Application Launcher");
        window.set_default_size(300, 200);

        let vbox = GtkBox::new(Orientation::Vertical, 1);
        let entry = Entry::new();
        let scrolled_window = ScrolledWindow::new(gtk::NONE_ADJUSTMENT, gtk::NONE_ADJUSTMENT);
        let list_box = ListBox::new();

        for app_name in &RECENT_APPS_CACHE.read().expect("Failed to acquire read lock").recent_apps {
            list_box.add(&create_row(app_name));
        }
        list_box.show_all();

        entry.connect_changed(clone!(@strong list_box, @strong applications => move |entry| {
            let input = entry.get_text().to_lowercase();
            list_box.foreach(|child| list_box.remove(child));

            let mut entries: Vec<_> = search_applications(&input, &applications);
            entries.sort();
            entries.truncate(9);

            for (app_name, _) in entries {
                list_box.add(&create_row(&app_name));
            }

            list_box.show_all();
        }));

        let entry_clone = entry.clone();
        entry.connect_activate(clone!(@strong list_box, @strong window, @strong applications => move |_entry| {
            if let Err(err) = handle_activate(&entry_clone, &list_box, &window, &applications) {
                eprintln!("Failed to handle activate: {}", err);
            }
        }));

        list_box.connect_row_activated(clone!(@strong window, @strong applications => move |_list_box, row| {
            if let Some(label) = row.get_child().and_then(|child| child.downcast::<Label>().ok()) {
                let app_name = label.get_text().to_string();
                if let Some((_, exec_cmd)) = applications.iter().find(|(name, _)| name == &app_name) {
                    if let Err(err) = launch_app(&app_name, exec_cmd, &window) {
                        eprintln!("Failed to launch app: {}", err);
                    }
                }
            }
        }));

        scrolled_window.add(&list_box);
        vbox.pack_start(&entry, false, false, 0);
        vbox.pack_start(&scrolled_window, true, true, 0);

        let separator = Separator::new(Orientation::Horizontal);
        vbox.pack_start(&separator, false, false, 0);

        let datetime: DateTime<Local> = SystemTime::now().into();
        let label = Label::new(Some(&datetime.format("%I:%M %p %m/%d/%Y").to_string()));
        label.set_halign(gtk::Align::Start);
        vbox.pack_start(&label, false, false, 0);

        let hbox = GtkBox::new(Orientation::Horizontal, 0);

        let power_button = Button::with_label("Power");
        power_button.connect_clicked(|_| { Command::new("shutdown").arg("-h").arg("now").spawn().expect("Failed to execute shutdown command"); });
        hbox.pack_start(&power_button, false, false, 0);

        let restart_button = Button::with_label("Restart");
        restart_button.connect_clicked(|_| { Command::new("reboot").spawn().expect("Failed to execute reboot command"); });
        hbox.pack_start(&restart_button, false, false, 0);

        let logout_button = Button::with_label("Logout");
        logout_button.connect_clicked(|_| { Command::new("logout").spawn().expect("Failed to execute logout command"); });
        hbox.pack_start(&logout_button, false, false, 0);

        vbox.pack_start(&hbox, false, false, 0);

        window.add(&vbox);
        window.show_all();
    });

    application.run(&[]);
}

