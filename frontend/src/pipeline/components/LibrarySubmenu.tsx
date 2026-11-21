import { useRef, useState } from "react";
import type { KeyboardEvent, MouseEvent, PointerEvent, SyntheticEvent } from "react";
import { BookOpen, Library, Search, Trash2 } from "lucide-react";
import { EmptyState } from "../../shared/ui/kit";
import ListEmpty from "../../shared/ui/leftList/ListEmpty";
import {
  DropdownMenuItem,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
} from "../../shared/ui/dropdown-menu";
import type { LibraryEntry } from "../model/library";
import { instantiateFromLibrary } from "../api/library";
import { libraryPortToPortDef } from "../model/library";
import { useEditStore } from "../store/editStore";
import { generateNodeId } from "../../lib/nanoid";
import { resolveIsolation, workspaceLabel } from "../../lib/nodeIsolation";
import { NodeTypeIcon } from "./NodeTypeIcon";
import type { NodeType } from "../../shared/generated/NodeType";
import type { EditorNodeDef } from "../model/pipeline";

/**
 * CORE-194: the Library is a side submenu of the toolbar's `+` menu (`Library ▸`),
 * no longer an icon of its own. Clicking an entry adds it to the canvas and closes
 * the whole menu; its trash removes it from the library and leaves the menu open.
 * "Node from YAML…" lives one level up, in the same `+` menu.
 */
export default function LibrarySubmenu({
  entries,
  refused = null,
  onDelete,
  getDropPosition,
}: {
  entries: LibraryEntry[];
  /** CORE-248: why the caller may not read the library: the submenu says it instead. */
  refused?: string | null;
  onDelete: (name: string) => void;
  getDropPosition?: () => { x: number; y: number };
}) {
  const [query, setQuery] = useState("");
  const filterRef = useRef<HTMLInputElement>(null);
  const popupRef = useRef<HTMLDivElement>(null);
  // The height the submenu had before its list first changed (see `holdHeight`).
  const [heldHeight, setHeldHeight] = useState<number | null>(null);
  const addNode = useEditStore((s) => s.addNode);
  const updatePrompt = useEditStore((s) => s.updatePrompt);

  const filtered = entries.filter((e) => e.name.toLowerCase().includes(query.toLowerCase()));

  async function handleAdd(name: string) {
    try {
      const result = await instantiateFromLibrary(name);
      const newId = generateNodeId();
      const view = getDropPosition?.() ?? { x: 200, y: 200 };
      const node: EditorNodeDef = {
        id: newId,
        name: result.spec.name,
        type: result.spec.type as NodeType,
        inputs: result.spec.inputs.map((p) => libraryPortToPortDef(p)),
        outputs: result.spec.outputs.map((p) => libraryPortToPortDef(p)),
        interactive: result.spec.interactive,
        // #296/#345: the library is model-aware — restore the per-node model.
        model: result.spec.model ?? null,
        // #424: effort-aware too.
        effort: result.spec.effort ?? null,
        // #655/ADR-0060: and isolation-aware. Without this the dropped node
        // falls back to its type's default, so an Agent starred in the Run's
        // worktree would fork one of its own on every instantiation.
        isolated_worktree: result.spec.isolated_worktree ?? null,
        // ADR-0086: and on demand when the entry is.
        on_demand: result.spec.on_demand ?? false,
        // ADR-0089: and its live artifacts.
        live_artifacts: result.spec.live_artifacts ?? [],
        free_live_artifacts: result.spec.free_live_artifacts ?? false,
        view,
      };
      addNode(node);
      updatePrompt(newId, result.prompt);
    } catch {
      // ignore
    }
  }

  // FP CORE-194 F1: a list that shrinks under a resting pointer leaves the pointer
  // outside the submenu, and Base UI's hover closes it on the next keystroke. Once
  // the list starts changing (filter, delete), the submenu keeps the height it opened
  // with, so the pointer stays where it was: inside.
  const holdHeight = () => {
    if (heldHeight === null && popupRef.current) setHeldHeight(popupRef.current.offsetHeight);
  };
  const filter = (next: string) => {
    holdHeight();
    setQuery(next);
  };
  const remove = (name: string) => {
    holdHeight();
    onDelete(name);
  };

  // The filter lives inside a Base UI menu (same pattern as `PipelineSelect`).
  const onFilterKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    // List navigation lives on the popup: let these bubble to walk into the rows.
    if (e.key === "ArrowDown" || e.key === "ArrowUp" || e.key === "Home" || e.key === "End") return;
    if (e.key === "Enter") {
      // The fragment narrowed to what you meant: take it without a second gesture.
      e.preventDefault();
      e.stopPropagation();
      // Clicking the first row takes the very path of the mouse: add, close the menu.
      filterRef.current
        ?.closest("[data-testid=library-submenu]")
        ?.querySelector<HTMLElement>("[data-testid^=library-entry-]")
        ?.click();
      return;
    }
    if (e.key === "Escape" && query) {
      // Empty the filter first; the next Escape closes the menu.
      e.preventDefault();
      e.stopPropagation();
      filter("");
      return;
    }
    if (e.key === "Escape") return;
    // Everything else is text: Base UI's typeahead would swallow printable keys.
    e.stopPropagation();
  };

  // One filtering gesture: a letter typed anywhere in the submenu (a row, or the
  // popup itself once Base UI parked the focus there) goes to the field, never to
  // the typeahead. Captured, so neither the rows nor the popup see it.
  const onPopupKeyDownCapture = (e: KeyboardEvent<HTMLDivElement>) => {
    const field = filterRef.current;
    if (!field || e.target === field) return;
    if (e.key.length !== 1 || e.key === " " || e.ctrlKey || e.metaKey || e.altKey) return;
    e.preventDefault();
    e.stopPropagation();
    filter(query + e.key);
    field.focus();
  };

  // FP CORE-194 F2: filtering moves the rows under a resting pointer, and Chrome
  // replays the pointer over them without it moving. Base UI would read that as a
  // hover: highlight the row now under the pointer, or park the focus on the popup
  // when the row leaves it — either way the field loses the next letters. While
  // the field holds the focus, only a pointer that really moved takes it back.
  const fieldHoldsFocus = () => filterRef.current !== null && document.activeElement === filterRef.current;
  const onRowMouseMove = (e: MouseEvent<HTMLDivElement>) => {
    if (fieldHoldsFocus() && e.movementX === 0 && e.movementY === 0) preventBaseUIHandler(e);
  };
  const onRowPointerLeave = (e: PointerEvent<HTMLDivElement>) => {
    // A real move into another row highlights it through its own `mousemove`.
    if (fieldHoldsFocus()) preventBaseUIHandler(e);
  };

  // A row holds the focus once hovered or reached by the arrows: `↑` from the first
  // goes back to the filter, and Delete removes the entry from the keyboard.
  const onRowKeyDown = (e: KeyboardEvent<HTMLDivElement>, index: number, name: string) => {
    if (e.key === "ArrowUp" && index === 0 && filterRef.current) {
      // The field sits above the first row: `↑` goes there, not to the bottom.
      e.preventDefault();
      e.stopPropagation();
      filterRef.current.focus();
      return;
    }
    if (e.key === "Delete") {
      e.preventDefault();
      e.stopPropagation();
      remove(name);
    }
  };

  return (
    <DropdownMenuSub
      onOpenChange={(open) => {
        if (!open) return;
        setQuery("");
        setHeldHeight(null);
      }}
    >
      <DropdownMenuSubTrigger data-testid="add-menu-library" className="cv-menu__item">
        <BookOpen size={14} strokeWidth={1.5} aria-hidden />
        <span>Library</span>
      </DropdownMenuSubTrigger>
      <DropdownMenuSubContent
        ref={popupRef}
        data-testid="library-submenu"
        className="cv-menu cv-menu--library"
        style={heldHeight === null ? undefined : { minHeight: heldHeight }}
        onKeyDownCapture={onPopupKeyDownCapture}
      >
        <div className="flex h-8 shrink-0 items-center justify-between border-b border-line px-2">
          <span className="d-caps">Library</span>
          {refused === null && <span className="d-chip font-mono text-2xs">{entries.length}</span>}
        </div>

        {refused === null && (
          <div className="shrink-0 border-b border-line px-2 py-1">
            <input
              ref={filterRef}
              value={query}
              onChange={(e) => filter(e.target.value)}
              onKeyDown={onFilterKeyDown}
              placeholder="Search nodes"
              aria-label="Search nodes"
              className="d-input w-full"
              spellCheck={false}
            />
          </div>
        )}

        <div className="cv-menu--library__list">
          {/* CORE-248: a refused library shows its sentence alone, no gesture. */}
          {refused !== null && <ListEmpty icon={Library} title={refused} testId="library-refused" />}
          {refused === null && entries.length === 0 && (
            <EmptyState icon={BookOpen} layout="row">
              No saved node
            </EmptyState>
          )}
          {refused === null && filtered.length === 0 && entries.length > 0 && (
            <EmptyState icon={Search} layout="row">
              No match
            </EmptyState>
          )}
          {refused === null &&
            filtered.map((entry, index) => (
              <LibraryRow
                key={entry.name}
                entry={entry}
                onAdd={() => void handleAdd(entry.name)}
                onDelete={() => remove(entry.name)}
                onKeyDown={(e) => onRowKeyDown(e, index, entry.name)}
                onMouseMove={onRowMouseMove}
                onPointerLeave={onRowPointerLeave}
              />
            ))}
        </div>
      </DropdownMenuSubContent>
    </DropdownMenuSub>
  );
}

function LibraryRow({
  entry,
  onAdd,
  onDelete,
  onKeyDown,
  onMouseMove,
  onPointerLeave,
}: {
  entry: LibraryEntry;
  onAdd: () => void;
  onDelete: () => void;
  onKeyDown: (e: KeyboardEvent<HTMLDivElement>) => void;
  onMouseMove: (e: MouseEvent<HTMLDivElement>) => void;
  onPointerLeave: (e: PointerEvent<HTMLDivElement>) => void;
}) {
  const preview = entry.prompt.slice(0, 60).replace(/\n/g, " ");
  // #655/ADR-0060: the preview names the entry's workspace, so "which of my two
  // Reviewers is the one that shares the Run's tree?" is answered before the
  // drop rather than after it. `null` on the types that carry no isolation.
  const isolation = resolveIsolation(entry.type, entry.isolated_worktree);
  // The trash sits inside the row: its gestures must not reach the row, whose
  // click adds the entry and closes the menu.
  const keep = (e: SyntheticEvent) => {
    e.stopPropagation();
    e.preventDefault();
  };

  return (
    <DropdownMenuItem
      data-testid={`library-entry-${entry.name}`}
      className="cv-menu__item cv-menu__item--library"
      title="Add to canvas · Delete removes it from the library"
      onClick={onAdd}
      onKeyDown={onKeyDown}
      onMouseMove={onMouseMove}
      onPointerLeave={onPointerLeave}
    >
      {/* #655: the same glyph the canvas and the inspector use for the type. */}
      <span className="shrink-0 text-text-3">
        <NodeTypeIcon type={entry.type as NodeType} size={12} />
      </span>
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline gap-1.5">
          <span className="truncate text-md font-medium text-text" title={entry.name}>
            {entry.name}
          </span>
          {isolation !== null && (
            <span
              data-testid={`library-workspace-${isolation ? "isolated" : "shared"}`}
              className="shrink-0 font-mono text-2xs text-text-3"
            >
              {workspaceLabel(isolation)}
            </span>
          )}
        </div>
        <div className="truncate text-xs text-text-3">{preview || "No prompt"}</div>
      </div>
      <button
        type="button"
        aria-label="Remove from library"
        title="Remove from library"
        data-testid={`library-delete-${entry.name}`}
        className="d-icon cv-menu__trash"
        tabIndex={-1}
        onPointerDown={keep}
        onMouseDown={keep}
        onMouseUp={keep}
        onPointerUp={keep}
        onClick={(e) => {
          keep(e);
          onDelete();
        }}
      >
        <Trash2 size={12} strokeWidth={1.5} aria-hidden />
      </button>
    </DropdownMenuItem>
  );
}

/** Base UI's `mergeProps` runs our handler first and lets it skip the library's own. */
function preventBaseUIHandler(e: SyntheticEvent) {
  (e as SyntheticEvent & { preventBaseUIHandler?: () => void }).preventBaseUIHandler?.();
}
