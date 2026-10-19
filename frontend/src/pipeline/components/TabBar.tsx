import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { BadgeCheck, Save, X } from "lucide-react";
import { useEditStore } from "../store/editStore";
import { hasUnsavedWork, isTabLocked } from "../store/editState";
import type { OpenPipeline } from "../store/editState";
import ConfirmCloseTabsModal from "./ConfirmCloseTabsModal";
import DraftPresence from "./DraftPresence";
import RunSaveGroup from "./RunSaveGroup";
import ValidationIndicator from "./ValidationIndicator";
import ValidatePipelineModal from "./ValidatePipelineModal";
import SaveVersionModal from "./SaveVersionModal";
import { validationOf } from "../model/pipelineValidation";
import { lastVersionOf } from "../model/versions";
import type { Project } from "../../workspace/model/project";
import { ExtensionMenuGroups } from "../../shared/extensions/ExtensionActions";
import type { PipelineEntity } from "../../shared/extensions/contract";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem } from "../../shared/ui/dropdown-menu";
import { PageTabActions, PageTabButtons } from "../../platform/components/PageTabs";
import { useMountedPages } from "../../platform/store/mountedPages";

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

export default function TabBar({ projects = [] }: { projects?: Project[] } = {}) {
  const openTabs = useEditStore((s) => s.openTabs);
  const activeTabId = useEditStore((s) => s.activeTabId);
  const setActiveTab = useEditStore((s) => s.setActiveTab);
  const closeTab = useEditStore((s) => s.closeTab);
  const closeTabs = useEditStore((s) => s.closeTabs);
  const save = useEditStore((s) => s.save);
  const requestSave = useEditStore((s) => s.requestSave);
  const saveVersionFor = useEditStore((s) => s.saveVersionFor);
  const closeSaveVersion = useEditStore((s) => s.closeSaveVersion);
  const lastSavedAt = useEditStore((s) => s.lastSavedAt);
  const tabRefs = useRef<Map<string, HTMLButtonElement>>(new Map());
  const pipelines = useEditStore((s) => s.pipelines);
  const loadPipelines = useEditStore((s) => s.loadPipelines);
  // A mounted page's tab shown in the center: no pipeline tab is the active one.
  const pageTabCount = useMountedPages((s) => s.tabs.length);
  const activePage = useMountedPages((s) => s.active);
  const showPage = useMountedPages((s) => s.showPage);
  // The Validate modal of the active Pipeline tab (CONTEXT.md « Pipeline validé »).
  const [validateOpen, setValidateOpen] = useState(false);

  // Right-click context menu (#342): viewport coords so it doesn't drift when
  // the tab strip scrolls. Null = closed.
  const [menu, setMenu] = useState<{ x: number; y: number; tabId: string } | null>(null);
  // A mass-close awaiting confirmation because it would discard unsaved work.
  const [pendingClose, setPendingClose] = useState<{ ids: string[]; victims: OpenPipeline[] } | null>(null);

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
  // ADR-0085: a Run tab in « pilotage » (or archived) has nothing to save, so no
  // Save is shown; in « Edit for this run » Save writes the Run's snapshot only.
  const activeTab = openTabs.find((t) => t.id === activeTabId);
  const activeLocked = isTabLocked(activeTab);
  // Validate and the indicator belong to a Pipeline's own tab — never a Run's
  // (its snapshot is not what the launch forms offer), and only once the
  // Pipeline is in the list (saved at least once under this id).
  const activeEntry =
    activeTab && activeTab.runId == null && activeTab.version == null
      ? pipelines.find((p) => p.id === activeTab.id)
      : undefined;
  // ADR-0088: a shared draft is saved as a version, through the « Save… » window.
  const activeDraft = activeTab?.draft != null;
  const saveVersionTab = saveVersionFor ? openTabs.find((t) => t.id === saveVersionFor) : undefined;
  const activeValidation = validationOf(activeEntry);
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

  if (openTabs.length === 0 && pageTabCount === 0) return null;

  return (
    <>
    <div className="flex h-[30px] shrink-0 items-end border-b border-line bg-bg-2">
      <div
        className="flex min-w-0 flex-1 items-end gap-px overflow-x-auto px-1"
        data-testid="tab-list"
        style={{ scrollbarWidth: "thin" }}
      >
        {openTabs.map((tab) => {
          const isActive = tab.id === activeTabId && activePage == null;
          // ADR-0088: a version shown read only is named after its pipeline.
          const file = tab.version ? `${tab.version.pipelineId}.yaml @ v${tab.version.number}` : `${tab.id}.yaml`;
          const label = tab.dirty ? `• ${file}` : file;
          return (
            <button
              key={tab.id}
              ref={(el) => setTabRef(tab.id, el)}
              onClick={() => {
                showPage(null);
                setActiveTab(tab.id);
              }}
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
        <PageTabButtons />
      </div>

      <div className="flex shrink-0 items-center gap-2 px-1.5 pb-0.5">
        {activePage != null ? (
          <PageTabActions name={activePage} />
        ) : (
          <>
            <DraftPresence />
            {/* Unsaved changes make « Saved … ago » stale: the draft line says them. */}
            {savedAgo && !activeTab?.dirty && (
              <span
                className="whitespace-nowrap font-mono text-fg-4"
                style={{ fontSize: "10px" }}
                data-testid="saved-ago"
              >
                {savedAgo}
              </span>
            )}
            {activeEntry && (
              <>
                <ValidationIndicator
                  validation={activeValidation}
                  projects={projects}
                  testId="toolbar-validation-indicator"
                />
                <button
                  onClick={() => setValidateOpen(true)}
                  className="flex cursor-pointer items-center gap-1 rounded-md border border-line-strong bg-bg-3 px-2 py-0.5 text-fg-2 transition-colors hover:bg-bg-4 hover:text-fg"
                  style={{ fontSize: "11px" }}
                  data-testid="validate-button"
                  title="Choose the projects this pipeline is offered to"
                >
                  <BadgeCheck size={11} />
                  Validate
                </button>
              </>
            )}
            {activeTab?.runId != null && !activeLocked ? (
              <RunSaveGroup tab={{ ...activeTab, runId: activeTab.runId }} />
            ) : activeLocked ? null : (
              <button
                onClick={() => { if (activeTabId) requestSave(activeTabId); }}
                disabled={activeDraft ? !activeTab?.dirty : !anyDirty}
                className="flex cursor-pointer items-center gap-1 rounded-md bg-acc px-2 py-0.5 font-medium text-on-acc transition-colors hover:bg-acc-dim disabled:opacity-40"
                style={{ fontSize: "11px" }}
                data-testid="save-button"
              >
                <Save size={11} />
                {activeDraft ? "Save…" : "Save"}
              </button>
            )}
          </>
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

      {activeEntry && (
        <ValidatePipelineModal
          open={validateOpen}
          pipelineId={activeEntry.id}
          pipelineName={activeEntry.name}
          validation={activeValidation}
          projects={projects}
          onClose={() => setValidateOpen(false)}
          onSaved={() => {
            void loadPipelines();
          }}
        />
      )}

      {saveVersionTab && (
        <SaveVersionModal
          key={saveVersionTab.id}
          pipelineName={saveVersionTab.pipeline.name}
          last={lastVersionOf(pipelines, saveVersionTab.id)}
          onClose={closeSaveVersion}
          onSave={async (saveAs) => {
            await save(saveVersionTab.id, saveAs);
            closeSaveVersion();
          }}
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
 * Right-click menu for a tab (#342), on the shared `DropdownMenu` (#30),
 * anchored where the pointer was — viewport coords, so it survives a scroll of
 * the tab strip. Dismisses on a backdrop click OR Escape. After the four close
 * entries, the extensions' `tab/menu` groups, called with the tab's pipeline.
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

  const anchor = useMemo(
    () => ({ getBoundingClientRect: () => DOMRect.fromRect({ x, y, width: 0, height: 0 }) }),
    [x, y],
  );
  const index = openTabs.findIndex((t) => t.id === tabId);
  const tab = index >= 0 ? openTabs[index] : undefined;
  const single = openTabs.length <= 1;
  const isLast = index >= 0 && index === openTabs.length - 1;
  const otherIds = openTabs.filter((t) => t.id !== tabId).map((t) => t.id);
  const rightIds = index < 0 ? [] : openTabs.slice(index + 1).map((t) => t.id);
  const allIds = openTabs.map((t) => t.id);
  const extra = useMemo(() => {
    const entity: PipelineEntity = {
      kind: "pipeline",
      id: tabId,
      name: tab?.pipeline.name ?? tabId,
      attributes: {},
    };
    return { entity };
  }, [tabId, tab?.pipeline.name]);

  return (
    <>
      <div className="fixed inset-0 z-40" onClick={onDismiss} />
      <DropdownMenu
        open
        // The backdrop above holds the outside clicks, as it always did.
        modal={false}
        onOpenChange={(open) => {
          if (!open) onDismiss();
        }}
      >
        <DropdownMenuContent
          anchor={anchor}
          side="bottom"
          align="start"
          sideOffset={0}
          className="w-auto min-w-[150px]"
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
          <ExtensionMenuGroups location="tab/menu" extra={extra} itemClassName={ITEM} />
        </DropdownMenuContent>
      </DropdownMenu>
    </>
  );
}

const ITEM = "cursor-pointer px-2 py-1.5 text-[11.5px] text-fg-2";

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
  // A disabled entry stays focusable for the arrow keys, `aria-disabled` (the
  // menu pattern of WAI-ARIA), and does nothing when clicked.
  return (
    <DropdownMenuItem data-testid={testid} disabled={disabled} onClick={onClick} className={ITEM}>
      {children}
    </DropdownMenuItem>
  );
}
