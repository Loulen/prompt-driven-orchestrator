import { SectionHead } from "../InspectorPrimitives";
import ProvisioningRulesEditor from "../../../workspace/components/ProvisioningRulesEditor";
import { EMPTY_PROVISIONING_RULES } from "../../../workspace/model/provisioning";
import { WORKSPACE_CHOICES, worktreePathFor } from "../../../lib/nodeIsolation";
import type { NodeState } from "../../../shared/generated/NodeState";
import type { ScopedProvisioningRules } from "../../../shared/generated/ScopedProvisioningRules";
import type { EditorNodeDef } from "../../model/pipeline";

/**
 * Workspace — #653/ADR-0060: where this Node's NodeRun works. A pair of named
 * places, not a switch: the working directory is a choice between two
 * locations, not a feature you turn on. The resolved path underneath is the
 * point — the author reads where the Node will write instead of deducing it.
 *
 * #653 / ADR-0060: the two places a NodeRun can work, named and described in one
 * breath. A radio pair rather than a switch — "off" would name neither place.
 *
 * The provisioning preview repository is the inspector's (it outlives a switch
 * to a node without this section); the block's disclosure is not — it opens
 * collapsed for every node selected (Notion #7).
 */
export default function WorkspaceSection({
  node,
  isolation,
  isolationFrozen,
  readOnly,
  runNode,
  provisioningPreviewRepository,
  setProvisioningPreviewRepository,
  provisioningRepository,
  provisioningFrozenAt,
  inheritedProvisioning,
  provisioningGitRef,
  handleField,
}: {
  node: EditorNodeDef;
  isolation: boolean;
  isolationFrozen: boolean;
  readOnly?: boolean;
  runNode?: NodeState | null;
  provisioningPreviewRepository: string;
  setProvisioningPreviewRepository: (repository: string) => void;
  provisioningRepository: string;
  provisioningFrozenAt?: string;
  inheritedProvisioning?: ScopedProvisioningRules[];
  provisioningGitRef: string;
  handleField: (field: keyof EditorNodeDef, value: unknown) => void;
}) {
  return (
    <>
      <SectionHead title="Workspace" />
      <div data-testid="workspace-choice" className="flex flex-col gap-1">
        {WORKSPACE_CHOICES.map((choice) => (
          <button
            key={String(choice.isolated)}
            data-testid={`workspace-${choice.isolated ? "isolated" : "shared"}`}
            aria-checked={isolation === choice.isolated}
            role="radio"
            disabled={isolationFrozen}
            onClick={() => handleField("isolated_worktree", choice.isolated)}
            className={`flex items-start gap-2 rounded border px-2 py-1.5 text-left transition-colors ${
              isolation === choice.isolated
                ? "border-acc bg-acc-bg"
                : "border-line-strong bg-bg-3 hover:border-fg-4"
            } ${isolationFrozen ? "cursor-default opacity-70" : "cursor-pointer"}`}
          >
            <span
              className={`mt-[3px] h-2.5 w-2.5 shrink-0 rounded-full border ${
                isolation === choice.isolated ? "border-acc bg-acc" : "border-fg-4"
              }`}
            />
            <span className="flex flex-col">
              <span
                className={`font-medium ${isolation === choice.isolated ? "text-acc" : "text-fg-2"}`}
                style={{ fontSize: "10.5px" }}
              >
                {choice.label}
              </span>
              <span className="text-fg-4" style={{ fontSize: "9.5px" }}>
                {choice.hint}
              </span>
            </span>
          </button>
        ))}
      </div>
      <span
        data-testid="workspace-path"
        className="break-all rounded border border-line bg-bg-3 px-2 py-1 font-mono text-fg-4"
        style={{ fontSize: "9.5px" }}
      >
        {worktreePathFor(node.id, isolation)}
      </span>
      {isolationFrozen && (
        <span data-testid="workspace-frozen" className="text-fg-4" style={{ fontSize: "9.5px" }}>
          Frozen at spawn — an edit applies to the next NodeRun.
        </span>
      )}
      {isolation && (
        <ProvisioningRulesEditor
          // Notion #7: a fresh, collapsed block for every node selected.
          key={node.id}
          level="isolated_node"
          repository={provisioningRepository || provisioningPreviewRepository}
          rules={
            (provisioningFrozenAt ? runNode?.provisioning : undefined) ??
            node.provisioning ??
            EMPTY_PROVISIONING_RULES
          }
          onChange={(rules) => handleField("provisioning", rules)}
          readOnly={readOnly || !!provisioningFrozenAt}
          frozenAt={provisioningFrozenAt}
          frozenPlan={provisioningFrozenAt ? runNode?.provisioning_plan : undefined}
          inherited={inheritedProvisioning}
          gitRef={provisioningGitRef}
          leading={
            !provisioningRepository && (
              <label className="block text-fg-3" style={{ fontSize: 10 }}>
                Resolve against
                <input
                  value={provisioningPreviewRepository}
                  onChange={(event) => setProvisioningPreviewRepository(event.target.value)}
                  placeholder="/absolute/path/to/repository"
                  className="mt-1 w-full rounded border border-line-strong bg-bg-3 px-2 py-1 font-mono text-fg outline-none focus:border-acc"
                />
              </label>
            )
          }
        />
      )}
    </>
  );
}
