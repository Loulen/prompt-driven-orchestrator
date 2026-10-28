import type { ReactNode } from "react";
import { Pencil, Play, SlidersHorizontal, type LucideIcon } from "lucide-react";
import type { InspectorTab } from "../../hooks/useInspectorTab";

interface Props {
  activeTab: InspectorTab;
  onTabChange: (tab: InspectorTab) => void;
  /** ADR-0085: a Run followed in « pilotage » (or archived): the second pane is
   *  the node's read-only « Config », and the tab says so. */
  configMode?: boolean;
  children: ReactNode;
}

export default function InspectorTabs({
  activeTab,
  onTabChange,
  configMode = false,
  children,
}: Props) {
  return (
    <div className="flex h-full flex-col bg-surface-1">
      <div className="flex h-8 shrink-0 border-b border-line" role="tablist" aria-label="Inspector">
        <TabButton
          active={activeTab === "run"}
          onClick={() => onTabChange("run")}
          label="Run"
          icon={Play}
          testId="inspector-tab-run"
        />
        <TabButton
          active={activeTab === "edit"}
          onClick={() => onTabChange("edit")}
          label={configMode ? "Config" : "Edit"}
          icon={configMode ? SlidersHorizontal : Pencil}
          testId={configMode ? "inspector-tab-config" : "inspector-tab-edit"}
        />
      </div>
      <div className="min-h-0 flex-1">{children}</div>
    </div>
  );
}

function TabButton({
  active,
  onClick,
  label,
  icon: Icon,
  testId,
}: {
  active: boolean;
  onClick: () => void;
  label: string;
  icon: LucideIcon;
  testId: string;
}) {
  return (
    <button
      data-testid={testId}
      data-active={active}
      role="tab"
      aria-selected={active}
      onClick={onClick}
      className={`-mb-px flex flex-1 cursor-pointer items-center justify-center gap-1.5 border-b-2 text-sm transition-colors ${
        active
          ? "border-primary font-medium text-text"
          : "border-transparent text-text-3 hover:text-text-2"
      }`}
    >
      <Icon size={13} strokeWidth={1.5} aria-hidden />
      {label}
    </button>
  );
}
