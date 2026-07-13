import { useCallback, useEffect, useRef, useState } from "react";
import { Save, X } from "lucide-react";
import { useEditStore, hasUnsavedWork } from "../stores/editStore";
import type { OpenPipeline } from "../stores/editStore";
import ConfirmCloseTabsModal from "./ConfirmCloseTabsModal";

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
        <button
          onClick={() => { if (activeTabId) save(activeTabId); }}
          disabled={!anyDirty}
          className="flex cursor-pointer items-center gap-1 rounded-md bg-acc px-2 py-0.5 font-medium text-[#04140d] transition-colors hover:bg-acc-dim disabled:opacity-40"
          style={{ fontSize: "11px" }}
          data-testid="save-button"
        >
          <Save size={11} />
          Save
        </button>
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
