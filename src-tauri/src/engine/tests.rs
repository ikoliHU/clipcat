use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
pub(super) static TEST_LOCK: Mutex<()> = Mutex::new(());
static ENCODER: AtomicUsize = AtomicUsize::new(0);
static RELEASED: AtomicUsize = AtomicUsize::new(0);
static SHUTDOWN: AtomicUsize = AtomicUsize::new(0);
static RECORD_RELEASED: AtomicUsize = AtomicUsize::new(0);
static RECORD_ACTIVE: AtomicBool = AtomicBool::new(false);

fn config() -> Config {
    Config {
        output_dir: std::env::temp_dir().to_string_lossy().into_owned(),
        buffer_seconds: 150,
        buffer_dir: None,
        base: (1920, 1080),
        output: (1920, 1080),
        fps: 60,
        bitrate_kbps: 30_000,
        hevc: false,
        capture_desktop: false,
        monitor_id: String::new(),
        mic_device: "default".into(),
        mic_enabled: false,
    }
}

unsafe extern "C" fn enum_encoders(index: usize, id: *mut *const c_char) -> bool {
    match index {
        0 => {
            *id = c"obs_nvenc_h264_tex".as_ptr();
            true
        }
        1 => {
            *id = c"obs_x264".as_ptr();
            true
        }
        _ => false,
    }
}
unsafe extern "C" fn create_encoder(id: *const c_char, _: *const c_char, _: Ptr, _: Ptr) -> Ptr {
    let value = if CStr::from_ptr(id).to_bytes() == b"obs_x264" { 2 } else { 1 };
    ENCODER.store(value, Ordering::SeqCst);
    value as Ptr
}
unsafe extern "C" fn release(_: Ptr) {
    RELEASED.fetch_add(1, Ordering::SeqCst);
}
unsafe extern "C" fn release_output(output: Ptr) {
    if output as usize == 20 {
        RECORD_RELEASED.fetch_add(1, Ordering::SeqCst);
    }
}
unsafe extern "C" fn shutdown() {
    SHUTDOWN.fetch_add(1, Ordering::SeqCst);
}
unsafe extern "C" fn active(output: Ptr) -> bool {
    output as usize == 20 && RECORD_ACTIVE.load(Ordering::SeqCst)
}
unsafe extern "C" fn stop(output: Ptr) {
    if output as usize == 20 {
        RECORD_ACTIVE.store(false, Ordering::SeqCst);
    }
}
unsafe extern "C" fn start(_: Ptr) -> bool {
    ENCODER.load(Ordering::SeqCst) == 2
}

fn engine() -> Engine {
    ENCODER.store(0, Ordering::SeqCst);
    RELEASED.store(0, Ordering::SeqCst);
    SHUTDOWN.store(0, Ordering::SeqCst);
    RECORD_RELEASED.store(0, Ordering::SeqCst);
    RECORD_ACTIVE.store(false, Ordering::SeqCst);
    SAVE_PENDING.store(false, Ordering::SeqCst);
    REPLAY_WANTED.store(false, Ordering::SeqCst);
    RECORDING_WANTED.store(false, Ordering::SeqCst);
    let mut api = Api::fake();
    api.obs_enum_encoder_types = enum_encoders;
    api.obs_video_encoder_create = create_encoder;
    api.obs_encoder_release = release;
    api.obs_source_release = release;
    api.obs_output_release = release_output;
    api.obs_shutdown = shutdown;
    api.obs_output_active = active;
    api.obs_output_stop = stop;
    api.obs_output_start = start;
    Engine {
        api: Box::leak(Box::new(api)),
        config: config(),
        scene: null_mut(),
        game: null_mut(),
        game_window: None,
        display: null_mut(),
        display_item: null_mut(),
        desktop_audio: null_mut(),
        mic: null_mut(),
        video_encoder: null_mut(),
        audio_encoder: null_mut(),
        output: null_mut(),
        replay_enabled: false,
        buffer_since: 0,
        record_output: null_mut(),
        record_path: String::new(),
        record_since: 0,
        save_worker: None,
        encoder_index: 0,
        encoder_name: String::new(),
        buffer_seconds: 150,
        memory_buffer_bytes: 0,
        initialized: true,
    }
}

#[cfg(windows)]
#[test]
fn capture_retargets_games_clears_on_focus_loss_and_does_not_restart_unchanged_target() {
    static SETTINGS: Mutex<(String, String, i64)> = Mutex::new((String::new(), String::new(), -1));
    static UPDATES: Mutex<Vec<(String, String, i64)>> = Mutex::new(Vec::new());
    unsafe extern "C" fn set_string(_: Ptr, key: *const c_char, value: *const c_char) {
        let mut settings = SETTINGS.lock().unwrap();
        let value = CStr::from_ptr(value).to_string_lossy().into_owned();
        match CStr::from_ptr(key).to_bytes() {
            b"capture_mode" => settings.0 = value,
            b"window" => settings.1 = value,
            _ => (),
        }
    }
    unsafe extern "C" fn set_int(_: Ptr, key: *const c_char, value: i64) {
        if CStr::from_ptr(key).to_bytes() == b"priority" {
            SETTINGS.lock().unwrap().2 = value;
        }
    }
    unsafe extern "C" fn update(source: Ptr, _: Ptr) {
        assert_eq!(source as usize, 50);
        UPDATES.lock().unwrap().push(SETTINGS.lock().unwrap().clone());
    }

    let _serial = TEST_LOCK.lock().unwrap();
    let mut e = engine();
    let mut api = Api::fake();
    api.obs_data_set_string = set_string;
    api.obs_data_set_int = set_int;
    api.obs_source_update = update;
    e.api = Box::leak(Box::new(api));
    e.game = 50 as Ptr;
    UPDATES.lock().unwrap().clear();

    e.update_game_window(None); // Fullscreen VLC is not a game target.
    let lol = Some("League of Legends:RiotWindowClass:League of Legends.exe".into());
    e.update_game_window(lol.clone());
    GAME_HOOKED.store(true, Ordering::SeqCst);
    e.update_game_window(lol.clone());
    assert!(GAME_HOOKED.load(Ordering::SeqCst));
    let minecraft = Some("Minecraft:GLFW30:javaw.exe".into());
    e.update_game_window(minecraft.clone());
    assert!(!GAME_HOOKED.load(Ordering::SeqCst));
    GAME_HOOKED.store(true, Ordering::SeqCst);
    e.update_game_window(None); // Alt-tab back to the video player releases the game.
    assert!(!GAME_HOOKED.load(Ordering::SeqCst));
    e.update_game_window(None);
    e.update_game_window(lol.clone());

    assert_eq!(*UPDATES.lock().unwrap(), vec![
        ("window".into(), lol.clone().unwrap(), 2),
        ("window".into(), minecraft.unwrap(), 2),
        ("window".into(), String::new(), 2),
        ("window".into(), lol.unwrap(), 2),
    ]);
}

#[test]
fn missing_encoder_ids_are_skipped_and_failed_hardware_start_reaches_x264() {
    let _serial = TEST_LOCK.lock().unwrap();
    let mut e = engine();
    assert!(!registered(e.api.obs_enum_encoder_types, "unknown"));
    e.video_encoder = e.create_video_encoder();
    e.output = 10 as Ptr;
    e.start_replay().unwrap();
    assert_eq!(e.encoder_name(), "obs_x264");
    assert_eq!(RELEASED.load(Ordering::SeqCst), 1);
}

#[test]
fn replay_restart_preserves_active_manual_output() {
    let _serial = TEST_LOCK.lock().unwrap();
    let mut e = engine();
    e.output = 10 as Ptr;
    e.record_output = 20 as Ptr;
    e.desktop_audio = 30 as Ptr;
    e.video_encoder = 2 as Ptr;
    RECORD_ACTIVE.store(true, Ordering::SeqCst);
    ENCODER.store(2, Ordering::SeqCst);
    e.restart().unwrap();
    assert!(e.recording_active());
    assert_eq!(RECORD_RELEASED.load(Ordering::SeqCst), 0);
    let mut changed = config();
    changed.fps = 30;
    assert!(e.apply(&changed).is_err());
    assert_eq!(e.config.fps, 60);
}

#[test]
fn failed_pipeline_construction_releases_partial_objects_on_every_cycle() {
    let _serial = TEST_LOCK.lock().unwrap();
    for _ in 0..100 {
        let mut e = engine();
        // Video succeeds; the fake AAC constructor fails.
        assert!(e.build_pipeline().is_err());
        drop(e);
        assert_eq!(RELEASED.load(Ordering::SeqCst), 1);
        assert_eq!(SHUTDOWN.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn shutdown_waits_for_microphone_borrow_before_releasing_source() {
    let _serial = TEST_LOCK.lock().unwrap();
    let mut e = engine();
    e.mic = 40 as Ptr;
    let borrow = MIC_SOURCE.lock().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        e.shutdown();
        tx.send(()).unwrap();
    });
    assert!(rx.recv_timeout(Duration::from_millis(50)).is_err());
    assert_eq!(RELEASED.load(Ordering::SeqCst), 0);
    drop(borrow);
    worker.join().unwrap();
    assert_eq!(RELEASED.load(Ordering::SeqCst), 1);
}

#[test]
fn stop_joins_pending_disk_save_before_releasing_output() {
    let _serial = TEST_LOCK.lock().unwrap();
    let mut e = engine();
    let finished = std::sync::Arc::new(AtomicBool::new(false));
    let worker_flag = finished.clone();
    e.save_worker = Some(std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        worker_flag.store(true, Ordering::SeqCst);
    }));
    e.stop_replay();
    assert!(finished.load(Ordering::SeqCst));
    assert!(e.save_worker.is_none());
}

#[test]
fn idle_audio_does_not_create_devices_and_off_releases_mic() {
    let _serial = TEST_LOCK.lock().unwrap();
    let mut e = engine();
    e.sync_audio().unwrap();
    assert!(e.mic.is_null());
    assert!(e.desktop_audio.is_null());
    e.mic = 40 as Ptr;
    e.desktop_audio = 30 as Ptr;
    e.sync_audio().unwrap();
    assert_eq!(RELEASED.load(Ordering::SeqCst), 2);
    assert!(e.mic.is_null());
    assert!(e.desktop_audio.is_null());
}

#[test]
fn failed_capture_start_clears_intent_and_releases_audio_devices() {
    let _serial = TEST_LOCK.lock().unwrap();
    let mut e = engine();
    e.video_encoder = 2 as Ptr;
    e.audio_encoder = 3 as Ptr;
    // No registered audio source: startup must roll back even before output creation.
    let dir = tempfile::tempdir().unwrap();
    assert!(e.start_recording(&dir.path().join("failed.mp4")).is_err());
    assert!(!RECORDING_WANTED.load(Ordering::SeqCst));
    assert!(e.mic.is_null());
    assert!(e.desktop_audio.is_null());
    e.output = 10 as Ptr;
    assert!(e.set_replay_enabled(true).is_err());
    assert!(!REPLAY_WANTED.load(Ordering::SeqCst));
    assert!(!e.replay_enabled());
}

#[test]
fn empty_or_failed_recordings_are_not_announced_as_successful() {
    let _serial = TEST_LOCK.lock().unwrap();
    for (content, code, success) in [
        (b"".as_slice(), 0, false),
        (b"partial".as_slice(), -1, false),
        (b"finished".as_slice(), 0, true),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clip.mp4");
        std::fs::write(&path, content).unwrap();
        let mut e = engine();
        e.record_output = 20 as Ptr;
        e.record_path = path.to_string_lossy().into_owned();
        RECORD_ACTIVE.store(true, Ordering::SeqCst);
        RECORD_STOP_CODE.store(code, Ordering::SeqCst);
        assert_eq!(e.stop_recording().is_ok(), success);
        assert!(path.exists(), "failed partial files remain available for recovery");
    }
}

#[test]
fn replay_save_reserves_room_for_the_buffer_and_releases_busy_flag_on_rejection() {
    let _serial = TEST_LOCK.lock().unwrap();
    let mut e = engine();
    e.output = 20 as Ptr;
    RECORD_ACTIVE.store(true, Ordering::SeqCst);
    e.memory_buffer_bytes = u64::MAX; // Force the low-space branch without filling a volume.
    assert!(e.save().is_err());
    assert!(!e.saving());
    RECORD_ACTIVE.store(false, Ordering::SeqCst);
}
