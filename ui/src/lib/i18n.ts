// Fordítások: a szövegek a Rust oldaltól jönnek (ui/locales/<nyelv>.json), így mindkét oldal ugyanazt használja.
// A nyelv futás közben nem változik, ezért a render előtt egyszer betöltjük, és sima függvényként érhető el.
import { invoke } from "./tauri";

export const i18n = { lang: "hu", messages: {} as Record<string, string> };

// A kulcshoz tartozó szöveg a {név} helyőrzők kitöltésével; hiányzó kulcsnál maga a kulcs
export function t(key: string, params?: Record<string, string | number>): string {
  const text = i18n.messages[key] ?? key;
  if (!params) return text;
  return text.replace(/\{(\w+)\}/g, (match, name: string) => (name in params ? String(params[name]) : match));
}

export async function loadLocale() {
  const { lang, messages } = await invoke<{ lang: string; messages: Record<string, string> }>("get_locale");
  i18n.lang = lang;
  i18n.messages = messages;
  document.documentElement.lang = lang;
}
