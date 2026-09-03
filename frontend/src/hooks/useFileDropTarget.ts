import { useCallback, useEffect, useRef, useState } from "react";
import { draggedFileCount, isFileDrag } from "../lib/skillFiles";

/**
 * Whole-surface drop target (#671 design 02): from `dragenter` anywhere on the
 * container, an overlay covers it and says how many files are coming; the drop
 * lands on the whole surface, not on the small bar. Returns the handlers to
 * spread on the container plus the overlay state.
 *
 * While mounted, `dragover`/`drop` are `preventDefault`ed at the window level:
 * without it a file released next to the zone navigates the tab to the file and
 * loses the session. Only file drags are claimed (the tree's own row drags keep
 * their `DataTransfer` types and are ignored here).
 */
export function useFileDropTarget(onDrop: (dataTransfer: DataTransfer) => void, onEnter?: () => void) {
  const [dragging, setDragging] = useState<number | null>(null);
  const depth = useRef(0);

  useEffect(() => {
    const swallow = (event: DragEvent) => {
      if (isFileDrag(event.dataTransfer)) event.preventDefault();
    };
    window.addEventListener("dragover", swallow);
    window.addEventListener("drop", swallow);
    return () => {
      window.removeEventListener("dragover", swallow);
      window.removeEventListener("drop", swallow);
    };
  }, []);

  const onDragEnter = useCallback(
    (event: React.DragEvent) => {
      if (!isFileDrag(event.dataTransfer)) return;
      event.preventDefault();
      depth.current += 1;
      if (depth.current === 1) {
        setDragging(draggedFileCount(event.dataTransfer));
        onEnter?.();
      }
    },
    [onEnter],
  );
  const onDragOver = useCallback((event: React.DragEvent) => {
    if (!isFileDrag(event.dataTransfer)) return;
    event.preventDefault();
    event.dataTransfer.dropEffect = "copy";
  }, []);
  const onDragLeave = useCallback((event: React.DragEvent) => {
    if (!isFileDrag(event.dataTransfer)) return;
    depth.current = Math.max(0, depth.current - 1);
    if (depth.current === 0) setDragging(null);
  }, []);
  const handleDrop = useCallback(
    (event: React.DragEvent) => {
      if (!isFileDrag(event.dataTransfer)) return;
      event.preventDefault();
      depth.current = 0;
      setDragging(null);
      onDrop(event.dataTransfer);
    },
    [onDrop],
  );

  return {
    /** `null` when no file drag hovers the surface; else the file count. */
    dragging,
    handlers: { onDragEnter, onDragOver, onDragLeave, onDrop: handleDrop },
  };
}
