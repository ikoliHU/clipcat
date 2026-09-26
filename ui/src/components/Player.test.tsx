import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { Player } from "./Player";
import { i18n } from "../lib/i18n";
import en from "../../locales/en-US.json";

vi.mock("../lib/tauri", () => ({ convertFileSrc: (path: string) => path, invoke: vi.fn() }));

const clip = { path: "C:/Clips/Game/clip.mp4", name: "clip.mp4", game: "Game", size: 1024, modified: 1_700_000_000 };

beforeEach(() => {
  i18n.lang = "en-US";
  i18n.messages = en;
  vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue(undefined);
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
});
afterEach(cleanup);

it("requires confirmation and releases the video before requesting deletion", () => {
  const onClose = vi.fn();
  const onDelete = vi.fn(() => ({
    hasSource: video.hasAttribute("src"),
    pauses: vi.mocked(video.pause).mock.calls.length,
    loads: vi.mocked(video.load).mock.calls.length,
    closes: onClose.mock.calls.length,
  }));
  const { container } = render(<Player clip={clip} onClose={onClose} onDelete={onDelete} />);
  const video = container.querySelector("video")!;

  fireEvent.click(screen.getByRole("button", { name: en["player.delete"] }));
  expect(onDelete).not.toHaveBeenCalled();
  expect(onClose).not.toHaveBeenCalled();
  expect(video.getAttribute("src")).toBe(clip.path);

  fireEvent.click(screen.getByRole("button", { name: en["player.deleteConfirm"] }));
  expect(onDelete).toHaveBeenCalledExactlyOnceWith(clip);
  expect(onDelete.mock.results[0].value).toEqual({ hasSource: false, pauses: 1, loads: 1, closes: 1 });
});
