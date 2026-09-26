// Thumbnails are still images (JPEG), not live video: tiles do not keep a decoder open.
// Render images from hidden videos, processing at most THUMB_WORKERS at once,
// and only for tiles on or near the screen. Store the results in IndexedDB.
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

const cache = new Map<string, Stored>(); // tileKey -> image
const urls = new Map<string, string>(); // tileKey -> object URL, created once per clip
const failed = new Set<string>(); // Failed in this session; do not retry
const inFlight = new Set<string>();
const listeners = new Map<string, Set<Listener>>();
const jobs = new Map<Element, Job>();
const queue = new Set<Job>(); // Visible tiles waiting for thumbnails
let active = 0;
let validKeys: Set<string> | null = null;
const MAX_CACHE_BYTES = 32 * 1024 * 1024;

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

// Persisted JPEGs load only when their tile becomes visible.
export const thumbsReady = db.then(() => {}).catch(() => {});

async function storedThumb(key: string): Promise<Stored | undefined> {
  try {
    const database = await db;
    return await new Promise((resolve, reject) => {
      const request = database.transaction("thumbs", "readonly").objectStore("thumbs").get(key);
      request.onsuccess = () => resolve(request.result as Stored | undefined);
      request.onerror = () => reject(request.error);
    });
  } catch { return undefined; }
}

function evict(key: string) {
  cache.delete(key);
  const url = urls.get(key);
  if (url) URL.revokeObjectURL(url);
  urls.delete(key);
}

function trimCache() {
  let bytes = [...cache.values()].reduce((total, value) => total + value.blob.size, 0);
  for (const [key, value] of cache) {
    if (bytes <= MAX_CACHE_BYTES) break;
    if (listeners.get(key)?.size) continue;
    bytes -= value.blob.size;
    evict(key);
  }
}

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

// Release images and stored thumbnails for clips that no longer exist
export function pruneThumbs(keep: Set<string>) {
  validKeys = keep;
  for (const key of cache.keys()) if (!keep.has(key)) evict(key);
  for (const key of failed) if (!keep.has(key)) failed.delete(key);
  for (const key of listeners.keys()) if (!keep.has(key)) listeners.delete(key);
  for (const [el, job] of jobs) if (!keep.has(job.key)) forget(el);
  store("readwrite", (store) => {
    store.openKeyCursor().onsuccess = (event) => {
      const cursor = (event.target as IDBRequest<IDBCursor | null>).result;
      if (!cursor) return;
      if (!keep.has(String(cursor.key))) store.delete(cursor.key);
      cursor.continue();
    };
  }).catch(() => {});
}

// Extract a frame from a hidden video; release the video on every code path
function capture(path: string): Promise<Stored> {
  return new Promise((resolve, reject) => {
    const video = document.createElement("video");
    video.muted = true;
    video.preload = "auto";
    video.crossOrigin = "anonymous"; // Otherwise the canvas would be tainted and toBlob would throw
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
      canvas.height = Math.min(1080, Math.max(1, Math.round((THUMB_WIDTH * video.videoHeight) / video.videoWidth)));
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
    const stored = await storedThumb(key) ?? await capture(path);
    if (validKeys && !validKeys.has(key)) return;
    cache.set(key, stored);
    trimCache();
    const thumb = getThumb(key)!;
    listeners.get(key)?.forEach((notify) => notify(thumb));
    store("readwrite", (s) => s.put(stored, key)).catch(() => {});
  } catch (e) {
    if (!validKeys || validKeys.has(key)) failed.add(key);
    console.warn("thumbnail failed", path, e);
  } finally {
    inFlight.delete(key);
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

// Remove offscreen tiles from the queue so fast scrolling cannot accumulate work
const observer = new IntersectionObserver((entries) => {
  for (const { target, isIntersecting } of entries) {
    const job = jobs.get(target);
    if (!job) continue;
    if (isIntersecting) queue.add(job);
    else queue.delete(job);
  }
  pump();
}, { rootMargin: "300px" });

// Request a thumbnail when the tile approaches the screen; the returned function cancels the request
export function requestThumb(key: string, path: string, el: Element, notify: Listener): () => void {
  if (failed.has(key)) return () => {};
  if (!listeners.has(key)) listeners.set(key, new Set());
  listeners.get(key)!.add(notify);
  const cached = getThumb(key);
  if (cached) notify(cached);
  else if (!inFlight.has(key)) {
    jobs.set(el, { key, path, el });
    observer.observe(el);
  }
  return () => {
    listeners.get(key)?.delete(notify);
    if (!listeners.get(key)?.size) listeners.delete(key);
    forget(el);
    trimCache();
  };
}
