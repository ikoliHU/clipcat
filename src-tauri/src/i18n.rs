//! Shared catalogs for the native UI and WebViews.
use std::collections::HashMap;
use std::fmt::Display;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    OnceLock,
};
type Messages = HashMap<String, String>;
static HUNGARIAN: AtomicBool = AtomicBool::new(false);

pub fn normalize(locale: &str) -> &'static str {
    match locale
        .split(['-', '_', '.', '@'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "hu" => "hu",
        _ => "en-US",
    }
}

pub fn system_language() -> String {
    #[cfg(windows)]
    {
        // Display language, independent of region and keyboard layout.
        let language = unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() };
        if language & 0x03ff == 0x000e { "hu" } else { "en-US" }.into()
    }
    #[cfg(target_os = "linux")]
    {
        ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .find_map(|key| std::env::var(key).ok().filter(|v| !v.is_empty()))
            .map(|value| normalize(&value).to_string())
            .unwrap_or_else(|| "en-US".into())
    }
}

pub fn set_language(language: &str) {
    HUNGARIAN.store(normalize(language) == "hu", Ordering::SeqCst);
}
pub fn lang() -> &'static str {
    if HUNGARIAN.load(Ordering::SeqCst) {
        "hu"
    } else {
        "en-US"
    }
}

fn catalogs() -> &'static (Messages, Messages) {
    static CATALOGS: OnceLock<(Messages, Messages)> = OnceLock::new();
    CATALOGS.get_or_init(|| {
        (
            serde_json::from_str(include_str!("../../ui/locales/hu.json")).expect("Hungarian catalog"),
            serde_json::from_str(include_str!("../../ui/locales/en-US.json")).expect("English catalog"),
        )
    })
}

pub fn messages() -> Messages {
    let (hu, en) = catalogs();
    let mut all = en.clone();
    if lang() == "hu" {
        all.extend(hu.clone());
    }
    all
}
pub fn t(key: &str) -> String {
    let (hu, en) = catalogs();
    let active = if lang() == "hu" { hu } else { en };
    active.get(key).or_else(|| en.get(key)).cloned().unwrap_or_else(|| key.into())
}
pub fn tf(key: &str, args: &[(&str, &dyn Display)]) -> String {
    args.iter().fold(t(key), |text, (name, value)| {
        text.replace(&format!("{{{name}}}"), &value.to_string())
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn locale_mapping_and_unsupported_fallback() {
        for value in ["hu", "hu-HU", "HU_hu.UTF-8"] {
            assert_eq!(normalize(value), "hu");
        }
        for value in ["en-US", "en-GB", "de-DE", "ja-JP", "", "hux"] {
            assert_eq!(normalize(value), "en-US");
        }
    }
    #[test]
    fn catalogs_have_the_same_keys() {
        let (hu, en) = catalogs();
        assert_eq!(hu.len(), en.len());
        for key in hu.keys() {
            assert!(en.contains_key(key), "missing {key}");
        }
    }
}
