// Prevents additional console window on Windows in release, when not using `#![windows_subsystem = "windows"]`
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    rgoat_desktop::run()
}
