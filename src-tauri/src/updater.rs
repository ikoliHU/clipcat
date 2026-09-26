//! ClipCat integration for the shared updater: UI events and capture lifecycle hooks.

use crate::i18n::{t, tf};
use crate::logfile;
use async_trait::async_trait;
use catninth_updater::{
    tauri::TauriInstaller, Config, Error, Event, InstallHooks, InstallPermit, Phase, PollingHandle,
    Release, Repository, Updater as SharedUpdater,
};
use serde::Serialize;
use std::sync::{atomic::Ordering, Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

const REPOSITORY: &str = "catninth/clipcat";
const CHECK_INTERVAL_MINUTES: u64 = 360;
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(20);

/// Keep the existing frontend contract while the library owns update state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateState {
    phase: Phase,
    current: String,
    version: Option<String>,
    notes: Option<String>,
    progress: Option<u8>,
    error: Option<String>,
}

impl From<catninth_updater::UpdateState> for UpdateState {
    fn from(state: catninth_updater::UpdateState) -> Self {
        let baseline = state.applied_version.as_ref().unwrap_or(&state.current);
        let available = state
            .latest
            .filter(|release| release.version.cmp_precedence(baseline).is_gt());
        Self {
            // A successful native install immediately restarts the application.
            phase: if state.phase == Phase::Installed {
                Phase::Installing
            } else {
                state.phase
            },
            current: state.current.to_string(),
            version: available
                .as_ref()
                .map(|release| release.version.to_string()),
            notes: available.and_then(|release| release.notes),
            progress: state.progress.and_then(|progress| progress.percent),
            error: state.error,
        }
    }
}

struct Updater {
    shared: SharedUpdater,
    _polling: PollingHandle,
}

pub fn setup(app: &AppHandle) -> catninth_updater::Result<()> {
    let repository = Repository::parse(REPOSITORY)?;
    let config = Config::new(
        repository.clone(),
        &app.package_info().version.to_string(),
        CHECK_INTERVAL_MINUTES,
    )?
    .with_first_check_delay(FIRST_CHECK_DELAY)?;
    let public_key = app
        .config()
        .plugins
        .0
        .get("updater")
        .and_then(|config| config.get("pubkey"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::InvalidConfig("missing updater public key".into()))?;
    let exit_app = app.clone();
    let installer = TauriInstaller::new(app.clone(), repository, public_key)?
        .with_timeout(config.request_timeout())?
        .on_before_exit(move || prepare_exit(&exit_app));
    let event_app = app.clone();
    // Byte-level download events can be frequent; publish only visible UI changes.
    let last_state = Mutex::new(None);
    let shared = SharedUpdater::builder(config)
        .installer(Arc::new(installer))
        .hooks(Arc::new(CaptureHooks(app.clone())))
        .on_event(move |event| match event {
            Event::StateChanged(state) => {
                let state = UpdateState::from(state.clone());
                let mut last = last_state.lock().unwrap();
                if last.as_ref() != Some(&state) {
                    *last = Some(state.clone());
                    drop(last);
                    let _ = event_app.emit("update", state);
                }
            }
            Event::UpdateAvailable(release) => {
                let version = release.version.to_string();
                logfile::write(&format!("Update available: v{version}"));
                crate::show_toast(
                    &event_app,
                    "ok",
                    &t("toast.updateAvailable"),
                    &tf("toast.updateHint", &[("version", &version)]),
                );
            }
            Event::Failed { operation, message } => {
                logfile::write(&format!("Update {operation:?} failed: {message}"));
            }
            _ => {}
        })
        .build()?;
    let polling = tauri::async_runtime::block_on(async { shared.start() })?;
    app.manage(Updater {
        shared,
        _polling: polling,
    });
    Ok(())
}

pub fn state(app: &AppHandle) -> UpdateState {
    app.state::<Updater>().shared.state().into()
}

pub async fn check(app: &AppHandle) -> Result<UpdateState, String> {
    let shared = app.state::<Updater>().shared.clone();
    match shared.fetch_version().await {
        Ok(_) | Err(Error::Busy) => Ok(shared.state().into()),
        Err(error) => Err(error.to_string()),
    }
}

pub async fn install(app: &AppHandle) -> Result<(), String> {
    let shared = app.state::<Updater>().shared.clone();
    shared.install().await.map_err(|error| match error {
        Error::Busy => t("update.busy"),
        Error::NoUpdate => t("update.none"),
        Error::Blocked(message) => message,
        error => tf("update.failed", &[("error", &error.to_string())]),
    })
}

struct CaptureHooks(AppHandle);

#[async_trait]
impl InstallHooks for CaptureHooks {
    async fn before_download(&self, release: &Release) -> catninth_updater::Result<()> {
        if capture_busy(&self.0) {
            return Err(Error::Blocked(t("update.recordingActive")));
        }
        logfile::write(&format!("Downloading update: v{}", release.version));
        Ok(())
    }

    async fn before_install(
        &self,
        release: &Release,
    ) -> catninth_updater::Result<Box<dyn InstallPermit>> {
        // Recording may have started during the download. Reserve installation
        // under the same operation lock used by capture starts and settings changes.
        let state = crate::state(&self.0);
        begin_install(
            &state.operations,
            &state.installing,
            &state.quitting,
            || capture_busy(&self.0),
        )
        .map_err(Error::Blocked)?;
        logfile::write(&format!("Installing update: v{}", release.version));
        Ok(Box::new(CaptureReservation(self.0.clone())))
    }

    async fn after_install(&self, _release: &Release) -> catninth_updater::Result<()> {
        prepare_exit(&self.0);
        self.0.restart();
    }
}

/// The framework retains this guard through installation and restart preparation.
/// On installation failure, dropping it permits capture to start again.
struct CaptureReservation(AppHandle);
impl Drop for CaptureReservation {
    fn drop(&mut self) {
        crate::state(&self.0)
            .installing
            .store(false, Ordering::SeqCst);
    }
}

fn prepare_exit(app: &AppHandle) {
    crate::stop_engine(app);
    tauri_plugin_single_instance::destroy(app);
}

fn capture_busy(app: &AppHandle) -> bool {
    let st = crate::state(app);
    st.save_in_progress.load(Ordering::SeqCst)
        || st
            .engine
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|e| e.recording_active() || e.saving())
}

fn begin_install(
    operations: &Mutex<()>,
    installing: &std::sync::atomic::AtomicBool,
    quitting: &std::sync::atomic::AtomicBool,
    capture_busy: impl FnOnce() -> bool,
) -> Result<(), String> {
    let _operation = operations.lock().unwrap();
    if capture_busy() {
        return Err(t("update.recordingActive"));
    }
    if quitting.load(Ordering::SeqCst) || installing.load(Ordering::SeqCst) {
        return Err(t("update.busy"));
    }
    installing.store(true, Ordering::SeqCst);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    fn snapshot(version: &str) -> catninth_updater::UpdateState {
        catninth_updater::UpdateState {
            phase: Phase::Available,
            current: "1.0.0".parse().unwrap(),
            latest: Some(Release {
                version: version.parse().unwrap(),
                tag: format!("v{version}"),
                notes: Some("Release notes".into()),
                published_at: None,
            }),
            progress: None,
            error: None,
            applied_version: None,
        }
    }

    #[test]
    fn ui_exposes_only_installable_releases_and_preserves_retry_notes() {
        for version in ["0.9.0", "1.0.0", "1.0.0+build.2"] {
            let state = UpdateState::from(snapshot(version));
            assert!(state.version.is_none());
            assert!(state.notes.is_none());
        }
        let mut state = snapshot("1.1.0");
        state.phase = Phase::Error;
        state.error = Some("Network unavailable".into());
        let state = UpdateState::from(state);
        assert_eq!(state.version.as_deref(), Some("1.1.0"));
        assert_eq!(state.notes.as_deref(), Some("Release notes"));
        assert_eq!(state.error.as_deref(), Some("Network unavailable"));
    }

    #[test]
    fn progress_and_restart_keep_the_frontend_contract() {
        let mut state = snapshot("1.1.0");
        state.phase = Phase::Downloading;
        state.progress = Some(catninth_updater::DownloadProgress::new(12, Some(40)));
        assert_eq!(UpdateState::from(state.clone()).progress, Some(30));
        state.progress = Some(catninth_updater::DownloadProgress::new(12, None));
        assert_eq!(UpdateState::from(state.clone()).progress, None);
        state.phase = Phase::Installed;
        state.applied_version = Some("1.1.0".parse().unwrap());
        let state = UpdateState::from(state);
        assert_eq!(state.phase, Phase::Installing);
        assert!(state.version.is_none());
        let json = serde_json::to_value(state).unwrap();
        assert_eq!(json["phase"], "installing");
        assert_eq!(json["current"], "1.0.0");
    }

    #[test]
    fn recording_started_during_download_prevents_install() {
        let operations = Mutex::new(());
        let installing = AtomicBool::new(false);
        let quitting = AtomicBool::new(false);
        let recording = AtomicBool::new(false);
        assert!(!recording.load(Ordering::SeqCst)); // initial download check
        recording.store(true, Ordering::SeqCst); // started while network was pending
        assert!(
            begin_install(&operations, &installing, &quitting, || recording
                .load(Ordering::SeqCst))
            .is_err()
        );
        assert!(!installing.load(Ordering::SeqCst));
    }

    #[test]
    fn quitting_or_another_install_blocks_installation() {
        let operations = Mutex::new(());
        let installing = AtomicBool::new(false);
        assert!(begin_install(&operations, &installing, &AtomicBool::new(true), || false).is_err());
        assert!(!installing.load(Ordering::SeqCst));
        installing.store(true, Ordering::SeqCst);
        assert!(
            begin_install(&operations, &installing, &AtomicBool::new(false), || false).is_err()
        );
    }

    #[test]
    fn final_install_gate_and_capture_start_cannot_both_succeed() {
        for _ in 0..100 {
            let state = Arc::new((
                Mutex::new(()),
                AtomicBool::new(false),
                AtomicBool::new(false),
            ));
            let capture_state = state.clone();
            let capture = std::thread::spawn(move || {
                let _lock = capture_state.0.lock().unwrap();
                if !capture_state.1.load(Ordering::SeqCst) {
                    capture_state.2.store(true, Ordering::SeqCst);
                }
            });
            let _ = begin_install(&state.0, &state.1, &AtomicBool::new(false), || {
                state.2.load(Ordering::SeqCst)
            });
            capture.join().unwrap();
            assert!(!(state.1.load(Ordering::SeqCst) && state.2.load(Ordering::SeqCst)));
        }
    }
}
