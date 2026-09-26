import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { captureInput, isCapturing } from "./capture";
import { invoke } from "./tauri";

vi.mock("./tauri", () => ({ invoke: vi.fn().mockResolvedValue(undefined) }));

beforeEach(() => { vi.useFakeTimers(); vi.mocked(invoke).mockReset().mockResolvedValue(undefined); });
afterEach(async () => {
  window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
  await vi.runOnlyPendingTimersAsync();
  vi.useRealTimers();
});

it("restores shortcuts when unmounted while suspend IPC is still pending", async () => {
  let suspended!: () => void;
  vi.mocked(invoke).mockImplementationOnce(() => new Promise<void>(resolve => { suspended = resolve; }) as never);
  const controller = new AbortController();
  const capture = captureInput(document.createElement("button"), false, controller.signal);
  controller.abort();
  suspended();
  expect(await capture).toBeNull();
  expect(invoke).toHaveBeenLastCalledWith("resume_hotkeys");
  expect(isCapturing()).toBe(false);
});

it("releases capture state and attempts resume after rejected IPC", async () => {
  vi.mocked(invoke).mockRejectedValueOnce(new Error("IPC rejected"));
  await expect(captureInput(document.createElement("button"))).rejects.toThrow("IPC rejected");
  expect(invoke).toHaveBeenLastCalledWith("resume_hotkeys");
  expect(isCapturing()).toBe(false);
});

it("restores shortcuts when focus is lost without another key press (audit 15)", async () => {
  const capture = captureInput(document.createElement("button"));
  await vi.advanceTimersByTimeAsync(1);
  window.dispatchEvent(new Event("blur"));
  await Promise.resolve();
  expect(invoke).toHaveBeenCalledWith("resume_hotkeys");
  expect(await capture).toBeNull();
  expect(isCapturing()).toBe(false);
});

it("restores shortcuts after the capture deadline (audit 15)", async () => {
  const capture = captureInput(document.createElement("button"));
  await vi.advanceTimersByTimeAsync(30_001);
  expect(invoke).toHaveBeenCalledWith("resume_hotkeys");
  expect(await capture).toBeNull();
});
