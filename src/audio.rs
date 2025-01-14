use std::error::Error;
use std::process::Command;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub struct AudioController {
    volume: Arc<Mutex<f32>>,
    max_volume: f32,
}

impl AudioController {
    pub fn new(max_volume: f32) -> Result<Self, Box<dyn Error>> {
        let volume = Self::get_current_volume()?;
        Ok(AudioController {
            volume: Arc::new(Mutex::new(volume)),
            max_volume,
        })
    }

    fn get_current_volume() -> Result<f32, Box<dyn Error>> {
        // Get the list of currently playing audio streams
        let output = Command::new("pw-dump")
            .output()?;
        
        let streams_str = String::from_utf8(output.stdout)?;
        
        // Parse the JSON output to find active streams and their volumes
        // For now, we'll just get the default sink volume as a placeholder
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
        let clamped_volume = new_volume.clamp(0.0, self.max_volume);
        
        // Get all active audio streams
        let output = Command::new("pw-dump")
            .output()?;
        
        if output.status.success() {
            // Parse the JSON to find active streams
            let streams_str = String::from_utf8(output.stdout)?;
            
            // For each active stream, set its volume
            // This is where we'll use pw-cli to set individual stream volumes
            Command::new("wpctl")
                .args(["set-volume", "@DEFAULT_AUDIO_SINK@", &format!("{:.2}", clamped_volume)])
                .output()?;
        }
            
        *self.volume.lock().unwrap() = clamped_volume;
        Ok(())
    }

    pub fn update_volume(&self) -> Result<(), Box<dyn Error>> {
        let current_volume = Self::get_current_volume()?;
        *self.volume.lock().unwrap() = current_volume;
        Ok(())
    }

    pub fn start_polling(&self, interval: Duration) {
        let volume_clone = Arc::clone(&self.volume);
        
        thread::spawn(move || loop {
            if let Ok(volume) = Self::get_current_volume() {
                let mut vol_lock = volume_clone.lock().unwrap();
                *vol_lock = volume;
            }
            thread::sleep(interval);
        });
    }

    pub fn get_volume(&self) -> f32 {
        *self.volume.lock().unwrap()
    }
}