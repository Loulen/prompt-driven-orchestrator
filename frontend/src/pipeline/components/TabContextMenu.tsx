import { useEffect, useMemo, type ReactNode } from "react";
import type { OpenPipeline } from "../store/editState";
import { ExtensionMenuGroups } from "../../shared/extensions/ExtensionActions";
import type { PipelineEntity } from "../../shared/extensions/contract";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem } from "../../shared/ui/dropdown-menu";

/**
 * Right-click menu for a tab (#342), on the shared `DropdownMenu` (#30),
 * anchored where the pointer was — viewport coords, so it survives a scroll of
 * the tab strip. Dismisses on a backdrop click OR Escape. After the four close
 * entries, the extensions' `tab/menu` groups, called with the tab's pipeline.
 */
export default function TabContextMenu({
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

const ITEM = "cursor-pointer px-2 py-1.5 text-sm text-text-2";

function MenuItem({
  testid,
  disabled,
  onClick,
  children,
}: {
  testid: string;
  disabled?: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  // A disabled entry stays focusable for the arrow keys, `aria-disabled` (the
  // menu pattern of WAI-ARIA), and does nothing when clicked.
  return (
    <DropdownMenuItem data-testid={testid} disabled={disabled} onClick={onClick} className={ITEM}>
      {children}
    </DropdownMenuItem>
  );
}
