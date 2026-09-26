import { useSyncExternalStore } from "react";
import { invoke } from "./tauri";
export const i18n = { lang: "en-US", messages: {} as Record<string, string> };
const subscribers = new Set<() => void>();
const subscribe = (notify: () => void) => { subscribers.add(notify); return () => { subscribers.delete(notify); }; };
export const useLocale = () => useSyncExternalStore(subscribe, () => i18n.lang);
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
  subscribers.forEach((notify) => notify());
}
