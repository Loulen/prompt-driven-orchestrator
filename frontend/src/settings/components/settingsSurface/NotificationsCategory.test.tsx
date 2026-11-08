import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  disposeWaitingAlertSettings,
  getWaitingAlertSettings,
  initWaitingAlertSettings,
  WAITING_ALERT_PREFS_STORAGE_KEY,
} from "../../../run/store/waitingAlertSettings";
import type { Project } from "../../../workspace/model/project";
import { findCategory, SETTINGS_CATEGORIES } from "../../model/settingsCategories";
import NotificationsCategory, { SOUND_NOTE } from "./NotificationsCategory";

const playChime = vi.fn(() => true);
vi.mock("../../../run/store/soundAdapter", () => ({
  playChime: (...args: unknown[]) => playChime(...(args as [])),
  canPlaySoundNow: () => true,
}));

let shown: string[];
let answer: NotificationPermission;

class FakeNotification {
  static permission: NotificationPermission = "default";
  static requestPermission = vi.fn(async () => {
    FakeNotification.permission = answer;
    return answer;
  });
  onclick: (() => void) | null = null;
  constructor(title: string) {
    shown.push(title);
  }
  close() {}
}

const projects = [
  { id: "p1", name: "PDO" },
  { id: "p2", name: "client-x" },
] as Project[];

function renderCategory() {
  return render(<NotificationsCategory sections={findCategory("notifications").sections} projects={projects} />);
}

const row = (id: string) => screen.getByTestId(`setting-row-${id}`);
const switchOf = (id: string) => within(row(id)).getByRole("switch");
const stored = () => JSON.parse(localStorage.getItem(WAITING_ALERT_PREFS_STORAGE_KEY) ?? "{}");

beforeEach(() => {
  shown = [];
  answer = "granted";
  FakeNotification.permission = "default";
  vi.stubGlobal("Notification", FakeNotification);
  initWaitingAlertSettings();
});

afterEach(() => {
  disposeWaitingAlertSettings();
  localStorage.clear();
  vi.unstubAllGlobals();
  playChime.mockClear();
});

describe("Settings › Notifications", () => {
  it("is a category of its own, after Appearance, one device-local section", () => {
    expect(SETTINGS_CATEGORIES.map((c) => c.id).slice(0, 3)).toEqual(["general", "appearance", "notifications"]);
    expect(findCategory("notifications").sections).toEqual([{ id: "waiting-alerts", label: "Waiting alerts", deviceLocal: true }]);
    renderCategory();
    expect(screen.getByTestId("settings-section-waiting-alerts-scope")).toHaveAttribute("data-scope", "device");
    expect(screen.getByTestId("waiting-alerts-description")).toHaveTextContent(
      "Alert this browser when a node starts waiting for you and no PDO tab is in view. Saved on this device only.",
    );
  });

  it("nothing on by default; each Test is greyed while its switch is off", () => {
    renderCategory();
    expect(switchOf("browser-notification")).toHaveAttribute("aria-checked", "false");
    expect(switchOf("sound")).toHaveAttribute("aria-checked", "false");
    expect(screen.getByTestId("setting-browser-notification-test")).toHaveAttribute("aria-disabled", "true");
    expect(screen.getByTestId("setting-sound-test")).toHaveAttribute("aria-disabled", "true");
    expect(screen.getByTestId("setting-browser-notification-status")).toHaveTextContent(
      "Turning it on asks your browser for permission.",
    );
    expect(screen.getByTestId("setting-sound-status")).toHaveTextContent(SOUND_NOTE);
    expect(within(row("alert-scope")).getByRole("radio", { name: "Runs I launched" })).toHaveAttribute("aria-checked", "true");
  });

  it("turning the notification on asks the browser; granted, it stays on and its Test fires", async () => {
    renderCategory();
    fireEvent.click(switchOf("browser-notification"));
    await waitFor(() => expect(switchOf("browser-notification")).toHaveAttribute("aria-checked", "true"));
    expect(FakeNotification.requestPermission).toHaveBeenCalled();
    expect(screen.getByTestId("setting-browser-notification-status")).toHaveTextContent("Allowed by this browser.");
    expect(stored()).toMatchObject({ notification: true });
    fireEvent.click(screen.getByTestId("setting-browser-notification-test"));
    expect(shown).toEqual(["PDO · test notification"]);
  });

  it("refused: the switch falls back off, greyed, and says the sound still works", async () => {
    answer = "denied";
    renderCategory();
    fireEvent.click(switchOf("browser-notification"));
    await waitFor(() => expect(switchOf("browser-notification")).toBeDisabled());
    expect(switchOf("browser-notification")).toHaveAttribute("aria-checked", "false");
    const status = screen.getByTestId("setting-browser-notification-status");
    expect(status.querySelector("[data-state]")).toHaveAttribute("data-state", "denied");
    expect(status).toHaveTextContent("Sound still works.");
  });

  it("not a secure context: unavailable, names the origin; the sound still turns on", () => {
    vi.stubGlobal("isSecureContext", false);
    initWaitingAlertSettings();
    renderCategory();
    expect(switchOf("browser-notification")).toBeDisabled();
    expect(screen.getByTestId("setting-browser-notification-status")).toHaveTextContent(
      `Browsers only allow notifications on https or localhost. You opened PDO at ${window.location.origin}. Sound still works.`,
    );
    fireEvent.click(switchOf("sound"));
    expect(switchOf("sound")).toHaveAttribute("aria-checked", "true");
    fireEvent.click(screen.getByTestId("setting-sound-test"));
    expect(playChime).toHaveBeenCalledWith("rising");
    expect(stored()).toMatchObject({ sound: true, notification: false });
  });

  it("Projects shows the Projects you can read, checked ones saved at once", () => {
    renderCategory();
    expect(screen.queryByTestId("setting-row-alert-projects")).toBeNull();
    fireEvent.click(within(row("alert-scope")).getByRole("radio", { name: "Projects" }));
    const boxes = within(screen.getByTestId("setting-alert-projects"));
    fireEvent.click(boxes.getByRole("checkbox", { name: "client-x" }));
    expect(stored()).toMatchObject({ scope: "projects", projects: ["p2"] });
    expect(boxes.getByRole("checkbox", { name: "PDO" })).not.toBeChecked();
    fireEvent.click(within(row("alert-scope")).getByRole("radio", { name: "All I can see" }));
    expect(getWaitingAlertSettings().prefs).toMatchObject({ scope: "all", projects: ["p2"] });
  });

  it("follows a switch flipped in another tab of the browser", () => {
    renderCategory();
    act(() => {
      localStorage.setItem(WAITING_ALERT_PREFS_STORAGE_KEY, JSON.stringify({ sound: true }));
      window.dispatchEvent(new StorageEvent("storage", { key: WAITING_ALERT_PREFS_STORAGE_KEY }));
    });
    expect(switchOf("sound")).toHaveAttribute("aria-checked", "true");
  });
});
