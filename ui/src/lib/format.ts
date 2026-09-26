import { i18n, t } from "./i18n";
import type { Clip } from "./tauri";

export function prettyHotkey(hotkey: string | undefined): string {
  if (!hotkey) return t("hotkey.none");
  return hotkey.split("+").map((p) => p.replace(/^Key/, "").replace(/^Digit/, "")).join("+");
}

export function formatDuration(seconds: number): string {
  if (!isFinite(seconds)) return "";
  const s = Math.round(seconds);
  return `${String(Math.floor(s / 60)).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
}

export function formatDate(unix: number): string {
  return new Intl.DateTimeFormat(i18n.lang, { dateStyle: "short", timeStyle: "short" }).format(new Date(unix * 1000));
}

export function formatSize(bytes: number): string {
  return bytes >= 1024 ** 3 ? `${(bytes / 1024 ** 3).toFixed(1)} GB` : `${Math.round(bytes / 1024 ** 2)} MB`;
}

// Clips without a game belong to the "Other" group
export const gameOf = (clip: Clip) => clip.game || t("gallery.otherGame");

export const cx = (...classes: (string | false | null | undefined)[]) => classes.filter(Boolean).join(" ");
