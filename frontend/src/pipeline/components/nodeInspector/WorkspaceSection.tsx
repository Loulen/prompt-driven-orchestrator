import { FormRow, FrozenMark } from "../InspectorPrimitives";
import { Field, TextInput } from "../../../shared/ui/kit";
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
  const path = worktreePathFor(node.id, isolation);
  return (
    <>
      <FormRow
        label="Worktree"
        help="Where this node's NodeRun works"
        aside={
          isolationFrozen ? (
            <FrozenMark title="Frozen at spawn: an edit applies to the next NodeRun" testid="workspace-frozen" />
          ) : undefined
        }
      >
        <div className="d-seg" role="radiogroup" aria-label="Worktree" data-testid="workspace-choice">
          {WORKSPACE_CHOICES.map((choice) => (
            <button
              key={String(choice.isolated)}
              type="button"
              role="radio"
              aria-checked={isolation === choice.isolated}
              tabIndex={isolation === choice.isolated ? 0 : -1}
              disabled={isolationFrozen}
              title={`${choice.label}: ${choice.hint}`}
              className="d-seg__btn"
              onClick={() => handleField("isolated_worktree", choice.isolated)}
              data-testid={`workspace-${choice.isolated ? "isolated" : "shared"}`}
            >
              {choice.isolated ? "Isolated" : "Shared"}
            </button>
          ))}
        </div>
      </FormRow>
      <FormRow label="Path" help="The node's working directory">
        <span className="d-mono ei-meta ei-trunc" title={path} data-testid="workspace-path">
          {path}
        </span>
      </FormRow>
      {isolation && (
        <div className="ei-provisioning">
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
                <Field label="Resolve against">
                  <TextInput
                    mono
                    value={provisioningPreviewRepository}
                    onChange={(event) => setProvisioningPreviewRepository(event.target.value)}
                    placeholder="/absolute/path/to/repository"
                  />
                </Field>
              )
            }
          />
        </div>
      )}
    </>
  );
}
