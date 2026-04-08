use std::{env, fs, path::PathBuf};

/// Returns `$XDG_CONFIG_HOME` if set and absolute, otherwise `$HOME/.config`.
pub fn config_home() -> PathBuf {
    env::var("XDG_CONFIG_HOME")
        .ok()
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(".config"))
}

/// Returns `$XDG_DATA_HOME` if set and absolute, otherwise `$HOME/.local/share`.
pub fn data_home() -> PathBuf {
    env::var("XDG_DATA_HOME")
        .ok()
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(".local/share"))
}

/// Returns the colon-separated `$XDG_DATA_DIRS` list, falling back to
/// `/usr/local/share:/usr/share`. Empty components are skipped.
pub fn data_dirs() -> Vec<PathBuf> {
    env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/local/share:/usr/share".into())
        .split(':')
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect()
}

/// Resolves `relative` under `config_home()`, creates all parent directories,
/// and returns the full path.
pub fn place_config_file(relative: &str) -> std::io::Result<PathBuf> {
    let path = config_home().join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(path)
}

fn home() -> PathBuf {
    PathBuf::from(env::var("HOME").unwrap_or_else(|_| ".".into()))
}
