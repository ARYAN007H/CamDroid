//! CamDroid Studio: the desktop application.
//!
//! A thin Tauri shell around the `camdroid-client` core. This crate owns the
//! window, persistence, the ring light overlay and the IPC surface; all video
//! work happens in the core.

mod commands;
mod error;
mod preview;
mod ring_light;
mod settings;
mod state;

use state::AppState;
use tauri::{Manager, RunEvent, WindowEvent};

pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    if let Err(error) = camdroid_client::CamdroidSession::init() {
        eprintln!("Failed to initialise FFmpeg: {error:#}");
        std::process::exit(1);
    }

    let app = tauri::Builder::default()
        .setup(|app| {
            let directory = app.path().app_config_dir()?;
            app.manage(AppState::new(&directory));
            Ok(())
        })
        .on_window_event(|window, event| {
            // The ring light is click-through and has no way to close itself,
            // so the app must not outlive the studio window.
            if window.label() == "main" && matches!(event, WindowEvent::Destroyed) {
                window.app_handle().exit(0);
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::session::discover_devices,
            commands::session::connect,
            commands::session::session_status,
            commands::session::disconnect,
            commands::session::start_preview,
            commands::camera::camera_command,
            commands::studio::filter_presets,
            commands::studio::get_studio_state,
            commands::studio::set_output_mode,
            commands::studio::set_mirror,
            commands::studio::set_filter,
            commands::studio::set_ring_light,
            commands::studio::set_ring_light_visible,
            commands::system::system_check,
            commands::system::quit_app,
        ])
        .build(tauri::generate_context!())
        .expect("error while building the CamDroid application");

    app.run(|handle, event| {
        if let RunEvent::Exit = event {
            handle.state::<AppState>().shutdown();
        }
    });
}
