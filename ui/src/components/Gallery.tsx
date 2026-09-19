import { memo, useCallback, useEffect, useRef, useState } from "react";
import { cx, formatDate, formatDuration, formatSize, gameOf, prettyHotkey } from "../lib/format";
import { i18n, t } from "../lib/i18n";
import { convertFileSrc } from "../lib/tauri";
import type { Clip } from "../lib/tauri";
import { getThumb, previewTime, requestThumb, tileKey } from "../lib/thumbs";
import { Button } from "./ui";

const HOVER_DELAY_MS = 200;

interface Props {
  clips: Clip[];
  filter: string;
  onFilter: (game: string) => void;
  hotkeySave: string | undefined;
  onRefresh: () => void;
  onOpen: (clip: Clip) => void;
}

export function Gallery({ clips, filter, onFilter, hotkeySave, onRefresh, onOpen }: Props) {
  // Egyszerre legfeljebb egy lejátszó előnézet él, kis késleltetéssel indul
  const [hovered, setHovered] = useState<string | null>(null);
  const hoverTimer = useRef(0);

  const games = [...new Set(clips.map(gameOf))].sort((a, b) => a.localeCompare(b, i18n.lang));
  // Ha a szűrt játék utolsó klipje is eltűnt, visszaáll az összesre
  const activeFilter = games.includes(filter) ? filter : "";
  const visible = clips.filter((c) => !activeFilter || gameOf(c) === activeFilter);

  const hover = useCallback((key: string | null) => {
    clearTimeout(hoverTimer.current);
    if (key) hoverTimer.current = window.setTimeout(() => setHovered(key), HOVER_DELAY_MS);
    else setHovered(null);
  }, []);
  useEffect(() => () => clearTimeout(hoverTimer.current), []);

  const open = useCallback((clip: Clip) => {
    hover(null);
    onOpen(clip);
  }, [hover, onOpen]);

  return (
    <section className="flex h-full animate-view-in flex-col overflow-hidden">
      <div className="flex items-baseline gap-3 px-8 pt-[26px] pb-3.5">
        <h1 className="font-display text-[26px] leading-[1.15] font-bold tracking-[-.02em]">{t("gallery.title")}</h1>
        <span className="text-muted tabular">{t("gallery.count", { count: visible.length })}</span>
        <span className="flex-1" />
        <Button className="self-center" onClick={onRefresh}>{t("gallery.refresh")}</Button>
      </div>

      <div className="flex flex-wrap gap-1.5 px-8 pb-4">
        {["", ...games].map((game) => (
          <button
            key={game}
            onClick={() => onFilter(game)}
            className={cx(
              "h-[30px] rounded-full px-3.5 text-[13px] font-medium transition duration-150 active:scale-[.96]",
              game === activeFilter
                ? "bg-fg text-canvas"
                : "bg-panel text-muted shadow-[inset_0_0_0_1px_var(--color-line)] hover:bg-panel-2 hover:text-fg",
            )}
          >
            {game || t("gallery.all")}
          </button>
        ))}
      </div>

      <div className="scroll-area flex-1 px-8 pt-1 pb-8">
        {visible.length > 0 ? (
          <div className="grid grid-cols-[repeat(auto-fill,minmax(250px,1fr))] gap-x-5 gap-y-6">
            {visible.map((clip) => {
              const key = tileKey(clip);
              return (
                <Tile key={key} tileKey={key} clip={clip} previewing={hovered === key} onHover={hover} onOpen={open} />
              );
            })}
          </div>
        ) : (
          <div className="mx-auto max-w-[420px] px-5 py-24 text-center text-muted">
            <strong className="mb-1.5 block text-[17px] font-semibold tracking-[-.01em] text-fg">{t("gallery.emptyTitle")}</strong>
            {t("gallery.emptyHint", { hotkey: prettyHotkey(hotkeySave) })}
          </div>
        )}
      </div>
    </section>
  );
}

interface TileProps {
  tileKey: string;
  clip: Clip;
  previewing: boolean;
  onHover: (key: string | null) => void;
  onOpen: (clip: Clip) => void;
}

const Tile = memo(function Tile({ tileKey: key, clip, previewing, onHover, onOpen }: TileProps) {
  const ref = useRef<HTMLDivElement>(null);
  const [thumb, setThumb] = useState(() => getThumb(key));

  useEffect(() => {
    if (thumb) return;
    return requestThumb(key, clip.path, ref.current!, setThumb);
  }, [key, clip.path, thumb]);

  return (
    <div
      ref={ref}
      tabIndex={0}
      title={clip.name}
      className="group cursor-pointer rounded-xl outline-none focus-visible:outline-none"
      onMouseEnter={() => onHover(key)}
      onMouseLeave={() => onHover(null)}
      onClick={() => onOpen(clip)}
      onKeyDown={(e) => { if (e.key === "Enter") onOpen(clip); }}
    >
      <div
        className={cx(
          "relative aspect-video overflow-hidden rounded-xl bg-[#0b0b0d] transition-[translate,scale,box-shadow] duration-300 ease-spring",
          "shadow-[0_0_0_1px_var(--color-line),0_1px_2px_rgba(0,0,0,.3)]",
          "group-hover:-translate-y-0.5 group-hover:shadow-[0_0_0_1px_var(--color-line-strong),0_12px_28px_rgba(0,0,0,.45)]",
          "group-focus-visible:shadow-[0_0_0_2px_var(--color-focus),0_12px_28px_rgba(0,0,0,.45)]",
          "group-active:scale-[.98] group-active:duration-100",
        )}
      >
        {thumb && <img src={thumb.url} alt="" decoding="async" className="absolute inset-0 block size-full object-cover" />}
        {previewing && <HoverPreview path={clip.path} />}
        {thumb && isFinite(thumb.duration) && (
          <span
            className={cx(
              "absolute right-2 bottom-2 rounded-md bg-black/55 px-[7px] py-0.5 text-xs font-semibold tracking-[.01em] tabular",
              "backdrop-blur-md backdrop-saturate-160 reduce-transparency:bg-black reduce-transparency:backdrop-blur-none",
            )}
          >
            {formatDuration(thumb.duration)}
          </span>
        )}
      </div>
      <div className="px-0.5 pt-2.5">
        <div className="truncate font-semibold tracking-[-.005em]">{clip.game || clip.name}</div>
        <div className="text-[12.5px] text-muted tabular">{`${formatDate(clip.modified)} · ${formatSize(clip.size)}`}</div>
      </div>
    </div>
  );
});

// Lejátszó előnézet; eltávolításkor a dekódert is elengedi
function HoverPreview({ path }: { path: string }) {
  const [playing, setPlaying] = useState(false);
  const attach = useCallback((video: HTMLVideoElement | null) => {
    if (!video) return;
    video.muted = true;
    video.src = convertFileSrc(path);
    video.play().catch(() => {});
    return () => {
      video.pause();
      video.removeAttribute("src");
      video.load();
    };
  }, [path]);
  return (
    <video
      ref={attach}
      loop
      playsInline
      onLoadedMetadata={(e) => { e.currentTarget.currentTime = previewTime(e.currentTarget.duration); }}
      onPlaying={() => setPlaying(true)}
      className={cx(
        "absolute inset-0 block size-full object-cover transition-opacity duration-150 ease-spring",
        playing ? "opacity-100" : "opacity-0",
      )}
    />
  );
}
