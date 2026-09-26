import "fake-indexeddb/auto";
import { expect, it, vi } from "vitest";
import { waitFor } from "@testing-library/react";
vi.mock("./tauri", () => ({ convertFileSrc: (path: string) => path }));

it("loads persisted thumbnails only when visible and revokes removed clips (audit 18)", async () => {
  let intersect!: IntersectionObserverCallback;
  vi.stubGlobal("IntersectionObserver", class {
    constructor(callback: IntersectionObserverCallback) { intersect = callback; }
    observe() {} unobserve() {} disconnect() {}
  });
  const createUrl = vi.fn(() => "blob:thumbnail");
  const revokeUrl = vi.fn();
  URL.createObjectURL = createUrl; URL.revokeObjectURL = revokeUrl;
  const database = await new Promise<IDBDatabase>((resolve, reject) => {
    const req = indexedDB.open("clipcat", 1);
    req.onupgradeneeded = () => req.result.createObjectStore("thumbs");
    req.onsuccess = () => resolve(req.result); req.onerror = () => reject(req.error);
  });
  await new Promise<void>((resolve) => {
    const tx = database.transaction("thumbs", "readwrite");
    tx.objectStore("thumbs").put({ blob: new Blob(["jpeg"], { type: "image/jpeg" }), duration: 5 }, "clip");
    tx.oncomplete = () => resolve();
  });
  const { thumbsReady, getThumb, requestThumb, pruneThumbs } = await import("./thumbs");
  await thumbsReady;
  expect(getThumb("clip")).toBeUndefined();
  expect(createUrl).not.toHaveBeenCalled();
  const el = document.createElement("div"); const listener = vi.fn();
  const cancel = requestThumb("clip", "clip.mp4", el, listener);
  expect(listener).not.toHaveBeenCalled();
  const rect = new DOMRect();
  intersect([{ target: el, isIntersecting: true, boundingClientRect: rect, intersectionRect: rect, rootBounds: null, intersectionRatio: 1, time: 0 }], {} as IntersectionObserver);
  await waitFor(() => expect(listener).toHaveBeenCalledWith({ url: "blob:thumbnail", duration: 5 }));
  cancel(); pruneThumbs(new Set());
  expect(getThumb("clip")).toBeUndefined();
  expect(revokeUrl).toHaveBeenCalledWith("blob:thumbnail");
  database.close(); vi.unstubAllGlobals();
});
