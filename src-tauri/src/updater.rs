//! Önfrissítés a GitHub legutóbbi kiadásából (tauri-plugin-updater, aláírt csomagokkal).
//!
//! Az ellenőrzés a háttérben fut (a ClipCat többnyire a tálcán, rejtett ablakkal él), az állapot
//! `update` eseményként megy a felületnek. Telepíteni csak kérésre lehet, kézi felvétel alatt nem.

use crate::i18n::{t, tf};
use crate::logfile;
use serde::Serialize;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

/// Az első ellenőrzés ennyivel az indulás után (ne a rögzítőmotor indulásával versenyezzen)
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(20);
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Clone, Copy, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    #[default]
    Idle,
    Checking,
    /// Az ellenőrzés nem talált újabb verziót
    Latest,
    Available,
    Downloading,
    Installing,
    Error,
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateState {
    phase: Phase,
    current: String,
    version: Option<String>,
    notes: Option<String>,
    /// Letöltés közben 0–100, ha a méret ismert
    progress: Option<u8>,
    error: Option<String>,
}

#[derive(Default)]
pub struct Updater {
    state: Mutex<UpdateState>,
    update: Mutex<Option<Update>>,
    /// Ennek a verziónak az elérhetőségéről már szólt értesítés
    announced: Mutex<Option<String>>,
}

fn updater(app: &AppHandle) -> tauri::State<'_, Updater> {
    app.state::<Updater>()
}

pub fn state(app: &AppHandle) -> UpdateState {
    let mut state = updater(app).state.lock().unwrap().clone();
    state.current = app.package_info().version.to_string();
    state
}

fn set(app: &AppHandle, change: impl FnOnce(&mut UpdateState)) {
    change(&mut updater(app).state.lock().unwrap());
    let _ = app.emit("update", state(app));
}

fn busy(phase: Phase) -> bool {
    matches!(phase, Phase::Checking | Phase::Downloading | Phase::Installing)
}

/// Indulás után, majd rendszeresen ellenőriz; a hibát csak naplózza (pl. nincs net).
pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(FIRST_CHECK_DELAY);
        loop {
            let _ = tauri::async_runtime::block_on(check(&app));
            std::thread::sleep(CHECK_INTERVAL);
        }
    });
}

pub async fn check(app: &AppHandle) -> Result<UpdateState, String> {
    {
        let updater = updater(app);
        let mut s = updater.state.lock().unwrap();
        if busy(s.phase) {
            drop(s);
            return Ok(state(app));
        }
        s.phase = Phase::Checking;
        s.error = None;
    }
    let _ = app.emit("update", state(app));

    let handle = app.clone();
    let result = async {
        app.updater_builder()
            // Windowson a telepítő indítása előtt: a telepítő a futó motor fájljait is cseréli
            .on_before_exit(move || crate::stop_engine(&handle))
            .build()
            .map_err(|e| e.to_string())?
            .check()
            .await
            .map_err(|e| e.to_string())
    }
    .await;

    match result {
        Ok(Some(update)) => {
            let version = update.version.clone();
            let notes = update.body.as_deref().map(str::trim).filter(|b| !b.is_empty()).map(String::from);
            *updater(app).update.lock().unwrap() = Some(update);
            set(app, |s| {
                s.phase = Phase::Available;
                s.version = Some(version.clone());
                s.notes = notes;
            });
            let first = updater(app).announced.lock().unwrap().replace(version.clone()).as_deref() != Some(&version);
            if first {
                logfile::write(&format!("Frissítés elérhető: v{version}"));
                crate::show_toast(app, "ok", &t("toast.updateAvailable"), &tf("toast.updateHint", &[("version", &version)]));
            }
        }
        Ok(None) => {
            *updater(app).update.lock().unwrap() = None;
            set(app, |s| {
                s.phase = Phase::Latest;
                s.version = None;
                s.notes = None;
            });
        }
        Err(e) => {
            logfile::write(&format!("Frissítés-ellenőrzés sikertelen: {e}"));
            set(app, |s| {
                s.phase = Phase::Error;
                s.error = Some(e.clone());
            });
            return Err(e);
        }
    }
    Ok(state(app))
}

/// Letölti és telepíti a talált frissítést, majd újraindítja a ClipCat-et.
pub async fn install(app: &AppHandle) -> Result<(), String> {
    let update = updater(app).update.lock().unwrap().clone().ok_or_else(|| t("update.none"))?;
    if crate::state(app).status.lock().unwrap().recording {
        return Err(t("update.recordingActive"));
    }
    {
        let updater = updater(app);
        let mut s = updater.state.lock().unwrap();
        if busy(s.phase) {
            return Err(t("update.busy"));
        }
        s.phase = Phase::Downloading;
        s.progress = Some(0);
        s.error = None;
    }
    let _ = app.emit("update", state(app));
    logfile::write(&format!("Frissítés letöltése: v{}", update.version));

    let mut received = 0u64;
    let mut shown = Some(0u8);
    let downloaded = update
        .download(
            |chunk, total| {
                received += chunk as u64;
                let progress = total.filter(|&t| t > 0).map(|t| (received * 100 / t).min(100) as u8);
                if progress != shown {
                    shown = progress;
                    set(app, |s| s.progress = progress);
                }
            },
            || {},
        )
        .await;
    let bytes = match downloaded {
        Ok(bytes) => bytes,
        Err(e) => return Err(fail(app, &e.to_string())),
    };

    set(app, |s| {
        s.phase = Phase::Installing;
        s.progress = None;
    });
    logfile::write(&format!("Frissítés telepítése: v{}", update.version));
    // Windowson ez nem tér vissza: a telepítő indítása után a folyamat kilép
    let installed = tauri::async_runtime::spawn_blocking(move || update.install(bytes))
        .await
        .map_err(|e| e.to_string())
        .and_then(|r| r.map_err(|e| e.to_string()));
    if let Err(e) = installed {
        return Err(fail(app, &e));
    }

    // Linuxon a fájl már cserélve; az új példánynak előbb az egypéldányos zárat el kell engedni
    crate::stop_engine(app);
    tauri_plugin_single_instance::destroy(app);
    app.restart();
}

fn fail(app: &AppHandle, error: &str) -> String {
    logfile::write(&format!("Frissítés sikertelen: {error}"));
    let message = tf("update.failed", &[("error", &error)]);
    set(app, |s| {
        // A talált frissítés megmarad, újra lehet próbálni
        s.phase = Phase::Error;
        s.progress = None;
        s.error = Some(message.clone());
    });
    message
}
