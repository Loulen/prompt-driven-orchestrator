import { LoaderCircle, Plug, Unplug, type LucideIcon } from "lucide-react";
import type { ConnectionStatus } from "../run/store/useDaemonSocket";
import type { DaemonStatus } from "../platform/model/daemon";
import type { UpdateStatus } from "../platform/model/update";
import { installedVersionLabel, isDistribution } from "../platform/model/updateStatus";
import SessionCounter from "../run/components/SessionCounter";
import ServiceHealthIndicator from "../platform/components/ServiceHealthIndicator";
import VersionBadge from "../platform/components/VersionBadge";
import ExtensionDecorations from "../shared/extensions/ExtensionDecorations";
import type { DecorationContext } from "../shared/extensions/contract";
import { Tooltip } from "../shared/ui/kit";

/** The status bar marks nothing in particular: its decorations get no entity. */
const STATUSBAR_ITEM: Omit<DecorationContext, "location" | "run" | "project"> = { entity: null };

const DAEMON: Record<ConnectionStatus, { icon: LucideIcon; label: string }> = {
  connected: { icon: Plug, label: "Daemon: connected" },
  reconnecting: { icon: LoaderCircle, label: "Daemon: reconnecting…" },
  disconnected: { icon: Unplug, label: "Daemon: disconnected" },
};

/** The status bar, 22 px, icons and numbers only (their names in the tooltips):
 *  the daemon connection and the live sessions on the left; the extensions'
 *  `statusbar` decorations, the service health and the installed version on
 *  the right. */
export default function StatusBar({
  status,
  sessions,
  update,
  onOpenVersion,
}: {
  status: ConnectionStatus;
  sessions: DaemonStatus;
  update: UpdateStatus | null;
  onOpenVersion: () => void;
}) {
  const { icon: Icon, label } = DAEMON[status];
  // The version string comes from `/sessions` (refreshed on reconnect, #139) with the
  // update payload as fallback — both are the daemon's own `CARGO_PKG_VERSION`. A
  // distribution (#24) is named by its release channel instead: its own name and
  // version, then the core's, which `/sessions` alone would pass off as its own.
  const distribution = update != null && isDistribution(update) ? update : null;
  const version = distribution
    ? distribution.installed_version
    : (sessions.version ?? update?.installed_version);
  const versionLabel = distribution ? installedVersionLabel(distribution) : undefined;

  return (
    <footer className="sh-sb" data-testid="statusbar">
      <Tooltip label={label} side="top">
        <span
          className="sh-sb__item"
          role="status"
          aria-label={label}
          data-st={status}
          data-testid="daemon-status"
          tabIndex={0}
        >
          <Icon size={13} strokeWidth={1.5} aria-hidden className={status === "reconnecting" ? "d-spin" : undefined} />
        </span>
      </Tooltip>
      <SessionCounter live={sessions.live} cap={sessions.cap} />
      <span className="d-grow" />
      {/* ADR-0081 (#31): the `statusbar` decorations, before the service health. */}
      <ExtensionDecorations<DecorationContext>
        location="statusbar"
        extra={STATUSBAR_ITEM}
        className="sh-sb__ext flex-nowrap"
      />
      <ServiceHealthIndicator service={sessions.service} />
      {version && (
        <VersionBadge version={version} label={versionLabel} update={update} onClick={onOpenVersion} />
      )}
    </footer>
  );
}
