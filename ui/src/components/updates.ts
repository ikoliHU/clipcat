import { t } from "../lib/i18n";
import { invoke } from "../lib/tauri";
import type { UpdateState } from "../lib/tauri";

export function updateProgressText(update: UpdateState): string | null {
  if (update.phase === "installing") return t("update.installing");
  if (update.phase !== "downloading") return null;
  return update.progress == null ? t("update.downloading") : t("update.downloadingProgress", { progress: update.progress });
}

// Keep the discovered version installable after an error (retry)
export const isInstallable = (update: UpdateState) =>
  !!update.version && (update.phase === "available" || update.phase === "error");

export async function installUpdate() {
  try {
    await invoke("install_update");
  } catch (e) {
    alert(e);
  }
}
