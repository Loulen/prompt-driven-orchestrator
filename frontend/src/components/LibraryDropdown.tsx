import { useState, useRef, useEffect } from "react";
import { BookOpen, Plus, Trash2 } from "lucide-react";
import type { LibraryEntry } from "../api";
import { instantiateFromLibrary, libraryPortToPortDef } from "../api";
import { useEditStore } from "../stores/editStore";
import { generateNodeId } from "../lib/nanoid";
import type { NodeDef, NodeType } from "../types";
import { Tooltip } from "./ui/tooltip";

const TYPE_ICONS: Record<string, string> = {
  "code-mutating": "CM",
  "doc-only": "DO",
};

export default function LibraryDropdown({
  entries,
  onDelete,
}: {
  entries: LibraryEntry[];
  onDelete: (name: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [search, setSearch] = useState("");
  const ref = useRef<HTMLDivElement>(null);
  const addNode = useEditStore((s) => s.addNode);
  const updatePrompt = useEditStore((s) => s.updatePrompt);

  useEffect(() => {
    if (!open) return;
    function handleClick(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpen(false);
      }
    }
    document.addEventListener("mousedown", handleClick);
    return () => document.removeEventListener("mousedown", handleClick);
  }, [open]);

  const filtered = entries.filter((e) =>
    e.name.toLowerCase().includes(search.toLowerCase()),
  );

  async function handleAdd(name: string) {
    try {
      const result = await instantiateFromLibrary(name);
      const newId = generateNodeId();
      const node: NodeDef = {
        id: newId,
        name: result.spec.name,
        type: result.spec.type as NodeType,
        inputs: result.spec.inputs.map((p) => libraryPortToPortDef(p, "left")),
        outputs: result.spec.outputs.map((p) => libraryPortToPortDef(p, "right")),
        interactive: result.spec.interactive,
        view: { x: 200, y: 200 },
      };
      addNode(node);
      updatePrompt(newId, result.prompt);
      setOpen(false);
    } catch {
      // ignore
    }
  }

  return (
    <div ref={ref} className="relative">
      <Tooltip content="Library · L">
        <button
          data-testid="toolbar-library"
          onClick={() => setOpen(!open)}
          className="grid h-7 w-7 place-items-center rounded-md border border-transparent bg-transparent text-fg-3 transition-colors hover:bg-bg-3 hover:text-fg"
        >
          <BookOpen size={14} />
        </button>
      </Tooltip>

      {open && (
        <div
          className="absolute left-0 top-full z-50 mt-1 flex w-[280px] flex-col overflow-hidden rounded-lg border border-line bg-bg-4 shadow-lg"
          style={{ maxHeight: "60vh" }}
        >
          <div className="flex items-center justify-between border-b border-line px-3 py-2">
            <span className="font-medium text-fg" style={{ fontSize: "12px" }}>
              Library
            </span>
            <span className="text-fg-4" style={{ fontSize: "10px" }}>
              {entries.length} {entries.length === 1 ? "entry" : "entries"}
            </span>
          </div>

          <div className="border-b border-line px-2 py-1.5">
            <input
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Search nodes..."
              className="w-full rounded border border-line-strong bg-bg-3 px-2 py-1 text-fg outline-none focus:border-acc"
              style={{ fontSize: "11px" }}
              autoFocus
            />
          </div>

          <div className="flex-1 overflow-y-auto">
            {filtered.length === 0 && entries.length === 0 && (
              <div className="px-3 py-6 text-center text-fg-4" style={{ fontSize: "11px" }}>
                No saved nodes yet. Star a node in the inspector to add it here.
              </div>
            )}
            {filtered.length === 0 && entries.length > 0 && (
              <div className="px-3 py-4 text-center text-fg-4" style={{ fontSize: "11px" }}>
                No matches.
              </div>
            )}
            {filtered.map((entry) => (
              <LibraryRow
                key={entry.name}
                entry={entry}
                onAdd={() => handleAdd(entry.name)}
                onDelete={() => onDelete(entry.name)}
              />
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

function LibraryRow({
  entry,
  onAdd,
  onDelete,
}: {
  entry: LibraryEntry;
  onAdd: () => void;
  onDelete: () => void;
}) {
  const [hovered, setHovered] = useState(false);
  const preview = entry.prompt.slice(0, 60).replace(/\n/g, " ");

  return (
    <div
      className="group flex items-center gap-2 px-3 py-1.5 transition-colors hover:bg-bg-3"
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      <span
        className="shrink-0 rounded bg-bg-5 px-1 py-0.5 font-mono text-fg-4"
        style={{ fontSize: "8px" }}
      >
        {TYPE_ICONS[entry.type] ?? "??"}
      </span>
      <div className="min-w-0 flex-1">
        <div className="truncate font-medium text-fg" style={{ fontSize: "11px" }}>
          {entry.name}
        </div>
        <div className="truncate text-fg-4" style={{ fontSize: "10px" }}>
          {preview || "No prompt"}
        </div>
      </div>
      {hovered && (
        <div className="flex shrink-0 items-center gap-1">
          <button
            onClick={(e) => {
              e.stopPropagation();
              onAdd();
            }}
            className="rounded p-0.5 text-acc hover:bg-acc/10"
            title="Add to canvas"
          >
            <Plus size={12} />
          </button>
          <button
            onClick={(e) => {
              e.stopPropagation();
              onDelete();
            }}
            className="rounded p-0.5 text-fg-4 hover:text-st-failed"
            title="Remove from library"
          >
            <Trash2 size={12} />
          </button>
        </div>
      )}
    </div>
  );
}
