import { useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { Check, Copy, Download, SlidersHorizontal, TriangleAlert, WifiOff } from "lucide-react";
import MarkdownArtifactModal from "./MarkdownArtifactModal";
import { fetchUpdateChangelog } from "../api";
import type { UpdateChangelog, UpdateStatus } from "../types";

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
 * Footer: the manual command with « Copy command », the **Update** button (#699 — the
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
      widthClass="w-[680px]"
      testId="changelog-modal"
      header={
        <div className="flex min-w-0 items-center gap-2.5" data-testid="changelog-header">
          <span className="font-medium text-fg" style={{ fontSize: "13px" }}>
            What's new
          </span>
          {installed && (
            <span className="flex items-center gap-1.5 font-mono text-fg-4" style={{ fontSize: "11px" }}>
              <span>v{installed}</span>
              {behind > 0 && latest && (
                <>
                  <span>→</span>
                  <span className="text-st-await" data-testid="changelog-latest">
                    v{latest}
                  </span>
                  <span data-testid="changelog-behind">
                    · {behind} version{behind > 1 ? "s" : ""} behind
                  </span>
                </>
              )}
              {upToDate && (
                <span
                  className="rounded-full border border-st-done/40 bg-st-done-bg px-1.5 leading-[14px] text-st-done"
                  style={{ fontSize: "9.5px" }}
                  data-testid="changelog-uptodate"
                >
                  up to date
                </span>
              )}
            </span>
          )}
        </div>
      }
      banner={
        error ? (
          <Banner tone="failed" testId="changelog-error">
            Could not load the changelog: {error}.
          </Banner>
        ) : changelog?.fallback_reason ? (
          <Banner tone="await" icon={<WifiOff size={13} />} testId="changelog-fallback">
            <span className="font-medium">Release notes unavailable</span> — {changelog.fallback_reason}{" "}
            Showing the changelog embedded in this build; it lists{" "}
            <strong>breaking changes only</strong>.
          </Banner>
        ) : upToDate ? (
          <Banner tone="muted" icon={<Check size={13} />} testId="changelog-uptodate-banner">
            You run the latest release. Below is the changelog embedded in this build (breaking
            changes only).
          </Banner>
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

function Banner({
  tone,
  icon,
  children,
  testId,
}: {
  tone: "muted" | "await" | "failed";
  icon?: ReactNode;
  children: ReactNode;
  testId?: string;
}) {
  const cls =
    tone === "await"
      ? "border-st-await/50 bg-st-await-bg text-st-await"
      : tone === "failed"
        ? "border-st-failed/50 bg-st-failed-bg text-st-failed"
        : "border-line bg-bg-0 text-fg-3";
  return (
    <div
      className={`mb-3 flex items-start gap-2 rounded border px-3 py-2 ${cls}`}
      style={{ fontSize: "11.5px", lineHeight: 1.45 }}
      data-testid={testId}
    >
      {icon && <span className="mt-px shrink-0">{icon}</span>}
      <span>{children}</span>
    </div>
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
    if (!update) return;
    try {
      await navigator.clipboard.writeText(update.manual_command);
      setCopied(true);
      if (timer.current) clearTimeout(timer.current);
      timer.current = setTimeout(() => setCopied(false), 1500);
    } catch {
      /* clipboard unavailable: the command is selectable text anyway */
    }
  };

  const unknown = update?.install_method === "unknown";

  return (
    <div className="flex items-end justify-between gap-4" data-testid="changelog-footer">
      <div className="flex min-w-0 flex-col gap-0.5" style={{ fontSize: "11px" }}>
        {update == null ? (
          <span className="text-fg-4">Loading the daemon's version state…</span>
        ) : unknown ? (
          <>
            <span className="flex items-center gap-1.5 text-fg-3">
              <TriangleAlert size={12} className="text-st-await" />
              Install method not detected — PDO will not update itself.
            </span>
            <span className="text-fg-4" data-testid="changelog-manual-command">
              {update.manual_command}
            </span>
            {update.apply_blocked_reason && (
              <span className="text-fg-4" data-testid="changelog-apply-blocked">
                {update.apply_blocked_reason}
              </span>
            )}
          </>
        ) : (
          <>
            <span className="text-fg-4">
              {upToDate
                ? "Nothing to update. To reinstall manually:"
                : "To update, run in a terminal (the daemon restarts, tmux sessions survive):"}
            </span>
            <code
              className="truncate font-mono text-fg-2"
              style={{ fontSize: "11px" }}
              data-testid="changelog-manual-command"
            >
              {update.manual_command}
            </code>
          </>
        )}
      </div>
      <div className="flex shrink-0 items-center gap-2">
        <button
          type="button"
          onClick={onOpenVersionSettings}
          className="flex items-center gap-1.5 rounded px-2 py-1.5 text-fg-3 hover:bg-bg-3 hover:text-fg-2"
          style={{ fontSize: "11px" }}
          data-testid="changelog-open-settings"
        >
          <SlidersHorizontal size={12} />
          Version & update
        </button>
        {update && !unknown && (
          <button
            type="button"
            onClick={() => void copy()}
            className="flex items-center gap-1.5 rounded border border-line-strong bg-bg-3 px-2.5 py-1.5 text-fg-2 hover:border-acc"
            style={{ fontSize: "11px" }}
            title="Copy the manual update command"
            data-testid="changelog-copy-command"
          >
            {copied ? <Check size={12} /> : <Copy size={12} />}
            {copied ? "Copied" : "Copy command"}
          </button>
        )}
        {update && !unknown && onRequestUpdate && (
          <button
            type="button"
            onClick={onRequestUpdate}
            disabled={!update.can_apply}
            className={`flex items-center gap-1.5 rounded border px-2.5 py-1.5 disabled:opacity-40 ${
              upToDate
                ? "border-line-strong bg-bg-3 text-fg-2 hover:border-acc"
                : "border-acc bg-acc text-bg-0 hover:bg-acc/90"
            }`}
            style={{ fontSize: "11px" }}
            title={
              update.apply_blocked_reason ??
              (upToDate ? "Re-run the install method's update command" : "Update PDO from the app")
            }
            data-testid="changelog-update"
          >
            <Download size={12} />
            {upToDate ? "Reinstall" : "Update"}
          </button>
        )}
      </div>
    </div>
  );
}
