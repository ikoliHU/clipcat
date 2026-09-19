// Az előnézet állókép (JPEG), nem élő videó: a csempék nem tartanak nyitva dekódert.
// A képet egy rejtett videóból rajzoljuk ki, egyszerre legfeljebb THUMB_WORKERS darabot,
// és csak a képernyőn (vagy közelében) lévő csempékhez. Az eredmény IndexedDB-be kerül.
import { convertFileSrc } from "./tauri";
import type { Clip } from "./tauri";

const THUMB_WORKERS = 2;
const THUMB_TIMEOUT_MS = 15000;
const THUMB_WIDTH = 480;

export const previewTime = (duration: number) => (isFinite(duration) ? Math.min(3, duration * 0.15) : 0);
export const tileKey = (clip: Clip) => `${clip.path}|${clip.modified}|${clip.size}`;

interface Stored {
  blob: Blob;
  duration: number;
}
export interface Thumb {
  url: string;
  duration: number;
}
type Listener = (thumb: Thumb) => void;
interface Job {
  key: string;
  path: string;
  el: Element;
}

const cache = new Map<string, Stored>(); // tileKey -> kép
const urls = new Map<string, string>(); // tileKey -> object URL, klipenként egyszer létrehozva
const failed = new Set<string>(); // ebben a munkamenetben nem sikerült; nem próbáljuk újra
const inFlight = new Set<string>();
const listeners = new Map<string, Set<Listener>>();
const jobs = new Map<Element, Job>();
const queue = new Set<Job>(); // előnézetre váró, látható csempék
let active = 0;

const db = new Promise<IDBDatabase>((resolve, reject) => {
  const req = indexedDB.open("clipcat", 1);
  req.onupgradeneeded = () => req.result.createObjectStore("thumbs");
  req.onsuccess = () => resolve(req.result);
  req.onerror = () => reject(req.error);
});

function store(mode: IDBTransactionMode, action: (store: IDBObjectStore) => void) {
  return db.then((db) => new Promise<void>((resolve, reject) => {
    const tx = db.transaction("thumbs", mode);
    action(tx.objectStore("thumbs"));
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
  }));
}

// Indításkor egyszer beolvassa a tárolt előnézeteket; ha az IndexedDB nem elérhető, csak memóriában gyorsítótáraz
export const thumbsReady = store("readonly", (s) => {
  s.openCursor().onsuccess = (e) => {
    const cursor = (e.target as IDBRequest<IDBCursorWithValue | null>).result;
    if (!cursor) return;
    cache.set(cursor.key as string, cursor.value as Stored);
    cursor.continue();
  };
}).catch(() => {});

export function getThumb(key: string): Thumb | undefined {
  const stored = cache.get(key);
  if (!stored) return undefined;
  let url = urls.get(key);
  if (!url) {
    url = URL.createObjectURL(stored.blob);
    urls.set(key, url);
  }
  return { url, duration: stored.duration };
}

// A már nem létező klipek képét és tárolt előnézetét elengedi
export function pruneThumbs(keep: Set<string>) {
  const stale = [...cache.keys()].filter((key) => !keep.has(key));
  if (!stale.length) return;
  for (const key of stale) {
    cache.delete(key);
    const url = urls.get(key);
    if (url) URL.revokeObjectURL(url);
    urls.delete(key);
  }
  store("readwrite", (s) => stale.forEach((key) => s.delete(key))).catch(() => {});
}

// Egy rejtett videóból kivesz egy képkockát; minden ágon elengedi a videót
function capture(path: string): Promise<Stored> {
  return new Promise((resolve, reject) => {
    const video = document.createElement("video");
    video.muted = true;
    video.preload = "auto";
    video.crossOrigin = "anonymous"; // enélkül a canvas "szennyezett" lenne, és a toBlob hibát dobna
    let done = false;
    const finish = (err: unknown, result?: Stored) => {
      if (done) return;
      done = true;
      clearTimeout(timer);
      video.removeAttribute("src");
      video.load();
      if (err) reject(err);
      else resolve(result!);
    };
    const timer = setTimeout(() => finish(new Error("timeout")), THUMB_TIMEOUT_MS);
    const draw = () => {
      if (!video.videoWidth) return finish(new Error("no video track"));
      const canvas = document.createElement("canvas");
      canvas.width = THUMB_WIDTH;
      canvas.height = Math.round((THUMB_WIDTH * video.videoHeight) / video.videoWidth);
      canvas.getContext("2d")!.drawImage(video, 0, 0, canvas.width, canvas.height);
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

async function generate({ key, path }: Job) {
  inFlight.add(key);
  try {
    const stored = await capture(path);
    cache.set(key, stored);
    const thumb = getThumb(key)!;
    listeners.get(key)?.forEach((notify) => notify(thumb));
    store("readwrite", (s) => s.put(stored, key)).catch(() => {});
  } catch (e) {
    failed.add(key);
    console.warn("thumbnail failed", path, e);
  } finally {
    inFlight.delete(key);
    listeners.delete(key);
  }
}

function pump() {
  while (active < THUMB_WORKERS && queue.size) {
    const job = queue.values().next().value!;
    forget(job.el);
    active++;
    generate(job).finally(() => {
      active--;
      pump();
    });
  }
}

function forget(el: Element) {
  const job = jobs.get(el);
  if (job) queue.delete(job);
  jobs.delete(el);
  observer.unobserve(el);
}

// A képernyőről elgörgetett csempe kikerül a sorból, így a gyors görgetés nem halmoz fel munkát
const observer = new IntersectionObserver((entries) => {
  for (const { target, isIntersecting } of entries) {
    const job = jobs.get(target);
    if (!job) continue;
    if (isIntersecting) queue.add(job);
    else queue.delete(job);
  }
  pump();
}, { rootMargin: "300px" });

// Előnézetet kér a csempéhez, amint a képernyő közelébe kerül; a visszaadott függvény lemondja
export function requestThumb(key: string, path: string, el: Element, notify: Listener): () => void {
  if (cache.has(key) || failed.has(key)) return () => {};
  if (!listeners.has(key)) listeners.set(key, new Set());
  listeners.get(key)!.add(notify);
  if (!inFlight.has(key)) {
    jobs.set(el, { key, path, el });
    observer.observe(el);
  }
  return () => {
    listeners.get(key)?.delete(notify);
    forget(el);
  };
}
