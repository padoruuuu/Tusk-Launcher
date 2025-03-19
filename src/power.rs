use std::process::Command;
use std::env;
use crate::gui::Config;

fn try_command(command_str: &str) -> bool {
    // Replace $XDG_SESSION_ID with actual session ID if present
    let command_str = if command_str.contains("$XDG_SESSION_ID") {
        if let Ok(session_id) = env::var("XDG_SESSION_ID") {
            command_str.replace("$XDG_SESSION_ID", &session_id)
        } else {
            return false;
        }
    } else {
        command_str.to_string()
    };

    // Split command string into program and arguments
    let mut parts = command_str.split_whitespace();
    if let Some(program) = parts.next() {
        Command::new(program)
            .args(parts)
            .spawn()
            .is_ok()
    } else {
        false
    }
}

fn try_commands(commands: &[String]) -> bool {
    commands.iter().any(|cmd| try_command(cmd))
}

pub fn power_off(config: &Config) {
    if !try_commands(&config.power_commands) {
        eprintln!("Failed to power off: No working power off commands found in config");
    }
}

pub fn restart(config: &Config) {
    if !try_commands(&config.restart_commands) {
        eprintln!("Failed to restart: No working restart commands found in config");
    }
}

pub fn logout(config: &Config) {
    if !try_commands(&config.logout_commands) {
        eprintln!("Failed to logout: No working logout commands found in config");
    }
}
