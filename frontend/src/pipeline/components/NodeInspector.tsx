import { useState, useRef, useEffect, useMemo } from "react";
import { useEditStore } from "../store/editStore";
import type { NodeState } from "../../shared/generated/NodeState";
import type { PortDef } from "../../shared/generated/PortDef";
import { SectionHead, Field } from "./InspectorPrimitives";
import OutputPortCard from "./OutputPortCard";
import { NodeTypeIcon } from "./NodeTypeIcon";
import PooledInputRow from "./PooledInputRow";
import { useHarnessCatalog } from "../../harness/store/useHarnessCatalog";
import { useAgentProfiles } from "../../settings/store/useAgentProfiles";
import { useSkillBank } from "../../skill/store/useSkillBank";
import { useSkillTiers } from "../../skill/store/useSkillTiers";
import type { InheritedTier } from "../../skill/model/skillSelection";
import { usePromptAddedSkills } from "../../skill/store/usePromptAddedSkills";
import { findHarnessOption, effortOffer, resolveEditorHarness } from "../../harness/model/catalog";
import { carriesIsolation, nodeIsolation } from "../../lib/nodeIsolation";
import DestroyLoopModal from "./DestroyLoopModal";
import AgentSection from "./nodeInspector/AgentSection";
import BehaviorSection from "./nodeInspector/BehaviorSection";
import NameInput from "./nodeInspector/NameInput";
import PromptSection from "./nodeInspector/PromptSection";
import StarButton from "./nodeInspector/StarButton";
import WorkspaceSection from "./nodeInspector/WorkspaceSection";
import { NodeConfigSections } from "./ExtensionInspectorSections";
import { derivePooledInputs } from "../../lib/derivePooledInputs";
import type { PooledInputSource } from "../../lib/derivePooledInputs";
import { carriedPorts, edgeCarryingOutput, outputRenameImpact, withCarriedPorts } from "../../lib/edgePorts";
import { requalifyWhen } from "../../lib/whenClause";
import { regionsDestroyedByEdgeRemoval } from "../../lib/loopRegions";
import type { LibraryEntry } from "../model/library";
import { useLibraryState } from "../../hooks/useLibrary";
import type { EditorNodeDef } from "../model/pipeline";
import { copyToClipboard } from "../../shared/lib/clipboard";

/** The inspector of the node selected on the canvas: its identity, type,
 *  workspace, behavior, agent, prompt, inputs and outputs. */
export default function NodeInspector({
  libraryEntries,
  onLibraryChanged,
  readOnly,
  provisioningRepository = "",
  provisioningFrozenAt,
  inheritedProvisioning,
  provisioningGitRef = "HEAD",
  runNode,
  runSkills,
}: {
  libraryEntries: LibraryEntry[];
  onLibraryChanged: () => void;
  /** #669: the Run tier's frozen skills, on a run canvas — shown as inherited. */
  runSkills?: import("../../shared/generated/SkillRef").SkillRef[];
  /** #653: the live NodeRun's projected state, on a run canvas. Its frozen
   *  `isolated_worktree` is what the Workspace section reads in run mode —
   *  where the iteration ACTUALLY works, which an edit no longer moves. */
  runNode?: NodeState | null;
  /** #339: hides the per-source input × — set for archived runs only
   * (mirrors the canvas readOnly, #315/ADR-0020). Scoped to the × alone;
   * the rest of the inspector's archived story is the #315 gap. */
  readOnly?: boolean;
  provisioningRepository?: string;
  provisioningFrozenAt?: string;
  inheritedProvisioning?: import("../../shared/generated/ScopedProvisioningRules").ScopedProvisioningRules[];
  provisioningGitRef?: string;
}) {
  const openTabs = useEditStore((s) => s.openTabs);
  const activeTabId = useEditStore((s) => s.activeTabId);
  const selection = useEditStore((s) => s.selection);
  const updateNode = useEditStore((s) => s.updateNode);
  const updatePrompt = useEditStore((s) => s.updatePrompt);
  const deleteEdge = useEditStore((s) => s.deleteEdge);
  const updateEdge = useEditStore((s) => s.updateEdge);
  const scrollToPort = useEditStore((s) => s.scrollToPort);
  const setScrollToPort = useEditStore((s) => s.setScrollToPort);

  const asideRef = useRef<HTMLElement>(null);
  const [highlightedPort, setHighlightedPort] = useState<string | null>(null);
  const [provisioningPreviewRepository, setProvisioningPreviewRepository] = useState("");
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
  // #586: the harness pin's options, dynamic from `/settings` (floor ∪ descriptors,
  // each installed/not). Called before the early return so the hook order is stable.
  const harnessCatalog = useHarnessCatalog();
  const { profiles: agentProfiles } = useAgentProfiles();
  // #669/ADR-0062: the bank (names, folders) and the coarser tiers this node
  // inherits — the instance selection, the Projet owning the Run's repo when one
  // is known (run canvas), and the Run's frozen list. Called before the early
  // return so the hook order is stable.
  const { bank: skillBank } = useSkillBank();
  const { inherited: inheritedTiers } = useSkillTiers(provisioningRepository || null);
  const inheritedSkillTiers = useMemo<InheritedTier[]>(
    () =>
      runSkills && runSkills.length > 0
        ? [...inheritedTiers, { tier: "run", skills: runSkills }]
        : inheritedTiers,
    [inheritedTiers, runSkills],
  );

  // The skills the prompt's `/` completion added to this node: the « added »
  // chips of its Skills and the line under its prompt.
  const promptAdds = usePromptAddedSkills(node?.id ?? "", node?.skills ?? [], (skills) => {
    if (node) updateNode(node.id, { skills });
  });

  if (!tab || !node) return null;
  const resolvedHarness = resolveEditorHarness(node);
  const harnessOption = findHarnessOption(harnessCatalog, resolvedHarness);
  // #798: the effort picker's offer is read against the node's SELECTED model —
  // a per-model key in the served `model_efforts` is authoritative for it
  // (`[]` included), a missing key retains the harness's global efforts. When
  // the offer is authoritative, a stored effort outside it renders as an
  // unsupported passthrough — warned, kept, never silently deleted (ADR-0001).
  const effort = effortOffer(harnessOption, node.model);

  // Inputs are emergent (#149): derived from the pipeline's incoming edges,
  // not declared on the node. Same-named edges pool into one list input.
  const pooledInputs = derivePooledInputs(tab.pipeline, node.id);

  // #248: a `script` node runs deterministic bash, not an agent — so it has no
  // model, and its "prompt" is a bash body whose I/O arrives as PDO_* env vars,
  // not a prose preamble.
  const isScript = node.type === "script";
  // #653/ADR-0060: the frozen isolation of the live NodeRun, when there is one.
  // In run mode the Workspace choice becomes a *state*: a Node that changed
  // working directory under its own feet would be silent corruption, so the
  // section shows where the iteration actually works and stops being editable.
  const frozenIsolation = runNode?.isolated_worktree;
  const isolation = frozenIsolation ?? nodeIsolation(node);
  const isolationFrozen = frozenIsolation != null;

  const isInteractive = node.interactive ?? false;
  const isOrchestrator = node.orchestrator ?? false;


  // #339: delete one contributing edge of a pooled input — the canonical
  // "delete an input" since inputs are emergent (#149/ADR-0011). Last-cycle
  // deletions go through the same destroy-loop confirmation as the canvas;
  // `keepSelection` keeps the inspector open on this node.
  function handleDeleteSource(source: PooledInputSource) {
    const edgeIndex = source.edgeIndex;
    const edge = tab!.pipeline.edges[edgeIndex];
    const ports = edge ? carriedPorts(edge.source) : [];
    // #843 / ADR-0073: one incoming edge can feed SEVERAL inputs here, one per
    // carried output. Dropping one of them unticks its port; deleting the edge
    // would silently take the sibling inputs with it.
    if (ports.length > 1) {
      const next = ports.filter((p) => p !== source.port);
      updateEdge(edgeIndex, {
        source: withCarriedPorts(edge.source, next),
        when: requalifyWhen(edge.when, ports, next),
      });
      return;
    }
    const destroyed = regionsDestroyedByEdgeRemoval(tab!.pipeline, edgeIndex);
    if (destroyed.length > 0) {
      setPendingDestroy({ edgeIndex, loopIds: destroyed });
    } else {
      deleteEdge(edgeIndex, { keepSelection: true });
    }
  }

  function handleField(field: keyof EditorNodeDef, value: unknown) {
    updateNode(node!.id, { [field]: value } as Partial<EditorNodeDef>);
  }

  // A NodeRun that spawned froze its skills, an archived run takes no edit: the
  // completion still inserts, it adds nothing.
  const addPromptSkill = readOnly || runNode?.skills != null ? undefined : promptAdds.add;

  function handleAddOutput() {
    const ports = [...node!.outputs];
    let name = "out";
    let counter = 1;
    while (ports.some((p) => p.name === name)) {
      name = `out-${++counter}`;
    }
    ports.push({ name });
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
            onClick={() => void copyToClipboard(node.id)}
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

        {/* Type — #653/ADR-0060: there is one agentic type, so this is a static
            label like `script` already was. The pair of buttons it replaces
            offered non-isolated/isolated, i.e. a working-directory choice
            wearing a type's clothes; that choice now lives in Workspace below. */}
        <SectionHead title="Type" />
        <div
          data-testid={isScript ? "script-type-label" : "agent-type-label"}
          className="flex items-center gap-2 rounded border border-fg-4 bg-bg-3 px-2 py-1 font-medium text-fg"
          style={{ fontSize: "10px" }}
        >
          <NodeTypeIcon type={node.type} size={11} className="shrink-0 text-fg-3" />
          <span>{isScript ? "script (deterministic bash)" : node.type}</span>
        </div>

        {/* Workspace — #653/ADR-0060: where this Node's NodeRun works. A pair of
            named places, not a switch: the working directory is a choice between
            two locations, not a feature you turn on. The resolved path underneath
            is the point — the author reads where the Node will write instead of
            deducing it. Absent on Start/End, because a greyed control invites you to look for the way to
            un-grey it. */}
        {carriesIsolation(node.type) && isolation !== null && (
          <WorkspaceSection
            node={node}
            isolation={isolation}
            isolationFrozen={isolationFrozen}
            readOnly={readOnly}
            runNode={runNode}
            provisioningPreviewRepository={provisioningPreviewRepository}
            setProvisioningPreviewRepository={setProvisioningPreviewRepository}
            provisioningRepository={provisioningRepository}
            provisioningFrozenAt={provisioningFrozenAt}
            inheritedProvisioning={inheritedProvisioning}
            provisioningGitRef={provisioningGitRef}
            handleField={handleField}
          />
        )}

        <BehaviorSection
          node={node}
          isScript={isScript}
          readOnly={readOnly}
          runNode={runNode}
          handleField={handleField}
        />

        {!isScript && (
          <AgentSection
            node={node}
            readOnly={readOnly}
            runNode={runNode}
            handleField={handleField}
            harnessCatalog={harnessCatalog}
            agentProfiles={agentProfiles}
            skillBank={skillBank}
            inheritedSkillTiers={inheritedSkillTiers}
            resolvedHarness={resolvedHarness}
            harnessOption={harnessOption}
            effort={effort}
            addedSkills={promptAdds.added}
          />
        )}

        <PromptSection
          nodeId={node.id}
          isScript={isScript}
          isInteractive={isInteractive}
          isOrchestrator={isOrchestrator}
          promptContent={promptContent}
          updatePrompt={updatePrompt}
          skillBank={skillBank}
          ownSkills={node.skills ?? []}
          inheritedSkillTiers={inheritedSkillTiers}
          onAddSkill={addPromptSkill}
          lastAddedSkill={promptAdds.added[promptAdds.added.length - 1]?.name ?? null}
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
        <SectionHead
          title="Outputs"
          count={node.outputs.length}
          onAdd={handleAddOutput}
          addTestId="add-output-port"
        />
        <div className="flex flex-col">
          {node.outputs.map((port, i) => (
            <OutputPortCard
              key={i}
              index={i}
              port={port}
              highlighted={highlightedPort === port.name}
              onUpdate={(updates) => handleUpdateOutput(i, updates)}
              onRemove={() => handleRemoveOutput(i)}
              schema={port.frontmatter}
              onSchemaChange={(fm) => handleUpdateOutput(i, { frontmatter: fm ?? null })}
              allowInstructions={node.type === "agent"}
              wiredBy={edgeCarryingOutput(tab.pipeline, node.id, port.name)}
              takenNames={node.outputs.filter((_, j) => j !== i).map((p) => p.name)}
              wiring={outputRenameImpact(tab.pipeline, node.id, port.name)}
            />
          ))}
        </div>

        {/* ADR-0081 (#31): the `node/config` sections of the extensions. */}
        <NodeConfigSections
          nodeId={node.id}
          nodeName={node.name ?? null}
          pipelineId={tab.id}
          pipelineName={tab.pipeline.name}
          readOnly={readOnly === true}
        />
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
