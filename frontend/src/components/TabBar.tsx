import { X } from "lucide-react";
import { useEditStore } from "../stores/editStore";

export default function TabBar() {
  const openTabs = useEditStore((s) => s.openTabs);
  const activeTabId = useEditStore((s) => s.activeTabId);
  const setActiveTab = useEditStore((s) => s.setActiveTab);
  const closeTab = useEditStore((s) => s.closeTab);

  if (openTabs.length === 0) return null;

  return (
    <div className="flex h-[30px] shrink-0 items-end gap-px border-b border-line bg-bg-2 px-1">
      {openTabs.map((tab) => {
        const isActive = tab.id === activeTabId;
        return (
          <button
            key={tab.id}
            onClick={() => setActiveTab(tab.id)}
            className={`group flex items-center gap-1.5 rounded-t-md border border-b-0 px-2.5 py-1 transition-colors ${
              isActive
                ? "border-line bg-bg-1 text-fg"
                : "border-transparent bg-bg-2 text-fg-3 hover:text-fg-2"
            }`}
            style={{ fontSize: "11px", maxWidth: 180 }}
          >
            <span className="truncate">{tab.id}.yaml</span>
            {tab.dirty && (
              <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-st-await" />
            )}
            {tab.externalDirty && (
              <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-st-blocked" />
            )}
            <span
              role="button"
              onClick={(e) => {
                e.stopPropagation();
                closeTab(tab.id);
              }}
              className="ml-auto hidden shrink-0 rounded p-0.5 text-fg-4 hover:bg-bg-3 hover:text-fg group-hover:inline-flex"
            >
              <X size={10} />
            </span>
          </button>
        );
      })}
    </div>
  );
}
