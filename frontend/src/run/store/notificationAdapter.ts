/**
 * The browser side of the Alerte d'attente's notification: read the
 * permission, ask for it, show a notification. The decision lives in
 * `run/model/waitingAlert.ts`; this module only talks to the Notification API,
 * and guards every call: a browser without the API, or a page that is not a
 * secure context, reads as a permission state, never as an error.
 */
import type { AlertPermission, NotificationSpec } from "../model/waitingAlert";

function notificationApi(): typeof Notification | null {
  if (typeof window === "undefined") return null;
  const api = (window as unknown as { Notification?: typeof Notification }).Notification;
  return typeof api === "function" ? api : null;
}

/**
 * Where this browser stands. Insecure first: on http through a LAN IP the
 * browser refuses notifications whatever the permission says, and what to tell
 * the user is the origin, not a permission.
 */
export function readNotificationPermission(): AlertPermission {
  if (typeof window === "undefined") return "unavailable";
  if (window.isSecureContext === false) return "insecure";
  const api = notificationApi();
  if (!api) return "unavailable";
  const permission = api.permission;
  return permission === "granted" || permission === "denied" ? permission : "default";
}

/** Ask the browser. Resolves to the new state. */
export async function requestNotificationPermission(): Promise<AlertPermission> {
  const api = notificationApi();
  if (!api || window.isSecureContext === false) return readNotificationPermission();
  try {
    await api.requestPermission();
  } catch {
    // The old callback-only signature, or a thrown refusal: read the outcome below.
  }
  return readNotificationPermission();
}

/**
 * Show one notification. Its click brings the tab back to the front, then runs
 * `onClick`. `false` when the browser would not show it.
 */
export function showNotification(spec: Pick<NotificationSpec, "title" | "body" | "tag">, onClick?: () => void): boolean {
  const api = notificationApi();
  if (!api || readNotificationPermission() !== "granted") return false;
  try {
    const notification = new api(spec.title, { body: spec.body, tag: spec.tag });
    notification.onclick = (event?: Event) => {
      event?.preventDefault?.();
      try {
        window.focus();
      } catch {
        // focus refused: the click still opens the Run
      }
      notification.close?.();
      onClick?.();
    };
    return true;
  } catch {
    // Chrome on Android refuses the constructor (service worker only).
    return false;
  }
}
