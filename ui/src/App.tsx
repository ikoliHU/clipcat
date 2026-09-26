import { useCallback, useEffect, useState } from "react";
import { Gallery } from "./components/Gallery";
import { Player } from "./components/Player";
import { SettingsView } from "./components/SettingsView";
import { Sidebar } from "./components/Sidebar";
import { invoke, listen } from "./lib/tauri";
import type { Clip, Settings, Status, UpdateState, View } from "./lib/tauri";
import { pruneThumbs, thumbsReady, tileKey } from "./lib/thumbs";
import { loadLocale, useLocale } from "./lib/i18n";

export function App() {
  useLocale();
  const [view, setView] = useState<View>("gallery");
  const [settings, setSettings] = useState<Settings | null>(null);
  const [status, setStatus] = useState<Status | null>(null);
  const [update, setUpdate] = useState<UpdateState | null>(null);
  const [clips, setClips] = useState<Clip[]>([]);
  const [gameFilter, setGameFilter] = useState("");
  const [playing, setPlaying] = useState<Clip | null>(null);

  // Also delete stored thumbnails for clips that no longer exist
  const showClips = useCallback((list: Clip[]) => {
    setClips(list);
    pruneThumbs(new Set(list.map(tileKey)));
  }, []);

  const loadClips = useCallback(async () => {
    const [list] = await Promise.all([invoke<Clip[]>("list_clips"), thumbsReady]);
    showClips(list);
  }, [showClips]);

  useEffect(() => {
    invoke<Settings>("get_settings").then(setSettings);
    invoke<Status>("get_status").then(setStatus);
    invoke<UpdateState>("get_update_state").then(setUpdate);

    const unlisten = [
      listen<Status>("status", (e) => setStatus(e.payload)),
      listen("clip-saved", () => loadClips()),
      listen<View>("open-view", (e) => setView(e.payload)),
      listen<UpdateState>("update", (e) => setUpdate(e.payload)),
      listen("locale-changed", () => { void loadLocale(); }),
      // Stop playback when the window is hidden
      listen("main-hidden", () => setPlaying(null)),
    ];
    return () => unlisten.forEach((p) => p.then((off) => off()));
  }, [loadClips]);

  // Refresh the gallery every time it opens
  useEffect(() => {
    if (view === "gallery") loadClips();
  }, [view, loadClips]);

  const closePlayer = useCallback(() => setPlaying(null), []);

  const deleteClip = useCallback(async (clip: Clip) => {
    try {
      await invoke("delete_clip", { path: clip.path });
      setClips((list) => {
        const next = list.filter((c) => c.path !== clip.path);
        pruneThumbs(new Set(next.map(tileKey)));
        return next;
      });
    } catch (e) {
      alert(e);
    }
  }, []);

  return (
    <>
      <div className="grid h-full grid-cols-[236px_1fr]">
        <Sidebar view={view} onView={setView} settings={settings} status={status} onStatus={setStatus} update={update} />
        <main className="flex flex-col overflow-hidden">
          {view === "gallery" ? (
            <Gallery clips={clips} filter={gameFilter} onFilter={setGameFilter} hotkeySave={settings?.hotkeySave} onRefresh={loadClips} onOpen={setPlaying} />
          ) : (
            settings && <SettingsView settings={settings} onSaved={setSettings} onStatus={setStatus} update={update} />
          )}
        </main>
      </div>
      <Player clip={playing} onClose={closePlayer} onDelete={deleteClip} />
    </>
  );
}
