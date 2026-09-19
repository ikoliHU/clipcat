use crate::i18n::{t, tf};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub output_dir: String,
    pub buffer_seconds: u32,
    /// "memory" (RAM) vagy "disk" (darabok a `buffer_dir` mappában)
    pub buffer_storage: String,
    pub buffer_dir: String,
    /// "native" vagy "SZÉLESSÉGxMAGASSÁG"
    pub resolution: String,
    pub fps: u32,
    pub bitrate_mbps: u32,
    /// "h264" vagy "hevc"
    pub codec: String,
    pub capture_desktop: bool,
    /// "off", "ptt" vagy "always"
    pub mic_mode: String,
    /// A hangrendszer eszközazonosítója; "default" = a rendszer alapértelmezett mikrofonja
    pub mic_device: String,
    /// Windows virtuális billentyűkód (a felület KeyboardEvent.keyCode-ja; 0x04-0x06: középső/oldalsó egérgombok)
    pub mic_ptt_vk: u32,
    pub mic_ptt_label: String,
    pub hotkey_save: String,
    pub hotkey_record: String,
    pub hotkey_open_folder: String,
    pub hotkey_gallery: String,
    pub show_notification: bool,
    pub notification_sound: bool,
    pub autostart: bool,
    pub keep_obs_running: bool,
    /// A visszajátszás a legutóbb bekapcsolva maradt-e. Tiszta telepítésnél ki van kapcsolva,
    /// a mező nélküli (régebbi) beállításfájlnál viszont be.
    #[serde(default = "enabled")]
    pub replay_enabled: bool,
    pub last_clip: Option<String>,
}

fn enabled() -> bool {
    true
}

fn default_output_dir() -> String {
    crate::platform::videos_dir().join("ClipCat").to_string_lossy().into_owned()
}

fn default_buffer_dir() -> String {
    crate::platform::state_dir().join("buffer").to_string_lossy().into_owned()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            output_dir: default_output_dir(),
            buffer_seconds: 150,
            buffer_storage: "memory".into(),
            buffer_dir: default_buffer_dir(),
            resolution: "1920x1080".into(),
            fps: 60,
            bitrate_mbps: 30,
            codec: "h264".into(),
            capture_desktop: true,
            mic_mode: "always".into(),
            mic_device: "default".into(),
            // A régi ShadowPlay push-to-talk gombja (VK_OEM_3, magyar billentyűzeten "ö")
            mic_ptt_vk: 0xC0,
            mic_ptt_label: "ö".into(),
            hotkey_save: "Alt+F10".into(),
            hotkey_record: "Alt+F9".into(),
            hotkey_open_folder: "Alt+F11".into(),
            hotkey_gallery: "Alt+KeyZ".into(),
            show_notification: true,
            notification_sound: true,
            autostart: true,
            keep_obs_running: true,
            replay_enabled: false,
            last_clip: None,
        }
    }
}

impl Settings {
    /// A rögzítési láncot érintő mezők; ha ezek változnak, a puffert újra kell építeni
    /// (a tartalma elvész). Az asztal rögzítése és a mikrofon azonnal, újraépítés nélkül változik.
    pub fn pipeline_fingerprint(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}|{}|{}",
            self.output_dir,
            self.buffer_seconds,
            self.buffer_storage,
            self.buffer_dir,
            self.resolution,
            self.fps,
            self.bitrate_mbps,
            self.codec
        )
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.output_dir.trim().is_empty() {
            return Err(t("validate.outputDir"));
        }
        if !(10..=1200).contains(&self.buffer_seconds) {
            return Err(tf("validate.buffer", &[("min", &10), ("max", &1200)]));
        }
        if !["memory", "disk"].contains(&self.buffer_storage.as_str()) {
            return Err(t("validate.bufferStorage"));
        }
        if self.buffer_storage == "disk" && self.buffer_dir.trim().is_empty() {
            return Err(t("validate.bufferDir"));
        }
        if ![30, 60, 120, 144].contains(&self.fps) {
            return Err(t("validate.fps"));
        }
        let max_bitrate = max_bitrate_mbps(self.fps);
        if !(MIN_BITRATE_MBPS..=max_bitrate).contains(&self.bitrate_mbps) {
            return Err(tf("validate.bitrate", &[("min", &MIN_BITRATE_MBPS), ("max", &max_bitrate), ("fps", &self.fps)]));
        }
        if !["h264", "hevc"].contains(&self.codec.as_str()) {
            return Err(t("validate.codec"));
        }
        if !["off", "ptt", "always"].contains(&self.mic_mode.as_str()) {
            return Err(t("validate.micMode"));
        }
        if self.resolution != "native" && parse_resolution(&self.resolution).is_none() {
            return Err(t("validate.resolution"));
        }
        if self.mic_device.trim().is_empty() {
            return Err(t("validate.micDevice"));
        }
        if self.mic_mode == "ptt" && !is_ptt_key_supported(self.mic_ptt_vk) {
            return Err(t("validate.pttKey"));
        }
        Ok(())
    }
}

pub const MIN_BITRATE_MBPS: u32 = 5;

/// A képkockasebességhez tartozó legnagyobb bitráta; alacsony FPS-nél ennél többől már nem
/// lesz szebb a kép, csak nagyobb a fájl. A felület (ui/app.js) ugyanezt a táblát használja.
pub fn max_bitrate_mbps(fps: u32) -> u32 {
    match fps {
        0..=30 => 80,
        31..=60 => 100,
        61..=120 => 130,
        _ => 150,
    }
}

/// Bármely billentyű vagy egérgomb jó, kivéve a bal és jobb egérgombot.
pub fn is_ptt_key_supported(vk: u32) -> bool {
    (0x03..=0xFE).contains(&vk)
}

pub fn parse_resolution(s: &str) -> Option<(u32, u32)> {
    let (w, h) = s.split_once('x')?;
    let (w, h) = (w.trim().parse().ok()?, h.trim().parse().ok()?);
    (w >= 320 && h >= 240).then_some((w, h))
}

fn settings_path() -> PathBuf {
    crate::platform::config_dir().join("settings.json")
}

pub fn load() -> Settings {
    let mut settings: Settings = std::fs::read_to_string(settings_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    // A régebbi, FPS-től független felső határral mentett bitrátát az új határra vágja
    settings.bitrate_mbps = settings.bitrate_mbps.min(max_bitrate_mbps(settings.fps));
    settings
}

pub fn save(settings: &Settings) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(settings).map_err(std::io::Error::other)?;
    // Előbb ideiglenes fájlba ír, így egy félbeszakadt mentés nem teszi tönkre a beállításokat
    let path = settings_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, &path)
}
