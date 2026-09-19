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

// Az előnézet állókép (JPEG), nem élő videó: a csempék nem tartanak nyitva dekódert.
// A képet egy rejtett videóból rajzoljuk ki, egyszerre legfeljebb THUMB_WORKERS darabot,
// és csak a képernyőn (vagy közelében) lévő csempékhez. Az eredmény IndexedDB-be kerül.
const THUMB_WORKERS = 2;
const THUMB_TIMEOUT_MS = 15000;
const THUMB_WIDTH = 480;
const HOVER_DELAY_MS = 200;
const previewTime = (duration) => (isFinite(duration) ? Math.min(3, duration * 0.15) : 0);

const thumbCache = new Map(); // tileKey -> { blob, duration }
const thumbFailed = new Set(); // ebben a munkamenetben nem sikerült; nem próbáljuk újra
const thumbQueue = new Set(); // előnézetre váró, látható csempék
let thumbActive = 0;

const thumbDb = new Promise((resolve, reject) => {
  const req = indexedDB.open("clipcat", 1);
  req.onupgradeneeded = () => req.result.createObjectStore("thumbs");
  req.onsuccess = () => resolve(req.result);
  req.onerror = () => reject(req.error);
});

function thumbStore(mode, action) {
  return thumbDb.then((db) => new Promise((resolve, reject) => {
    const tx = db.transaction("thumbs", mode);
    action(tx.objectStore("thumbs"));
    tx.oncomplete = resolve;
    tx.onerror = () => reject(tx.error);
  }));
}

// Indításkor egyszer beolvassa a tárolt előnézeteket; ha az IndexedDB nem elérhető, csak memóriában gyorsítótáraz
const thumbCacheReady = thumbStore("readonly", (store) => {
  store.openCursor().onsuccess = (e) => {
    const cursor = e.target.result;
    if (!cursor) return;
    thumbCache.set(cursor.key, cursor.value);
    cursor.continue();
  };
}).catch(() => {});

function pruneThumbCache(keep) {
  const stale = [...thumbCache.keys()].filter((key) => !keep.has(key));
  if (!stale.length) return;
  for (const key of stale) thumbCache.delete(key);
  thumbStore("readwrite", (store) => stale.forEach((key) => store.delete(key))).catch(() => {});
}

// Egy rejtett videóból kivesz egy képkockát; minden ágon elengedi a videót
function captureThumb(path) {
  return new Promise((resolve, reject) => {
    const video = document.createElement("video");
    video.muted = true;
    video.preload = "auto";
    video.crossOrigin = "anonymous"; // enélkül a canvas "szennyezett" lenne, és a toBlob hibát dobna
    let done = false;
    const finish = (err, result) => {
      if (done) return;
      done = true;
      clearTimeout(timer);
      video.removeAttribute("src");
      video.load();
      err ? reject(err) : resolve(result);
    };
    const timer = setTimeout(() => finish(new Error("timeout")), THUMB_TIMEOUT_MS);
    const draw = () => {
      if (!video.videoWidth) return finish(new Error("no video track"));
      const canvas = document.createElement("canvas");
      canvas.width = THUMB_WIDTH;
      canvas.height = Math.round((THUMB_WIDTH * video.videoHeight) / video.videoWidth);
      canvas.getContext("2d").drawImage(video, 0, 0, canvas.width, canvas.height);
      const duration = video.duration;
      canvas.toBlob((blob) => (blob ? finish(null, { blob, duration }) : finish(new Error("encode failed"))), "image/jpeg", 0.8);
    };
    video.addEventListener("error", () => finish(video.error || new Error("load failed")), { once: true });
    video.addEventListener("loadedmetadata", () => {
      const at = previewTime(video.duration);
      if (at > 0) {
        video.addEventListener("seeked", draw, { once: true });
        video.currentTime = at;
      } else {
        video.addEventListener("loadeddata", draw, { once: true });
      }
    }, { once: true });
    video.src = convertFileSrc(path);
  });
}

function applyThumb(tile, { blob, duration }) {
  const img = tile.querySelector("img");
  if (img.src) URL.revokeObjectURL(img.src);
  img.src = URL.createObjectURL(blob);
  img.hidden = false;
  tile.querySelector(".badge").textContent = formatDuration(duration);
}

function pumpThumbs() {
  while (thumbActive < THUMB_WORKERS && thumbQueue.size) {
    const tile = thumbQueue.values().next().value;
    thumbQueue.delete(tile);
    thumbObserver.unobserve(tile);
    thumbActive++;
    generateThumb(tile).finally(() => {
      thumbActive--;
      pumpThumbs();
    });
  }
}

async function generateThumb(tile) {
  const key = tile.dataset.key;
  try {
    const thumb = await captureThumb(tile.dataset.path);
    thumbCache.set(key, thumb);
    if (tiles.get(key) === tile) applyThumb(tile, thumb);
    thumbStore("readwrite", (store) => store.put(thumb, key)).catch(() => {});
  } catch (e) {
    thumbFailed.add(key);
    console.warn("thumbnail failed", tile.dataset.path, e);
  }
}

// A képernyőről elgörgetett csempe kikerül a sorból, így a gyors görgetés nem halmoz fel munkát
const thumbObserver = new IntersectionObserver((entries) => {
  for (const { target, isIntersecting } of entries) {
    if (isIntersecting) thumbQueue.add(target);
    else thumbQueue.delete(target);
  }
  pumpThumbs();
}, { rootMargin: "300px" });

// Egyszerre legfeljebb egy lejátszó előnézet él; elhagyáskor teljesen felszabadul
let hover = null; // { tile, video, timer }

function stopHover() {
  if (!hover) return;
  clearTimeout(hover.timer);
  if (hover.video) {
    hover.video.pause();
    hover.video.removeAttribute("src");
    hover.video.load();
    hover.video.remove();
  }
  hover = null;
}

function startHover(tile) {
  stopHover();
  const current = { tile, video: null, timer: 0 };
  current.timer = setTimeout(() => {
    const video = document.createElement("video");
    video.muted = true;
    video.playsInline = true;
    video.loop = true;
    video.addEventListener("loadedmetadata", () => { video.currentTime = previewTime(video.duration); }, { once: true });
    video.addEventListener("playing", () => video.classList.add("playing"), { once: true });
    video.src = convertFileSrc(tile.dataset.path);
    tile.querySelector(".thumb").insertBefore(video, tile.querySelector(".badge"));
    current.video = video;
    video.play().catch(() => {});
  }, HOVER_DELAY_MS);
  hover = current;
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
  tile.dataset.key = tileKey(clip);
  tile.innerHTML = `
    <div class="thumb"><img alt="" decoding="async" hidden><span class="badge"></span></div>
    <div class="meta"><div class="title"></div><div class="sub"></div></div>`;
  tile.querySelector(".title").textContent = clip.game || clip.name;
  tile.querySelector(".sub").textContent = `${formatDate(clip.modified)} · ${formatSize(clip.size)}`;
  tile.title = clip.name;

  tile.addEventListener("mouseenter", () => startHover(tile));
  tile.addEventListener("mouseleave", () => { if (hover?.tile === tile) stopHover(); });
  tile.addEventListener("click", () => openPlayer(clip));
  tile.addEventListener("keydown", (e) => { if (e.key === "Enter") openPlayer(clip); });

  const cached = thumbCache.get(tile.dataset.key);
  if (cached) applyThumb(tile, cached);
  else if (!thumbFailed.has(tile.dataset.key)) thumbObserver.observe(tile);
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

// A már nem létező klipek csempéit, képét és tárolt előnézetét elengedi
function pruneTiles() {
  const keep = new Set(clips.map(tileKey));
  for (const [key, tile] of tiles) {
    if (keep.has(key)) continue;
    thumbObserver.unobserve(tile);
    thumbQueue.delete(tile);
    if (hover?.tile === tile) stopHover();
    const img = tile.querySelector("img");
    if (img.src) URL.revokeObjectURL(img.src);
    tile.remove();
    tiles.delete(key);
  }
  pruneThumbCache(keep);
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
  const [list] = await Promise.all([invoke("list_clips"), thumbCacheReady]);
  clips = list;
  renderClips();
}

$("#refresh").addEventListener("click", loadClips);

// ---------- Lejátszó ----------

function openPlayer(clip) {
  stopHover();
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

// Ffmpeg nélkül a lemezes puffer nem választható (a már beállított érték látszik, de nem menthető)
async function loadStorageOptions() {
  const available = await invoke("disk_buffer_available").catch(() => false);
  const option = $("#storage-disk");
  option.disabled = !available;
  option.textContent = t(available ? "settings.storage.disk" : "settings.storage.diskMissing");
}

async function fillForm() {
  if (!settings) return;
  await Promise.all([loadMics(settings.micDevice), loadStorageOptions()]);
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
  $("#storage-hint").textContent = t("settings.storage.hint", { size: Math.round((buffer * bitrate) / 8) });
  $("#buffer-dir-hint").textContent = t("settings.bufferDir.hint", { size: Math.round((bitrate * 3600) / 8 / 1000) });
  $("#buffer-dir-row").hidden = form.bufferStorage.value !== "disk";
  $("#ptt-row").hidden = form.micMode.value !== "ptt";
  $("#mic-device-row").hidden = form.micMode.value === "off";
}

form.addEventListener("input", () => { updateDerived(); updateDirty(); });
form.addEventListener("change", updateDirty);

for (const [button, field] of [["#browse", "outputDir"], ["#browse-buffer", "bufferDir"]]) {
  $(button).addEventListener("click", async () => {
    const dir = await invoke("pick_folder");
    if (dir) form[field].value = dir;
    updateDirty();
  });
}

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

// ---------- Frissítés ----------

let update = null;

function renderUpdate(next) {
  update = next;
  const { phase, version, progress } = update;
  // A talált verzió hiba után is telepíthető marad (újrapróbálás), újraellenőrzés alatt pedig látszik
  const installable = !!version && (phase === "available" || phase === "error");
  const working = phase === "downloading" || phase === "installing";

  const pill = $("#update-btn");
  pill.hidden = !installable && !working && !(version && phase === "checking");
  pill.disabled = !installable;
  pill.title = version ? t("update.pillTitle", { version }) : "";
  $("#update-label").textContent = updateProgressText() ?? t("update.pill", { version });

  $("#update-version").textContent = t("settings.update.version", { version: update.current });
  const hint = $("#update-hint");
  hint.classList.toggle("error", phase === "error");
  hint.textContent = updateProgressText() ?? {
    checking: t("update.checking"),
    latest: t("update.latest"),
    available: t("update.available", { version }),
    error: update.error,
  }[phase] ?? t("update.idle");
  if (phase === "available" && update.notes) hint.textContent += `
${update.notes}`;

  const action = $("#update-action");
  action.textContent = t(installable ? "update.install" : "update.check");
  action.classList.toggle("primary", installable);
  action.disabled = working || phase === "checking";
}

function updateProgressText() {
  if (update.phase === "installing") return t("update.installing");
  if (update.phase !== "downloading") return null;
  return update.progress == null ? t("update.downloading") : t("update.downloadingProgress", { progress: update.progress });
}

async function installUpdate() {
  try {
    await invoke("install_update");
  } catch (e) {
    alert(e);
  }
}

$("#update-btn").addEventListener("click", installUpdate);
$("#update-action").addEventListener("click", () => {
  if (update?.version && (update.phase === "available" || update.phase === "error")) installUpdate();
  else invoke("check_update").catch(() => {}); // a hiba az update eseménnyel érkezik
});

// ---------- Indulás ----------

async function init() {
  await loadLocale();
  moveNavIndicator();
  settings = await invoke("get_settings");
  renderHotkeyHints();
  renderStatus(await invoke("get_status"));
  renderUpdate(await invoke("get_update_state"));
  await loadClips();

  listen("status", (e) => renderStatus(e.payload));
  listen("clip-saved", () => loadClips());
  listen("open-view", (e) => showView(e.payload));
  listen("update", (e) => renderUpdate(e.payload));
}

init();
