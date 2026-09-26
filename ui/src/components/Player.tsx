import { useEffect, useRef, useState } from "react";
import { cx, formatDate, formatSize, gameOf } from "../lib/format";
import { t } from "../lib/i18n";
import { convertFileSrc, invoke } from "../lib/tauri";
import type { Clip } from "../lib/tauri";
import { Button } from "./ui";

interface Props {
  clip: Clip | null;
  onClose: () => void;
  onDelete: (clip: Clip) => void;
}

// Highlight by dimming: the background recedes while the player arrives as a material surface.
// Keep the modal in the DOM so its entrance and exit can be animated.
export function Player({ clip, onClose, onDelete }: Props) {
  const video = useRef<HTMLVideoElement>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  // Keep the last clip's details visible while the modal fades out after closing
  const [shown, setShown] = useState<Clip | null>(null);
  const open = !!clip;

  useEffect(() => {
    const player = video.current!;
    setConfirmDelete(false);
    if (clip) {
      setShown(clip);
      player.src = convertFileSrc(clip.path);
      player.play().catch(() => {});
    } else {
      player.pause();
      player.removeAttribute("src");
      player.load();
    }
  }, [clip]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  const withClose = (action: (clip: Clip) => void) => () => {
    const current = clip!;
    onClose();
    action(current);
  };

  return (
    <div
      className={cx(
        "fixed inset-0 z-10 grid place-items-center bg-black/60 p-8",
        open
          ? "visible opacity-100 backdrop-blur-[16px] [transition:opacity_200ms_ease,visibility_0s,backdrop-filter_250ms_ease] reduce-transparency:bg-black/85 reduce-transparency:backdrop-blur-none"
          : "invisible opacity-0 backdrop-blur-none [transition:opacity_200ms_ease,visibility_0s_linear_250ms,backdrop-filter_250ms_ease]",
      )}
      onClick={(e) => { if (e.target === e.currentTarget) onClose(); }}
    >
      <div
        className={cx(
          "flex w-[min(1200px,100%)] flex-col gap-3.5 transition-[scale] duration-350 ease-spring motion-reduce:scale-100",
          open ? "scale-100" : "scale-[.96]",
        )}
      >
        <video
          ref={video}
          controls
          className="max-h-[calc(100vh-180px)] w-full rounded-xl bg-black shadow-[0_0_0_1px_var(--color-line),0_24px_64px_rgba(0,0,0,.6)]"
        />
        <div className="flex items-center gap-2">
          <div className="min-w-0 flex-1">
            <div className="truncate text-[15px] font-semibold tracking-[-.01em]">{shown?.name}</div>
            <div className="text-[12.5px] text-muted tabular">
              {shown && `${gameOf(shown)} · ${formatDate(shown.modified)} · ${formatSize(shown.size)}`}
            </div>
          </div>
          <Button onClick={() => invoke("reveal_clip", { path: clip!.path })}>{t("player.reveal")}</Button>
          <Button onClick={withClose((c) => invoke("open_clip", { path: c.path }))}>{t("player.openExternal")}</Button>
          <Button
            variant={confirmDelete ? "confirm" : "danger"}
            onClick={confirmDelete ? withClose(onDelete) : () => setConfirmDelete(true)}
          >
            {t(confirmDelete ? "player.deleteConfirm" : "player.delete")}
          </Button>
          <Button onClick={onClose}>{t("player.close")}</Button>
        </div>
      </div>
    </div>
  );
}
