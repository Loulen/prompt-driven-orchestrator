import { useState, type DragEvent } from "react";

/** Which side of a tab a dragged tab would land on. */
type Side = "before" | "after";

/** What a tab needs to be dragged along its group, from `useTabDrag`. */
export interface TabDragProps {
  draggable: true;
  dragging: boolean;
  drop: Side | null;
  onDragStart: (e: DragEvent<HTMLElement>) => void;
  onDragOver: (e: DragEvent<HTMLElement>) => void;
  onDragLeave: () => void;
  onDrop: (e: DragEvent<HTMLElement>) => void;
  onDragEnd: () => void;
}

export interface TabDrag {
  props: (group: string, id: string, index: number) => TabDragProps;
}

/**
 * Reordering by drag in the tab bar: a tab moves within its group (the Run
 * and pipeline tabs, the page tabs), dropped before or after the tab under
 * the pointer, a ruby edge marking where. `move(group, id, index)` applies it.
 */
export function useTabDrag(move: (group: string, id: string, index: number) => void): TabDrag {
  const [dragged, setDragged] = useState<{ group: string; id: string; index: number } | null>(null);
  const [target, setTarget] = useState<{ id: string; side: Side } | null>(null);
  const end = () => {
    setDragged(null);
    setTarget(null);
  };
  return {
    props: (group, id, index) => ({
      draggable: true,
      dragging: dragged?.id === id && dragged.group === group,
      drop: target?.id === id && dragged?.group === group && dragged.id !== id ? target.side : null,
      onDragStart: (e) => {
        e.dataTransfer.effectAllowed = "move";
        e.dataTransfer.setData("text/plain", id);
        setDragged({ group, id, index });
      },
      onDragOver: (e) => {
        if (!dragged || dragged.group !== group) return;
        e.preventDefault();
        e.dataTransfer.dropEffect = "move";
        const rect = e.currentTarget.getBoundingClientRect();
        const side: Side = e.clientX < rect.left + rect.width / 2 ? "before" : "after";
        if (target?.id !== id || target.side !== side) setTarget({ id, side });
      },
      onDragLeave: () => setTarget((t) => (t?.id === id ? null : t)),
      onDrop: (e) => {
        if (!dragged || dragged.group !== group) return;
        e.preventDefault();
        const rect = e.currentTarget.getBoundingClientRect();
        const after = e.clientX >= rect.left + rect.width / 2;
        // The index once the dragged tab has left its place.
        const to = (dragged.index < index ? index - 1 : index) + (after ? 1 : 0);
        if (dragged.id !== id) move(group, dragged.id, to);
        end();
      },
      onDragEnd: end,
    }),
  };
}
