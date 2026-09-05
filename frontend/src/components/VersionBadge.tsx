import type { UpdateStatus } from "../types";
import { newerAvailable } from "../lib/updateStatus";

/**
 * The status-bar version (#697): a BUTTON that opens Settings › General › Version &
 * update. When the daemon's cache knows a newer release, an amber pill `→ <latest>`
 * says how far behind we are (same family as the `ephemeral` pill). Nothing else:
 * up to date, offline, disabled or never checked all render the bare version — the
 * bar never claims "up to date".
 */
export default function VersionBadge({
  version,
  update,
  onClick,
}: {
  version: string;
  update: UpdateStatus | null;
  onClick: () => void;
}) {
  const newer = newerAvailable(update);
  const title = newer
    ? `v${update!.latest_version} is available — open Version & update`
    : "Open Version & update";
  return (
    <button
      type="button"
      onClick={onClick}
      title={title}
      className="-mx-1 flex items-center gap-1.5 rounded px-1 hover:bg-bg-3 hover:text-fg-2 focus:outline-none focus-visible:ring-1 focus-visible:ring-acc"
      data-testid="statusbar-version"
    >
      v{version}
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
