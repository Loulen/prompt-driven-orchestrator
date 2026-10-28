import { GitFork, Lock, Play, TriangleAlert } from "lucide-react";
import type { NodeState } from "../../../shared/generated/NodeState";
import type { SkillRef } from "../../../shared/generated/SkillRef";
import { SectionHead } from "../InspectorPrimitives";
import { Tooltip } from "../../../shared/ui/tooltip";
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
        {/* Behavior */}
        <SectionHead title="Behavior" />
        <Tooltip
          content="The runtime never launches nor skips this node: it waits for your ▶ in the Run. It reads what its upstream nodes already produced, and holds no downstream node until you launch it."
          side="left"
        >
          <div className="flex flex-col gap-1">
            <div className="flex items-center justify-between">
              <span className="flex items-center gap-1.5 text-text-3">
                <Play size={11} className="text-text-3" />
                On demand
                {onDemandFrozen && <Lock size={9} className="text-text-3" />}
              </span>
              <button
                data-testid="node-on-demand-toggle"
                aria-pressed={isOnDemand}
                disabled={onDemandFrozen || readOnly}
                onClick={() => handleField("on_demand", !isOnDemand)}
                className={`relative h-5 w-9 rounded-full transition-colors ${
                  onDemandFrozen || readOnly ? "cursor-not-allowed opacity-60" : "cursor-pointer"
                } ${isOnDemand ? "bg-primary" : "bg-surface-2"}`}
              >
                <span
                  className={`absolute top-0.5 left-0.5 h-4 w-4 rounded-full bg-text transition-transform ${
                    isOnDemand ? "translate-x-[16px]" : "translate-x-0"
                  }`}
                />
              </button>
            </div>
            {onDemandFrozen && (
              <span className="text-text-3" style={{ fontSize: "9.5px" }}>
                Already launched in this run.
              </span>
            )}
          </div>
        </Tooltip>
        {/* #588/ADR-0069: the « Interactive » toggle guards the node's completion
            (released from the run-mode UI) and seeds the `pdo-interactive` skill —
            the conduct (declare the wait with `pdo wait-user`, never block the
            conversation, the release flow) travels in the skill, and PDO files the
            `/pdo-interactive` line under the prompt at spawn. The wait itself is
            declared by the agent, never assumed at spawn. */}
        <Tooltip
          content="A human talks to this node. Its completion is guarded until you release it from the run-mode UI; the agent declares when it waits for you (pdo wait-user). Adds the pdo-interactive skill and a fixed /pdo-interactive line to the prompt PDO files at spawn."
          side="left"
        >
          <div className="flex flex-col gap-1">
            <div className="flex items-center justify-between">
              <span className="text-text-3">Interactive</span>
              <button
                data-testid="node-interactive-toggle"
                aria-pressed={isInteractive}
                disabled={readOnly}
                onClick={() => setInteractive(!isInteractive)}
                className={`relative h-5 w-9 rounded-full transition-colors ${
                  readOnly ? "cursor-not-allowed opacity-60" : "cursor-pointer"
                } ${isInteractive ? "bg-primary" : "bg-surface-2"}`}
              >
                <span
                  className={`absolute top-0.5 left-0.5 h-4 w-4 rounded-full bg-text transition-transform ${
                    isInteractive ? "translate-x-[16px]" : "translate-x-0"
                  }`}
                />
              </button>
            </div>
            {isInteractive && !isScript && !hasInteractiveSkill && (
              <span
                data-testid="node-interactive-skill-missing"
                className="flex items-start gap-1 rounded border border-st-blocked/40 bg-st-blocked/10 px-2 py-1 text-st-blocked"
                style={{ fontSize: "9.5px", lineHeight: 1.4 }}
              >
                <TriangleAlert size={10} className="mt-[1px] shrink-0" />
                <span>
                  The pdo-interactive skill was removed: the agent will get the /pdo-interactive line but no
                  instructions.{" "}
                  {!readOnly && (
                    <button type="button" className="cursor-pointer underline" onClick={seedInteractiveSkill}>
                      Re-add it
                    </button>
                  )}
                </span>
              </span>
            )}
          </div>
        </Tooltip>

        {/* #723/ADR-0064: the « Orchestrator » toggle — a behavior of the node,
            like Interactive, one row under it. Agent nodes only (a script runs
            no agent, so a child-run contract there would silently do nothing).
            Turning it on also seeds the `pdo-orchestrate` skill reference and
            PDO files the fixed /pdo-orchestrate amendment into the prompt at
            spawn (rendered below the textarea; never editable there). The
            toggle is the single source of intent: removing the skill by hand
            keeps the toggle on and warns, with a one-click re-add. */}
        {!isScript && (
          <Tooltip
            content="Lets this node create child runs (pdo run create). Adds the pdo-orchestrate skill to the node and a fixed /pdo-orchestrate line to the prompt PDO files at spawn. Frozen once the NodeRun spawns."
            side="left"
          >
            <div className="flex flex-col gap-1">
              <div className="flex items-center justify-between">
                <span className="flex items-center gap-1.5 text-text-3">
                  <GitFork size={11} className="text-text-3" />
                  Orchestrator
                  {orchestratorFrozen && <Lock size={9} className="text-text-3" />}
                </span>
                <button
                  data-testid="node-orchestrator-toggle"
                  aria-pressed={isOrchestrator}
                  disabled={orchestratorFrozen || readOnly}
                  onClick={() => setOrchestrator(!isOrchestrator)}
                  className={`relative h-5 w-9 rounded-full transition-colors ${
                    orchestratorFrozen || readOnly ? "cursor-not-allowed opacity-60" : "cursor-pointer"
                  } ${isOrchestrator ? "bg-primary" : "bg-surface-2"}`}
                >
                  <span
                    className={`absolute top-0.5 left-0.5 h-4 w-4 rounded-full bg-text transition-transform ${
                      isOrchestrator ? "translate-x-[16px]" : "translate-x-0"
                    }`}
                  />
                </button>
              </div>
              {orchestratorFrozen && (
                <span className="text-text-3" style={{ fontSize: "9.5px" }}>
                  Frozen at spawn — an edit applies to the next NodeRun.
                </span>
              )}
              {isOrchestrator && !hasOrchestrateSkill && (
                <span
                  data-testid="node-orchestrator-skill-missing"
                  className="flex items-start gap-1 rounded border border-st-blocked/40 bg-st-blocked/10 px-2 py-1 text-st-blocked"
                  style={{ fontSize: "9.5px", lineHeight: 1.4 }}
                >
                  <TriangleAlert size={10} className="mt-[1px] shrink-0" />
                  <span>
                    The pdo-orchestrate skill was removed: the agent will get the /pdo-orchestrate line but no
                    instructions.{" "}
                    {!orchestratorFrozen && !readOnly && (
                      <button
                        type="button"
                        className="cursor-pointer underline"
                        onClick={seedOrchestrateSkill}
                      >
                        Re-add it
                      </button>
                    )}
                  </span>
                </span>
              )}
            </div>
          </Tooltip>
        )}
    </>
  );
}
