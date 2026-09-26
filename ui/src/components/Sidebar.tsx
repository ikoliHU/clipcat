import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { cx, formatDuration, prettyHotkey } from "../lib/format";
import { t } from "../lib/i18n";
import { invoke } from "../lib/tauri";
import type { Settings, Status, UpdateState, View } from "../lib/tauri";
import { DownloadIcon, FolderIcon, GearIcon, GridIcon } from "./icons";
import { Button, Kbd, LinkButton, Switch } from "./ui";
import { installUpdate, isInstallable, updateProgressText } from "./updates";

interface Props {
  view: View;
  onView: (view: View) => void;
  settings: Settings | null;
  status: Status | null;
  onStatus: (status: Status) => void;
  update: UpdateState | null;
}

export function Sidebar({ view, onView, settings, status, onStatus, update }: Props) {
  return (
    <aside className="flex flex-col gap-5 border-r border-line bg-sidebar px-3 pt-[18px] pb-3">
      <div className="flex items-center gap-2.5 px-2 py-0.5 font-display text-[17px] leading-[1.2] font-semibold tracking-[-.01em]">
        <img src="/logo.png" alt="" className="size-[26px]" />
        ClipCat
      </div>
      <Nav view={view} onView={onView} />
      <StatusCard settings={settings} status={status} onStatus={onStatus} update={update} />
    </aside>
  );
}

const NAV_ITEMS: { view: View; Icon: typeof GridIcon; label: string }[] = [
  { view: "gallery", Icon: GridIcon, label: "nav.gallery" },
  { view: "settings", Icon: GearIcon, label: "nav.settings" },
];

// One shared selection indicator slides between buttons, avoiding leftover shadows on each button
function Nav({ view, onView }: { view: View; onView: (view: View) => void }) {
  const buttons = useRef(new Map<View, HTMLButtonElement>());
  const [indicator, setIndicator] = useState<{ top: number; height: number } | null>(null);
  const [ready, setReady] = useState(false);

  useLayoutEffect(() => {
    const active = buttons.current.get(view);
    if (active) setIndicator({ top: active.offsetTop, height: active.offsetHeight });
  }, [view]);

  // Enable transitions after initial placement so the indicator does not slide in from the top
  useEffect(() => {
    if (!indicator || ready) return;
    const frame = requestAnimationFrame(() => setReady(true));
    return () => cancelAnimationFrame(frame);
  }, [indicator, ready]);

  return (
    <nav className="relative flex flex-col gap-0.5">
      <span
        aria-hidden
        className={cx(
          "pointer-events-none absolute inset-x-0 top-0 h-9 rounded-lg bg-panel-2",
          "[transition:transform_350ms_var(--ease-spring),height_350ms_var(--ease-spring),opacity_150ms_ease]",
          ready ? "opacity-100" : "opacity-0",
        )}
        style={indicator ? { transform: `translateY(${indicator.top}px)`, height: indicator.height } : undefined}
      />
      {NAV_ITEMS.map(({ view: target, Icon, label }) => {
        const active = view === target;
        return (
          <button
            key={target}
            ref={(el) => { if (el) buttons.current.set(target, el); }}
            onClick={() => onView(target)}
            className={cx(
              "relative flex h-9 items-center gap-2.5 rounded-lg px-2.5 text-left transition duration-150 active:scale-[.98]",
              active ? "font-semibold text-fg" : "text-muted hover:bg-white/4 hover:text-fg",
            )}
          >
            <Icon className={cx("size-[18px] flex-none transition-colors duration-150", active && "text-accent")} />
            <span>{t(label)}</span>
          </button>
        );
      })}
    </nav>
  );
}

// Redraw once per second to calculate buffer fullness and recording duration
function useNow() {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, []);
  return now;
}

function statusText(status: Status | null): [string, string] {
  if (!status) return [t("status.connecting"), ""];
  if (status.replayActive) return [t("status.replayRunning"), ""];
  if (!status.obsInstalled) return [t("status.engineMissing"), t("status.engineMissingHint")];
  if (!status.obsRunning) return [t("status.captureNotStarted"), status.error || t("status.starting")];
  if (!status.replayEnabled) return [t("status.replayDisabled"), t("status.replayDisabledHint")];
  return [t("status.replayStopped"), t("status.restarting")];
}

function StatusCard({ settings, status, onStatus, update }: Omit<Props, "view" | "onView">) {
  const now = useNow();
  // Keep the control disabled until its operation completes
  const [pending, setPending] = useState<{ record?: boolean; replay?: boolean }>({});

  async function busy(key: "record" | "replay", value: boolean, action: () => Promise<unknown>) {
    setPending((p) => ({ ...p, [key]: value }));
    try {
      await action();
    } catch (e) {
      alert(e);
    } finally {
      setPending((p) => ({ ...p, [key]: undefined }));
      onStatus(await invoke<Status>("get_status"));
    }
  }

  const since = (ms: number) => Math.max(0, Math.floor((now - ms) / 1000));
  let [title, sub] = statusText(status);
  let fill = 0;
  if (status?.replayActive) {
    const total = status.bufferSeconds || settings?.bufferSeconds || 0;
    const filled = Math.min(total, since(status.bufferSince));
    sub = t("status.buffer", { filled: formatDuration(filled), total: formatDuration(total) });
    fill = total ? (filled / total) * 100 : 0;
  }
  const recording = !!status?.recording;

  const installable = !!update && isInstallable(update);
  const working = update?.phase === "downloading" || update?.phase === "installing";
  const showUpdate = installable || working || (!!update?.version && update.phase === "checking");

  return (
    <div className="mt-auto flex flex-col overflow-hidden rounded-xl bg-panel shadow-[inset_0_0_0_1px_var(--color-line),0_8px_24px_rgba(0,0,0,.25)]">
      <section className="flex flex-col gap-2.5 p-3">
        <div className="flex min-h-[22px] items-center gap-2">
          <span
            className={cx(
              "size-2 flex-none rounded-full transition-colors duration-200",
              status?.replayActive ? "bg-accent shadow-[0_0_0_3px_var(--color-accent-soft)]" : "bg-faint",
            )}
          />
          <span className="min-w-0 flex-1 text-[13.5px] font-semibold tracking-[-.005em]">{title}</span>
          {status?.obsRunning && (
            <Switch
              small
              title={t("status.replayToggleTitle")}
              checked={pending.replay ?? status.replayEnabled}
              disabled={pending.replay !== undefined}
              onChange={(e) => {
                const enabled = e.currentTarget.checked;
                busy("replay", enabled, () => invoke("set_replay_enabled", { enabled }));
              }}
            />
          )}
        </div>
        {sub && <div className="-mt-1 text-xs leading-[1.35] text-muted tabular">{sub}</div>}
        {status?.error && <div className="text-xs text-danger-soft">{status.error}</div>}
        {status?.encoder && <div className="text-xs text-muted">{t("status.encoder", { name: status.encoder })}</div>}
        {status?.encoder === "obs_x264" && <div className="text-xs text-warn-soft">{t("status.cpuEncoder")}</div>}
        {status?.replayActive && (
          <div className="h-[3px] overflow-hidden rounded-[3px] bg-panel-3">
            <span className="block h-full rounded-[inherit] bg-accent transition-[width] duration-1000 ease-linear" style={{ width: `${fill}%` }} />
          </div>
        )}
        <Button variant="primary" wide disabled={!status?.replayActive} onClick={() => invoke("save_replay").catch(() => {})}>
          <span>{t("actions.saveNow")}</span>
          <Kbd>{prettyHotkey(settings?.hotkeySave)}</Kbd>
        </Button>
      </section>

      <section className="flex flex-col gap-2.5 p-3 pt-0">
        <Button
          variant={recording ? "recording" : "default"}
          wide
          disabled={!status?.obsRunning || !!pending.record}
          onClick={() => busy("record", true, () => invoke("toggle_record"))}
        >
          <span
            className={cx(
              "size-2 flex-none transition-[border-radius,background-color] duration-200 ease-spring",
              recording ? "rounded-[2px] bg-white" : "rounded-full bg-danger",
            )}
          />
          <span className="mr-auto">
            {recording ? t("actions.stopRecord", { time: formatDuration(since(status!.recordingSince)) }) : t("actions.record")}
          </span>
          {settings?.hotkeyRecord && <Kbd>{prettyHotkey(settings.hotkeyRecord)}</Kbd>}
        </Button>
      </section>

      {update && showUpdate && (
        <LinkButton
          accent
          className="disabled:cursor-default"
          disabled={!installable}
          title={update.version ? t("update.pillTitle", { version: update.version }) : ""}
          onClick={installUpdate}
        >
          <DownloadIcon />
          <span>{updateProgressText(update) ?? t("update.pill", { version: update.version ?? "" })}</span>
        </LinkButton>
      )}

      <LinkButton onClick={() => invoke("open_output_folder")}>
        <FolderIcon />
        <span>{t("actions.openFolder")}</span>
      </LinkButton>
    </div>
  );
}
