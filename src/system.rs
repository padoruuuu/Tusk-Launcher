use std::env;
use std::error::Error;
use std::process::Command;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use time::OffsetDateTime;
use crate::gui::{Config, format_datetime};

// ============================================================================
// Shared Helpers
// ============================================================================

/// Spawns a command from a string like "program arg1 arg2", optionally
/// substituting `$XDG_SESSION_ID` with the real value. Returns `true` on success.
fn spawn_command(command_str: &str) -> bool {
    let command_str = if command_str.contains("$XDG_SESSION_ID") {
        match env::var("XDG_SESSION_ID") {
            Ok(id) => command_str.replace("$XDG_SESSION_ID", &id),
            Err(_) => return false,
        }
    } else {
        command_str.to_string()
    };

    let mut parts = command_str.split_whitespace();
    match parts.next() {
        Some(program) => Command::new(program).args(parts).spawn().is_ok(),
        None => false,
    }
}

/// Tries each command in order, stopping at the first one that succeeds.
fn try_commands(commands: &[String]) -> bool {
    commands.iter().any(|cmd| spawn_command(cmd))
}

// ============================================================================
// Clock
// ============================================================================

pub fn get_current_time(config: &Config) -> String {
    format_datetime(
        &OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc()),
        config,
    )
}

// ============================================================================
// Audio
// ============================================================================

pub struct AudioController {
    volume: Arc<Mutex<f32>>,
    max_volume: f32,
    enabled: bool,
}

impl AudioController {
    pub fn new(config: &Config) -> Result<Self, Box<dyn Error>> {
        let volume = if config.enable_audio_control {
            Self::get_current_volume()?
        } else {
            0.0
        };

        Ok(AudioController {
            volume: Arc::new(Mutex::new(volume)),
            max_volume: config.max_volume,
            enabled: config.enable_audio_control,
        })
    }

    fn get_current_volume() -> Result<f32, Box<dyn Error>> {
        let output = Command::new("wpctl")
            .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
            .output()?;

        let volume_str = String::from_utf8(output.stdout)?;
        let volume = volume_str
            .split_whitespace()
            .nth(1)
            .and_then(|v| f32::from_str(v).ok())
            .ok_or("Failed to parse volume")?;

        Ok(volume)
    }

    pub fn set_volume(&self, new_volume: f32) -> Result<(), Box<dyn Error>> {
        if !self.enabled {
            return Ok(());
        }

        let clamped = new_volume.clamp(0.0, self.max_volume);

        Command::new("wpctl")
            .args(["set-volume", "@DEFAULT_AUDIO_SINK@", &format!("{:.2}", clamped)])
            .output()?;

        *self.volume.lock().unwrap() = clamped;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn update_volume(&self) -> Result<(), Box<dyn Error>> {
        if !self.enabled {
            return Ok(());
        }

        let current = Self::get_current_volume()?;
        *self.volume.lock().unwrap() = current;
        Ok(())
    }

    pub fn start_polling(&self, config: &Config) {
        if !config.enable_audio_control {
            return;
        }

        let volume_clone = Arc::clone(&self.volume);
        let interval = Duration::from_millis(config.volume_update_interval_ms);

        thread::spawn(move || loop {
            if let Ok(vol) = Self::get_current_volume() {
                *volume_clone.lock().unwrap() = vol;
            }
            thread::sleep(interval);
        });
    }

    pub fn get_volume(&self) -> f32 {
        if !self.enabled {
            return 0.0;
        }
        *self.volume.lock().unwrap()
    }

    #[allow(dead_code)]
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

// ============================================================================
// Power
// ============================================================================

fn execute_power_action(label: &str, commands: &[String]) {
    if !try_commands(commands) {
        eprintln!("Failed to {}: No working commands found in config", label);
    }
}

pub fn power_off(config: &Config) {
    execute_power_action("power off", &config.power_commands);
}

pub fn restart(config: &Config) {
    execute_power_action("restart", &config.restart_commands);
}

pub fn logout(config: &Config) {
    execute_power_action("logout", &config.logout_commands);
}

// System tray functionality has moved to src/sni.rs which implements the full
// StatusNotifierItem host/watcher, rendering icons directly inside the egui window.

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gui::{Config, TimeOrder};

    #[test]
    fn test_get_current_time_not_empty() {
        assert!(!get_current_time(&Config::default()).is_empty());
    }

    #[test]
    fn test_get_current_time_custom_format() {
        let mut config = Config::default();
        config.time_format = "%H:%M:%S".into();
        config.time_order = TimeOrder::YmdHms;
        assert!(!get_current_time(&config).is_empty());
    }
}