import type { UpdateStatus } from "../model/update";
import { newerAvailable } from "../model/updateStatus";

/**
 * The status-bar version (#697): a BUTTON that opens the « What's new » changelog
 * modal (#698 — Settings › Version & update stays one click away from its footer).
 * When the daemon's cache knows a newer release, an amber pill `→ <latest>` says how
 * far behind we are (same family as the `ephemeral` pill). Nothing else: up to date,
 * offline, disabled or never checked all render the bare version — the bar never
 * claims "up to date".
 *
 * `label` replaces the bare `v<version>`: a distribution (#24) reads as
 * `acme-pdo 0.3.0 (core 2.0.0)`.
 */
export default function VersionBadge({
  version,
  label,
  update,
  onClick,
}: {
  version: string;
  label?: string;
  update: UpdateStatus | null;
  onClick: () => void;
}) {
  const newer = newerAvailable(update);
  const title = newer
    ? `v${update!.latest_version} is available — see what's new`
    : "What's new in this build";
  return (
    <button
      type="button"
      onClick={onClick}
      title={title}
      className="-mx-1 flex items-center gap-1.5 rounded px-1 hover:bg-bg-3 hover:text-fg-2 focus:outline-none focus-visible:ring-1 focus-visible:ring-acc"
      data-testid="statusbar-version"
    >
      {label ?? `v${version}`}
      {newer && (
        <span
          className="rounded-full border border-st-await/40 bg-st-await-bg px-1.5 leading-[14px] text-st-await"
          style={{ fontSize: "9.5px" }}
          data-testid="statusbar-version-badge"
        >
          → {update!.latest_version}
        </span>
      )}
    </button>
  );
}
