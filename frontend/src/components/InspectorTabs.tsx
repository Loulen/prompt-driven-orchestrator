import type { ReactNode } from "react";
import type { InspectorTab } from "../hooks/useInspectorTab";

interface Props {
  activeTab: InspectorTab;
  onTabChange: (tab: InspectorTab) => void;
  children: ReactNode;
}

export default function InspectorTabs({
  activeTab,
  onTabChange,
  children,
}: Props) {
  return (
    <div className="flex h-full flex-col bg-bg-2">
      <div className="flex shrink-0 border-b border-line">
        <TabButton
          active={activeTab === "run"}
          onClick={() => onTabChange("run")}
          label="Run"
          testId="inspector-tab-run"
        />
        <TabButton
          active={activeTab === "edit"}
          onClick={() => onTabChange("edit")}
          label="Edit"
          testId="inspector-tab-edit"
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
  testId,
}: {
  active: boolean;
  onClick: () => void;
  label: string;
  testId: string;
}) {
  return (
    <button
      data-testid={testId}
      data-active={active}
      onClick={onClick}
      className={`flex-1 cursor-pointer py-2 text-center transition-colors ${
        active
          ? "border-b-2 border-acc font-medium text-fg"
          : "text-fg-3 hover:text-fg-2"
      }`}
      style={{ fontSize: "11.5px" }}
    >
      {label}
    </button>
  );
}
