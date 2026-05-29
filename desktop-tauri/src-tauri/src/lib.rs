// CamDroid Desktop GUI — Tauri App Entry Point

mod commands;

use commands::AppState;
use std::sync::Mutex;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize FFmpeg once at startup
    if let Err(e) = camdroid_client::CamdroidSession::init() {
        eprintln!("Failed to initialize FFmpeg: {:#}", e);
        std::process::exit(1);
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(AppState {
            session: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            commands::discover_devices,
            commands::connect_and_stream,
            commands::send_control,
            commands::get_status,
            commands::stop_stream,
            commands::set_zoom,
            commands::set_focus,
            commands::set_exposure,
            commands::set_white_balance,
            commands::set_flash,
            commands::switch_camera,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
