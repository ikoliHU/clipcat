import { useEffect, useRef, useState } from "react";
import type { FormEvent, ReactNode } from "react";
import { captureInput, isCapturing } from "../lib/capture";
import type { Captured } from "../lib/capture";
import { cx, formatDuration, prettyHotkey } from "../lib/format";
import { loadLocale, t } from "../lib/i18n";
import { invoke } from "../lib/tauri";
import type { HotkeyField, Mic, Settings, Status, UpdateState } from "../lib/tauri";
import { Button, Card, Keycap, Range, Row, Select, Switch, TextInput, Value } from "./ui";
import { installUpdate, isInstallable, updateProgressText } from "./updates";
import { GithubIcon } from "./icons";

// Ajánlott bitráta 1080p H.264-hez; a gyors NVENC preset és a kikapcsolt B-képkockák miatt
// magasabb a ShadowPlay értékeinél. A felső határ a settings.rs max_bitrate_mbps táblája.
const BITRATE_1080P: Record<number, number> = { 30: 20, 60: 30, 120: 45, 144: 50 };
const MAX_BITRATE: Record<number, number> = { 30: 80, 60: 100, 120: 130, 144: 150 };
const MIN_BITRATE = 5;

const AUDIO_MBPS = 0.192;
const DISK_DAILY_HOURS = 4;

function outputPixels(resolution: string) {
  const [w, h] = resolution === "native"
    ? [screen.width * devicePixelRatio, screen.height * devicePixelRatio]
    : resolution.split("x").map(Number);
  return w * h;
}

function recommendedBitrate({ fps, resolution, codec }: Pick<Settings, "fps" | "resolution" | "codec">) {
  const pixelFactor = outputPixels(resolution) / (1920 * 1080);
  const value = BITRATE_1080P[fps] * pixelFactor * (codec === "hevc" ? 0.7 : 1);
  return Math.min(MAX_BITRATE[fps], Math.max(MIN_BITRATE, Math.round(value / 5) * 5));
}

// A mentett alak: a szövegmezők körüli szóköz nem számít változásnak
const normalize = (s: Settings): Settings => ({ ...s, outputDir: s.outputDir.trim(), bufferDir: s.bufferDir.trim() });

type Message = { text: string; tone?: "ok" | "error" };

interface Props {
  settings: Settings;
  onSaved: (settings: Settings) => void;
  onStatus: (status: Status) => void;
  update: UpdateState | null;
}

export function SettingsView({ settings, onSaved, onStatus, update }: Props) {
  const [draft, setDraft] = useState<Settings | null>(null);
  const [mics, setMics] = useState<Mic[]>([]);
  const [diskAvailable, setDiskAvailable] = useState(false);
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState<Message | null>(null);
  const messageTimer = useRef(0);
  const [budget, setBudget] = useState<{ maxMb: number; seconds: number } | null>(null);
  useEffect(() => {
    if (!draft || draft.bufferStorage !== "memory") return;
    let alive = true;
    invoke<{ maxMb: number; seconds: number }>("buffer_budget", { seconds: draft.bufferSeconds, bitrateMbps: draft.bitrateMbps })
      .then((value) => { if (alive) setBudget(value); }).catch(() => {});
    return () => { alive = false; };
  }, [draft?.bufferStorage, draft?.bufferSeconds, draft?.bitrateMbps]);

  // A mikrofonlista a futó motortól jön; ffmpeg nélkül a lemezes puffer nem választható
  useEffect(() => {
    let alive = true;
    Promise.all([
      invoke<Mic[]>("list_mics").catch(() => []),
      invoke<boolean>("disk_buffer_available").catch(() => false),
    ]).then(([mics, disk]) => {
      if (!alive) return;
      setMics(mics);
      setDiskAvailable(disk);
      // A csúszka a mentett értéket az FPS-hez tartozó tartományba szorítja
      const max = MAX_BITRATE[settings.fps] ?? MAX_BITRATE[60];
      setDraft({ ...settings, bitrateMbps: Math.min(max, Math.max(MIN_BITRATE, settings.bitrateMbps)) });
    });
    return () => { alive = false; };
    // Csak megnyitáskor tölt; mentés után a draft már a friss beállításokból épül
  }, []);

  useEffect(() => () => clearTimeout(messageTimer.current), []);

  const dirty = !!draft && JSON.stringify(normalize(draft)) !== JSON.stringify(settings);

  // Újabb módosításnál a "Mentve" visszajelzés eltűnik
  useEffect(() => {
    if (dirty && message?.tone === "ok") setMessage(null);
  }, [dirty, message]);

  if (!draft) return <SettingsFrame />;

  const set = <K extends keyof Settings>(key: K, value: Settings[K]) => setDraft((d) => ({ ...d!, [key]: value }));

  // FPS-, felbontás- vagy kodekváltáskor az ajánlott bitrátára ugrik, amit utána szabadon át lehet írni
  const setQuality = (patch: Partial<Pick<Settings, "fps" | "resolution" | "codec">>) =>
    setDraft((d) => {
      const next = { ...d!, ...patch };
      return { ...next, bitrateMbps: recommendedBitrate(next) };
    });

  const pickFolder = async (field: "outputDir" | "bufferDir") => {
    const dir = await invoke<string | null>("pick_folder");
    if (dir) set(field, dir);
  };

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (isCapturing() || saving || !draft) return;
    setSaving(true);
    setMessage({ text: t("settings.saving") });
    try {
      const warning = await invoke<string | null>("save_settings", { settings: normalize(draft) });
      const fresh = await invoke<Settings>("get_settings");
      await loadLocale();
      onSaved(fresh);
      setDraft(fresh);
      setMessage(warning ? { text: warning, tone: "error" } : { text: t("settings.saved"), tone: "ok" });
      clearTimeout(messageTimer.current);
      messageTimer.current = window.setTimeout(() => setMessage((m) => (m?.tone === "ok" ? null : m)), 2500);
      onStatus(await invoke<Status>("get_status"));
    } catch (err) {
      setMessage({ text: String(err), tone: "error" });
    } finally {
      setSaving(false);
    }
  }

  const { bufferSeconds: buffer, bitrateMbps: bitrate, fps } = draft;
  const clipSize = Math.round((buffer * bitrate) / 8);
  // Lemezes puffernél a teljes videó- és hangfolyam az SSD-re kerül; becslés napi DISK_DAILY_HOURS óra
  // játékra, egy átlagos 1 TB-os SSD 600 TBW-os tanúsított írási keretéhez viszonyítva
  const gbPerHour = ((bitrate + AUDIO_MBPS) * 3600) / 8 / 1000;
  const tbPerYear = (gbPerHour * DISK_DAILY_HOURS * 365) / 1000;

  const micOptions: [string, string][] = [["default", t("settings.micDevice.default")], ...mics.map((m): [string, string] => [m.id, m.name])];
  // A mentett, de most nem csatlakoztatott eszköz is választható marad
  if (!micOptions.some(([id]) => id === draft.micDevice)) micOptions.push([draft.micDevice, t("settings.micDevice.unavailable")]);

  const hotkeyCapture = (field: HotkeyField) => (captured: Captured) => {
    if (!captured.keyboard) return;
    const { keyboard } = captured;
    if (keyboard.key === "Backspace" || keyboard.key === "Delete") return set(field, "");
    const mods = [];
    if (keyboard.ctrlKey) mods.push("Ctrl");
    if (keyboard.altKey) mods.push("Alt");
    if (keyboard.shiftKey) mods.push("Shift");
    if (keyboard.metaKey) mods.push("Super");
    // Módosító + billentyű, vagy önálló F-billentyű
    if (!mods.length && !/^F\d+$/.test(keyboard.code)) return { text: t("hotkey.needModifier"), ms: 1500 };
    set(field, [...mods, keyboard.code].join("+"));
  };

  const pttCapture = async (captured: Captured) => {
    let vk: number, label: string;
    if (captured.keyboard) {
      const { keyboard } = captured;
      vk = keyboard.keyCode;
      label = keyboard.key.length === 1 ? keyboard.key.toUpperCase() : keyboard.key;
    } else {
      const map: Record<number, [number, string]> = { 1: [0x04, t("mouse.middle")], 3: [0x05, t("mouse.x1")], 4: [0x06, t("mouse.x2")] };
      const { button } = captured.mouse;
      if (!map[button]) return;
      [vk, label] = map[button];
    }
    if (!(await invoke<boolean>("is_ptt_key_supported", { vk }))) return { text: t("hotkey.pttUnsupported"), ms: 1800 };
    setDraft((d) => ({ ...d!, micPttVk: vk, micPttLabel: label }));
  };

  const hotkeyRows: [HotkeyField, string, string?][] = [
    ["hotkeySave", "settings.hotkeySave.label"],
    ["hotkeyRecord", "settings.hotkeyRecord.label", "settings.hotkeyRecord.hint"],
    ["hotkeyOpenFolder", "settings.hotkeyOpenFolder.label", "settings.hotkeyOpenFolder.hint"],
    ["hotkeyGallery", "settings.hotkeyGallery.label"],
  ];

  const systemSwitches: [keyof Settings, string, string?][] = [
    ["showNotification", "settings.showNotification.label", "settings.showNotification.hint"],
    ["notificationSound", "settings.notificationSound.label"],
    ["autostart", "settings.autostart.label"],
    ["keepObsRunning", "settings.keepRunning.label", "settings.keepRunning.hint"],
  ];

  return (
    <SettingsFrame>
      <form className="mx-auto flex w-full max-w-[800px] flex-col gap-7" autoComplete="off" onSubmit={submit}>
        <Card title={t("settings.capture.title")}>
          <Row label={t("settings.outputDir.label")} hints={[t("settings.outputDir.hint")]}>
            <TextInput wide readOnly aria-label={t("settings.outputDir.label")} value={draft.outputDir} />
            <Button onClick={() => pickFolder("outputDir")}>{t("settings.browse")}</Button>
          </Row>
          <Row label={t("settings.buffer.label")} hints={[t("settings.buffer.hint")]}>
            <Range min={10} max={1200} step={10} value={buffer} onChange={(e) => set("bufferSeconds", Number(e.target.value))} />
            <Value>{formatDuration(buffer)}</Value>
          </Row>
          <Row label={t("settings.storage.label")} hints={[
            t("settings.storage.hint", { size: clipSize }),
            draft.bufferStorage === "memory" && budget && t("settings.memoryBudget", { size: budget.maxMb, duration: formatDuration(budget.seconds) }),
          ]}>
            <Select value={draft.bufferStorage} onChange={(e) => set("bufferStorage", e.target.value as Settings["bufferStorage"])}>
              <option value="memory">{t("settings.storage.memory")}</option>
              <option value="disk" disabled={!diskAvailable}>
                {t(diskAvailable ? "settings.storage.disk" : "settings.storage.diskMissing")}
              </option>
            </Select>
          </Row>
          {draft.bufferStorage === "disk" && (
            <Row
              label={t("settings.bufferDir.label")}
              hints={[{
                tone: "warn",
                text: t("settings.bufferDir.hint", {
                  hour: Math.round(gbPerHour),
                  daily: DISK_DAILY_HOURS,
                  year: Math.round(tbPerYear),
                  percent: Math.max(1, Math.round((tbPerYear / 600) * 100)),
                }),
              }]}
            >
              <TextInput wide readOnly aria-label={t("settings.bufferDir.label")} value={draft.bufferDir} />
              <Button onClick={() => pickFolder("bufferDir")}>{t("settings.browse")}</Button>
            </Row>
          )}
          <Row label={t("settings.resolution.label")}>
            <Select value={draft.resolution} onChange={(e) => setQuality({ resolution: e.target.value })}>
              <option value="native">{t("settings.resolution.native")}</option>
              <option value="2560x1440">1440p (2560×1440)</option>
              <option value="1920x1080">1080p (1920×1080)</option>
              <option value="1280x720">720p (1280×720)</option>
            </Select>
          </Row>
          <Row label={t("settings.fps.label")}>
            <Select value={fps} onChange={(e) => setQuality({ fps: Number(e.target.value) })}>
              {[30, 60, 120, 144].map((v) => <option key={v} value={v}>{v} FPS</option>)}
            </Select>
          </Row>
          <Row
            label={t("settings.bitrate.label")}
            hints={[
              t("settings.bitrate.sizeHint", { duration: formatDuration(buffer), size: clipSize }),
              t("settings.bitrate.recommended", { value: recommendedBitrate(draft), max: MAX_BITRATE[fps] }),
            ]}
          >
            {/* A felső határ az FPS-től függ */}
            <Range min={MIN_BITRATE} max={MAX_BITRATE[fps]} step={5} value={bitrate} onChange={(e) => set("bitrateMbps", Number(e.target.value))} />
            <Value>{bitrate} Mbps</Value>
          </Row>
          <Row label={t("settings.codec.label")}>
            <Select value={draft.codec} onChange={(e) => setQuality({ codec: e.target.value as Settings["codec"] })}>
              <option value="h264">{t("settings.codec.h264")}</option>
              <option value="hevc">{t("settings.codec.hevc")}</option>
            </Select>
          </Row>
          <Row label={t("settings.captureDesktop.label")} hints={[t("settings.captureDesktop.hint")]}>
            <Switch checked={draft.captureDesktop} onChange={(e) => set("captureDesktop", e.target.checked)} />
          </Row>
        </Card>

        <Card title={t("settings.audio.title")}>
          <Row label={t("settings.mic.label")}>
            <Select value={draft.micMode} onChange={(e) => set("micMode", e.target.value as Settings["micMode"])}>
              <option value="off">{t("settings.mic.off")}</option>
              <option value="ptt">{t("settings.mic.ptt")}</option>
              <option value="always">{t("settings.mic.always")}</option>
            </Select>
          </Row>
          {draft.micMode !== "off" && (
            <Row label={t("settings.micDevice.label")}>
              <Select value={draft.micDevice} onChange={(e) => set("micDevice", e.target.value)}>
                {micOptions.map(([id, name]) => <option key={id} value={id}>{name}</option>)}
              </Select>
            </Row>
          )}
          {draft.micMode === "ptt" && (
            <Row label={t("settings.pttKey.label")} hints={[t("settings.pttKey.hint")]}>
              <CaptureButton label={draft.micPttLabel || "?"} modifiersOnly onCapture={pttCapture} />
            </Row>
          )}
        </Card>

        <Card title={t("settings.hotkeys.title")}>
          {hotkeyRows.map(([field, label, hint]) => (
            <Row key={field} label={t(label)} hints={[hint && t(hint)]}>
              <CaptureButton label={prettyHotkey(draft[field])} onCapture={hotkeyCapture(field)} />
            </Row>
          ))}
        </Card>

        <Card title={t("settings.system.title")}>
          <Row label={t("settings.language.label")} hints={[t("settings.language.hint")]}>
            <Select aria-label={t("settings.language.label")} value={draft.language} onChange={(e) => set("language", e.target.value as Settings["language"])}>
              <option value="hu">Magyar</option>
              <option value="en-US">English US</option>
            </Select>
          </Row>
          {systemSwitches.map(([field, label, hint]) => (
            <Row key={field} label={t(label)} hints={[hint && t(hint)]}>
              <Switch checked={draft[field] as boolean} onChange={(e) => set(field, e.target.checked)} />
            </Row>
          ))}
        </Card>

        <Card title={t("settings.info.title")}>
          {update && <UpdateRow update={update} />}
          <Row label={t("settings.info.license")}>
            <Button onClick={() => invoke("open_project_link", { target: "license" }).catch((e) => setMessage({ text: String(e), tone: "error" }))}>
              {t("settings.info.license")}
            </Button>
          </Row>
          <Row label="GitHub" hints={["catninth/clipcat"]}>
            <Button aria-label={t("settings.info.repository")} title={t("settings.info.repository")} onClick={() => invoke("open_project_link", { target: "repository" }).catch((e) => setMessage({ text: String(e), tone: "error" }))}>
              <GithubIcon className="size-5" aria-hidden="true" />
            </Button>
          </Row>
        </Card>

        {/* Lebegő, áttetsző mentés sáv: csak akkor látszik, ha változott valami (vagy épp üzenetet mutat) */}
        {(dirty || saving || message) && (
          <div
            className={cx(
              "sticky bottom-0 -mt-2 flex animate-bar-in items-center gap-3.5 rounded-xl bg-[rgba(38,38,43,.72)] py-2.5 pr-2.5 pl-4",
              "shadow-[inset_0_1px_0_rgba(255,255,255,.06),0_0_0_1px_var(--color-line),0_12px_32px_rgba(0,0,0,.45)]",
              "backdrop-blur-[20px] backdrop-saturate-160 reduce-transparency:bg-panel-2 reduce-transparency:backdrop-blur-none",
            )}
          >
            <span className="flex-1 text-[12.5px] text-muted">{t("settings.saveNote")}</span>
            {message && (
              <span
                className={cx(
                  "text-[13px]",
                  message.tone === "ok" && "text-accent",
                  message.tone === "error" && "whitespace-pre-line text-danger-soft",
                )}
              >
                {message.text}
              </span>
            )}
            <Button type="submit" variant="primary" disabled={!dirty || saving}>{t("settings.save")}</Button>
          </div>
        )}
      </form>
    </SettingsFrame>
  );
}

// Olvasható szélességű, középre igazított oszlop; a cím ugyanahhoz az élhez igazodik
function SettingsFrame({ children }: { children?: ReactNode }) {
  return (
    <section className="flex h-full animate-view-in flex-col overflow-hidden">
      <div className="mx-auto flex w-full max-w-[864px] items-baseline gap-3 px-8 pt-[26px] pb-3.5">
        <h1 className="font-display text-[26px] leading-[1.15] font-bold tracking-[-.02em]">{t("settings.title")}</h1>
      </div>
      <div className="scroll-area flex-1 px-8 pt-1 pb-8">{children}</div>
    </section>
  );
}

function UpdateRow({ update }: { update: UpdateState }) {
  const installable = isInstallable(update);
  const working = update.phase === "downloading" || update.phase === "installing";
  const phaseText: Partial<Record<UpdateState["phase"], string | null>> = {
    checking: t("update.checking"),
    latest: t("update.latest"),
    available: t("update.available", { version: update.version ?? "" }),
    error: update.error,
  };
  let hint = updateProgressText(update) ?? phaseText[update.phase] ?? t("update.idle");
  if (update.phase === "available" && update.notes) hint += `\n${update.notes}`;

  return (
      <Row
        label={t("settings.update.version", { version: update.current })}
        hints={[{ text: hint, tone: update.phase === "error" ? "error" : undefined, className: "whitespace-pre-line" }]}
      >
        <Button
          variant={installable ? "primary" : "default"}
          disabled={working || update.phase === "checking"}
          // A hiba az update eseménnyel érkezik
          onClick={() => (installable ? installUpdate() : invoke("check_update").catch(() => {}))}
        >
          {t(installable ? "update.install" : "update.check")}
        </Button>
      </Row>
  );
}

type Flash = { text: string; ms: number };

// Rögzítés alatt felszólítást mutat; a feldolgozó rövid üzenetet adhat vissza (pl. hiányzó módosító)
function CaptureButton({ label, modifiersOnly, onCapture }: {
  label: string;
  modifiersOnly?: boolean;
  onCapture: (captured: Captured) => Flash | void | Promise<Flash | void>;
}) {
  const [capturing, setCapturing] = useState(false);
  const [flash, setFlash] = useState<string | null>(null);
  const flashTimer = useRef(0);
  const controller = useRef<AbortController | null>(null);
  useEffect(() => () => { clearTimeout(flashTimer.current); controller.current?.abort(); }, []);

  async function start(button: HTMLButtonElement) {
    if (isCapturing()) return;
    setCapturing(true);
    const abort = new AbortController();
    controller.current = abort;
    let captured: Captured | null;
    try { captured = await captureInput(button, modifiersOnly, abort.signal); }
    catch (error) { if (!abort.signal.aborted) setFlash(String(error)); captured = null; }
    if (abort.signal.aborted) return;
    setCapturing(false);
    if (!captured) return;
    let result: Flash | void;
    try { result = await onCapture(captured); }
    catch (error) { if (!abort.signal.aborted) setFlash(String(error)); return; }
    if (abort.signal.aborted) return;
    if (!result) return;
    clearTimeout(flashTimer.current);
    setFlash(result.text);
    flashTimer.current = window.setTimeout(() => setFlash(null), result.ms);
  }

  return (
    <Keycap active={capturing} onClick={(e) => start(e.currentTarget)}>
      {capturing ? t("hotkey.capturePrompt") : flash ?? label}
    </Keycap>
  );
}
