import { useState, useRef, useEffect } from "react";
import { Star } from "lucide-react";
import { useEditStore } from "../stores/editStore";
import type { NodeDef, NodeType, PortDef } from "../types";
import { SectionHead, Field } from "./InspectorPrimitives";
import OutputPortCard from "./OutputPortCard";
import PooledInputRow from "./PooledInputRow";
import ModelPicker from "./ModelPicker";
import DestroyLoopModal from "./DestroyLoopModal";
import { derivePooledInputs } from "../lib/derivePooledInputs";
import { regionsDestroyedByEdgeRemoval } from "../lib/loopRegions";
import { Tooltip } from "./ui/tooltip";
import type { LibraryEntry } from "../api";
import { saveToLibrary, deleteFromLibrary, instantiateFromLibrary, libraryPortToPortDef } from "../api";
import { useLibraryState } from "../hooks/useLibrary";
import type { LibrarySyncState } from "../hooks/useLibrary";

const TYPE_TOOLTIPS: Record<string, string> = {
  "code-mutating": "Receives a forked sub-worktree. Can edit, commit, and merge code.",
  "doc-only": "Reads code in read-only. Only writes Markdown artifacts to the Blackboard.",
};

export default function NodeInspector({
  libraryEntries,
  onLibraryChanged,
  readOnly,
}: {
  libraryEntries: LibraryEntry[];
  onLibraryChanged: () => void;
  /** #339: hides the per-source input × — set for archived runs only
   * (mirrors the canvas readOnly, #315/ADR-0020). Scoped to the × alone;
   * the rest of the inspector's archived story is the #315 gap. */
  readOnly?: boolean;
}) {
  const openTabs = useEditStore((s) => s.openTabs);
  const activeTabId = useEditStore((s) => s.activeTabId);
  const selection = useEditStore((s) => s.selection);
  const updateNode = useEditStore((s) => s.updateNode);
  const updatePrompt = useEditStore((s) => s.updatePrompt);
  const deleteEdge = useEditStore((s) => s.deleteEdge);
  const scrollToPort = useEditStore((s) => s.scrollToPort);
  const setScrollToPort = useEditStore((s) => s.setScrollToPort);

  const asideRef = useRef<HTMLElement>(null);
  const [highlightedPort, setHighlightedPort] = useState<string | null>(null);
  // Pending destroy-loop confirmation (#339, mirrors EditCanvas #150): set when
  // deleting an input source would remove a bounded region's last cycle.
  const [pendingDestroy, setPendingDestroy] = useState<{
    edgeIndex: number;
    loopIds: string[];
  } | null>(null);

  useEffect(() => {
    if (!scrollToPort) return;
    const escaped = CSS.escape(scrollToPort);
    const el = asideRef.current?.querySelector(`[data-port="${escaped}"]`);
    if (el) {
      el.scrollIntoView({ behavior: "smooth", block: "nearest" });
      setHighlightedPort(scrollToPort);
      const timer = setTimeout(() => setHighlightedPort(null), 1500);
      setScrollToPort(null);
      return () => clearTimeout(timer);
    }
    setScrollToPort(null);
  }, [scrollToPort, setScrollToPort]);

  const tab = openTabs.find((t) => t.id === activeTabId);
  const node = tab && selection.kind === "node" && selection.id
    ? tab.pipeline.nodes.find((n) => n.id === selection.id) ?? null
    : null;
  const promptContent = node ? (tab!.prompts[node.id] ?? "") : "";
  const syncState = useLibraryState(node, promptContent, libraryEntries);

  if (!tab || !node) return null;

  // Inputs are emergent (#149): derived from the pipeline's incoming edges,
  // not declared on the node. Same-named edges pool into one list input.
  const pooledInputs = derivePooledInputs(tab.pipeline, node.id);

  // #248: a `script` node runs deterministic bash, not an agent — so its type is
  // fixed (no doc-only↔code-mutating toggle), it has no model, and its "prompt"
  // is a bash body whose I/O arrives as PDO_* env vars, not a prose preamble.
  const isScript = node.type === "script";

  // #339: delete one contributing edge of a pooled input — the canonical
  // "delete an input" since inputs are emergent (#149/ADR-0011). Last-cycle
  // deletions go through the same destroy-loop confirmation as the canvas;
  // `keepSelection` keeps the inspector open on this node.
  function handleDeleteSource(edgeIndex: number) {
    const destroyed = regionsDestroyedByEdgeRemoval(tab!.pipeline, edgeIndex);
    if (destroyed.length > 0) {
      setPendingDestroy({ edgeIndex, loopIds: destroyed });
    } else {
      deleteEdge(edgeIndex, { keepSelection: true });
    }
  }

  function handleField(field: keyof NodeDef, value: unknown) {
    updateNode(node!.id, { [field]: value } as Partial<NodeDef>);
  }

  function handleAddOutput() {
    const ports = [...node!.outputs];
    let name = "out";
    let counter = 1;
    while (ports.some((p) => p.name === name)) {
      name = `out-${++counter}`;
    }
    ports.push({ name, repeated: false, side: "right" });
    updateNode(node!.id, { outputs: ports });
  }

  function handleUpdateOutput(index: number, updates: Partial<PortDef>) {
    const ports = node!.outputs.map((p, i) => (i === index ? { ...p, ...updates } : p));
    updateNode(node!.id, { outputs: ports });
  }

  function handleRemoveOutput(index: number) {
    const ports = node!.outputs.filter((_, i) => i !== index);
    updateNode(node!.id, { outputs: ports });
  }

  return (
    <aside ref={asideRef} className="flex h-full flex-col bg-bg-2 overflow-y-auto">
      <div
        className="flex h-[36px] items-center justify-between border-b border-line px-3 font-medium text-fg-2"
        style={{ fontSize: "11.5px" }}
      >
        <span>Node Inspector</span>
        <StarButton
          syncState={syncState}
          node={node}
          prompt={promptContent}
          onLibraryChanged={onLibraryChanged}
          updateNodeFn={(updates) => updateNode(node.id, updates)}
          updatePromptFn={(content) => updatePrompt(node.id, content)}
        />
      </div>

      <div className="flex flex-col gap-3 p-3" style={{ fontSize: "11.5px" }}>
        {/* Identity */}
        <SectionHead title="Identity" />
        <Field label="ID">
          <span
            className="block w-full cursor-pointer select-all rounded border border-line bg-bg-3 px-2 py-1 font-mono text-fg-3"
            style={{ fontSize: "10px" }}
            title="Click to copy"
            onClick={() => navigator.clipboard.writeText(node.id)}
          >
            {node.id}
          </span>
        </Field>
        <Field label="Name">
          <NameInput
            key={node.id}
            value={node.name ?? ""}
            placeholder={node.id}
            onCommit={(v) => handleField("name", v || null)}
          />
        </Field>

        {/* Type */}
        <SectionHead title="Type" />
        {isScript ? (
          // #248: a script node's type is fixed. Show a static label rather than
          // the doc-only↔code-mutating toggle (which can't even express "script"
          // and would silently retype the node away on click).
          <div
            data-testid="script-type-label"
            className="rounded border border-fg-4 bg-bg-3 px-2 py-1 font-medium text-fg"
            style={{ fontSize: "10px" }}
          >
            script (deterministic bash)
          </div>
        ) : (
          <div className="flex gap-1">
            {(["code-mutating", "doc-only"] as NodeType[]).map((t) => (
              <Tooltip key={t} content={TYPE_TOOLTIPS[t] ?? t}>
                <button
                  onClick={() => handleField("type", t)}
                  className={`flex-1 cursor-pointer rounded border px-2 py-1 font-medium transition-colors ${
                    node.type === t
                      ? t === "code-mutating"
                        ? "border-acc bg-acc-bg text-acc"
                        : "border-fg-4 bg-bg-3 text-fg"
                      : "border-line-strong bg-bg-3 text-fg-4 hover:text-fg-3"
                  }`}
                  style={{ fontSize: "10px" }}
                >
                  {t}
                </button>
              </Tooltip>
            ))}
          </div>
        )}

        {/* Behavior */}
        <SectionHead title="Behavior" />
        <Tooltip content="Pauses for human interaction. The node never auto-completes — mark complete from the run-mode UI." side="left">
          <div className="flex items-center justify-between">
            <span className="text-fg-3">Interactive</span>
            <button
              onClick={() => handleField("interactive", !node.interactive)}
              className={`relative h-5 w-9 cursor-pointer rounded-full transition-colors ${
                node.interactive ? "bg-acc" : "bg-bg-5"
              }`}
            >
              <span
                className={`absolute top-0.5 left-0.5 h-4 w-4 rounded-full bg-fg transition-transform ${
                  node.interactive ? "translate-x-[16px]" : "translate-x-0"
                }`}
              />
            </button>
          </div>
        </Tooltip>

        {/* Model (#296/#324): dropdown + Custom… escape hatch (see ModelPicker).
            Hidden for a script node (#248): it launches no agent, so it has no
            model. */}
        {!isScript && (
          <Field label="Model">
            <ModelPicker
              value={node.model ?? null}
              onChange={(v) => handleField("model", v)}
              testid="node-model"
            />
          </Field>
        )}

        {/* Prompt / Script body. For a script node (#248) this textarea holds the
            bash body; its I/O arrives as PDO_* env vars, not a prose preamble. */}
        <SectionHead title={isScript ? "Script (bash)" : "Prompt"} />
        {isScript && (
          <p
            data-testid="script-help"
            className="rounded border border-acc/30 bg-acc/5 px-2 py-1.5 text-fg-3"
            style={{ fontSize: "10.5px", lineHeight: "1.5" }}
          >
            Author-written bash, run deterministically (no LLM). Inputs/outputs
            arrive as env vars: <code>$PDO_INPUT_&lt;PORT&gt;</code>,{" "}
            <code>$PDO_OUTPUT_&lt;PORT&gt;</code>, <code>$PDO_ARTIFACTS_DIR</code>,{" "}
            <code>$PDO_VAR_&lt;NAME&gt;</code>. Write your <code>output.md</code> to{" "}
            <code>$PDO_OUTPUT_&lt;PORT&gt;</code> (add YAML frontmatter to drive a{" "}
            <code>when:</code> edge). Exit 0 ⇒ node completes; non-zero or timeout
            ⇒ fails. Runs in the run's shared worktree — leave tracked files clean.
          </p>
        )}
        <textarea
          data-testid={isScript ? "script-body" : undefined}
          value={promptContent}
          onChange={(e) => updatePrompt(node.id, e.target.value)}
          className="min-h-[120px] w-full resize-y rounded border border-line-strong bg-bg-3 px-2 py-1.5 font-mono text-fg outline-none focus:border-acc"
          style={{ fontSize: "11px", lineHeight: "1.5" }}
          placeholder={isScript ? "#!/usr/bin/env bash\n# e.g. curl -X POST \"$DISCORD_WEBHOOK\" ..." : "Enter the node's role prompt..."}
        />

        {/* Inputs — emergent (#149): derived from incoming edges, read-only.
            Same-named edges pool into one logical list input that spells out
            every contributing source node (e.g. `review ← sec-reviewer,
            perf-reviewer`). The node declares no inputs. */}
        <SectionHead title="Inputs" count={pooledInputs.length} />
        {pooledInputs.length === 0 ? (
          <p className="px-1 py-1 text-fg-4" style={{ fontSize: "10px" }}>
            No inputs — wire an output into this node to create one.
          </p>
        ) : (
          <div className="flex flex-col">
            {pooledInputs.map((input, i) => (
              <PooledInputRow
                key={input.name}
                input={input}
                highlighted={highlightedPort === input.name}
                isLast={i === pooledInputs.length - 1}
                onDeleteSource={readOnly ? undefined : handleDeleteSource}
              />
            ))}
          </div>
        )}

        {/* Outputs — declared: the node's production contract (CONTEXT.md § Node). */}
        <SectionHead title="Outputs" count={node.outputs.length} onAdd={handleAddOutput} />
        <div className="flex flex-col">
          {node.outputs.map((port, i) => (
            <OutputPortCard
              key={i}
              port={port}
              highlighted={highlightedPort === port.name}
              onUpdate={(updates) => handleUpdateOutput(i, updates)}
              onRemove={() => handleRemoveOutput(i)}
              schema={port.frontmatter}
              onSchemaChange={(fm) => handleUpdateOutput(i, { frontmatter: fm ?? null })}
            />
          ))}
        </div>
      </div>

      <DestroyLoopModal
        open={pendingDestroy != null}
        loopIds={pendingDestroy?.loopIds ?? []}
        onClose={() => setPendingDestroy(null)}
        onConfirm={() => {
          if (pendingDestroy) deleteEdge(pendingDestroy.edgeIndex, { keepSelection: true });
          setPendingDestroy(null);
        }}
      />
    </aside>
  );
}

function NameInput({
  value,
  placeholder,
  onCommit,
}: {
  value: string;
  placeholder: string;
  onCommit: (v: string) => void;
}) {
  const [draft, setDraft] = useState(value);
  const escaping = useRef(false);

  return (
    <input
      value={draft}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={() => {
        if (escaping.current) {
          escaping.current = false;
          setDraft(value);
        } else {
          onCommit(draft);
        }
      }}
      onKeyDown={(e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          onCommit(draft);
          (e.target as HTMLInputElement).blur();
        } else if (e.key === "Escape") {
          e.preventDefault();
          escaping.current = true;
          (e.target as HTMLInputElement).blur();
        }
      }}
      className="w-full rounded border border-line-strong bg-bg-3 px-2 py-1 text-fg outline-none focus:border-acc"
      placeholder={placeholder}
    />
  );
}

const STAR_TOOLTIPS: Record<LibrarySyncState, string> = {
  outline: "Save to library",
  synced: "In your library — synced",
  diverged: "In your library — out of sync",
};

function StarButton({
  syncState,
  node,
  prompt,
  onLibraryChanged,
  updateNodeFn,
  updatePromptFn,
}: {
  syncState: LibrarySyncState;
  node: NodeDef;
  prompt: string;
  onLibraryChanged: () => void;
  updateNodeFn: (updates: Partial<NodeDef>) => void;
  updatePromptFn: (content: string) => void;
}) {
  const [popoverOpen, setPopoverOpen] = useState(false);
  const popoverRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!popoverOpen) return;
    function handleClickOutside(e: MouseEvent) {
      if (popoverRef.current && !popoverRef.current.contains(e.target as Node)) {
        setPopoverOpen(false);
      }
    }
    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, [popoverOpen]);

  function librarySpec() {
    return {
      name: node.name ?? "",
      type: node.type,
      inputs: node.inputs.map((p) => ({
        name: p.name,
        repeated: p.repeated,
        side: p.side,
        ...(p.port_type && p.port_type !== "markdown" ? { port_type: p.port_type } : {}),
        ...(p.frontmatter ? { frontmatter: p.frontmatter } : {}),
        ...(p.when ? { when: p.when } : {}),
      })),
      outputs: node.outputs.map((p) => ({
        name: p.name,
        repeated: p.repeated,
        side: p.side,
        ...(p.port_type && p.port_type !== "markdown" ? { port_type: p.port_type } : {}),
        ...(p.frontmatter ? { frontmatter: p.frontmatter } : {}),
        ...(p.when ? { when: p.when } : {}),
      })),
      interactive: node.interactive,
      // #296/#345: persist the per-node model so the library is model-aware and
      // a modelled node stays synced instead of flipping to diverged.
      model: node.model ?? null,
      prompt,
    };
  }

  async function handleStarClick() {
    if (syncState === "outline") {
      try {
        await saveToLibrary(librarySpec());
        onLibraryChanged();
      } catch {
        // ignore
      }
    } else {
      setPopoverOpen(!popoverOpen);
    }
  }

  async function handleUpdateLibrary() {
    try {
      await saveToLibrary(librarySpec());
      onLibraryChanged();
      setPopoverOpen(false);
    } catch {
      // ignore
    }
  }

  async function handleResetFromLibrary() {
    try {
      const result = await instantiateFromLibrary(node.name ?? "");
      updateNodeFn({
        name: result.spec.name,
        type: result.spec.type as NodeType,
        inputs: result.spec.inputs.map((p) => libraryPortToPortDef(p, "left")),
        outputs: result.spec.outputs.map((p) => libraryPortToPortDef(p, "right")),
        interactive: result.spec.interactive,
        // #296/#345: reset the per-node model from the library entry too.
        model: result.spec.model ?? null,
      });
      updatePromptFn(result.prompt);
      setPopoverOpen(false);
    } catch {
      // ignore
    }
  }

  async function handleRemoveFromLibrary() {
    try {
      await deleteFromLibrary(node.name ?? "");
      onLibraryChanged();
      setPopoverOpen(false);
    } catch {
      // ignore
    }
  }

  const isFilled = syncState !== "outline";
  const tooltip = STAR_TOOLTIPS[syncState];

  return (
    <div className="relative" ref={popoverRef}>
      <button
        onClick={handleStarClick}
        className="grid h-6 w-6 place-items-center rounded transition-colors hover:bg-bg-3"
        title={tooltip}
      >
        <span className="relative">
          <Star
            size={14}
            className={
              isFilled ? "fill-acc text-acc" : "fill-none text-fg-4"
            }
          />
          {syncState === "diverged" && (
            <span
              className="absolute -bottom-0.5 -right-0.5 h-1.5 w-1.5 rounded-full bg-st-blocked"
            />
          )}
        </span>
      </button>

      {popoverOpen && (
        <div
          className="absolute right-0 top-full z-50 mt-1 w-[200px] rounded-lg border border-line bg-bg-4 py-1 shadow-lg"
          style={{ fontSize: "11px" }}
        >
          {syncState === "synced" && (
            <button
              onClick={handleRemoveFromLibrary}
              className="w-full px-3 py-1.5 text-left text-fg-3 transition-colors hover:bg-bg-3 hover:text-fg"
            >
              Remove from library
            </button>
          )}
          {syncState === "diverged" && (
            <>
              <button
                onClick={handleUpdateLibrary}
                className="w-full px-3 py-1.5 text-left text-fg-3 transition-colors hover:bg-bg-3 hover:text-fg"
              >
                Update library entry
              </button>
              <button
                onClick={handleResetFromLibrary}
                className="w-full px-3 py-1.5 text-left text-fg-3 transition-colors hover:bg-bg-3 hover:text-fg"
              >
                Reset from library
              </button>
              <button
                onClick={handleRemoveFromLibrary}
                className="w-full px-3 py-1.5 text-left text-fg-3 transition-colors hover:bg-bg-3 hover:text-fg"
              >
                Remove from library
              </button>
            </>
          )}
        </div>
      )}
    </div>
  );
}
