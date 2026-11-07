import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { act, renderHook } from "@testing-library/react";
import {
  disposeWaitingAlertSettings,
  initWaitingAlertSettings,
  useWaitingAlertSettings,
} from "./useWaitingAlertSettings";
import { WAITING_ALERT_PREFS_STORAGE_KEY } from "../lib/waitingAlertPrefs";
import { installFakeNotification, uninstallFakeNotification } from "../test/fakeNotification";

function stored(): unknown {
  const raw = localStorage.getItem(WAITING_ALERT_PREFS_STORAGE_KEY);
  return raw ? JSON.parse(raw) : null;
}

function otherTabWrites(value: string | null) {
  if (value === null) localStorage.removeItem(WAITING_ALERT_PREFS_STORAGE_KEY);
  else localStorage.setItem(WAITING_ALERT_PREFS_STORAGE_KEY, value);
  window.dispatchEvent(new StorageEvent("storage", { key: WAITING_ALERT_PREFS_STORAGE_KEY, newValue: value }));
}

beforeEach(() => {
  localStorage.clear();
});

afterEach(() => {
  disposeWaitingAlertSettings();
  uninstallFakeNotification();
  Reflect.deleteProperty(window, "isSecureContext");
  localStorage.clear();
});

describe("useWaitingAlertSettings (#1011)", () => {
  it("starts off, permission default, nothing stored", () => {
    installFakeNotification("default");
    act(() => void initWaitingAlertSettings());
    const { result } = renderHook(() => useWaitingAlertSettings());
    expect(result.current.prefs).toEqual({ notification: false, sound: false });
    expect(result.current.permission).toBe("default");
    expect(stored()).toBeNull();
  });

  it("turning it on asks the browser; granted keeps it on and stores it", async () => {
    const fake = installFakeNotification("default");
    act(() => void initWaitingAlertSettings());
    const { result } = renderHook(() => useWaitingAlertSettings());
    await act(() => result.current.setNotificationEnabled(true));
    expect(fake.requests).toBe(1);
    expect(result.current.permission).toBe("granted");
    expect(result.current.prefs.notification).toBe(true);
    expect(stored()).toEqual({ notification: true, sound: false });
  });

  it("is `requesting` while the browser's prompt is open", async () => {
    const fake = installFakeNotification("default");
    fake.hold = true;
    act(() => void initWaitingAlertSettings());
    const { result } = renderHook(() => useWaitingAlertSettings());
    let done!: Promise<void>;
    act(() => {
      done = result.current.setNotificationEnabled(true);
    });
    expect(result.current.requesting).toBe(true);
    expect(result.current.prefs.notification).toBe(false);
    await act(async () => {
      fake.release();
      await done;
    });
    expect(result.current.requesting).toBe(false);
    expect(result.current.prefs.notification).toBe(true);
  });

  it("a refusal leaves it off, permission denied", async () => {
    const fake = installFakeNotification("default");
    fake.answer = "denied";
    act(() => void initWaitingAlertSettings());
    const { result } = renderHook(() => useWaitingAlertSettings());
    await act(() => result.current.setNotificationEnabled(true));
    expect(result.current.permission).toBe("denied");
    expect(result.current.prefs.notification).toBe(false);
  });

  it("a dismissed prompt (still default) leaves it off", async () => {
    const fake = installFakeNotification("default");
    fake.answer = "default";
    act(() => void initWaitingAlertSettings());
    const { result } = renderHook(() => useWaitingAlertSettings());
    await act(() => result.current.setNotificationEnabled(true));
    expect(result.current.prefs.notification).toBe(false);
  });

  it("already granted: turns on without asking again", async () => {
    const fake = installFakeNotification("granted");
    act(() => void initWaitingAlertSettings());
    const { result } = renderHook(() => useWaitingAlertSettings());
    await act(() => result.current.setNotificationEnabled(true));
    expect(fake.requests).toBe(0);
    expect(result.current.prefs.notification).toBe(true);
  });

  it("restores the stored switch at boot", () => {
    installFakeNotification("granted");
    localStorage.setItem(WAITING_ALERT_PREFS_STORAGE_KEY, JSON.stringify({ notification: true, sound: false }));
    act(() => void initWaitingAlertSettings());
    const { result } = renderHook(() => useWaitingAlertSettings());
    expect(result.current.prefs.notification).toBe(true);
  });

  it("falls back to off when the permission was revoked since", () => {
    const fake = installFakeNotification("granted");
    localStorage.setItem(WAITING_ALERT_PREFS_STORAGE_KEY, JSON.stringify({ notification: true, sound: false }));
    act(() => void initWaitingAlertSettings());
    const { result } = renderHook(() => useWaitingAlertSettings());
    expect(result.current.prefs.notification).toBe(true);
    fake.setPermission("denied");
    act(() => {
      window.dispatchEvent(new Event("focus"));
    });
    expect(result.current.permission).toBe("denied");
    expect(result.current.prefs.notification).toBe(false);
    expect(stored()).toEqual({ notification: false, sound: false });
  });

  it("unlocks on its own when the site is allowed again (re-read on focus)", () => {
    const fake = installFakeNotification("denied");
    act(() => void initWaitingAlertSettings());
    const { result } = renderHook(() => useWaitingAlertSettings());
    expect(result.current.permission).toBe("denied");
    fake.setPermission("default");
    act(() => {
      window.dispatchEvent(new Event("focus"));
    });
    expect(result.current.permission).toBe("default");
  });

  it("follows a switch made in another tab", () => {
    installFakeNotification("granted");
    act(() => void initWaitingAlertSettings());
    const { result } = renderHook(() => useWaitingAlertSettings());
    act(() => otherTabWrites(JSON.stringify({ notification: true, sound: false })));
    expect(result.current.prefs.notification).toBe(true);
    act(() => otherTabWrites(null));
    expect(result.current.prefs.notification).toBe(false);
  });

  it("reads `unavailable` without the API and `insecure` outside a secure context", () => {
    act(() => void initWaitingAlertSettings());
    expect(renderHook(() => useWaitingAlertSettings()).result.current.permission).toBe("unavailable");
    installFakeNotification("granted");
    Object.defineProperty(window, "isSecureContext", { configurable: true, value: false });
    act(() => void initWaitingAlertSettings());
    expect(renderHook(() => useWaitingAlertSettings()).result.current.permission).toBe("insecure");
  });

  it("an unreadable stored value reads as the defaults", () => {
    localStorage.setItem(WAITING_ALERT_PREFS_STORAGE_KEY, "{not json");
    act(() => void initWaitingAlertSettings());
    expect(renderHook(() => useWaitingAlertSettings()).result.current.prefs).toEqual({
      notification: false,
      sound: false,
    });
  });
});
