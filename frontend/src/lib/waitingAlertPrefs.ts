/**
 * The waiting alert's switches (#1011, ADR-0082 §2): a preference of THIS
 * browser, in the `pdo.*` localStorage namespace like the theme and the
 * colour-blind palette — never `PUT /settings`. The notification permission is
 * granted per browser and a sound only makes sense on the machine playing it:
 * a shared instance must not ring for everyone.
 *
 * Both off by default. Read and write are guarded: private mode, a full store or
 * an unreadable value fall back to the defaults, without an error.
 */
import { DEFAULT_WAITING_ALERT_PREFS, type WaitingAlertPrefs } from "./waitingAlert";

export const WAITING_ALERT_PREFS_STORAGE_KEY = "pdo.ui.waitingAlerts";

export function loadWaitingAlertPrefs(): WaitingAlertPrefs {
  try {
    const raw = localStorage.getItem(WAITING_ALERT_PREFS_STORAGE_KEY);
    if (!raw) return { ...DEFAULT_WAITING_ALERT_PREFS };
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object") return { ...DEFAULT_WAITING_ALERT_PREFS };
    const record = parsed as Record<string, unknown>;
    return {
      notification: record.notification === true,
      sound: record.sound === true,
    };
  } catch {
    return { ...DEFAULT_WAITING_ALERT_PREFS };
  }
}

export function saveWaitingAlertPrefs(prefs: WaitingAlertPrefs): void {
  try {
    localStorage.setItem(
      WAITING_ALERT_PREFS_STORAGE_KEY,
      JSON.stringify({ notification: prefs.notification, sound: prefs.sound }),
    );
  } catch {
    // quota / disabled / private mode → in-memory only for this session
  }
}
