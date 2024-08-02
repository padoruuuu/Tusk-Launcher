use std::path::PathBuf;
use std::fs;
use xdg::BaseDirectories;
use rayon::prelude::*;

pub fn get_desktop_entries() -> Vec<PathBuf> {
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

pub fn parse_desktop_entry(path: &PathBuf) -> Option<(String, String)> {
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