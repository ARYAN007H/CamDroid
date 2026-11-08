//! Application state shared by all commands.

use crate::error::{CommandError, CommandResult};
use crate::preview::PreviewPump;
use crate::settings::SettingsStore;
use camdroid_client::CamdroidSession;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub struct AppState {
    pub settings: SettingsStore,
    session: Mutex<Option<CamdroidSession>>,
    preview: Mutex<Option<PreviewPump>>,
    connecting: AtomicBool,
}

/// Marks a connection attempt as in progress until dropped.
pub struct ConnectGuard<'a>(&'a AtomicBool);

impl Drop for ConnectGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

impl AppState {
    pub fn new(config_directory: &Path) -> Self {
        Self {
            settings: SettingsStore::open(config_directory),
            session: Mutex::new(None),
            preview: Mutex::new(None),
            connecting: AtomicBool::new(false),
        }
    }

    /// Claims the right to connect; fails if another attempt is running.
    pub fn begin_connect(&self) -> CommandResult<ConnectGuard<'_>> {
        if self.connecting.swap(true, Ordering::SeqCst) {
            return Err(CommandError::Busy(
                "A connection attempt is already in progress.",
            ));
        }
        Ok(ConnectGuard(&self.connecting))
    }

    /// Runs `action` against the live session, or fails with `NotConnected`.
    pub fn with_session<R>(&self, action: impl FnOnce(&CamdroidSession) -> R) -> CommandResult<R> {
        locked(&self.session)
            .as_ref()
            .map(action)
            .ok_or(CommandError::NotConnected)
    }

    /// Like [`Self::with_session`] but yields `None` instead of an error.
    pub fn try_with_session<R>(&self, action: impl FnOnce(&CamdroidSession) -> R) -> Option<R> {
        locked(&self.session).as_ref().map(action)
    }

    pub fn install_session(&self, session: CamdroidSession) {
        *locked(&self.session) = Some(session);
    }

    pub fn take_session(&self) -> Option<CamdroidSession> {
        self.stop_preview();
        locked(&self.session).take()
    }

    pub fn start_preview(&self, pump: PreviewPump) {
        // Dropping the previous pump stops and joins its thread.
        *locked(&self.preview) = Some(pump);
    }

    pub fn stop_preview(&self) {
        locked(&self.preview).take();
    }

    /// Stops everything and persists settings. Called on exit.
    pub fn shutdown(&self) {
        if let Some(session) = self.take_session() {
            if let Err(error) = session.stop() {
                log::warn!("Session did not stop cleanly: {error:#}");
            }
        }
        self.settings.flush();
    }
}
