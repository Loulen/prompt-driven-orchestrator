import type { AgentCombination } from "../../../settings/model/settings";
import type { NodeState } from "../../../shared/generated/NodeState";
import type { SkillRef } from "../../../shared/generated/SkillRef";
import type { AgentProfile } from "../../../shared/generated/AgentProfile";
import NodeAgentControl from "./NodeAgentControl";
import { FormRow, FrozenMark } from "../InspectorPrimitives";
import SkillSelector from "../../../skill/components/SkillSelector";
import type { InheritedTier } from "../../../skill/model/skillSelection";
import HarnessSelect from "../../../harness/components/HarnessSelect";
import ModelPicker from "../../../harness/components/ModelPicker";
import EffortPicker from "../../../harness/components/EffortPicker";
import type { HarnessCatalog } from "../../../harness/model/catalog";
import { effortOffer, findHarnessOption } from "../../../harness/model/catalog";
import type { EditorNodeDef } from "../../model/pipeline";

/**
 * The agent of an agentic node: its profile or combination, its Node tier of
 * the skills (#669/ADR-0062), and the harness pin, model and effort the tests
 * and the tours read (kept off-screen).
 */
export default function AgentSection({
  node,
  readOnly,
  runNode,
  handleField,
  harnessCatalog,
  agentProfiles,
  inheritedAgent,
  skillBank,
  inheritedSkillTiers,
  resolvedHarness,
  harnessOption,
  effort,
  addedSkills,
}: {
  node: EditorNodeDef;
  readOnly?: boolean;
  runNode?: NodeState | null;
  handleField: (field: keyof EditorNodeDef, value: unknown) => void;
  harnessCatalog: HarnessCatalog;
  agentProfiles: AgentProfile[];
  /** What « Inherit » falls to: the instance's choice, then the Default profile. */
  inheritedAgent?: AgentCombination;
  skillBank: Parameters<typeof SkillSelector>[0]["bank"];
  inheritedSkillTiers: InheritedTier[];
  resolvedHarness: string;
  harnessOption: ReturnType<typeof findHarnessOption>;
  effort: ReturnType<typeof effortOffer>;
  /** Skills the prompt's `/` completion just added: chips under the selector. */
  addedSkills?: SkillRef[];
}) {
  return (
    <>
      <FormRow label="Model" help="Inherit, a profile, or a combination of its own">
        <NodeAgentControl
          key={node.id}
          choice={node.agent_choice}
          onChange={(agentChoice) => handleField("agent_choice", agentChoice)}
          profiles={agentProfiles}
          catalog={harnessCatalog}
          inherited={inheritedAgent ?? { harness: "claude", model: null, effort: null }}
          readOnly={readOnly}
          testId="node-agent-control"
        />
      </FormRow>
      {/* #669/ADR-0062: the Node tier of the skills selection. Own skills are
          live; inherited ones (instance, Projet, Run) are greyed with their
          origin; the total is the strict additive union. An id the bank lost
          warns here and on the pipeline banner — the node still runs.
          A NodeRun that already spawned froze its skills (the line below says
          with which): the selector goes read-only rather than taking edits
          that would change nothing (#849), and still opens for reading. */}
      <FormRow
        label="Skills"
        aside={runNode?.skills ? <FrozenMark title="Frozen at spawn" /> : undefined}
      >
        <SkillSelector
          tier="node"
          own={node.skills ?? []}
          onChange={(skills) => handleField("skills", skills.length > 0 ? skills : undefined)}
          inherited={inheritedSkillTiers}
          bank={skillBank}
          label="Skills"
          testId="node-skill-selector"
          readOnly={readOnly || runNode?.skills != null}
          added={addedSkills}
        />
      </FormRow>
      {runNode?.skills && (
        <FormRow label="At spawn">
          <span className="ei-meta d-mono ei-trunc" data-testid="node-skills-frozen" title={frozenTitle(runNode)}>
            {runNode.skills.length === 0 ? "none" : runNode.skills.map((skill) => skill.name).join(", ")}
            {runNode.missing_skills && runNode.missing_skills.length > 0 && (
              <span className="ei-warn">
                {" "}· missing: {runNode.missing_skills.map((skill) => skill.name || skill.id).join(", ")}
              </span>
            )}
            {runNode.skipped_skills && runNode.skipped_skills.length > 0 && (
              <span
                className="ei-warn"
                data-testid="node-skills-skipped"
                title={runNode.skipped_skills.map((skill) => `${skill.name}: ${skill.reason}`).join("\n")}
              >
                {" "}· not delivered: {runNode.skipped_skills.map((skill) => skill.name || skill.id).join(", ")}
              </span>
            )}
          </span>
        </FormRow>
      )}
      <div className="sr-only">
        <div data-testid="node-harness" data-resolved={resolvedHarness}>
          <HarnessSelect
            data-testid="node-harness-select"
            value={node.pin_harness ?? ""}
            onChange={(value) => handleField("pin_harness", value || null)}
            catalog={harnessCatalog}
            inheritLabel="Default (claude floor)"
          />
        </div>
        <span data-testid="node-harness-resolved">{resolvedHarness}</span>
        <ModelPicker
          value={node.model ?? null}
          onChange={(value) => handleField("model", value)}
          models={harnessOption?.models ?? []}
          contexts={harnessOption?.modelContexts}
          testid="node-model"
          subject={node.id}
        />
        <EffortPicker
          value={node.effort ?? null}
          onChange={(value) => handleField("effort", value)}
          efforts={effort.levels}
          strict={effort.authoritative}
          testid="node-effort"
          disabled={!(harnessOption?.hasEffort ?? true)}
        />
      </div>
    </>
  );
}

/** The skills the NodeRun froze at spawn, in full. */
function frozenTitle(runNode: NodeState): string {
  const skills = runNode.skills ?? [];
  return `Frozen at spawn: ${skills.length === 0 ? "none" : skills.map((skill) => skill.name).join(", ")}`;
}
