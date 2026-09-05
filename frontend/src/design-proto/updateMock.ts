// DESIGN PROTOTYPE (#697) — throwaway. Mock of the future `GET /update` payload, driven
// by `?proto=<scenario>&badge=<a|b|c>` so screenshots can cover every data state.
export type InstallMethod = "homebrew" | "script" | "unknown";
export type Supervision = "systemd" | "launchd" | "none";

export interface UpdateStatus {
  installed_version: string;
  latest_version: string | null;
  checked_at: string | null;
  source: string;
  check_enabled: boolean;
  install_method: InstallMethod;
  manual_command: string;
  supervision: Supervision;
  /** Why latest is unknown, when it is. */
  reason?: string | null;
}

export type ProtoScenario = "newer" | "uptodate" | "offline" | "disabled" | "never" | "unknown";
export type BadgeVariant = "a" | "b" | "c";

const params = new URLSearchParams(window.location.search);
export const PROTO_SCENARIO = (params.get("proto") as ProtoScenario | null) ?? "newer";
export const BADGE_VARIANT = (params.get("badge") as BadgeVariant | null) ?? "b";

const base: UpdateStatus = {
  installed_version: "1.58.1",
  latest_version: "1.59.0",
  checked_at: new Date(Date.now() - 2 * 3600_000 - 7 * 60_000).toISOString(),
  source: "GitHub Releases",
  check_enabled: true,
  install_method: "homebrew",
  manual_command: "brew update && brew upgrade Loulen/tap/pdo",
  supervision: "systemd",
  reason: null,
};

export function mockUpdateStatus(scenario: ProtoScenario = PROTO_SCENARIO): UpdateStatus {
  switch (scenario) {
    case "uptodate":
      return { ...base, latest_version: "1.58.1" };
    case "offline":
      return { ...base, latest_version: null, reason: "Release source unreachable at last check." };
    case "disabled":
      return { ...base, latest_version: null, check_enabled: false, checked_at: null, reason: "Update check is off." };
    case "never":
      return { ...base, latest_version: null, checked_at: null, reason: "Not checked yet." };
    case "unknown":
      return { ...base, install_method: "unknown", manual_command: "Build from source, then restart the daemon.", supervision: "none" };
    default:
      return base;
  }
}

export function newerAvailable(s: UpdateStatus): boolean {
  return s.check_enabled && s.latest_version != null && s.latest_version !== s.installed_version;
}

export function relativeTime(iso: string): string {
  const diff = Date.now() - new Date(iso).getTime();
  const m = Math.round(diff / 60_000);
  if (m < 1) return "just now";
  if (m < 60) return `${m} min ago`;
  const h = Math.round(m / 60);
  if (h < 48) return `${h} h ago`;
  return `${Math.round(h / 24)} d ago`;
}

export function absoluteTime(iso: string): string {
  return new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(new Date(iso));
}
