// CamDroid Desktop GUI — main binary entry point
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    camdroid_gui_lib::run()
}
