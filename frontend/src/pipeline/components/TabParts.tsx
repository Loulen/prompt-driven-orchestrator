import { ChevronDown, FileCode2 } from "lucide-react";
import { useEditStore } from "../store/editStore";
import type { OpenPipeline } from "../store/editState";
import { useMountedPages } from "../../platform/store/mountedPages";
import type { RunListEntry } from "../../shared/generated/RunListEntry";
import { IconButton, Menu, StatusIcon } from "../../shared/ui/kit";
import { tabTitle } from "./tabTitle";

/** A Run's status, or the file icon of a pipeline. */
export function TabKind({ tab, run }: { tab: OpenPipeline; run: RunListEntry | undefined }) {
  if (tab.runId != null && run) {
    return <StatusIcon kind="run" status={run.stalled && run.status === "running" ? "stalled" : run.status} size={13} />;
  }
  return <FileCode2 size={13} strokeWidth={1.5} className="sh-tab__kind" aria-hidden />;
}

/** The label: a pipeline's name with its `.yaml` dimmed. */
export function TabLabel({ tab, run }: { tab: OpenPipeline; run: RunListEntry | undefined }) {
  if (tab.runId != null) return <>{tabTitle(tab, run)}</>;
  if (tab.version) {
    return (
      <>
        {tab.version.pipelineId}
        <span className="sh-tab__ext">.yaml @ v{tab.version.number}</span>
      </>
    );
  }
  return (
    <>
      {tab.id}
      <span className="sh-tab__ext">.yaml</span>
    </>
  );
}

/** When the tabs overflow: a chevron whose menu lists them all, the active one ticked. */
export function OpenTabsMenu({ tabs, runsById }: { tabs: OpenPipeline[]; runsById: Map<string, RunListEntry> }) {
  const activeTabId = useEditStore((s) => s.activeTabId);
  const setActiveTab = useEditStore((s) => s.setActiveTab);
  const pages = useMountedPages((s) => s.tabs);
  const activePage = useMountedPages((s) => s.active);
  const showPage = useMountedPages((s) => s.showPage);
  return (
    <span className="sh-tabs__more">
      <Menu
        label="Open tabs"
        align="end"
        trigger={({ open, toggle }) => (
          <IconButton icon={ChevronDown} label="Open tabs" pressed={open} onClick={toggle} data-testid="tab-overflow" />
        )}
        entries={[
          ...tabs.map((tab) => ({
            label: tabTitle(tab, tab.runId != null ? runsById.get(tab.runId) : undefined),
            checked: activePage == null && tab.id === activeTabId,
            onSelect: () => {
              showPage(null);
              setActiveTab(tab.id);
            },
          })),
          ...pages.map((page) => ({
            label: page.name,
            checked: page.name === activePage,
            onSelect: () => showPage(page.name),
          })),
        ]}
      />
    </span>
  );
}
