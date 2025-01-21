use std::error::Error;
use std::process::Command;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use crate::config::Config;

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
        // Get the list of currently playing audio streams
        Command::new("pw-dump")
            .output()?;
        
        // For now, we'll just get the default sink volume
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

        let clamped_volume = new_volume.clamp(0.0, self.max_volume);
        
        // Get all active audio streams
        let output = Command::new("pw-dump")
            .output()?;
        
        if output.status.success() {
            // For each active stream, set its volume
            Command::new("wpctl")
                .args(["set-volume", "@DEFAULT_AUDIO_SINK@", &format!("{:.2}", clamped_volume)])
                .output()?;
        }
            
        *self.volume.lock().unwrap() = clamped_volume;
        Ok(())
    }

    pub fn update_volume(&self) -> Result<(), Box<dyn Error>> {
        if !self.enabled {
            return Ok(());
        }

        let current_volume = Self::get_current_volume()?;
        *self.volume.lock().unwrap() = current_volume;
        Ok(())
    }

    pub fn start_polling(&self, config: &Config) {
        if !config.enable_audio_control {
            return;
        }

        let volume_clone = Arc::clone(&self.volume);
        let enabled = self.enabled;
        let interval = Duration::from_millis(config.volume_update_interval_ms);
        
        thread::spawn(move || loop {
            if !enabled {
                break;
            }

            if let Ok(volume) = Self::get_current_volume() {
                let mut vol_lock = volume_clone.lock().unwrap();
                *vol_lock = volume;
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

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}