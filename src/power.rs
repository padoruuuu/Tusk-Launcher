use std::process::Command;

pub fn power_off() {
    Command::new("shutdown")
        .arg("-h")
        .arg("now")
        .spawn()
        .expect("Failed to execute shutdown command");
}

pub fn restart() {
    Command::new("reboot")
        .spawn()
        .expect("Failed to execute reboot command");
}

pub fn logout() {
    Command::new("logout")
        .spawn()
        .expect("Failed to execute logout command");
}
