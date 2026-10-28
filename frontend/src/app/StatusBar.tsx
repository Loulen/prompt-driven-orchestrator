import type { ConnectionStatus } from "../run/store/useDaemonSocket";
import type { DaemonStatus } from "../platform/model/daemon";
import type { UpdateStatus } from "../platform/model/update";
import { installedVersionLabel, isDistribution } from "../platform/model/updateStatus";
import SessionCounter from "../run/components/SessionCounter";
import ServiceHealthIndicator from "../platform/components/ServiceHealthIndicator";
import VersionBadge from "../platform/components/VersionBadge";
import ExtensionDecorations from "../shared/extensions/ExtensionDecorations";
import type { DecorationContext } from "../shared/extensions/contract";

/** The status bar marks nothing in particular: its decorations get no entity. */
const STATUSBAR_ITEM: Omit<DecorationContext, "location" | "run" | "project"> = { entity: null };

const STATUS_CONFIG: Record<ConnectionStatus, { dot: string; label: string }> = {
  connected: { dot: "bg-st-done", label: "Daemon: connected" },
  reconnecting: { dot: "bg-st-await", label: "Daemon: reconnecting…" },
  disconnected: { dot: "bg-st-failed", label: "Daemon: disconnected" },
};
/** The status bar: the daemon connection, its service health, the live
 *  session count and the installed version. */
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
  const { dot: dotClass, label } = STATUS_CONFIG[status];
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
    <footer
      className="flex h-[22px] shrink-0 items-center gap-3.5 border-t border-line bg-surface-1 px-3 font-mono text-text-3"
      style={{ fontSize: "11px" }}
    >
      <span className="flex items-center gap-1.5">
        <span className={`h-1.5 w-1.5 rounded-full ${dotClass}`} />
        {label}
      </span>
      <span className="flex-1" />
      {/* ADR-0081 (#31): the `statusbar` decorations, before the service health. */}
      <ExtensionDecorations<DecorationContext>
        location="statusbar"
        extra={STATUSBAR_ITEM}
        className="flex-nowrap"
      />
      <ServiceHealthIndicator service={sessions.service} />
      <SessionCounter live={sessions.live} cap={sessions.cap} />
      {version && (
        <VersionBadge version={version} label={versionLabel} update={update} onClick={onOpenVersion} />
      )}
    </footer>
  );
}
