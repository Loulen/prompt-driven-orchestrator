import { useEffect, useState } from "react";
import { Save, X } from "lucide-react";
import { useEditStore } from "../stores/editStore";

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
  const save = useEditStore((s) => s.save);
  const lastSavedAt = useEditStore((s) => s.lastSavedAt);

  const anyDirty = openTabs.some((t) => t.dirty);
  const activeLastSaved = activeTabId ? lastSavedAt[activeTabId] : undefined;
  const savedAgo = useRelativeTime(activeLastSaved);

  if (openTabs.length === 0) return null;

  return (
    <div className="flex h-[30px] shrink-0 items-end gap-px border-b border-line bg-bg-2 px-1">
      {openTabs.map((tab) => {
        const isActive = tab.id === activeTabId;
        const label = tab.dirty ? `• ${tab.id}.yaml` : `${tab.id}.yaml`;
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
              className="ml-auto hidden shrink-0 rounded p-0.5 text-fg-4 hover:bg-bg-3 hover:text-fg group-hover:inline-flex"
            >
              <X size={10} />
            </span>
          </button>
        );
      })}

      <div className="ml-auto flex items-center gap-2 px-1.5 pb-0.5">
        {savedAgo && (
          <span
            className="font-mono text-fg-4"
            style={{ fontSize: "10px" }}
            data-testid="saved-ago"
          >
            {savedAgo}
          </span>
        )}
        <button
          onClick={() => { if (activeTabId) save(activeTabId); }}
          disabled={!anyDirty}
          className="flex items-center gap-1 rounded-md bg-acc px-2 py-0.5 font-medium text-[#04140d] transition-colors hover:bg-acc-dim disabled:opacity-40"
          style={{ fontSize: "11px" }}
          data-testid="save-button"
        >
          <Save size={11} />
          Save
        </button>
      </div>
    </div>
  );
}
