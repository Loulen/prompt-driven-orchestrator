import { useEffect, useRef, useState } from "react";
import { Check, Copy, Download, FileText, RefreshCw, TriangleAlert } from "lucide-react";
import { checkForUpdateNow, fetchUpdateAttemptLog } from "../api/update";
import { useUpdateStatus } from "../store/useUpdateStatus";
import {
  INSTALL_METHOD_LABEL,
  SUPERVISION_LABEL,
  absoluteTime,
  announceUpdateStatus,
  newerAvailable,
  relativeTime,
  upToDate,
} from "../model/updateStatus";
import { KeyValueRow } from "../../shared/ui/key-value-row";

/**
 * Version & update (#697) — the daemon's cached version check, read from `GET /update`
 * (once when the surface opens, after « Check now », and on the shared bus), plus two
 * controls with their OWN persistence: « Check now » posts at once, and the check
 * switch writes `update_check` through the host's `onToggleCheck` (`PUT /settings`) at
 * the change — neither joins the form's dirty set (Trap B), and the section renders
 * even when `GET /settings` failed (Trap A), since it does not depend on it.
 *
 * States: newer known → amber latest; up to date → green pill on the installed row;
 * unknown (off / unreachable / never) → `—` + the reason. The bar's badge follows via
 * the bus, so disabling the check removes it immediately.
 */
export default function VersionUpdateSection({
  active,
  onOpenChangelog,
  onRequestUpdate,
  onToggleCheck,
}: {
  active: boolean;
  onOpenChangelog?: () => void;
  onRequestUpdate?: () => void;
  /** Write the `update_check` setting — the Settings surface owns it. */
  onToggleCheck: (on: boolean) => Promise<void>;
}) {
  const { status, setStatus, refresh } = useUpdateStatus(active);
  // #699: the last attempt's journal, fetched on demand (« View log »).
  const [logOpen, setLogOpen] = useState(false);
  const [log, setLog] = useState<string | null>(null);
  const [logError, setLogError] = useState<string | null>(null);
  const attemptId = status?.last_attempt?.attempt_id ?? null;
  useEffect(() => {
    if (!logOpen || !attemptId) return;
    let cancelled = false;
    fetchUpdateAttemptLog(attemptId)
      .then((text) => {
        if (!cancelled) setLog(text);
      })
      .catch((e: unknown) => {
        if (!cancelled) setLogError(e instanceof Error ? e.message : String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [logOpen, attemptId]);
  const [checking, setChecking] = useState(false);
  const [toggling, setToggling] = useState(false);
  const [checkError, setCheckError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const copiedTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(
    () => () => {
      if (copiedTimer.current) clearTimeout(copiedTimer.current);
    },
    [],
  );

  const checkNow = async () => {
    setChecking(true);
    setCheckError(null);
    try {
      const fresh = await checkForUpdateNow();
      setStatus(fresh);
      announceUpdateStatus(fresh);
    } catch (e) {
      // 502 carries the refreshed state (date moved, last good values kept): re-read it
      // so the row shows the new date next to the error.
      setCheckError(e instanceof Error ? e.message : String(e));
      await refresh();
    } finally {
      setChecking(false);
    }
  };

  const toggle = async (on: boolean) => {
    setToggling(true);
    setCheckError(null);
    try {
      await onToggleCheck(on);
      // Off: the daemon answers `null` + "Update check is off." at once. On: it runs a
      // check on its own; until it lands the read says "Not checked yet".
      const fresh = await refresh();
      if (fresh && on && fresh.latest_version == null && !fresh.last_error) {
        const pending = { ...fresh, reason: "Not checked yet since re-enabling." };
        setStatus(pending);
        announceUpdateStatus(pending);
        // The re-enable check is detached on the daemon; pick up its result shortly.
        window.setTimeout(() => {
          void refresh();
        }, 1500);
      }
    } catch (e) {
      setCheckError(e instanceof Error ? e.message : String(e));
    } finally {
      setToggling(false);
    }
  };

  const copy = async () => {
    if (!status) return;
    try {
      await navigator.clipboard.writeText(status.manual_command);
      setCopied(true);
      if (copiedTimer.current) clearTimeout(copiedTimer.current);
      copiedTimer.current = setTimeout(() => setCopied(false), 1500);
    } catch {
      /* clipboard unavailable: the command is selectable text anyway */
    }
  };

  if (!status) {
    return (
      <div className="text-fg-4" style={{ fontSize: "10.5px" }} data-testid="setting-version-update">
        Loading the daemon's version state…
      </div>
    );
  }

  const newer = newerAvailable(status);
  const current = upToDate(status);
  const enabled = status.check_enabled;

  return (
    <div className="flex flex-col gap-1.5" data-testid="setting-version-update">
      <KeyValueRow label="Installed version" testId="setting-version-installed">
        <span className="font-mono">v{status.installed_version}</span>
        {current && (
          <span
            className="ml-2 rounded-full bg-st-done-bg px-1.5 py-px text-st-done"
            style={{ fontSize: "9.5px" }}
            data-testid="setting-version-uptodate"
          >
            up to date
          </span>
        )}
      </KeyValueRow>
      <KeyValueRow label="Latest release" testId="setting-version-latest">
        {status.latest_version ? (
          <>
            <span className={`font-mono ${newer ? "text-st-await" : ""}`}>v{status.latest_version}</span>
            {onOpenChangelog && (
              <button
                type="button"
                onClick={onOpenChangelog}
                className="ml-2 inline-flex items-center gap-1 text-fg-3 underline decoration-fg-5 underline-offset-2 hover:text-fg"
                data-testid="setting-version-whats-new"
                title={newer ? "See what changed since your version" : "See the changelog of this build"}
              >
                <FileText size={10} />
                What's new
              </button>
            )}
          </>
        ) : (
          <>
            <span className="font-mono">—</span>
            {status.reason && <span className="ml-2 text-fg-4">{status.reason}</span>}
          </>
        )}
      </KeyValueRow>
      <KeyValueRow label="Last check" testId="setting-version-checked-at">
        {status.checked_at ? (
          <>
            {absoluteTime(status.checked_at)}
            <span className="text-fg-4">
              {" "}
              · {relativeTime(status.checked_at)} · {status.source}
            </span>
          </>
        ) : (
          <span className="font-mono">—</span>
        )}
      </KeyValueRow>
      <KeyValueRow label="Install method" testId="setting-version-install-method">
        {INSTALL_METHOD_LABEL[status.install_method]}
        <span className="text-fg-4"> · {SUPERVISION_LABEL[status.supervision]}</span>
      </KeyValueRow>
      <div
        className="flex items-center justify-between gap-3 rounded border border-line bg-bg-2 px-2.5 py-1.5"
        style={{ fontSize: "10.5px" }}
        data-testid="setting-version-manual-command"
      >
        <div className="flex min-w-0 flex-col gap-0.5">
          <span className="text-fg-4">
            {status.install_method === "unknown"
              ? "Install method not detected — PDO will not update itself. To update:"
              : "To update manually (the Update button runs exactly this, detached from the daemon):"}
          </span>
          <code className="truncate font-mono text-fg-2">{status.manual_command}</code>
        </div>
        {status.install_method !== "unknown" && (
          <button
            type="button"
            onClick={copy}
            className="flex shrink-0 items-center gap-1 rounded border border-line-strong bg-bg-3 px-2 py-1 text-fg-2 hover:border-acc"
            style={{ fontSize: 10.5 }}
            title="Copy command"
            data-testid="setting-version-copy-command"
          >
            {copied ? <Check size={11} /> : <Copy size={11} />}
            {copied ? "Copied" : "Copy"}
          </button>
        )}
      </div>
      {status.install_method === "unknown" && status.apply_blocked_reason && (
        <div
          className="flex items-start gap-1.5 text-fg-4"
          style={{ fontSize: "10.5px" }}
          data-testid="setting-version-apply-blocked"
        >
          <TriangleAlert size={11} className="mt-px shrink-0 text-st-await" />
          <span>{status.apply_blocked_reason}</span>
        </div>
      )}
      {status.last_attempt && (
        <div className="flex flex-col gap-1" data-testid="setting-version-last-attempt">
          <KeyValueRow
            label="Last update"
            tone={status.last_attempt.status === "failed" ? "failed" : undefined}
            testId="setting-version-last-attempt-row"
          >
            <span
              className={`rounded-full px-1.5 py-px ${
                status.last_attempt.status === "succeeded"
                  ? "bg-st-done-bg text-st-done"
                  : status.last_attempt.status === "failed"
                    ? "bg-st-failed-bg text-st-failed"
                    : "bg-st-await-bg text-st-await"
              }`}
              style={{ fontSize: "9.5px" }}
              data-testid="setting-version-last-attempt-status"
            >
              {status.last_attempt.status === "failed" && status.last_attempt.exit_code != null
                ? `failed (exit ${status.last_attempt.exit_code})`
                : status.last_attempt.status}
            </span>
            <span className="ml-2">{absoluteTime(status.last_attempt.started_at)}</span>
            <span className="text-fg-4">
              {" "}
              · {relativeTime(status.last_attempt.started_at)} · from v{status.last_attempt.from_version} ·{" "}
              {INSTALL_METHOD_LABEL[status.last_attempt.method]}
            </span>
            <button
              type="button"
              onClick={() => {
                setLog(null);
                setLogError(null);
                setLogOpen((o) => !o);
              }}
              className="ml-2 inline-flex items-center gap-1 text-fg-3 underline decoration-fg-5 underline-offset-2 hover:text-fg"
              data-testid="setting-version-attempt-log-toggle"
              title={status.last_attempt.log_path}
            >
              <FileText size={10} />
              {logOpen ? "Hide log" : "View log"}
            </button>
          </KeyValueRow>
          {logOpen && (
            <pre
              className="max-h-56 overflow-auto rounded border border-line bg-bg-0 px-2.5 py-2 font-mono text-fg-2"
              style={{ fontSize: "10.5px", lineHeight: 1.4 }}
              data-testid="setting-version-attempt-log"
            >
              {logError ? `Could not load the log: ${logError}` : log ?? "Loading…"}
            </pre>
          )}
        </div>
      )}
      {checkError && (
        <KeyValueRow label="Check failed" tone="failed" testId="setting-version-check-error">
          {checkError}
        </KeyValueRow>
      )}
      <div className="mt-1 flex flex-wrap items-center gap-4">
        <button
          type="button"
          onClick={() => onRequestUpdate?.()}
          disabled={!status.can_apply || !onRequestUpdate}
          data-testid="setting-version-update-button"
          title={
            status.apply_blocked_reason ??
            (newer ? `Update to v${status.latest_version}` : "Re-run the install method's update command")
          }
          className={`flex items-center gap-1.5 rounded border px-2.5 py-1.5 disabled:opacity-40 ${
            newer
              ? "border-acc bg-acc text-bg-0 hover:bg-acc/90"
              : "border-line-strong bg-bg-3 text-fg-2 hover:border-acc"
          }`}
          style={{ fontSize: 11 }}
        >
          <Download size={11} />
          {newer && status.latest_version ? `Update to v${status.latest_version}` : "Update"}
        </button>
        <button
          type="button"
          onClick={() => void checkNow()}
          disabled={checking || !enabled}
          data-testid="setting-version-check-now"
          title={!enabled ? "Turn the update check on to check now." : undefined}
          className="flex items-center gap-1.5 rounded border border-line-strong bg-bg-3 px-2.5 py-1.5 text-fg-2 hover:border-acc disabled:opacity-40"
          style={{ fontSize: 11 }}
        >
          <RefreshCw size={11} className={checking ? "animate-spin" : ""} />
          {checking ? "Checking…" : "Check now"}
        </button>
        <div className="flex items-center gap-2.5">
          <button
            type="button"
            role="switch"
            aria-checked={enabled}
            aria-label="Check for updates"
            disabled={toggling}
            data-testid="setting-update-check"
            onClick={() => void toggle(!enabled)}
            className={`relative h-3.5 w-6 shrink-0 rounded-full transition-colors ${
              enabled ? "bg-acc" : "bg-fg-5"
            }`}
          >
            <span
              className={`absolute top-0.5 h-2.5 w-2.5 rounded-full bg-bg-1 transition-all ${
                enabled ? "left-3" : "left-0.5"
              }`}
            />
          </button>
          <span className="font-medium text-fg-2" style={{ fontSize: "11.5px" }}>
            Check for updates
          </span>
          <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
            at start, every 6 h and on demand. Off: no request ever leaves the daemon.
          </span>
        </div>
      </div>
    </div>
  );
}
