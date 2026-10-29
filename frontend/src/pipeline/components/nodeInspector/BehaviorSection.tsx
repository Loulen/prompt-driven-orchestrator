import { TriangleAlert } from "lucide-react";
import type { NodeState } from "../../../shared/generated/NodeState";
import type { SkillRef } from "../../../shared/generated/SkillRef";
import { FormRow, FrozenMark, Toggle } from "../InspectorPrimitives";
import type { EditorNodeDef } from "../../model/pipeline";

/**
 * Behavior: the « Interactive » and « Orchestrator » toggles, each seeding its
 * own skill (#588/ADR-0069, #723/ADR-0064), and the « On demand » toggle
 * (ADR-0086).
 */
export default function BehaviorSection({
  node,
  isScript,
  readOnly,
  runNode,
  handleField,
}: {
  node: EditorNodeDef;
  isScript: boolean;
  readOnly?: boolean;
  runNode?: NodeState | null;
  handleField: (field: keyof EditorNodeDef, value: unknown) => void;
}) {
  // #723/ADR-0064 — the « Orchestrator » toggle's state and freeze. In run mode
  // the toggle follows the same ADR-0007 discipline as the Workspace choice:
  // once the NodeRun spawned, its frozen copy applies, so the switch reads off
  // and only says so — an edit applies to the next NodeRun.
  const ORCHESTRATE_SKILL_REF: SkillRef = { id: "pdo-orchestrate", name: "pdo-orchestrate" };
  // #588/ADR-0069 — the « Interactive » toggle seeds its own skill the same way.
  const INTERACTIVE_SKILL_REF: SkillRef = { id: "pdo-interactive", name: "pdo-interactive" };
  const isInteractive = node.interactive ?? false;
  const hasInteractiveSkill = (node.skills ?? []).some((s) => s.id === INTERACTIVE_SKILL_REF.id);
  function setInteractive(next: boolean) {
    const own = node.skills ?? [];
    const skills = next
      ? (own.some((s) => s.id === INTERACTIVE_SKILL_REF.id) ? own : [...own, INTERACTIVE_SKILL_REF])
      : own.filter((s) => s.id !== INTERACTIVE_SKILL_REF.id);
    handleField("skills", skills.length > 0 ? skills : undefined);
    handleField("interactive", next);
  }
  function seedInteractiveSkill() {
    if (hasInteractiveSkill) return;
    handleField("skills", [...(node.skills ?? []), INTERACTIVE_SKILL_REF]);
  }
  const isOrchestrator = node.orchestrator ?? false;
  const orchestratorFrozen = runNode != null && runNode.status !== "pending";
  const hasOrchestrateSkill = (node.skills ?? []).some((s) => s.id === ORCHESTRATE_SKILL_REF.id);
  // Toggle ON ⇒ seed the skill reference (unless it is already there). The
  // seeded skill is visible — and removable — in the Skills selector below;
  // the toggle stays the single source of intent (amber warning + re-add).
  function setOrchestrator(next: boolean) {
    const own = node.skills ?? [];
    const skills = next
      ? (own.some((s) => s.id === ORCHESTRATE_SKILL_REF.id) ? own : [...own, ORCHESTRATE_SKILL_REF])
      : own.filter((s) => s.id !== ORCHESTRATE_SKILL_REF.id);
    handleField("skills", skills.length > 0 ? skills : undefined);
    handleField("orchestrator", next);
  }
  function seedOrchestrateSkill() {
    if (hasOrchestrateSkill) return;
    handleField("skills", [...(node.skills ?? []), ORCHESTRATE_SKILL_REF]);
  }

  // ADR-0086 — « On demand »: the runtime never launches the node, the user's
  // ▶ does. Frozen once the node was launched in this Run, like Orchestrator.
  const isOnDemand = node.on_demand ?? false;
  const onDemandFrozen = runNode != null && runNode.status !== "pending";

  return (
    <>
      <FormRow
        label="On demand"
        help="Never launched by the runtime: it waits for your ▶ in the Run"
        aside={onDemandFrozen ? <FrozenMark title="Already launched in this run" /> : undefined}
      >
        <Toggle
          label="On demand"
          checked={isOnDemand}
          disabled={onDemandFrozen || readOnly}
          onChange={(next) => handleField("on_demand", next)}
          testid="node-on-demand-toggle"
        />
      </FormRow>
      {/* #588/ADR-0069: the « Interactive » toggle guards the node's completion
          (released from the run-mode UI) and seeds the `pdo-interactive` skill —
          the conduct travels in the skill, and PDO files the `/pdo-interactive`
          line under the prompt at spawn. */}
      <FormRow label="Interactive" help="A human talks to this node · adds the pdo-interactive skill">
        <Toggle
          label="Interactive"
          checked={isInteractive}
          disabled={readOnly}
          onChange={setInteractive}
          testid="node-interactive-toggle"
        />
      </FormRow>
      {isInteractive && !isScript && !hasInteractiveSkill && (
        <div className="ei-note" data-testid="node-interactive-skill-missing">
          <TriangleAlert size={11} strokeWidth={1.75} aria-hidden />
          <span>
            pdo-interactive skill was removed{" "}
            {!readOnly && (
              <button type="button" onClick={seedInteractiveSkill}>
                Re-add it
              </button>
            )}
          </span>
        </div>
      )}

      {/* #723/ADR-0064: the « Orchestrator » toggle — agent nodes only (a
          script runs no agent). Turning it on seeds the `pdo-orchestrate`
          skill; PDO files the fixed /pdo-orchestrate line at spawn. The toggle
          is the single source of intent: removing the skill by hand keeps it on
          and warns, with a one-click re-add. */}
      {!isScript && (
        <>
          <FormRow
            label="Orchestrator"
            help="May start child runs · adds the pdo-orchestrate skill"
            aside={orchestratorFrozen ? <FrozenMark title="Frozen at spawn: an edit applies to the next NodeRun" /> : undefined}
          >
            <Toggle
              label="Orchestrator"
              checked={isOrchestrator}
              disabled={orchestratorFrozen || readOnly}
              onChange={setOrchestrator}
              testid="node-orchestrator-toggle"
            />
          </FormRow>
          {isOrchestrator && !hasOrchestrateSkill && (
            <div className="ei-note" data-testid="node-orchestrator-skill-missing">
              <TriangleAlert size={11} strokeWidth={1.75} aria-hidden />
              <span>
                pdo-orchestrate skill was removed{" "}
                {!orchestratorFrozen && !readOnly && (
                  <button type="button" onClick={seedOrchestrateSkill}>
                    Re-add it
                  </button>
                )}
              </span>
            </div>
          )}
        </>
      )}
    </>
  );
}
