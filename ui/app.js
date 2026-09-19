const { invoke, convertFileSrc } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (sel) => document.querySelector(sel);
const $$ = (sel) => [...document.querySelectorAll(sel)];

let settings = null;
let clips = [];
let gameFilter = "";
let currentClip = null;

// ---------- Segédfüggvények ----------

function prettyHotkey(hotkey) {
  if (!hotkey) return t("hotkey.none");
  return hotkey.split("+").map((p) => p.replace(/^Key/, "").replace(/^Digit/, "")).join("+");
}

function formatDuration(seconds) {
  if (!isFinite(seconds)) return "";
  const s = Math.round(seconds);
  return `${String(Math.floor(s / 60)).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
}

function formatDate(unix) {
  const d = new Date(unix * 1000);
  const pad = (n) => String(n).padStart(2, "0");
  return `${d.getFullYear()}.${pad(d.getMonth() + 1)}.${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

// A játék nélküli klipek az "Egyéb" csoportba kerülnek
const gameOf = (clip) => clip.game || t("gallery.otherGame");

function formatSize(bytes) {
  return bytes >= 1024 ** 3 ? `${(bytes / 1024 ** 3).toFixed(1)} GB` : `${Math.round(bytes / 1024 ** 2)} MB`;
}

// ---------- Nézetek ----------

// A kijelölő a jelenlegi helyzetéből csúszik az új gombhoz (a CSS-átmenet menet közben is átirányítható)
function moveNavIndicator() {
  const active = $(".nav button.active");
  const indicator = $(".nav-indicator");
  if (!active) return;
  indicator.style.transform = `translateY(${active.offsetTop}px)`;
  indicator.style.height = `${active.offsetHeight}px`;
  if (!indicator.classList.contains("ready")) requestAnimationFrame(() => indicator.classList.add("ready"));
}

function showView(name) {
  $$(".nav button").forEach((b) => b.classList.toggle("active", b.dataset.view === name));
  moveNavIndicator();
  $$(".view").forEach((v) => v.classList.toggle("active", v.id === `view-${name}`));
  if (name === "gallery") loadClips();
  if (name === "settings") fillForm();
}

$$(".nav button").forEach((b) => b.addEventListener("click", () => showView(b.dataset.view)));

// ---------- Állapot ----------

let status = null;
// A folyamatban lévő művelet gombja addig tiltva marad, amíg a művelet be nem fejeződik
const pending = new Set();

function renderStatus(next) {
  status = next;
  let text, sub = "";
  if (status.replayActive) {
    text = t("status.replayRunning");
  } else if (!status.obsInstalled) {
    text = t("status.engineMissing");
    sub = t("status.engineMissingHint");
  } else if (!status.obsRunning) {
    text = t("status.captureNotStarted");
    sub = status.error || t("status.starting");
  } else if (!status.replayEnabled) {
    text = t("status.replayDisabled");
    sub = t("status.replayDisabledHint");
  } else {
    text = t("status.replayStopped");
    sub = t("status.restarting");
  }
  $("#status-dot").classList.toggle("on", status.replayActive);
  $("#status-text").textContent = text;
  $("#status-sub").textContent = sub;
  $("#buffer-bar").hidden = !status.replayActive;
  $("#save-now").disabled = !status.replayActive;
  $("#record").disabled = !status.obsRunning || pending.has("record");
  $("#record").classList.toggle("recording", status.recording);
  $("#replay-switch").hidden = !status.obsRunning;
  const toggle = $("#replay-toggle");
  toggle.checked = status.replayEnabled;
  toggle.disabled = pending.has("replay-toggle");
  tick();
}

// Másodpercenként frissülő részek: a puffer telítettsége és a felvétel ideje
function tick() {
  if (!status) return;
  const since = (ms) => Math.max(0, Math.floor((Date.now() - ms) / 1000));
  if (status.replayActive) {
    const total = settings?.bufferSeconds ?? 0;
    const filled = Math.min(total, since(status.bufferSince));
    $("#status-sub").textContent = t("status.buffer", { filled: formatDuration(filled), total: formatDuration(total) });
    $("#buffer-bar span").style.width = `${total ? (filled / total) * 100 : 0}%`;
  }
  $("#record-label").textContent = status.recording
    ? t("actions.stopRecord", { time: formatDuration(since(status.recordingSince)) })
    : t("actions.record");
}

setInterval(tick, 1000);

function renderHotkeyHints() {
  $("#save-kbd").textContent = prettyHotkey(settings.hotkeySave);
  $("#record-kbd").textContent = settings.hotkeyRecord ? prettyHotkey(settings.hotkeyRecord) : "";
}

async function busy(button, action) {
  pending.add(button.id);
  button.disabled = true;
  try {
    await action();
  } catch (e) {
    alert(e);
  } finally {
    pending.delete(button.id);
    renderStatus(await invoke("get_status"));
  }
}

$("#save-now").addEventListener("click", () => invoke("save_replay").catch(() => {}));
$("#record").addEventListener("click", (e) => busy(e.currentTarget, () => invoke("toggle_record")));
$("#replay-toggle").addEventListener("change", (e) =>
  busy(e.currentTarget, () => invoke("set_replay_enabled", { enabled: e.currentTarget.checked })),
);
$("#open-folder").addEventListener("click", () => invoke("open_output_folder"));

// ---------- Galéria ----------

const thumbObserver = new IntersectionObserver((entries) => {
  for (const entry of entries) {
    if (!entry.isIntersecting) continue;
    thumbObserver.unobserve(entry.target);
    loadThumb(entry.target);
  }
}, { rootMargin: "300px" });

function loadThumb(tile) {
  const video = tile.querySelector("video");
  video.addEventListener("loadedmetadata", () => {
    tile.querySelector(".badge").textContent = formatDuration(video.duration);
    video.currentTime = Math.min(3, video.duration * 0.15);
  }, { once: true });
  video.src = convertFileSrc(tile.dataset.path);
}

function renderChips() {
  const games = [...new Set(clips.map(gameOf))].sort((a, b) => a.localeCompare(b, i18n.lang));
  const chips = $("#chips");
  chips.replaceChildren();
  for (const game of ["", ...games]) {
    const chip = document.createElement("button");
    chip.className = "chip" + (game === gameFilter ? " active" : "");
    chip.textContent = game || t("gallery.all");
    chip.addEventListener("click", () => { gameFilter = game; renderChips(); renderGrid(); });
    chips.append(chip);
  }
}

// A csempék (a betöltött előnézettel együtt) megmaradnak; csak az új vagy módosult klip töltődik be
const tiles = new Map();
const tileKey = (clip) => `${clip.path}|${clip.modified}|${clip.size}`;

function createTile(clip) {
  const tile = document.createElement("div");
  tile.className = "tile";
  tile.tabIndex = 0;
  tile.dataset.path = clip.path;
  tile.innerHTML = `
    <div class="thumb"><video muted preload="metadata" playsinline></video><span class="badge"></span></div>
    <div class="meta"><div class="title"></div><div class="sub"></div></div>`;
  tile.querySelector(".title").textContent = clip.game || clip.name;
  tile.querySelector(".sub").textContent = `${formatDate(clip.modified)} · ${formatSize(clip.size)}`;
  tile.title = clip.name;

  const video = tile.querySelector("video");
  tile.addEventListener("mouseenter", () => { if (video.src) video.play().catch(() => {}); });
  tile.addEventListener("mouseleave", () => {
    video.pause();
    if (isFinite(video.duration)) video.currentTime = Math.min(3, video.duration * 0.15);
  });
  tile.addEventListener("click", () => openPlayer(clip));
  tile.addEventListener("keydown", (e) => { if (e.key === "Enter") openPlayer(clip); });
  thumbObserver.observe(tile);
  return tile;
}

function tileFor(clip) {
  const key = tileKey(clip);
  let tile = tiles.get(key);
  if (!tile) {
    tile = createTile(clip);
    tiles.set(key, tile);
  }
  return tile;
}

// A már nem létező klipek csempéit (és a videójukat) elengedi
function pruneTiles() {
  const keep = new Set(clips.map(tileKey));
  for (const [key, tile] of tiles) {
    if (keep.has(key)) continue;
    thumbObserver.unobserve(tile);
    const video = tile.querySelector("video");
    video.removeAttribute("src");
    video.load();
    tile.remove();
    tiles.delete(key);
  }
}

function renderGrid() {
  const visible = clips.filter((c) => !gameFilter || gameOf(c) === gameFilter);
  $("#grid").replaceChildren(...visible.map(tileFor));
  $("#clip-count").textContent = t("gallery.count", { count: visible.length });

  const empty = $("#empty");
  empty.hidden = visible.length > 0;
  if (!visible.length) {
    const title = document.createElement("strong");
    title.textContent = t("gallery.emptyTitle");
    empty.replaceChildren(title, t("gallery.emptyHint", { hotkey: prettyHotkey(settings?.hotkeySave) }));
  }
}

function renderClips() {
  if (gameFilter && !clips.some((c) => gameOf(c) === gameFilter)) gameFilter = "";
  pruneTiles();
  renderChips();
  renderGrid();
}

async function loadClips() {
  clips = await invoke("list_clips");
  renderClips();
}

$("#refresh").addEventListener("click", loadClips);

// ---------- Lejátszó ----------

function openPlayer(clip) {
  currentClip = clip;
  $("#player-title").textContent = clip.name;
  $("#player-sub").textContent = `${gameOf(clip)} · ${formatDate(clip.modified)} · ${formatSize(clip.size)}`;
  resetDelete();
  const player = $("#player");
  player.src = convertFileSrc(clip.path);
  $("#modal").classList.add("open");
  player.play().catch(() => {});
}

function closePlayer() {
  const player = $("#player");
  player.pause();
  player.removeAttribute("src");
  player.load();
  $("#modal").classList.remove("open");
  currentClip = null;
}

function resetDelete() {
  const btn = $("#player-delete");
  btn.classList.remove("confirm");
  btn.textContent = t("player.delete");
}

$("#player-close").addEventListener("click", closePlayer);
$("#modal").addEventListener("click", (e) => { if (e.target.id === "modal") closePlayer(); });
document.addEventListener("keydown", (e) => { if (e.key === "Escape" && currentClip && !capturing) closePlayer(); });
$("#player-reveal").addEventListener("click", () => invoke("reveal_clip", { path: currentClip.path }));
$("#player-open").addEventListener("click", () => { const path = currentClip.path; closePlayer(); invoke("open_clip", { path }); });
$("#player-delete").addEventListener("click", async () => {
  const btn = $("#player-delete");
  if (!btn.classList.contains("confirm")) {
    btn.classList.add("confirm");
    btn.textContent = t("player.deleteConfirm");
    return;
  }
  const path = currentClip.path;
  closePlayer();
  try {
    await invoke("delete_clip", { path });
    clips = clips.filter((c) => c.path !== path);
    renderClips();
  } catch (e) {
    alert(e);
  }
});

// ---------- Beállítások ----------

const form = $("#settings-form");
let draft = null;
let capturing = null;

// A mikrofonlista a futó motortól jön; a mentett, de most nem csatlakoztatott eszköz is választható marad
async function loadMics(selected) {
  const mics = await invoke("list_mics").catch(() => []);
  const select = form.micDevice;
  const options = [["default", t("settings.micDevice.default")], ...mics.map((m) => [m.id, m.name])];
  if (!options.some(([id]) => id === selected)) options.push([selected, t("settings.micDevice.unavailable")]);
  select.replaceChildren(...options.map(([id, name]) => new Option(name, id)));
}

async function fillForm() {
  if (!settings) return;
  await loadMics(settings.micDevice);
  draft = structuredClone(settings);
  for (const el of form.elements) {
    if (!el.name || !(el.name in draft)) continue;
    if (el.type === "checkbox") el.checked = draft[el.name];
    else el.value = draft[el.name];
  }
  $$("[data-hotkey]").forEach((b) => (b.textContent = prettyHotkey(draft[b.dataset.hotkey])));
  $("#ptt-key").textContent = draft.micPttLabel || "?";
  $("#save-msg").textContent = "";
  updateDerived();
  updateDirty();
}

function readForm() {
  const next = { ...draft };
  for (const el of form.elements) {
    if (!el.name || !(el.name in next)) continue;
    if (el.type === "checkbox") next[el.name] = el.checked;
    else if (el.type === "range" || el.name === "fps") next[el.name] = Number(el.value);
    else next[el.name] = el.value.trim();
  }
  return next;
}

// A mentés sáv csak akkor látszik, ha a mentetthez képest változott valami (vagy épp üzenetet mutat)
let savingSettings = false;
let savedMsgTimer = 0;
function updateDirty() {
  const dirty = !!settings && !!draft && JSON.stringify(readForm()) !== JSON.stringify(settings);
  $("#save-settings").disabled = !dirty || savingSettings;
  const msg = $("#save-msg");
  if (dirty && msg.classList.contains("ok")) {
    msg.className = "save-msg";
    msg.textContent = "";
  }
  $(".save-bar").hidden = !dirty && !savingSettings && !msg.textContent;
}

function updateDerived() {
  const buffer = Number(form.bufferSeconds.value);
  const bitrate = Number(form.bitrateMbps.value);
  for (const range of form.querySelectorAll("input[type=range]")) {
    range.style.setProperty("--fill", `${((range.value - range.min) / (range.max - range.min)) * 100}%`);
  }
  $("#buffer-value").textContent = formatDuration(buffer);
  $("#bitrate-value").textContent = `${bitrate} Mbps`;
  $("#size-hint").textContent = t("settings.bitrate.sizeHint", { duration: formatDuration(buffer), size: Math.round((buffer * bitrate) / 8) });
  $("#ptt-row").hidden = form.micMode.value !== "ptt";
  $("#mic-device-row").hidden = form.micMode.value === "off";
}

form.addEventListener("input", () => { updateDerived(); updateDirty(); });
form.addEventListener("change", updateDirty);

$("#browse").addEventListener("click", async () => {
  const dir = await invoke("pick_folder");
  if (dir) form.outputDir.value = dir;
  updateDirty();
});

// Gyorsbillentyű-rögzítés: módosító + billentyű (vagy önálló F-billentyű)
function startCapture(button, onDone) {
  if (capturing) return;
  capturing = button;
  const previous = button.textContent;
  button.classList.add("capturing");
  button.textContent = t("hotkey.capturePrompt");
  invoke("suspend_hotkeys");

  const finish = (value) => {
    window.removeEventListener("keydown", onKey, true);
    window.removeEventListener("mousedown", onMouse, true);
    button.classList.remove("capturing");
    capturing = null;
    invoke("resume_hotkeys");
    if (value === undefined) button.textContent = previous;
    else onDone(value);
  };
  const onKey = (e) => {
    e.preventDefault();
    e.stopPropagation();
    if (["Control", "Alt", "Shift", "Meta", "AltGraph"].includes(e.key)) return onDone.modifiersOnly?.(e, finish);
    if (e.key === "Escape") return finish(undefined);
    finish({ keyboard: e });
  };
  const onMouse = (e) => {
    if (e.target === button && e.button === 0) return; // a rögzítést indító kattintás
    e.preventDefault();
    finish({ mouse: e });
  };
  setTimeout(() => {
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("mousedown", onMouse, true);
  });
}

$$("[data-hotkey]").forEach((button) => {
  button.addEventListener("click", () =>
    startCapture(button, ({ keyboard }) => {
      const field = button.dataset.hotkey;
      if (!keyboard) return (button.textContent = prettyHotkey(draft[field]));
      if (keyboard.key === "Backspace" || keyboard.key === "Delete") {
        draft[field] = "";
      } else {
        const mods = [];
        if (keyboard.ctrlKey) mods.push("Ctrl");
        if (keyboard.altKey) mods.push("Alt");
        if (keyboard.shiftKey) mods.push("Shift");
        if (keyboard.metaKey) mods.push("Super");
        const isFKey = /^F\d+$/.test(keyboard.code);
        if (!mods.length && !isFKey) {
          button.textContent = t("hotkey.needModifier");
          setTimeout(() => (button.textContent = prettyHotkey(draft[field])), 1500);
          return;
        }
        draft[field] = [...mods, keyboard.code].join("+");
      }
      button.textContent = prettyHotkey(draft[field]);
      updateDirty();
    }),
  );
});

$("#ptt-key").addEventListener("click", () => {
  const button = $("#ptt-key");
  const done = async ({ keyboard, mouse }) => {
    let vk, label;
    if (keyboard) {
      vk = keyboard.keyCode;
      label = keyboard.key.length === 1 ? keyboard.key.toUpperCase() : keyboard.key;
    } else {
      const map = { 1: [0x04, t("mouse.middle")], 3: [0x05, t("mouse.x1")], 4: [0x06, t("mouse.x2")] };
      if (!map[mouse.button]) return (button.textContent = draft.micPttLabel);
      [vk, label] = map[mouse.button];
    }
    if (!(await invoke("is_ptt_key_supported", { vk }))) {
      button.textContent = t("hotkey.pttUnsupported");
      setTimeout(() => (button.textContent = draft.micPttLabel), 1800);
      return;
    }
    draft.micPttVk = vk;
    draft.micPttLabel = label;
    button.textContent = label;
    updateDirty();
  };
  // Push-to-talknál egy módosítóbillentyű önmagában is jó (pl. Shift)
  done.modifiersOnly = (e, finish) => finish({ keyboard: e });
  startCapture(button, done);
});

form.addEventListener("submit", async (e) => {
  e.preventDefault();
  if (capturing || savingSettings) return;
  const next = readForm();

  const msg = $("#save-msg");
  savingSettings = true;
  updateDirty();
  msg.className = "save-msg";
  msg.textContent = t("settings.saving");
  try {
    const warning = await invoke("save_settings", { settings: next });
    settings = await invoke("get_settings");
    draft = structuredClone(settings);
    renderHotkeyHints();
    msg.className = warning ? "save-msg error" : "save-msg ok";
    msg.textContent = warning || t("settings.saved");
    clearTimeout(savedMsgTimer);
    savedMsgTimer = setTimeout(() => {
      if (!msg.classList.contains("ok")) return;
      msg.textContent = "";
      updateDirty();
    }, 2500);
    renderStatus(await invoke("get_status"));
  } catch (err) {
    msg.className = "save-msg error";
    msg.textContent = String(err);
  } finally {
    savingSettings = false;
    updateDirty();
  }
});

// ---------- Indulás ----------

async function init() {
  await loadLocale();
  moveNavIndicator();
  settings = await invoke("get_settings");
  renderHotkeyHints();
  renderStatus(await invoke("get_status"));
  await loadClips();

  listen("status", (e) => renderStatus(e.payload));
  listen("clip-saved", () => loadClips());
  listen("open-view", (e) => showView(e.payload));
}

init();
