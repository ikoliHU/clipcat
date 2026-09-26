import { expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { Gallery } from "./Gallery";
import { i18n } from "../lib/i18n";
import en from "../../locales/en-US.json";

vi.mock("../lib/thumbs", () => ({ getThumb: () => undefined, requestThumb: () => () => {}, tileKey: (c: { path: string }) => c.path, previewTime: () => 0 }));
it("allows opening the 501st clip without rendering all tiles at once (audit 16)", () => {
  i18n.lang = "en-US"; i18n.messages = en;
  const clips = Array.from({ length: 501 }, (_, i) => ({ path: `C:/Clips/Game/clip-${i}.mp4`, name: `clip-${i}.mp4`, game: "Game", size: 1024, modified: 1_700_000_000 + i }));
  const open = vi.fn();
  render(<Gallery clips={clips} filter="" onFilter={vi.fn()} onRefresh={vi.fn()} onOpen={open} hotkeySave="Alt+F10" />);
  expect(screen.queryByTitle("clip-500.mp4")).toBeNull();
  for (let page = 1; page < 9; page++) fireEvent.click(screen.getByRole("button", { name: en["gallery.next"] }));
  const last = screen.getByTitle("clip-500.mp4");
  fireEvent.click(last);
  expect(open).toHaveBeenCalledWith(clips[500]);
  expect(screen.queryByTitle("clip-0.mp4")).toBeNull();
  cleanup();
});
