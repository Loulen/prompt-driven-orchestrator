import { useEffect, useState } from "react";
import { Plus } from "lucide-react";
import type { PipelineListEntry, PipelineScope } from "../types";
import { useEditStore } from "../stores/editStore";
import { createPipeline } from "../api";

const SCOPE_BADGE: Record<PipelineScope, { label: string; cls: string }> = {
  repo: { label: "repo", cls: "border-acc text-acc" },
  user: { label: "user", cls: "border-st-await text-st-await" },
};

type FilterChip = "all" | "repo" | "user";

export default function PipelinesListPanel() {
  const pipelines = useEditStore((s) => s.pipelines);
  const loadPipelines = useEditStore((s) => s.loadPipelines);
  const openPipeline = useEditStore((s) => s.openPipeline);
  const activeTabId = useEditStore((s) => s.activeTabId);
  const [filter, setFilter] = useState<FilterChip>("all");
  const [showNewModal, setShowNewModal] = useState(false);

  useEffect(() => {
    loadPipelines();
  }, [loadPipelines]);

  const filtered =
    filter === "all"
      ? pipelines
      : pipelines.filter((p) => p.scope === filter);

  return (
    <aside className="flex w-[220px] shrink-0 flex-col border-r border-line bg-bg-2">
      <div
        className="flex h-[36px] items-center justify-between border-b border-line px-3 font-medium text-fg-2"
        style={{ fontSize: "11.5px" }}
      >
        <span>Pipelines</span>
        <button
          onClick={() => setShowNewModal(true)}
          className="grid h-5 w-5 place-items-center rounded border border-line-strong bg-bg-3 text-fg-3 transition-colors hover:bg-bg-4 hover:text-fg"
          title="New pipeline"
        >
          <Plus size={12} />
        </button>
      </div>

      <div
        className="flex items-center gap-1 border-b border-line px-3 py-1.5"
        style={{ fontSize: "10px" }}
      >
        {(["all", "repo", "user"] as FilterChip[]).map((chip) => (
          <button
            key={chip}
            onClick={() => setFilter(chip)}
            className={`rounded px-1.5 py-0.5 font-medium transition-colors ${
              filter === chip
                ? "bg-bg-4 text-fg"
                : "text-fg-4 hover:text-fg-3"
            }`}
          >
            {chip.charAt(0).toUpperCase() + chip.slice(1)}
          </button>
        ))}
      </div>

      <div className="flex-1 overflow-y-auto">
        {filtered.length === 0 && (
          <div
            className="px-3 py-4 text-center text-fg-4"
            style={{ fontSize: "11px" }}
          >
            No pipelines found
          </div>
        )}
        {filtered.map((p) => (
          <PipelineRow
            key={`${p.scope}-${p.id}`}
            pipeline={p}
            isSelected={p.id === activeTabId}
            onSelect={() => openPipeline(p.id)}
          />
        ))}
      </div>

      {showNewModal && (
        <NewPipelineModal onClose={() => setShowNewModal(false)} />
      )}
    </aside>
  );
}

function PipelineRow({
  pipeline,
  isSelected,
  onSelect,
}: {
  pipeline: PipelineListEntry;
  isSelected: boolean;
  onSelect: () => void;
}) {
  const badge = SCOPE_BADGE[pipeline.scope];

  return (
    <button
      onClick={onSelect}
      className={`flex w-full items-center gap-2 border-b border-line-soft px-3 py-2 text-left transition-colors ${
        isSelected ? "bg-bg-3 text-fg" : "text-fg-2 hover:bg-bg-3/50"
      }`}
      style={{ fontSize: "11.5px" }}
    >
      <div className="min-w-0 flex-1">
        <div className="truncate font-medium">{pipeline.name}</div>
        <div
          className="mt-0.5 flex items-center gap-1.5 text-fg-4"
          style={{ fontSize: "10px" }}
        >
          <span>{pipeline.node_count} nodes</span>
        </div>
      </div>
      <span
        className={`shrink-0 rounded border px-1 py-px ${badge.cls}`}
        style={{ fontSize: "9px", fontWeight: 500 }}
      >
        {badge.label}
      </span>
    </button>
  );
}

function NewPipelineModal({ onClose }: { onClose: () => void }) {
  const [name, setName] = useState("");
  const [scope, setScope] = useState<PipelineScope>("repo");
  const loadPipelines = useEditStore((s) => s.loadPipelines);
  const openPipeline = useEditStore((s) => s.openPipeline);

  async function handleCreate() {
    if (!name.trim()) return;
    try {
      const result = await createPipeline(name.trim(), scope);
      await loadPipelines();
      await openPipeline(result.id);
      onClose();
    } catch {
      // ignore
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div
        className="w-[360px] rounded-lg border border-line bg-bg-4 p-4"
        style={{ fontSize: "12px" }}
      >
        <div className="mb-3 font-medium text-fg">New Pipeline</div>

        <label className="mb-1 block text-fg-3" style={{ fontSize: "11px" }}>
          Name
        </label>
        <input
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="my-pipeline"
          className="mb-3 w-full rounded border border-line-strong bg-bg-3 px-2 py-1.5 text-fg outline-none focus:border-acc"
          autoFocus
          onKeyDown={(e) => e.key === "Enter" && handleCreate()}
        />

        <label className="mb-1 block text-fg-3" style={{ fontSize: "11px" }}>
          Scope
        </label>
        <div className="mb-4 flex gap-1">
          {(["repo", "user"] as PipelineScope[]).map((s) => (
            <button
              key={s}
              onClick={() => setScope(s)}
              className={`rounded border px-3 py-1 font-medium transition-colors ${
                scope === s
                  ? "border-acc bg-acc-bg text-acc"
                  : "border-line-strong bg-bg-3 text-fg-3 hover:text-fg"
              }`}
              style={{ fontSize: "11px" }}
            >
              {s}
            </button>
          ))}
        </div>

        <div className="flex justify-end gap-2">
          <button
            onClick={onClose}
            className="rounded border border-line-strong bg-bg-3 px-3 py-1 text-fg-3 transition-colors hover:text-fg"
          >
            Cancel
          </button>
          <button
            onClick={handleCreate}
            disabled={!name.trim()}
            className="rounded bg-acc px-3 py-1 font-medium text-bg-0 transition-colors hover:bg-acc-dim disabled:opacity-50"
          >
            Create
          </button>
        </div>
      </div>
    </div>
  );
}
