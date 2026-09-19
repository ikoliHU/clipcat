// Gyorsbillentyű-rögzítés: a következő billentyű vagy egérgomb, amíg a globális gyorsbillentyűk szünetelnek
import { invoke } from "./tauri";

export type Captured = { keyboard: KeyboardEvent; mouse?: never } | { mouse: MouseEvent; keyboard?: never };

const MODIFIERS = ["Control", "Alt", "Shift", "Meta", "AltGraph"];
let capturing = false;

export const isCapturing = () => capturing;

// Escape-re (vagy ha már fut egy rögzítés) null; a módosító önmagában csak modifiersOnly esetén elég
export function captureInput(trigger: HTMLElement, modifiersOnly = false): Promise<Captured | null> {
  if (capturing) return Promise.resolve(null);
  capturing = true;
  invoke("suspend_hotkeys");

  return new Promise((resolve) => {
    const finish = (value: Captured | null) => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("mousedown", onMouse, true);
      capturing = false;
      invoke("resume_hotkeys");
      resolve(value);
    };
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (MODIFIERS.includes(e.key) && !modifiersOnly) return;
      if (e.key === "Escape") return finish(null);
      finish({ keyboard: e });
    };
    const onMouse = (e: MouseEvent) => {
      if (e.target === trigger && e.button === 0) return; // a rögzítést indító kattintás
      e.preventDefault();
      finish({ mouse: e });
    };
    setTimeout(() => {
      window.addEventListener("keydown", onKey, true);
      window.addEventListener("mousedown", onMouse, true);
    });
  });
}
