import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import NotificationsSection from "./NotificationsSection";
import {
  disposeWaitingAlertSettings,
  initWaitingAlertSettings,
} from "../hooks/useWaitingAlertSettings";
import { WAITING_ALERT_PREFS_STORAGE_KEY } from "../lib/waitingAlertPrefs";
import { installFakeNotification, uninstallFakeNotification } from "../test/fakeNotification";
import { installFakeAudioContext, uninstallFakeAudioContext } from "../test/fakeAudioContext";
import { resetSoundAdapter } from "../lib/soundAdapter";

function boot() {
  act(() => void initWaitingAlertSettings());
  render(<NotificationsSection />);
}

const toggle = () => screen.getByTestId("setting-browser-notification");
const status = () => screen.getByTestId("setting-browser-notification-status");
const testButton = () => screen.getByTestId("setting-waiting-alert-test");
const soundToggle = () => screen.getByTestId("setting-sound");

beforeEach(() => localStorage.clear());

afterEach(() => {
  disposeWaitingAlertSettings();
  uninstallFakeNotification();
  uninstallFakeAudioContext();
  resetSoundAdapter();
  Reflect.deleteProperty(window, "isSecureContext");
  localStorage.clear();
});

describe("Settings › Notifications (#1011)", () => {
  it("default: off, says turning it on asks for permission, test disabled with a reason", () => {
    installFakeNotification("default");
    boot();
    expect(toggle()).toHaveAttribute("role", "switch");
    expect(toggle()).toHaveAttribute("aria-checked", "false");
    expect(toggle()).toBeEnabled();
    expect(status()).toHaveAttribute("data-state", "default");
    expect(status()).toHaveTextContent("Turning it on asks your browser for permission.");
    expect(testButton()).toBeDisabled();
    expect(testButton()).toHaveAttribute("title");
    expect(screen.getByText("Fires now, even with this tab in view.")).toBeInTheDocument();
  });

  it("has the help on the info icon and for screen readers", () => {
    installFakeNotification("default");
    boot();
    expect(toggle()).toHaveAttribute("aria-describedby", "setting-browser-notification-help");
    expect(document.getElementById("setting-browser-notification-help")).toHaveTextContent(
      /no PDO tab is in view. Clicking it opens the Run with the node selected/,
    );
  });

  it("asks while the prompt is open, then stays on once granted", async () => {
    const fake = installFakeNotification("default");
    fake.hold = true;
    boot();
    fireEvent.click(toggle());
    await waitFor(() => expect(status()).toHaveAttribute("data-state", "requesting"));
    expect(status()).toHaveTextContent("Waiting for your browser's answer — look near the address bar.");
    expect(toggle()).toBeDisabled();
    await act(async () => fake.release());
    await waitFor(() => expect(toggle()).toHaveAttribute("aria-checked", "true"));
    expect(status()).toHaveAttribute("data-state", "granted");
    expect(status()).toHaveTextContent("Allowed by this browser.");
    expect(JSON.parse(localStorage.getItem(WAITING_ALERT_PREFS_STORAGE_KEY)!)).toMatchObject({
      notification: true,
    });
    expect(testButton()).toBeEnabled();
  });

  it("refused: back off, greyed and locked, with how to unblock", async () => {
    const fake = installFakeNotification("default");
    fake.answer = "denied";
    boot();
    fireEvent.click(toggle());
    await waitFor(() => expect(status()).toHaveAttribute("data-state", "denied"));
    expect(toggle()).toHaveAttribute("aria-checked", "false");
    expect(toggle()).toBeDisabled();
    expect(status()).toHaveTextContent(/Blocked by this browser for this site\..*the switch unlocks on its own\./);
  });

  it("not a secure context: greyed, names the origin and the way out", () => {
    installFakeNotification("default");
    Object.defineProperty(window, "isSecureContext", { configurable: true, value: false });
    boot();
    expect(toggle()).toBeDisabled();
    expect(status()).toHaveAttribute("data-state", "insecure");
    expect(status()).toHaveTextContent(
      `Browsers only allow notifications on https or localhost. You opened PDO at ${window.location.origin}`,
    );
    expect(status()).toHaveTextContent("Sound still works.");
    // …and it does: the Sound switch stays usable.
    expect(soundToggle()).toBeEnabled();
  });

  it("no Notification API: greyed with its reason", () => {
    boot();
    expect(toggle()).toBeDisabled();
    expect(status()).toHaveAttribute("data-state", "unavailable");
    expect(status()).toHaveTextContent("This browser has no notification support. Sound still works.");
  });

  it("Send a test fires a notification even with the tab in view; its click opens no Run", async () => {
    const fake = installFakeNotification("granted");
    localStorage.setItem(WAITING_ALERT_PREFS_STORAGE_KEY, JSON.stringify({ notification: true, sound: false }));
    boot();
    fireEvent.click(testButton());
    expect(fake.shown).toHaveLength(1);
    expect(fake.shown[0].title).toBe("PDO — test notification");
    const focus = vi.spyOn(window, "focus").mockImplementation(() => {});
    act(() => fake.shown[0].onclick?.());
    expect(focus).toHaveBeenCalled();
    expect(fake.shown[0].closed).toBe(true);
    focus.mockRestore();
  });

  it("turning it off stores it at once", async () => {
    installFakeNotification("granted");
    localStorage.setItem(WAITING_ALERT_PREFS_STORAGE_KEY, JSON.stringify({ notification: true, sound: false }));
    boot();
    expect(toggle()).toHaveAttribute("aria-checked", "true");
    fireEvent.click(toggle());
    await waitFor(() => expect(toggle()).toHaveAttribute("aria-checked", "false"));
    expect(JSON.parse(localStorage.getItem(WAITING_ALERT_PREFS_STORAGE_KEY)!)).toMatchObject({
      notification: false,
    });
  });

  describe("Sound (#1012)", () => {
    it("off by default, with its help; independent of the notification permission", () => {
      installFakeNotification("denied");
      boot();
      expect(soundToggle()).toHaveAttribute("role", "switch");
      expect(soundToggle()).toHaveAttribute("aria-checked", "false");
      expect(soundToggle()).toBeEnabled();
      expect(screen.getByTestId("setting-sound-status")).toHaveTextContent(
        "A short chime, at most once every 10 s. Needs no permission.",
      );
      expect(document.getElementById("setting-sound-help")).toHaveTextContent(
        "A short chime under the same conditions. At most one every 10 seconds, however many nodes start waiting.",
      );
    });

    it("turning it on stores it at once, asks for nothing, and enables Send a test", () => {
      const fake = installFakeNotification("default");
      boot();
      fireEvent.click(soundToggle());
      expect(soundToggle()).toHaveAttribute("aria-checked", "true");
      expect(fake.requests).toBe(0);
      expect(JSON.parse(localStorage.getItem(WAITING_ALERT_PREFS_STORAGE_KEY)!)).toEqual({
        notification: false,
        sound: true,
      });
      expect(testButton()).toBeEnabled();
      expect(testButton()).not.toHaveAttribute("title");
    });

    it("restored at load, and followed when another tab flips it", () => {
      localStorage.setItem(WAITING_ALERT_PREFS_STORAGE_KEY, JSON.stringify({ notification: false, sound: true }));
      boot();
      expect(soundToggle()).toHaveAttribute("aria-checked", "true");
      act(() => {
        localStorage.setItem(WAITING_ALERT_PREFS_STORAGE_KEY, JSON.stringify({ notification: false, sound: false }));
        window.dispatchEvent(new StorageEvent("storage", { key: WAITING_ALERT_PREFS_STORAGE_KEY }));
      });
      expect(soundToggle()).toHaveAttribute("aria-checked", "false");
    });

    it("Send a test with the sound on plays the Celesta (and only the sound when the notification is off)", () => {
      const fake = installFakeNotification("granted");
      const audio = installFakeAudioContext();
      localStorage.setItem(WAITING_ALERT_PREFS_STORAGE_KEY, JSON.stringify({ notification: false, sound: true }));
      boot();
      fireEvent.click(testButton());
      expect(audio.chimes).toHaveLength(1);
      expect(audio.chimes[0].frequencies).toEqual([1046.5, 783.99]);
      expect(fake.shown).toHaveLength(0);
    });

    it("Send a test with both on fires both, every click (a test is not throttled)", () => {
      const fake = installFakeNotification("granted");
      const audio = installFakeAudioContext();
      localStorage.setItem(WAITING_ALERT_PREFS_STORAGE_KEY, JSON.stringify({ notification: true, sound: true }));
      boot();
      fireEvent.click(testButton());
      fireEvent.click(testButton());
      expect(fake.shown).toHaveLength(2);
      expect(audio.chimes).toHaveLength(2);
    });
  });
});
