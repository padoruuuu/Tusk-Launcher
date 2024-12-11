# RustRocket
A Blazing fast application launcher for wlroots based wayland compositors written in rust.

for use in sway add this to your config: for_window [title="Application Launcher"] floating enable, resize set 300 200, move position center

![image](https://github.com/user-attachments/assets/8f8fa1b5-3117-40fa-b7ec-c277492cccfb)
![image](https://github.com/user-attachments/assets/8c53b192-1d0f-45de-85e9-cef055f1d353)

# Installation
Follow these steps to build and run RustRocket on Arch Linux.
Prerequisites

    Install Rust and Cargo
    Ensure you have Rust installed. The easiest way on Arch is via the AUR package rustup:

sudo pacman -Syu base-devel git
yay -S rustup
rustup default stable

Clone the Repository
Download the RustRocket repository:

    git clone https://github.com/padoruuuu/RustRocket.git
    cd RustRocket

Building and Running

    Build the Project
    Compile the project with Cargo:

cargo build --release

Run the Application
Run the binary:

./target/release/rustrocket

Global Installation (Optional)
To install the application system-wide for easy access:

cargo install --path .
