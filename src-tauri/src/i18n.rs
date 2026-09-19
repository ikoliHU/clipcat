//! Fordítások: a felület és a Rust oldal ugyanazt a `ui/locales/<nyelv>.json` fájlt használja.
//! Új nyelv: a JSON-fájl mellé egy sor a `LOCALES` listába.

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::OnceLock;

const LOCALES: &[(&str, &str)] = &[("hu", include_str!("../../ui/locales/hu.json"))];
/// A hiányzó kulcsok ebből a nyelvből jönnek
const FALLBACK: &str = "hu";

type Messages = HashMap<String, String>;

fn parse(lang: &str) -> Messages {
    LOCALES
        .iter()
        .find(|(code, _)| *code == lang)
        .and_then(|(_, json)| serde_json::from_str(json).ok())
        .unwrap_or_default()
}

fn catalogs() -> &'static (Messages, Messages) {
    static CATALOGS: OnceLock<(Messages, Messages)> = OnceLock::new();
    CATALOGS.get_or_init(|| (parse(lang()), parse(FALLBACK)))
}

/// Az aktív nyelv kódja (egyelőre csak magyar van)
pub fn lang() -> &'static str {
    FALLBACK
}

/// Az aktív nyelv összes szövege a felületnek, a hiányzókat a tartalék nyelvből pótolva
pub fn messages() -> Messages {
    let (active, fallback) = catalogs();
    let mut all = fallback.clone();
    all.extend(active.iter().map(|(k, v)| (k.clone(), v.clone())));
    all
}

/// A kulcshoz tartozó szöveg; ha sehol nincs meg, maga a kulcs
pub fn t(key: &str) -> String {
    let (active, fallback) = catalogs();
    active.get(key).or_else(|| fallback.get(key)).cloned().unwrap_or_else(|| key.to_string())
}

/// Mint a `t`, de a `{név}` helyőrzőket kitölti
pub fn tf(key: &str, args: &[(&str, &dyn Display)]) -> String {
    args.iter().fold(t(key), |text, (name, value)| text.replace(&format!("{{{name}}}"), &value.to_string()))
}
