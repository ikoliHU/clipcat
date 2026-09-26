import { invoke } from "./tauri";
export type Captured = { keyboard: KeyboardEvent; mouse?: never } | { mouse: MouseEvent; keyboard?: never };
const MODIFIERS = ["Control", "Alt", "Shift", "Meta", "AltGraph"];
let capturing = false;
export const isCapturing = () => capturing;

// Install cancellation before awaiting IPC: blur/unmount during suspension is safe too.
export async function captureInput(trigger: HTMLElement, modifiersOnly = false, signal?: AbortSignal): Promise<Captured | null> {
  if (capturing || signal?.aborted) return null;
  capturing = true;
  let finish!: (value: Captured | null) => void;
  let cleanup = () => {};
  const input = new Promise<Captured | null>((resolve) => {
    let done = false;
    finish = (value) => {
      if (done) return;
      done = true;
      cleanup();
      resolve(value);
    };
    const cancel = () => finish(null);
    const visibility = () => { if (document.hidden) cancel(); };
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (MODIFIERS.includes(e.key) && !modifiersOnly) return;
      finish(e.key === "Escape" ? null : { keyboard: e });
    };
    const onMouse = (e: MouseEvent) => {
      if (e.target === trigger && e.button === 0) return;
      e.preventDefault();
      finish({ mouse: e });
    };
    const timer = window.setTimeout(cancel, 30_000);
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("mousedown", onMouse, true);
    window.addEventListener("blur", cancel);
    document.addEventListener("visibilitychange", visibility);
    signal?.addEventListener("abort", cancel, { once: true });
    cleanup = () => {
      clearTimeout(timer);
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("mousedown", onMouse, true);
      window.removeEventListener("blur", cancel);
      document.removeEventListener("visibilitychange", visibility);
      signal?.removeEventListener("abort", cancel);
    };
  });
  try {
    await invoke("suspend_hotkeys");
    return await input;
  } finally {
    finish(null);
    try { await invoke("resume_hotkeys"); }
    finally { capturing = false; }
  }
}
