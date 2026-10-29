import { useState, type ReactNode } from "react";
import { BarSlot } from "./inspectorBarSlot";
import { Lock, Play, SlidersHorizontal, type LucideIcon } from "lucide-react";
import type { InspectorTab } from "../../hooks/useInspectorTab";
import "../../run/components/inspector.css";

interface Props {
  activeTab: InspectorTab;
  onTabChange: (tab: InspectorTab) => void;
  /** ADR-0085: a Run followed in « pilotage » (or archived): the second pane is
   *  the node's read-only « Config », and the tab says so. */
  configMode?: boolean;
  children: ReactNode;
}

/**
 * The Run / Edit (Config) tabs of a node in a Run: underlined, icon and label,
 * at the left of a 32 px bar whose right end carries the node's controls.
 */
export default function InspectorTabs({
  activeTab,
  onTabChange,
  configMode = false,
  children,
}: Props) {
  const [slot, setSlot] = useState<HTMLElement | null>(null);
  return (
    <div className="ri">
      <div className="ri-bar">
        <div className="ri-tabs" role="tablist" aria-label="Inspector">
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
            icon={configMode ? Lock : SlidersHorizontal}
            testId={configMode ? "inspector-tab-config" : "inspector-tab-edit"}
          />
        </div>
        <div className="ri-bar__actions" ref={setSlot} data-testid="inspector-bar-actions" />
      </div>
      <BarSlot.Provider value={{ element: slot, runActive: activeTab === "run" }}>
        <div className="min-h-0 flex-1">{children}</div>
      </BarSlot.Provider>
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
      type="button"
      data-testid={testId}
      data-active={active}
      role="tab"
      aria-selected={active}
      title={label}
      onClick={onClick}
      className="ri-tab"
    >
      <Icon size={13} strokeWidth={1.5} aria-hidden />
      <span className="ri-tab__label">{label}</span>
    </button>
  );
}
