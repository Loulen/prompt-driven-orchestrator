import type { UpdateStatus } from "../types";

/**
 * Version-check helpers (#697). The daemon does the check and caches it; the frontend
 * only READS `GET /update` — at load, after « Check now », on WebSocket reconnect — and
 * shares the result between the status bar and the Settings section through a window
 * event (same bus idiom as `pdo:settings-changed`), so the badge reflects a toggle or
 * a manual check without a second fetch.
 */
export const UPDATE_STATUS_CHANGED = "pdo:update-status-changed";

export function announceUpdateStatus(status: UpdateStatus) {
  window.dispatchEvent(new CustomEvent<UpdateStatus>(UPDATE_STATUS_CHANGED, { detail: status }));
}

/** A newer release is known — and the check is on. Off or unknown ⇒ no badge. */
export function newerAvailable(s: UpdateStatus | null | undefined): boolean {
  return !!s && s.check_enabled && s.latest_version != null && s.newer_available;
}

/** The daemon runs the latest published version. Never claimed when unverified. */
export function upToDate(s: UpdateStatus | null | undefined): boolean {
  return (
    !!s && s.check_enabled && s.latest_version != null && s.latest_version === s.installed_version
  );
}

export function relativeTime(iso: string, now: number = Date.now()): string {
  const diff = now - new Date(iso).getTime();
  const m = Math.round(diff / 60_000);
  if (m < 1) return "just now";
  if (m < 60) return `${m} min ago`;
  const h = Math.round(m / 60);
  if (h < 48) return `${h} h ago`;
  return `${Math.round(h / 24)} d ago`;
}

export function absoluteTime(iso: string): string {
  return new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(
    new Date(iso),
  );
}

export const INSTALL_METHOD_LABEL: Record<UpdateStatus["install_method"], string> = {
  homebrew: "Homebrew",
  script: "Install script (cargo-dist)",
  unknown: "Unknown",
};

export const SUPERVISION_LABEL: Record<UpdateStatus["supervision"], string> = {
  systemd: "systemd service",
  launchd: "launchd agent",
  none: "manual (no service)",
};
