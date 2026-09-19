//! Melyik mappába kerüljön a klip: a mentés pillanatában előtérben lévő ablak alapján.

use crate::platform::WindowInfo;

pub const DESKTOP_FOLDER: &str = "Desktop";

struct Rule {
    /// exe neve kisbetűvel
    exe: &'static str,
    /// Az ablakcím elejének egyeznie kell (pl. Java alapú játékoknál)
    title_prefix: Option<&'static str>,
    folder: &'static str,
}

/// Az exe-nevek Windowson kiterjesztéssel, Linuxon anélkül szerepelnek.
const RULES: &[Rule] = &[
    Rule { exe: "league of legends.exe", title_prefix: None, folder: "League of Legends" },
    Rule { exe: "leagueclientux.exe", title_prefix: None, folder: "League of Legends" },
    Rule { exe: "javaw.exe", title_prefix: Some("Minecraft"), folder: "Minecraft" },
    Rule { exe: "java.exe", title_prefix: Some("Minecraft"), folder: "Minecraft" },
    Rule { exe: "minecraft.windows.exe", title_prefix: None, folder: "Minecraft" },
    Rule { exe: "java", title_prefix: Some("Minecraft"), folder: "Minecraft" },
];

/// Nem játékok: ha ezek futnak teljes képernyőn (film, böngésző), a klip a Desktop mappába kerül,
/// különben az ablakcím (pl. egy film fájlneve) lenne a mappa neve.
const NON_GAMES: &[&str] = &[
    "vlc.exe", "mpv.exe", "mpc-hc.exe", "mpc-hc64.exe", "mpc-be64.exe", "potplayermini64.exe",
    "wmplayer.exe", "microsoft.media.player.exe", "video.ui.exe", "photos.exe", "chrome.exe",
    "msedge.exe", "firefox.exe", "opera.exe", "brave.exe", "explorer.exe", "applicationframehost.exe",
    "discord.exe", "spotify.exe", "code.exe", "obs64.exe", "replaytray.exe", "clipcat.exe",
    // Linux
    "vlc", "mpv", "totem", "celluloid", "haruna", "smplayer", "firefox", "firefox-bin", "chrome", "chromium",
    "brave", "opera", "vivaldi-bin", "nautilus", "dolphin", "discord", "spotify", "code", "obs", "clipcat",
];

pub fn folder_for(window: Option<WindowInfo>) -> String {
    let Some(window) = window else { return DESKTOP_FOLDER.into() };
    let exe = window.exe.to_lowercase();
    if let Some(rule) = RULES
        .iter()
        .find(|r| r.exe == exe && r.title_prefix.is_none_or(|p| window.title.starts_with(p)))
    {
        return rule.folder.into();
    }
    if !window.fullscreen || NON_GAMES.contains(&exe.as_str()) {
        return DESKTOP_FOLDER.into();
    }
    let from_title = sanitize(&clean_title(&window.title));
    if !from_title.is_empty() {
        return from_title;
    }
    let from_exe = sanitize(exe.trim_end_matches(".exe"));
    if from_exe.is_empty() { DESKTOP_FOLDER.into() } else { from_exe }
}

/// "Minecraft* 1.21.1 - Multiplayer" -> "Minecraft"
fn clean_title(title: &str) -> String {
    let mut name = title;
    for separator in [" - ", " | ", " – "] {
        if let Some(i) = name.find(separator) {
            name = &name[..i];
        }
    }
    let name = name.replace('*', "");
    let mut words: Vec<&str> = name.split_whitespace().collect();
    // Záró verziószám eltávolítása: "1.21.1", "v2.0"
    if words.len() > 1 {
        let last = words[words.len() - 1].trim_start_matches(['v', 'V']);
        if !last.is_empty() && last.chars().all(|c| c.is_ascii_digit() || c == '.') {
            words.pop();
        }
    }
    words.join(" ")
}

fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| !c.is_control() && !r#"<>:"/\|?*"#.contains(*c))
        .collect();
    cleaned.trim().trim_end_matches(['.', ' ']).chars().take(80).collect()
}
