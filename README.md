# Tusk-Launcher
The name is a combination of "taskbar," "Rust," and "launcher." The goal is to create a taskbar replacement that doesn’t need to be constantly visible but doubles as an app launcher. 
This design choice ensures that your taskbar replacement is integrated into a form you’ll frequently interact with, resulting in a more efficient and compact experience. 
While it's primarily aimed at tiling window manager users, it’s versatile enough to be used in any environment.

![image](https://github.com/user-attachments/assets/8f8fa1b5-3117-40fa-b7ec-c277492cccfb)
![image](https://github.com/user-attachments/assets/8c53b192-1d0f-45de-85e9-cef055f1d353)
![image](https://github.com/user-attachments/assets/acc5f17b-36ae-4344-bf08-5b71d883e1f9)

TODO:
tray icons ❌
volume slider ❌
css customization ❌
config file ✅

for use in sway add this to your config: for_window [title="Application Launcher"] floating enable, resize set 300 200, move position center

Tusk-Launcher Installation Arch Linux
Prerequisites

Install Rust and Cargo. Ensure you have Rust installed. The easiest way on Arch is via the AUR package rustup.

sudo pacman -Syu base-devel git  
yay -S rustup  
rustup default stable  

Clone the Repository

Download the Tusk-Launcher repository:
git clone https://github.com/padoruuuu/Tusk-Launcher.git  
cd Tusk-Launcher  

Building and Running
Build the Project

Compile the project with Cargo:
cargo build --release  

Run the Application

Run the binary:

./target/release/Tusk-Launcher  

Global Installation (Optional)

To install the application system-wide for easy access:

cargo install --path .  

