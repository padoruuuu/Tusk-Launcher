// audio.rs
use std::error::Error;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::str::FromStr;

pub struct AudioController {
    volume: Arc<Mutex<f32>>,
}

impl AudioController {
    pub fn new() -> Result<Self, Box<dyn Error>> {
        // Get initial volume using wpctl
        let output = Command::new("wpctl")
            .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
            .output()?;

        let volume_str = String::from_utf8(output.stdout)?;
        // Parse volume from output like "Volume: 0.50"
        let volume = volume_str
            .split_whitespace()
            .nth(1)
            .and_then(|v| f32::from_str(v).ok())
            .unwrap_or(1.0);

        Ok(AudioController {
            volume: Arc::new(Mutex::new(volume)),
        })
    }

    pub fn set_volume(&mut self, new_volume: f32) -> Result<(), Box<dyn Error>> {
        let clamped_volume = new_volume.clamp(0.0, 1.0);
        
        // Set volume using wpctl
        Command::new("wpctl")
            .args(["set-volume", "@DEFAULT_AUDIO_SINK@", &format!("{:.2}", clamped_volume)])
            .output()?;

        // Update our cached volume value
        *self.volume.lock().unwrap() = clamped_volume;
        
        Ok(())
    }
    }

#[derive(Debug)]
pub enum AudioError {
    CommandFailed(String),
    InvalidVolume,
}

impl std::fmt::Display for AudioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AudioError::CommandFailed(msg) => write!(f, "Audio command failed: {}", msg),
            AudioError::InvalidVolume => write!(f, "Invalid volume value"),
        }
    }
}

impl Error for AudioError {}