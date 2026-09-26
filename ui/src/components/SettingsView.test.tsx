import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { SettingsView } from "./SettingsView";
import { invoke } from "../lib/tauri";
import type { Settings } from "../lib/tauri";
import { i18n, useLocale } from "../lib/i18n";
import hu from "../../locales/hu.json";
import en from "../../locales/en-US.json";

vi.mock("../lib/tauri", () => ({ invoke: vi.fn(), listen: vi.fn().mockResolvedValue(() => {}) }));
afterEach(() => { cleanup(); vi.clearAllMocks(); });

export const settings = {
  language: "hu", outputDir: "C:/Clips", bufferDir: "C:/Buffer", bufferSeconds: 150,
  bufferStorage: "memory", resolution: "1920x1080", fps: 60, bitrateMbps: 5, codec: "h264",
  captureDesktop: true, micMode: "off", micDevice: "default", micPttVk: 192, micPttLabel: "ö",
  hotkeySave: "Alt+F10", hotkeyRecord: "Alt+F9", hotkeyOpenFolder: "Alt+F11", hotkeyGallery: "Alt+KeyZ",
  showNotification: true, notificationSound: true, autostart: true, keepObsRunning: true,
  replayEnabled: false, lastClip: null,
} as Settings;

it("preserves a valid 5 Mbps setting when another preference is saved (audit 18)", async () => {
  i18n.messages = hu;
  vi.mocked(invoke).mockImplementation(async (command) => {
    if (command === "list_mics") return [] as never;
    if (command === "get_settings") return settings as never;
    if (command === "get_locale") return { lang: "hu", messages: hu } as never;
    return false as never;
  });
  render(<SettingsView settings={settings} onSaved={vi.fn()} onStatus={vi.fn()} update={null} />);
  await screen.findByText("5 Mbps");
  fireEvent.click(screen.getAllByRole("checkbox")[0]);
  fireEvent.click(screen.getByRole("button", { name: "Mentés" }));
  await waitFor(() => expect(invoke).toHaveBeenCalledWith("save_settings", {
    settings: expect.objectContaining({ bitrateMbps: 5 }),
  }));
});

it("saves English US and updates all visible settings without restarting", async () => {
  i18n.lang = "hu"; i18n.messages = hu;
  let saved = settings;
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    if (command === "list_mics") return [] as never;
    if (command === "save_settings") { saved = (args as { settings: Settings }).settings; return "" as never; }
    if (command === "get_settings") return saved as never;
    if (command === "get_locale") return { lang: saved.language, messages: saved.language === "hu" ? hu : en } as never;
    return false as never;
  });
  function Page() { useLocale(); return <SettingsView settings={settings} onSaved={vi.fn()} onStatus={vi.fn()} update={null} />; }
  render(<Page />);
  const language = await screen.findByRole("combobox", { name: hu["settings.language.label"] });
  expect([...language.querySelectorAll("option")].map(o => o.textContent)).toEqual(["Magyar", "English US"]);
  fireEvent.change(language, { target: { value: "en-US" } });
  fireEvent.click(screen.getByRole("button", { name: "Mentés" }));
  await screen.findByRole("heading", { name: "Settings" });
  expect(saved.language).toBe("en-US");
  expect(document.documentElement.lang).toBe("en-US");
});

it("places update, license and accessible GitHub button in the bottom Info section", async () => {
  i18n.lang = "en-US"; i18n.messages = en;
  vi.mocked(invoke).mockImplementation(async command => command === "list_mics" ? [] as never : false as never);
  render(<SettingsView settings={{ ...settings, language: "en-US" }} onSaved={vi.fn()} onStatus={vi.fn()}
    update={{ current: "0.4.1", phase: "latest", version: null, notes: null, progress: null, error: null }} />);
  const github = await screen.findByRole("button", { name: en["settings.info.repository"] });
  fireEvent.click(github);
  fireEvent.click(screen.getByRole("button", { name: en["settings.info.license"] }));
  expect(invoke).toHaveBeenCalledWith("open_project_link", { target: "repository" });
  expect(invoke).toHaveBeenCalledWith("open_project_link", { target: "license" });
  expect(screen.getByText("ClipCat v0.4.1")).toBeTruthy();
  expect(screen.getAllByRole("heading").at(-1)?.textContent).toBe("Info");
});
