import type { UpdateStatus } from "./update";
import { brandName, CORE_NAME } from "../../shared/ui/brand/brand";

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

/** The name of the vanilla binary's release channel: the core's. */
export const VANILLA_CHANNEL_NAME = CORE_NAME;

/**
 * The binary is a distribution (#24): a build of PDO with its own extensions, its own
 * version and its own release channel, named otherwise than `PDO`.
 */
export function isDistribution(s: UpdateStatus): boolean {
  return s.channel_name !== VANILLA_CHANNEL_NAME;
}

/** The product name a sentence uses: the distribution's own, or the Marque's for the vanilla channel. */
export function channelLabel(s: UpdateStatus): string {
  return isDistribution(s) ? s.channel_name : brandName();
}

/** The product and its installed version: `PDO 2.76.0`, `Acogni Driver 1.4.0`. */
export function installedProductLabel(s: UpdateStatus): string {
  return `${channelLabel(s)} ${s.installed_version}`;
}

/** The core a distribution is built on, `core PDO 2.76.0`; `null` for the vanilla binary. */
export function coreLabel(s: UpdateStatus): string | null {
  return isDistribution(s) ? `core ${CORE_NAME} ${s.core_version}` : null;
}

/**
 * How the installed version reads in a short slot: `v2.0.0` for the vanilla binary,
 * `acme-pdo 0.3.0 (core PDO 2.0.0)` for a distribution, whose version alone says
 * nothing about the core.
 */
export function installedVersionLabel(s: UpdateStatus): string {
  const core = coreLabel(s);
  return core ? `${installedProductLabel(s)} (${core})` : `v${s.installed_version}`;
}

/**
 * Offer the Update button only when it would do something: a newer release is known,
 * and the daemon would accept the apply (known method or declared command, no attempt
 * running). Never a « re-run » of the upgrade command on a guess.
 */
export function canOfferUpdate(s: UpdateStatus | null | undefined): boolean {
  return !!s && s.can_apply && newerAvailable(s);
}

/** Why the Update button is not offered, for its tooltip. */
export function updateNotOfferedReason(s: UpdateStatus): string | null {
  if (s.apply_blocked_reason) return s.apply_blocked_reason;
  if (!newerAvailable(s)) return "No newer version is known: nothing to update.";
  return null;
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
  declared: "Declared by the distribution",
};

export const SUPERVISION_LABEL: Record<UpdateStatus["supervision"], string> = {
  systemd: "systemd service",
  launchd: "launchd agent",
  none: "manual (no service)",
};
