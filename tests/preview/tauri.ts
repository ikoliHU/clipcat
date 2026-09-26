import type { Settings } from "../../ui/src/lib/tauri";
import hu from "../../ui/locales/hu.json";
import en from "../../ui/locales/en-US.json";
let settings: Settings = {
  language: "hu", outputDir: "C:/Users/Ris/Videos/ClipCat", bufferDir: "C:/Users/Ris/AppData/Local/ClipCat/buffer",
  bufferSeconds: 150, bufferStorage: "memory", resolution: "1920x1080", fps: 60, bitrateMbps: 30, codec: "h264",
  captureDesktop: true, micMode: "off", micDevice: "default", micPttVk: 192, micPttLabel: "ö", hotkeySave: "Alt+F10",
  hotkeyRecord: "Alt+F9", hotkeyOpenFolder: "Alt+F11", hotkeyGallery: "Alt+KeyZ", showNotification: true,
  notificationSound: true, autostart: false, keepObsRunning: true, replayEnabled: false, lastClip: null,
};
const status = { encoder: "obs_nvenc_h264_tex", bufferSeconds: 150, obsInstalled: true, obsRunning: true, replayEnabled: false, replayActive: false, bufferSince: 0, recording: false, recordingSince: 0, error: null };
export async function invoke(command: string, args?: Record<string, unknown>): Promise<unknown> {
  switch (command) {
    case "get_locale": return { lang: settings.language, messages: settings.language === "hu" ? hu : en };
    case "get_settings": return settings;
    case "save_settings": settings = args?.settings as Settings; return "";
    case "get_status": return status;
    case "get_update_state": return { current: "0.4.1", phase: "latest", version: null, notes: null, progress: null, error: null };
    case "list_clips": case "list_mics": return [];
    case "disk_buffer_available": return true;
    case "buffer_budget": return { maxMb: 1024, seconds: args?.seconds };
    case "open_project_link": document.body.dataset.linkTarget = String(args?.target); return;
    default: return null;
  }
}
export async function listen() { return () => {}; }
export const convertFileSrc = (path: string) => path;
