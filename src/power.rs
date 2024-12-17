use std::process::Command;
use std::path::Path;
use std::env;

/// Power management configuration for different environments
struct PowerConfig {
    power_off_commands: Vec<(&'static str, Vec<&'static str>)>,
    restart_commands: Vec<(&'static str, Vec<&'static str>)>,
    logout_commands: Vec<(&'static str, Vec<&'static str>)>,
}

/// Generic power management implementation
impl PowerConfig {
    /// Create a default configuration with universal Linux commands
    fn default() -> Self {
        Self {
            power_off_commands: vec![
                ("systemctl", vec!["poweroff"]),
                ("shutdown", vec!["-h", "now"]),
                ("poweroff", vec![]),
                ("halt", vec![]),
            ],
            restart_commands: vec![
                ("systemctl", vec!["reboot"]),
                ("reboot", vec![]),
                ("shutdown", vec!["-r", "now"]),
            ],
            logout_commands: vec![
                // Hyprland
                ("hyprctl", vec!["dispatch", "exit"]),
                // Sway
                ("swaymsg", vec!["exit"]),
                // GNOME
                ("gnome-session-quit", vec!["--logout", "--no-prompt"]),
                // KDE
                ("qdbus", vec!["org.kde.KSMServer", "/KSMServer", "logout", "0", "0", "0"]),
                // Generic fallback
                ("loginctl", vec!["terminate-session", "$XDG_SESSION_ID"]),
            ],
        }
    }

    /// Check if a command exists in standard paths
    fn command_exists(command: &str) -> bool {
        let paths = ["/usr/bin/", "/bin/", "/usr/local/bin/"];
        paths.iter().any(|path| Path::new(&format!("{}{}", path, command)).exists())
    }

    /// Execute a system command
    fn execute_command(command: &str, args: &[&str]) -> Result<(), String> {
        Command::new(command)
            .args(args)
            .spawn()
            .map_err(|e| format!("Failed to execute {}: {}", command, e))?;
        Ok(())
    }

    /// Attempt to power off the system
    pub fn power_off(&self) {
        for (cmd, args) in &self.power_off_commands {
            if Self::command_exists(cmd) {
                if Self::execute_command(cmd, args).is_ok() {
                    return;
                }
            }
        }
        eprintln!("Failed to power off: No known command available");
    }

    /// Attempt to restart the system
    pub fn restart(&self) {
        for (cmd, args) in &self.restart_commands {
            if Self::command_exists(cmd) {
                if Self::execute_command(cmd, args).is_ok() {
                    return;
                }
            }
        }
        eprintln!("Failed to restart: No known command available");
    }

    /// Attempt to logout of the current session
    pub fn logout(&self) {
        // Special handling for environment-specific logout
        if let Ok(desktop_session) = env::var("XDG_CURRENT_DESKTOP") {
            match desktop_session.as_str() {
                "Hyprland" => {
                    if Self::command_exists("hyprctl") {
                        let _ = Self::execute_command("hyprctl", &["dispatch", "exit"]);
                        return;
                    }
                },
                "GNOME" => {
                    if Self::command_exists("gnome-session-quit") {
                        let _ = Self::execute_command("gnome-session-quit", &["--logout", "--no-prompt"]);
                        return;
                    }
                },
                "KDE" => {
                    if Self::command_exists("qdbus") {
                        let _ = Self::execute_command("qdbus", &["org.kde.KSMServer", "/KSMServer", "logout", "0", "0", "0"]);
                        return;
                    }
                },
                _ => {}
            }
        }

        // Fallback to generic logout commands
        for (cmd, args) in &self.logout_commands {
            if Self::command_exists(cmd) {
                if Self::execute_command(cmd, args).is_ok() {
                    return;
                }
            }
        }
        eprintln!("Failed to logout: No known command available");
    }
}

// Public functions for easy usage
pub fn power_off() {
    PowerConfig::default().power_off();
}

pub fn restart() {
    PowerConfig::default().restart();
}

pub fn logout() {
    PowerConfig::default().logout();
}