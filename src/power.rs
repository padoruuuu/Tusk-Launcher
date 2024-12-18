use std::process::Command;
use std::path::Path;
use std::env;

const POWER_COMMANDS: [(&str, &[&str]); 4] = [
    ("systemctl", &["poweroff"]),
    ("shutdown", &["-h", "now"]),
    ("poweroff", &[]),
    ("halt", &[]),
];

const RESTART_COMMANDS: [(&str, &[&str]); 3] = [
    ("systemctl", &["reboot"]),
    ("reboot", &[]),
    ("shutdown", &["-r", "now"]),
];

const LOGOUT_COMMANDS: [(&str, &[&str]); 5] = [
    ("hyprctl", &["dispatch", "exit"]),
    ("swaymsg", &["exit"]),
    ("gnome-session-quit", &["--logout", "--no-prompt"]),
    ("qdbus", &["org.kde.KSMServer", "/KSMServer", "logout", "0", "0", "0"]),
    ("loginctl", &["terminate-session", "$XDG_SESSION_ID"]),
];

fn command_exists(cmd: &str) -> bool {
    ["/usr/bin/", "/bin/", "/usr/local/bin/"]
        .iter()
        .any(|path| Path::new(&format!("{}{}", path, cmd)).exists())
}

fn try_commands(commands: &[(&str, &[&str])]) -> bool {
    commands.iter().any(|(cmd, args)| 
        command_exists(cmd) && 
        Command::new(cmd).args(*args).spawn().is_ok()
    )
}

fn desktop_specific_logout() -> bool {
    env::var("XDG_CURRENT_DESKTOP")
        .map(|session| match session.as_str() {
            "Hyprland" => command_exists("hyprctl") && 
                Command::new("hyprctl").args(&["dispatch", "exit"]).spawn().is_ok(),
            "GNOME" => command_exists("gnome-session-quit") && 
                Command::new("gnome-session-quit").args(&["--logout", "--no-prompt"]).spawn().is_ok(),
            "KDE" => command_exists("qdbus") && 
                Command::new("qdbus").args(&["org.kde.KSMServer", "/KSMServer", "logout", "0", "0", "0"]).spawn().is_ok(),
            _ => false
        })
        .unwrap_or(false)
}

pub fn power_off() {
    if !try_commands(&POWER_COMMANDS) {
        eprintln!("Failed to power off: No known command available");
    }
}

pub fn restart() {
    if !try_commands(&RESTART_COMMANDS) {
        eprintln!("Failed to restart: No known command available");
    }
}

pub fn logout() {
    if desktop_specific_logout() || !try_commands(&LOGOUT_COMMANDS) {
        eprintln!("Failed to logout: No known command available");
    }
}