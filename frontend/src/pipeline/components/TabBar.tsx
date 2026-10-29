import { useCallback, useEffect, useRef, useState } from "react";
import { BadgeCheck, Save } from "lucide-react";
import { useEditStore } from "../store/editStore";
import { hasUnsavedWork, isTabLocked } from "../store/editState";
import type { OpenPipeline } from "../store/editState";
import ConfirmCloseTabsModal from "./ConfirmCloseTabsModal";
import RunSaveGroup from "./RunSaveGroup";
import ValidationIndicator from "./ValidationIndicator";
import ValidatePipelineModal from "./ValidatePipelineModal";
import SaveVersionModal from "./SaveVersionModal";
import { validationOf } from "../model/pipelineValidation";
import { lastVersionOf } from "../model/versions";
import type { Project } from "../../workspace/model/project";
import { PageTabActions, PageTabButtons } from "../../platform/components/PageTabs";
import { useMountedPages } from "../../platform/store/mountedPages";
import type { RunListEntry } from "../../shared/generated/RunListEntry";
import { Button, Tooltip } from "../../shared/ui/kit";
import { useOverflowing } from "../../hooks/useOverflowing";
import { useActiveTabInView } from "../../hooks/useActiveTabInView";
import TabContextMenu from "./TabContextMenu";
import { OpenTabsMenu, TabKind, TabLabel } from "./TabParts";
import { tabTitle } from "./tabTitle";
import { DocTab } from "../../shared/ui/DocTab";
import { useTabDrag } from "../../shared/ui/useTabDrag";

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

/**
 * The tab bar of the centre, editor style: one tab per open Run, pipeline and
 * mounted page (the active one on the canvas colour, a ruby line on top),
 * scrolled sideways when they overflow, with a menu of them all; drag a tab to
 * reorder it. On the right, what the active tab offers: Validate and Save.
 */
export default function TabBar({
  projects = [],
  runs = [],
}: {
  /** The Projets the validation indicator names. */
  projects?: Project[];
  /** The Runs, for the name and the status of a Run's tab. */
  runs?: RunListEntry[];
} = {}) {
  const openTabs = useEditStore((s) => s.openTabs);
  const activeTabId = useEditStore((s) => s.activeTabId);
  const setActiveTab = useEditStore((s) => s.setActiveTab);
  const closeTab = useEditStore((s) => s.closeTab);
  const closeTabs = useEditStore((s) => s.closeTabs);
  const moveTab = useEditStore((s) => s.moveTab);
  const movePage = useMountedPages((s) => s.movePage);
  const drag = useTabDrag((group, id, index) => (group === "page" ? movePage(id, index) : moveTab(id, index)));
  const listRef = useRef<HTMLDivElement>(null);
  const overflowing = useOverflowing(listRef);
  const save = useEditStore((s) => s.save);
  const requestSave = useEditStore((s) => s.requestSave);
  const saveVersionFor = useEditStore((s) => s.saveVersionFor);
  const closeSaveVersion = useEditStore((s) => s.closeSaveVersion);
  const lastSavedAt = useEditStore((s) => s.lastSavedAt);

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

  // The active tab stays in view: a Run, a pipeline or a mounted page.
  useActiveTabInView(listRef, activePage != null ? `page:${activePage}` : activeTabId);

  if (openTabs.length === 0 && pageTabCount === 0) return null;

  const runsById = new Map(runs.map((r) => [r.run_id, r]));

  return (
    <>
    <div className="sh-tabs" data-testid="tab-bar">
      <div
        ref={listRef}
        className="sh-tabs__list"
        role="tablist"
        aria-label="Open documents"
        data-testid="tab-list"
        onWheel={(e) => {
          // A wheel over the strip scrolls it sideways.
          if (listRef.current && e.deltaY) listRef.current.scrollLeft += e.deltaY;
        }}
      >
        {openTabs.map((tab, index) => {
          const isActive = tab.id === activeTabId && activePage == null;
          const run = tab.runId != null ? runsById.get(tab.runId) : undefined;
          return (
            <DocTab
              key={tab.id}
              active={isActive}
              dirty={tab.dirty}
              external={tab.externalDirty}
              title={tabTitle(tab, run)}
              closeLabel={`Close ${tabTitle(tab, run)}`}
              kind={<TabKind tab={tab} run={run} />}
              label={<TabLabel tab={tab} run={run} />}
              labelTestId={`tab-title-${tab.id}`}
              drag={drag.props("doc", tab.id, index)}
              onActivate={() => {
                showPage(null);
                setActiveTab(tab.id);
              }}
              onClose={() => closeTab(tab.id)}
              onContextMenu={(e) => {
                e.preventDefault();
                setMenu({ x: e.clientX, y: e.clientY, tabId: tab.id });
              }}
            />
          );
        })}
        <PageTabButtons drag={drag} />
      </div>
      {overflowing && <OpenTabsMenu tabs={openTabs} runsById={runsById} />}
      <span className="sh-tabs__fill" />

      <div className="sh-tabs__actions">
        {activePage != null ? (
          <PageTabActions name={activePage} />
        ) : (
          <>
            {/* Unsaved changes make « Saved … ago » stale: the line under the tabs says them. */}
            {savedAgo && !activeTab?.dirty && (
              <span className="sh-tabs__saved" data-testid="saved-ago">
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
                <Tooltip label="Validate" description="Projects this pipeline is offered to">
                  <Button
                    size="sm"
                    icon={BadgeCheck}
                    onClick={() => setValidateOpen(true)}
                    data-testid="validate-button"
                  >
                    Validate
                  </Button>
                </Tooltip>
              </>
            )}
            {activeTab?.runId != null && !activeLocked ? (
              <RunSaveGroup tab={{ ...activeTab, runId: activeTab.runId }} />
            ) : activeLocked ? null : (
              <Tooltip label={activeDraft ? "Save…" : "Save"} kbd="Ctrl+S">
                <Button
                  size="sm"
                  tone="primary"
                  icon={Save}
                  onClick={() => { if (activeTabId) requestSave(activeTabId); }}
                  disabled={activeDraft ? !activeTab?.dirty : !anyDirty}
                  data-testid="save-button"
                >
                  {activeDraft ? "Save…" : "Save"}
                </Button>
              </Tooltip>
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
            const result = await save(saveVersionTab.id, saveAs);
            closeSaveVersion();
            return result;
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
