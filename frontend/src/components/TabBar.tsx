import { useCallback, useEffect, useRef, useState } from "react";
import { ChevronDown, Save, TriangleAlert, X } from "lucide-react";
import { useEditStore, hasUnsavedWork, isRunTabLocked } from "../stores/editStore";
import type { OpenPipeline } from "../stores/editStore";
import { fetchRunPipelineOverwritePreview, type OverwritePreview } from "../api";
import ConfirmCloseTabsModal from "./ConfirmCloseTabsModal";
import {
  DropdownMenu,
  DropdownMenuTrigger,
  DropdownMenuContent,
  DropdownMenuItem,
} from "./ui/dropdown-menu";

function useRelativeTime(ts: number | undefined): string | null {
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (!ts) return;
    const id = setInterval(() => setNow(Date.now()), 10_000);
    return () => clearInterval(id);
  }, [ts]);

  if (!ts) return null;
  const secs = Math.max(0, Math.floor((now - ts) / 1000));
  if (secs < 5) return "Saved just now";
  if (secs < 60) return `Saved ${secs}s ago`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `Saved ${mins}m ago`;
  const hrs = Math.floor(mins / 60);
  return `Saved ${hrs}h ago`;
}

export default function TabBar() {
  const openTabs = useEditStore((s) => s.openTabs);
  const activeTabId = useEditStore((s) => s.activeTabId);
  const setActiveTab = useEditStore((s) => s.setActiveTab);
  const closeTab = useEditStore((s) => s.closeTab);
  const closeTabs = useEditStore((s) => s.closeTabs);
  const save = useEditStore((s) => s.save);
  const lastSavedAt = useEditStore((s) => s.lastSavedAt);
  const tabRefs = useRef<Map<string, HTMLButtonElement>>(new Map());

  // Right-click context menu (#342): viewport coords so it doesn't drift when
  // the tab strip scrolls. Null = closed.
  const [menu, setMenu] = useState<{ x: number; y: number; tabId: string } | null>(null);
  // A mass-close awaiting confirmation because it would discard unsaved work.
  const [pendingClose, setPendingClose] = useState<{ ids: string[]; victims: OpenPipeline[] } | null>(null);
  // ADR-0080: the tab whose « Overwrite default pipeline » warning is open.
  const [overwriteTabId, setOverwriteTabId] = useState<string | null>(null);

  // Gate a close request on unsaved work: confirm if any target tab is unsaved,
  // else close atomically. The single × keeps its silent drop (unchanged); the
  // menu is the explicit path, so it never loses work without asking.
  const requestClose = useCallback(
    (ids: string[]) => {
      const victims = openTabs.filter((t) => ids.includes(t.id));
      if (victims.some(hasUnsavedWork)) {
        setPendingClose({ ids, victims });
      } else {
        closeTabs(ids);
      }
    },
    [openTabs, closeTabs],
  );

  const anyDirty = openTabs.some((t) => t.dirty);
  const activeTab = openTabs.find((t) => t.id === activeTabId);
  // ADR-0080: a run tab in « pilotage » has nothing to save; in « Edit for this
  // run » Save writes the Run's snapshot only, and the shared pipeline moves
  // through the menu's explicit, warned overwrite.
  const activeLocked = isRunTabLocked(activeTab);
  const runEditing = activeTab?.runId != null && !activeLocked;
  const overwriteTab = openTabs.find((t) => t.id === overwriteTabId && t.runId != null);
  const activeLastSaved = activeTabId ? lastSavedAt[activeTabId] : undefined;
  const savedAgo = useRelativeTime(activeLastSaved);

  const setTabRef = useCallback((id: string, el: HTMLButtonElement | null) => {
    if (el) {
      tabRefs.current.set(id, el);
    } else {
      tabRefs.current.delete(id);
    }
  }, []);

  useEffect(() => {
    if (!activeTabId) return;
    const el = tabRefs.current.get(activeTabId);
    if (el) {
      el.scrollIntoView({ block: "nearest", inline: "nearest", behavior: "smooth" });
    }
  }, [activeTabId]);

  if (openTabs.length === 0) return null;

  return (
    <>
    <div className="flex h-[30px] shrink-0 items-end border-b border-line bg-bg-2">
      <div
        className="flex min-w-0 flex-1 items-end gap-px overflow-x-auto px-1"
        data-testid="tab-list"
        style={{ scrollbarWidth: "thin" }}
      >
        {openTabs.map((tab) => {
          const isActive = tab.id === activeTabId;
          const label = tab.dirty ? `• ${tab.id}.yaml` : `${tab.id}.yaml`;
          return (
            <button
              key={tab.id}
              ref={(el) => setTabRef(tab.id, el)}
              onClick={() => setActiveTab(tab.id)}
              onContextMenu={(e) => {
                e.preventDefault();
                setMenu({ x: e.clientX, y: e.clientY, tabId: tab.id });
              }}
              className={`group flex shrink-0 cursor-pointer items-center gap-1.5 rounded-t-md border border-b-0 px-2.5 py-1 transition-colors ${
                isActive
                  ? "border-line bg-bg-1 text-fg"
                  : "border-transparent bg-bg-2 text-fg-3 hover:text-fg-2"
              }`}
              style={{ fontSize: "11px", maxWidth: 180 }}
            >
              <span className="truncate" data-testid={`tab-title-${tab.id}`}>{label}</span>
              {tab.externalDirty && (
                <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-st-blocked" />
              )}
              <span
                role="button"
                onClick={(e) => {
                  e.stopPropagation();
                  closeTab(tab.id);
                }}
                className="ml-auto hidden shrink-0 cursor-pointer rounded p-0.5 text-fg-4 hover:bg-bg-3 hover:text-fg group-hover:inline-flex"
              >
                <X size={10} />
              </span>
            </button>
          );
        })}
      </div>

      <div className="flex shrink-0 items-center gap-2 px-1.5 pb-0.5">
        {savedAgo && (
          <span
            className="whitespace-nowrap font-mono text-fg-4"
            style={{ fontSize: "10px" }}
            data-testid="saved-ago"
          >
            {savedAgo}
          </span>
        )}
        {runEditing ? (
          <div className="flex items-center" data-testid="run-save-group">
            <button
              onClick={() => { if (activeTabId) save(activeTabId); }}
              disabled={!activeTab?.dirty}
              className="flex cursor-pointer items-center gap-1 rounded-l-md bg-acc px-2 py-0.5 font-medium text-on-acc transition-colors hover:bg-acc-dim disabled:opacity-40"
              style={{ fontSize: "11px" }}
              data-testid="save-button"
            >
              <Save size={11} />
              Save for this run
            </button>
            <DropdownMenu>
              <DropdownMenuTrigger
                data-testid="save-menu"
                aria-label="More save options"
                className="flex cursor-pointer items-center self-stretch rounded-r-md border-l border-bg-0/30 bg-acc px-1 text-on-acc transition-colors hover:bg-acc-dim"
              >
                <ChevronDown size={11} />
              </DropdownMenuTrigger>
              <DropdownMenuContent
                className="min-w-[230px] rounded-md border border-line-strong bg-bg-3 p-1 shadow-lg"
                side="bottom"
                align="end"
              >
                <DropdownMenuItem
                  data-testid="save-menu-overwrite"
                  className="flex cursor-pointer flex-col items-start gap-0.5 rounded px-2 py-1.5 text-fg-2 transition-colors hover:bg-bg-4"
                  style={{ fontSize: "11.5px" }}
                  onClick={() => setOverwriteTabId(activeTabId)}
                >
                  <span>Overwrite default pipeline…</span>
                  <span className="text-fg-4" style={{ fontSize: "10px" }}>
                    Every future run will use this run's version
                  </span>
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
        ) : (
          <button
            onClick={() => { if (activeTabId) save(activeTabId); }}
            disabled={!anyDirty || activeLocked}
            className="flex cursor-pointer items-center gap-1 rounded-md bg-acc px-2 py-0.5 font-medium text-on-acc transition-colors hover:bg-acc-dim disabled:opacity-40"
            style={{ fontSize: "11px" }}
            data-testid="save-button"
          >
            <Save size={11} />
            Save
          </button>
        )}
      </div>
    </div>

      {menu && (
        <TabContextMenu
          x={menu.x}
          y={menu.y}
          tabId={menu.tabId}
          openTabs={openTabs}
          onDismiss={() => setMenu(null)}
          onSelect={(ids) => {
            // Close the menu BEFORE the confirm modal opens: both are z-50, so
            // leaving the menu up would let it sit over the modal.
            setMenu(null);
            requestClose(ids);
          }}
        />
      )}

      {overwriteTab?.runId && (
        <OverwriteDefaultPipelineModal
          tab={overwriteTab}
          onClose={() => setOverwriteTabId(null)}
        />
      )}

      <ConfirmCloseTabsModal
        open={pendingClose != null}
        tabs={pendingClose?.victims ?? []}
        onCancel={() => setPendingClose(null)}
        onConfirm={() => {
          if (pendingClose) closeTabs(pendingClose.ids);
          setPendingClose(null);
        }}
      />
    </>
  );
}

/**
 * Right-click menu for a tab (#342). A local `fixed z-50` div in the house
 * style (clone of `EditCanvas`'s ContextMenu, not base-ui) — positioned in
 * viewport coords so it survives a scroll of the tab strip. Dismisses on a
 * backdrop click OR Escape (`LibraryDropdown` only handles mousedown; the menu
 * must also close on Escape).
 */
function TabContextMenu({
  x,
  y,
  tabId,
  openTabs,
  onSelect,
  onDismiss,
}: {
  x: number;
  y: number;
  tabId: string;
  openTabs: OpenPipeline[];
  onSelect: (ids: string[]) => void;
  onDismiss: () => void;
}) {
  useEffect(() => {
    function handleKey(e: KeyboardEvent) {
      if (e.key === "Escape") onDismiss();
    }
    document.addEventListener("keydown", handleKey);
    return () => document.removeEventListener("keydown", handleKey);
  }, [onDismiss]);

  const index = openTabs.findIndex((t) => t.id === tabId);
  const single = openTabs.length <= 1;
  const isLast = index >= 0 && index === openTabs.length - 1;
  const otherIds = openTabs.filter((t) => t.id !== tabId).map((t) => t.id);
  const rightIds = index < 0 ? [] : openTabs.slice(index + 1).map((t) => t.id);
  const allIds = openTabs.map((t) => t.id);

  return (
    <>
      <div className="fixed inset-0 z-40" onClick={onDismiss} />
      <div
        className="fixed z-50 rounded-lg border border-line bg-bg-4 py-1 shadow-lg"
        style={{ left: x, top: y, fontSize: "11.5px", minWidth: 150 }}
        data-testid="tab-context-menu"
      >
        <MenuItem testid="tab-ctx-close" onClick={() => onSelect([tabId])}>
          Close
        </MenuItem>
        <MenuItem testid="tab-ctx-close-others" disabled={single} onClick={() => onSelect(otherIds)}>
          Close others
        </MenuItem>
        <MenuItem testid="tab-ctx-close-right" disabled={isLast} onClick={() => onSelect(rightIds)}>
          Close to the right
        </MenuItem>
        <MenuItem testid="tab-ctx-close-all" disabled={single} onClick={() => onSelect(allIds)}>
          Close all
        </MenuItem>
      </div>
    </>
  );
}

function MenuItem({
  testid,
  disabled,
  onClick,
  children,
}: {
  testid: string;
  disabled?: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      data-testid={testid}
      disabled={disabled}
      onClick={onClick}
      className="flex w-full cursor-pointer items-center px-3 py-1.5 text-left text-fg-2 hover:bg-bg-3 hover:text-fg disabled:pointer-events-none disabled:opacity-40"
    >
      {children}
    </button>
  );
}

/**
 * ADR-0080: the warning before « Overwrite default pipeline ». It says who is
 * touched — every future run, the Triggers that launch the pipeline — and
 * whether a colleague changed the shared pipeline since this run was launched.
 * Confirmed, the run's whole snapshot (YAML + prompts) replaces it. No Enter
 * binding: the gesture is the dangerous one, it must be clicked.
 */
export function OverwriteDefaultPipelineModal({
  tab,
  onClose,
}: {
  tab: OpenPipeline;
  onClose: () => void;
}) {
  const overwrite = useEditStore((s) => s.overwriteDefaultPipeline);
  const runId = tab.runId!;
  const [preview, setPreview] = useState<OverwritePreview | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    fetchRunPipelineOverwritePreview(runId)
      .then((p) => { if (live) setPreview(p); })
      .catch((e: unknown) => { if (live) setLoadError(e instanceof Error ? e.message : String(e)); });
    return () => { live = false; };
  }, [runId]);

  useEffect(() => {
    function handleKey(e: KeyboardEvent) {
      if (e.key === "Escape" && !busy) onClose();
    }
    document.addEventListener("keydown", handleKey);
    return () => document.removeEventListener("keydown", handleKey);
  }, [onClose, busy]);

  async function confirm() {
    setBusy(true);
    setError(null);
    try {
      await overwrite(tab.id);
      onClose();
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e));
      setBusy(false);
    }
  }

  const pipelineName = preview?.pipeline_id ?? tab.pipeline.name;
  const triggers = preview?.triggers ?? [];
  const canConfirm = preview != null && preview.pipeline_exists && !busy;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      data-testid="overwrite-default-backdrop"
      onClick={() => { if (!busy) onClose(); }}
    >
      <div
        role="dialog"
        aria-label="Overwrite default pipeline"
        data-testid="overwrite-default-modal"
        className="flex max-h-[85vh] w-[440px] max-w-[90vw] flex-col rounded-lg border border-line bg-bg-2 p-4 shadow-lg"
        style={{ fontSize: "12px" }}
        onClick={(e) => e.stopPropagation()}
      >
        <h3 className="shrink-0 font-medium text-fg" style={{ fontSize: "13px" }}>
          Overwrite the default pipeline?
        </h3>
        <div className="mt-2 flex min-h-0 flex-col gap-2 overflow-y-auto text-fg-2" data-testid="overwrite-default-body">
          <p data-testid="overwrite-default-future-runs">
            This run's version (graph and prompts) replaces{" "}
            <code className="rounded bg-bg-4 px-1 py-0.5 font-mono text-fg">{pipelineName}</code>.{" "}
            <strong className="font-medium text-fg">All future runs will use it.</strong> Runs already
            started keep their own copy.
          </p>
          {loadError && (
            <p className="text-st-failed" data-testid="overwrite-default-load-error">
              Could not read what the overwrite would touch: {loadError}
            </p>
          )}
          {preview == null && loadError == null && (
            <p className="text-fg-4" data-testid="overwrite-default-loading">Checking triggers and changes…</p>
          )}
          {preview != null && !preview.pipeline_exists && (
            <p className="text-st-failed" data-testid="overwrite-default-missing">
              The default pipeline no longer exists (renamed or deleted): there is nothing to overwrite.
            </p>
          )}
          {preview != null && preview.pipeline_exists && (
            <>
              <div data-testid="overwrite-default-triggers">
                {triggers.length === 0 ? (
                  <p>No trigger launches this pipeline.</p>
                ) : (
                  <>
                    <p>
                      {triggers.length === 1
                        ? "1 trigger launches this pipeline and will use the new version:"
                        : `${triggers.length} triggers launch this pipeline and will use the new version:`}
                    </p>
                    <ul className="mt-1 flex flex-col gap-0.5 pl-3">
                      {triggers.map((t) => (
                        <li key={t.id} className="list-disc text-fg" data-testid="overwrite-default-trigger">
                          {t.name}
                          {!t.enabled && <span className="text-fg-4"> (disabled)</span>}
                        </li>
                      ))}
                    </ul>
                  </>
                )}
              </div>
              {preview.modified_since_launch === true && (
                <p
                  className="flex items-start gap-1.5 rounded border border-st-await/40 bg-st-await/10 px-2 py-1.5 text-st-await"
                  data-testid="overwrite-default-modified"
                >
                  <TriangleAlert size={12} className="mt-[2px] shrink-0" />
                  <span>
                    The default pipeline was changed since this run was launched. Overwriting replaces
                    those changes.
                  </span>
                </p>
              )}
              {preview.modified_since_launch == null && (
                <p className="text-fg-4" data-testid="overwrite-default-modified-unknown">
                  PDO cannot tell whether the default pipeline changed since this run was launched.
                </p>
              )}
            </>
          )}
          {tab.dirty && (
            <p className="text-fg-3" data-testid="overwrite-default-saves-first">
              Your unsaved changes are saved for this run first.
            </p>
          )}
          {error && (
            <p className="text-st-failed" data-testid="overwrite-default-error">{error}</p>
          )}
        </div>
        <div className="mt-4 flex shrink-0 justify-end gap-2">
          <button
            onClick={onClose}
            disabled={busy}
            data-testid="overwrite-default-cancel"
            className="cursor-pointer rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 transition-colors hover:bg-bg-4 disabled:opacity-40"
            style={{ fontSize: "11.5px" }}
          >
            Cancel
          </button>
          <button
            onClick={() => void confirm()}
            disabled={!canConfirm}
            data-testid="overwrite-default-confirm"
            className="cursor-pointer rounded-md bg-st-failed px-3 py-1.5 font-medium text-white transition-colors hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-40"
            style={{ fontSize: "11.5px" }}
          >
            {busy ? "Overwriting…" : "Overwrite default pipeline"}
          </button>
        </div>
      </div>
    </div>
  );
}
