import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export { convertFileSrc, invoke, listen };

// A Rust oldali struktúrák (serde camelCase) tükörképei

export interface Settings {
  outputDir: string;
  bufferSeconds: number;
  bufferStorage: "memory" | "disk";
  bufferDir: string;
  resolution: string;
  fps: number;
  bitrateMbps: number;
  codec: "h264" | "hevc";
  captureDesktop: boolean;
  micMode: "off" | "ptt" | "always";
  micDevice: string;
  micPttVk: number;
  micPttLabel: string;
  hotkeySave: string;
  hotkeyRecord: string;
  hotkeyOpenFolder: string;
  hotkeyGallery: string;
  showNotification: boolean;
  notificationSound: boolean;
  autostart: boolean;
  keepObsRunning: boolean;
  replayEnabled: boolean;
  lastClip: string | null;
}

export type HotkeyField = "hotkeySave" | "hotkeyRecord" | "hotkeyOpenFolder" | "hotkeyGallery";

export interface Status {
  obsInstalled: boolean;
  obsRunning: boolean;
  replayEnabled: boolean;
  replayActive: boolean;
  bufferSince: number;
  recording: boolean;
  recordingSince: number;
  error: string | null;
}

export interface Clip {
  path: string;
  name: string;
  game: string;
  size: number;
  modified: number;
}

export interface Mic {
  id: string;
  name: string;
}

export interface UpdateState {
  phase: "idle" | "checking" | "latest" | "available" | "downloading" | "installing" | "error";
  current: string;
  version: string | null;
  notes: string | null;
  progress: number | null;
  error: string | null;
}

export interface Toast {
  kind: "ok" | "error";
  title: string;
  detail: string;
}

export type View = "gallery" | "settings";
