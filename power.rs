use std::process::Command;
use std::path::Path;

fn execute_command(command: &str, args: &[&str]) -> Result<(), String> {
    Command::new(command)
        .args(args)
        .spawn()
        .map_err(|e| format!("Failed to execute {}: {}", command, e))?;
    Ok(())
}

pub fn power_off() {
    let commands = vec![
        ("shutdown", vec!["-h", "now"]),
        ("systemctl", vec!["poweroff"]),
        ("poweroff", vec![]),
        ("halt", vec![]),
    ];
    
    for (cmd, args) in commands {
        if Path::new(&format!("/usr/bin/{}", cmd)).exists() || Path::new(&format!("/bin/{}", cmd)).exists() {
            if execute_command(cmd, &args).is_ok() {
                return;
            }
        }
    }
    eprintln!("Failed to power off: No known command available");
}

pub fn restart() {
    let commands = vec![
        ("reboot", vec![]),
        ("systemctl", vec!["reboot"]),
        ("shutdown", vec!["-r", "now"]),
    ];
    
    for (cmd, args) in commands {
        if Path::new(&format!("/usr/bin/{}", cmd)).exists() || Path::new(&format!("/bin/{}", cmd)).exists() {
            if execute_command(cmd, &args).is_ok() {
                return;
            }
        }
    }
    eprintln!("Failed to restart: No known command available");
}

pub fn logout() {
    let commands = vec![
        ("swaymsg", vec!["exit"]),
        ("gnome-session-quit", vec!["--logout", "--no-prompt"]),
        ("kdeinit5", vec!["--logout"]),
        ("logout", vec![]),
    ];
    
    for (cmd, args) in commands {
        if Path::new(&format!("/usr/bin/{}", cmd)).exists() || Path::new(&format!("/bin/{}", cmd)).exists() {
            if execute_command(cmd, &args).is_ok() {
                return;
            }
        }
    }
    eprintln!("Failed to logout: No known command available");
}
