import { useEffect, useRef, useState } from "react";
import { Check, Copy, Download, Loader2, RefreshCw, TriangleAlert } from "lucide-react";
import { checkForUpdateNow, fetchUpdateAttemptLog } from "../api/update";
import { useUpdateStatus } from "../store/useUpdateStatus";
import {
  INSTALL_METHOD_LABEL,
  SUPERVISION_LABEL,
  absoluteTime,
  announceUpdateStatus,
  canOfferUpdate,
  installedVersionLabel,
  newerAvailable,
  relativeTime,
  updateNotOfferedReason,
  upToDate,
} from "../model/updateStatus";
import { KeyValueRow } from "../../shared/ui/key-value-row";
import { Button, Chip, EmptyState, IconButton } from "../../shared/ui/kit";
import { copyToClipboard } from "../../shared/lib/clipboard";
import type { SettingsSection } from "../../settings/model/settingsCategories";
import { SwitchRow } from "../../settings/components/settingsSurface/fields";
import Section, { SectionExtra } from "../../settings/components/settingsSurface/Section";

/**
 * Settings › General › Version & update (#697) — the daemon's cached version check, read from `GET /update`
 * (once when the surface opens, after « Check now », and on the shared bus), plus two
 * controls with their OWN persistence: « Check now » posts at once, and the check
 * switch writes `update_check` through the host's `onToggleCheck` (`PUT /settings`) at
 * the change — neither joins the form's dirty set (Trap B), and the section renders
 * even when `GET /settings` failed (Trap A), since it does not depend on it.
 *
 * States: newer known → amber latest; up to date → a green chip on the installed row;
 * unknown (off / unreachable / never / no source) → `—` + the reason. The bar's badge
 * follows via the bus, so disabling the check removes it immediately.
 *
 * A distribution (#24) shows its own name and version, then the core's; Update is
 * offered only when a newer release is known and the daemon would run it.
 */
export default function VersionUpdateSection({
  section,
  active,
  onOpenChangelog,
  onRequestUpdate,
  onToggleCheck,
}: {
  section: SettingsSection;
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
    if (!status?.manual_command) return;
    // Clipboard unavailable: the command is selectable text anyway.
    if (!(await copyToClipboard(status.manual_command))) return;
    setCopied(true);
    if (copiedTimer.current) clearTimeout(copiedTimer.current);
    copiedTimer.current = setTimeout(() => setCopied(false), 1500);
  };

  const tools = (
    <>
      {status && (
        <Button
          size="sm"
          tone={newerAvailable(status) ? "primary" : "default"}
          icon={Download}
          onClick={() => onRequestUpdate?.()}
          disabled={!canOfferUpdate(status) || !onRequestUpdate}
          data-testid="setting-version-update-button"
          title={updateNotOfferedReason(status) ?? `Update to v${status.latest_version}`}
        >
          {newerAvailable(status) && status.latest_version ? `Update to v${status.latest_version}` : "Update"}
        </Button>
      )}
      <IconButton
        icon={RefreshCw}
        label={checking ? "Checking…" : "Check now"}
        size="sm"
        className={checking ? "[&>svg]:animate-spin" : undefined}
        onClick={() => void checkNow()}
        disabled={!status || checking || !status.check_enabled || status.source_url == null}
        description={
          status?.source_url == null
            ? "No release source for this build"
            : status && !status.check_enabled
              ? "Turn the update check on first"
              : undefined
        }
        data-testid="setting-version-check-now"
      />
    </>
  );

  if (!status) {
    return (
      <Section section={section} tools={tools}>
        <SectionExtra testId="setting-version-update">
          <EmptyState icon={Loader2} spin layout="row">
            Loading
          </EmptyState>
        </SectionExtra>
      </Section>
    );
  }

  const newer = newerAvailable(status);
  const current = upToDate(status);
  const unknown = status.install_method === "unknown";
  // A distribution that declares no upgrade command: nothing to run, nothing to copy.
  const noCommand = status.manual_command == null;
  const commandTip = noCommand
    ? `${status.channel_name} declares no update command: update it the way it was installed`
    : unknown
      ? "Install method not detected: no update from here; to update, run"
      : "The Update button runs exactly this, detached from the daemon";

  return (
    <Section section={section} tools={tools}>
      <SwitchRow
        label="Update check"
        help="At start, every 6 h and on demand; off, no request ever leaves the daemon"
        scope="live"
        testId="setting-row-update-check"
        checked={status.check_enabled}
        disabled={toggling}
        onChange={(on) => void toggle(on)}
      />
      <SectionExtra testId="setting-version-update">
        <KeyValueRow label="Installed" testId="setting-version-installed">
          <span className="set-mono set-ellipsis">{installedVersionLabel(status)}</span>
          {current && (
            <span className="d-chip set-ok" data-testid="setting-version-uptodate">
              up to date
            </span>
          )}
        </KeyValueRow>
        <KeyValueRow label="Latest" testId="setting-version-latest">
          {status.latest_version ? (
            <>
              <span className={`set-mono ${newer ? "text-st-await" : ""}`}>v{status.latest_version}</span>
              {onOpenChangelog && (
                <button
                  type="button"
                  onClick={onOpenChangelog}
                  className="set-link"
                  data-testid="setting-version-whats-new"
                  title={newer ? "What changed since your version" : "The changelog of this build"}
                >
                  What's new
                </button>
              )}
            </>
          ) : (
            <>
              <span className="set-mono">—</span>
              {status.reason && (
                <span className="set-faint set-ellipsis" title={status.reason}>
                  {status.reason}
                </span>
              )}
            </>
          )}
        </KeyValueRow>
        <KeyValueRow label="Checked" testId="setting-version-checked-at">
          {status.checked_at ? (
            <span className="set-ellipsis" title={absoluteTime(status.checked_at)}>
              {relativeTime(status.checked_at)}
              {status.source && <span className="set-faint"> · {status.source}</span>}
            </span>
          ) : (
            <span className="set-mono">—</span>
          )}
        </KeyValueRow>
        <KeyValueRow label="Method" testId="setting-version-install-method">
          <span className="set-ellipsis">
            {INSTALL_METHOD_LABEL[status.install_method]}
            <span className="set-faint"> · {SUPERVISION_LABEL[status.supervision]}</span>
          </span>
        </KeyValueRow>
        <KeyValueRow label="Command" testId="setting-version-manual-command">
          {status.manual_command != null ? (
            <>
              <code className="set-mono set-ellipsis" title={commandTip}>
                {status.manual_command}
              </code>
              {!unknown && (
                <IconButton
                  icon={copied ? Check : Copy}
                  label={copied ? "Copied" : "Copy command"}
                  size="sm"
                  onClick={copy}
                  data-testid="setting-version-copy-command"
                />
              )}
            </>
          ) : (
            <span className="set-faint set-ellipsis" title={commandTip}>
              none
            </span>
          )}
        </KeyValueRow>
        {(unknown || noCommand) && status.apply_blocked_reason && (
          <div className="set-warn" title={status.apply_blocked_reason} data-testid="setting-version-apply-blocked">
            <TriangleAlert size={13} strokeWidth={1.5} aria-hidden />
            <span className="set-ellipsis">{status.apply_blocked_reason}</span>
          </div>
        )}
        {status.last_attempt && (
          <div data-testid="setting-version-last-attempt">
            <KeyValueRow
              label="Last update"
              tone={status.last_attempt.status === "failed" ? "failed" : undefined}
              testId="setting-version-last-attempt-row"
            >
              <Chip
                tone={
                  status.last_attempt.status === "succeeded"
                    ? "done"
                    : status.last_attempt.status === "failed"
                      ? "failed"
                      : "awaiting"
                }
              >
                <span data-testid="setting-version-last-attempt-status">
                  {status.last_attempt.status === "failed" && status.last_attempt.exit_code != null
                    ? `failed (exit ${status.last_attempt.exit_code})`
                    : status.last_attempt.status}
                </span>
              </Chip>
              <span className="set-ellipsis" title={absoluteTime(status.last_attempt.started_at)}>
                {relativeTime(status.last_attempt.started_at)}
                <span className="set-faint">
                  {" "}
                  · from v{status.last_attempt.from_version} · {INSTALL_METHOD_LABEL[status.last_attempt.method]}
                </span>
              </span>
              <button
                type="button"
                onClick={() => {
                  setLog(null);
                  setLogError(null);
                  setLogOpen((o) => !o);
                }}
                className="set-link"
                data-testid="setting-version-attempt-log-toggle"
                title={status.last_attempt.log_path}
              >
                {logOpen ? "Hide log" : "View log"}
              </button>
            </KeyValueRow>
            {logOpen && (
              <pre className="set-log" data-testid="setting-version-attempt-log">
                {logError ? `Could not load the log: ${logError}` : log ?? "Loading…"}
              </pre>
            )}
          </div>
        )}
        {checkError && (
          <KeyValueRow label="Check failed" tone="failed" title={checkError} testId="setting-version-check-error">
            <span className="set-ellipsis">{checkError}</span>
          </KeyValueRow>
        )}
      </SectionExtra>
    </Section>
  );
}
