import { useEffect, useRef, useState } from "react";
import { Check, Copy, Download, SlidersHorizontal, TriangleAlert, WifiOff } from "lucide-react";
import MarkdownArtifactModal from "../../run/components/MarkdownArtifactModal";
import { fetchUpdateChangelog } from "../api/update";
import type { UpdateChangelog } from "../../shared/generated/UpdateChangelog";
import type { UpdateStatus } from "../model/update";
import {
  canOfferUpdate,
  channelLabel,
  installedVersionLabel,
  isDistribution,
  updateNotOfferedReason,
} from "../model/updateStatus";
import { copyToClipboard } from "../../shared/lib/clipboard";
import { Button, IconButton } from "../../shared/ui/kit";

/**
 * « What's new » (#698): the changelog of the versions missed since the installed one,
 * opened from the status-bar version. Composes the markdown artifact viewer with an
 * in-memory source fed by `GET /update/changelog`, and the shared update status for
 * the footer (install method, manual command).
 *
 * Header: `v<installed> → v<latest> · N version(s) behind` (latest in amber, the pill's
 * token) — or a green `up to date` pill — or the bare version when the latest is unknown.
 * Banner: grey for up to date (« You run the latest release… »), amber when the body is
 * the embedded changelog as a FALLBACK (source unreachable / check off), red when the
 * endpoint itself failed. The words « up to date » appear here and in Settings only —
 * never in the bar (#697 rule).
 * Footer: the manual command with its copy icon, the **Update** button (#699 — the
 * host owns the confirm and the waiting flow; disabled with the reason when the daemon
 * refuses: unknown method, attempt running), and a « Version & update » link to
 * Settings. An unknown install method declares its absence: warning + the manual text,
 * no Copy, no Update (ADR-0045).
 */
export default function ChangelogModal({
  update,
  onClose,
  onOpenVersionSettings,
  onRequestUpdate,
}: {
  update: UpdateStatus | null;
  onClose: () => void;
  onOpenVersionSettings: () => void;
  onRequestUpdate?: () => void;
}) {
  const [changelog, setChangelog] = useState<UpdateChangelog | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  // The modal is MOUNTED when open (App renders it conditionally), so every open is a
  // fresh fetch — a reopen after an update never shows stale notes. The daemon memoises
  // the release list, so this costs no egress.
  useEffect(() => {
    let cancelled = false;
    fetchUpdateChangelog()
      .then((doc) => {
        if (cancelled) return;
        setChangelog(doc);
        setLoading(false);
      })
      .catch((e: unknown) => {
        if (cancelled) return;
        setError(e instanceof Error ? e.message : String(e));
        setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const installed = changelog?.installed_version ?? update?.installed_version ?? null;
  const latest = changelog?.latest_version ?? null;
  const missed = changelog?.missed_versions ?? [];
  const upToDate = changelog != null && latest != null && missed.length === 0 && changelog.fallback_reason == null;
  const behind = missed.length;

  return (
    <MarkdownArtifactModal
      runId=""
      portName="What's new"
      source={{ kind: "inline", content: changelog?.markdown ?? null, loading }}
      onClose={onClose}
      size="m"
      testId="changelog-modal"
      header={
        <span className="art__title" data-testid="changelog-header">
          <span className="font-medium text-text">What's new</span>
          {installed && (
            <span className="flex min-w-0 items-center gap-1 font-mono text-xs text-text-3">
              <span className="truncate">
                {update != null && isDistribution(update) ? installedVersionLabel(update) : `v${installed}`}
              </span>
              {behind > 0 && latest && (
                <>
                  <span aria-hidden="true">→</span>
                  <span className="text-st-await" data-testid="changelog-latest">
                    v{latest}
                  </span>
                  <span data-testid="changelog-behind">
                    · {behind} version{behind > 1 ? "s" : ""} behind
                  </span>
                </>
              )}
              {upToDate && (
                <span className="d-chip text-st-done" data-testid="changelog-uptodate">
                  <Check size={11} strokeWidth={2} aria-hidden />
                  up to date
                </span>
              )}
            </span>
          )}
        </span>
      }
      banner={
        error ? (
          <div className="art__banner" data-tone="failed" data-testid="changelog-error">
            <TriangleAlert size={13} strokeWidth={1.5} aria-hidden />
            Could not load the changelog: {error}.
          </div>
        ) : changelog?.fallback_reason ? (
          <div
            className="art__banner"
            data-tone="await"
            data-testid="changelog-fallback"
            title="The changelog embedded in this build lists breaking changes only"
          >
            <WifiOff size={13} strokeWidth={1.5} aria-hidden />
            <span>
              Release notes unavailable · {changelog.fallback_reason} Embedded changelog, breaking changes only.
            </span>
          </div>
        ) : upToDate ? (
          <div className="art__banner" data-testid="changelog-uptodate-banner">
            <Check size={13} strokeWidth={1.5} aria-hidden />
            You run the latest release · embedded changelog, breaking changes only
          </div>
        ) : null
      }
      footer={
        <ChangelogFooter
          update={update}
          upToDate={upToDate}
          onOpenVersionSettings={onOpenVersionSettings}
          onRequestUpdate={onRequestUpdate}
        />
      }
    />
  );
}

function ChangelogFooter({
  update,
  upToDate,
  onOpenVersionSettings,
  onRequestUpdate,
}: {
  update: UpdateStatus | null;
  upToDate: boolean;
  onOpenVersionSettings: () => void;
  onRequestUpdate?: () => void;
}) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(
    () => () => {
      if (timer.current) clearTimeout(timer.current);
    },
    [],
  );

  const copy = async () => {
    if (!update?.manual_command) return;
    // Clipboard unavailable: the command is selectable text anyway.
    if (!(await copyToClipboard(update.manual_command))) return;
    setCopied(true);
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(() => setCopied(false), 1500);
  };

  // Not updatable from here: the method is unknown, or the distribution (#24)
  // declares no upgrade command.
  const noCommand = update != null && update.manual_command == null;
  const unknown = update?.install_method === "unknown" || noCommand;

  return (
    <div className="flex w-full min-w-0 items-center gap-3" data-testid="changelog-footer">
      <div className="flex min-w-0 flex-1 flex-col gap-0.5 text-xs text-text-3">
        {update == null ? (
          <span>Loading the version state…</span>
        ) : unknown ? (
          <>
            <span className="flex items-center gap-1.5">
              <TriangleAlert size={12} strokeWidth={1.5} className="shrink-0 text-st-await" aria-hidden />
              {noCommand ? `${channelLabel(update)} declares no update command` : "Install method not detected"}
            </span>
            {update.manual_command != null && (
              <span className="truncate" title={update.manual_command} data-testid="changelog-manual-command">
                {update.manual_command}
              </span>
            )}
            {update.apply_blocked_reason && (
              <span className="truncate" title={update.apply_blocked_reason} data-testid="changelog-apply-blocked">
                {update.apply_blocked_reason}
              </span>
            )}
          </>
        ) : (
          <span
            className="flex min-w-0 items-center gap-1"
            title={
              upToDate
                ? "Nothing to update. Reinstall manually"
                : "To update, run in a terminal: the daemon restarts, tmux sessions survive"
            }
          >
            <code className="min-w-0 truncate font-mono text-text-2" data-testid="changelog-manual-command">
              {update.manual_command}
            </code>
            <IconButton
              icon={copied ? Check : Copy}
              label={copied ? "Copied" : "Copy command"}
              size="sm"
              onClick={() => void copy()}
              data-testid="changelog-copy-command"
            />
          </span>
        )}
      </div>
      <div className="flex shrink-0 items-center gap-2">
        <Button
          tone="ghost"
          icon={SlidersHorizontal}
          onClick={onOpenVersionSettings}
          title="Settings › General › Version & update"
          data-testid="changelog-open-settings"
        >
          Version & update
        </Button>
        {update && !unknown && onRequestUpdate && (
          <Button
            tone={upToDate ? "default" : "primary"}
            icon={Download}
            onClick={onRequestUpdate}
            // Offered only when a newer release is known (never a « reinstall » on a guess).
            disabled={!canOfferUpdate(update)}
            title={updateNotOfferedReason(update) ?? `Update ${channelLabel(update)} from the app`}
            data-testid="changelog-update"
          >
            Update
          </Button>
        )}
      </div>
    </div>
  );
}
