import { useMemo } from "react";
import { Lock } from "lucide-react";
import type { NodeState } from "../../shared/generated/NodeState";
import type { SkillRef } from "../../shared/generated/SkillRef";
import { useEditStore } from "../store/editStore";
import { FormRow } from "./InspectorPrimitives";
import { Chip, Section } from "../../shared/ui/kit";
import "./inspect.css";
import { NodeTypeIcon } from "./NodeTypeIcon";
import SkillSelector from "../../skill/components/SkillSelector";
import { useSkillBank } from "../../skill/store/useSkillBank";
import { useSkillTiers } from "../../skill/store/useSkillTiers";
import type { InheritedTier } from "../../skill/model/skillSelection";

/**
 * ADR-0085: the inspector's « Config » tab while a Run is followed
 * (« pilotage ») or archived: what the node runs with, its prompt and its
 * skills, read-only. Changing it goes through « Edit for this run », never
 * through this pane.
 */
export default function NodeConfigView({
  provisioningRepository = "",
  runSkills,
  runNode,
  archived = false,
}: {
  provisioningRepository?: string;
  runSkills?: SkillRef[];
  runNode?: NodeState | null;
  /** An archived Run has no Edit control: the hint must not point to it. */
  archived?: boolean;
}) {
  const openTabs = useEditStore((s) => s.openTabs);
  const activeTabId = useEditStore((s) => s.activeTabId);
  const selection = useEditStore((s) => s.selection);
  const { bank: skillBank } = useSkillBank();
  const { inherited: inheritedTiers } = useSkillTiers(provisioningRepository || null);
  const inheritedSkillTiers = useMemo<InheritedTier[]>(
    () =>
      runSkills && runSkills.length > 0
        ? [...inheritedTiers, { tier: "run", skills: runSkills }]
        : inheritedTiers,
    [inheritedTiers, runSkills],
  );

  const tab = openTabs.find((t) => t.id === activeTabId);
  const node =
    tab && selection.kind === "node" && selection.id
      ? tab.pipeline.nodes.find((n) => n.id === selection.id) ?? null
      : null;
  if (!tab || !node) return null;
  const prompt = tab.prompts[node.id] ?? "";
  const isScript = node.type === "script";
  const ownSkills = node.skills ?? [];
  const invocations = isScript
    ? []
    : [node.interactive && "/pdo-interactive", node.orchestrator && "/pdo-orchestrate"].filter(
        (line): line is string => typeof line === "string",
      );

  const hint = archived
    ? "This run is archived and read-only."
    : "Following this run: change it with Edit for this run, in the canvas toolbar.";
  return (
    <aside className="ei" data-testid="node-config-view">
      <div className="ei-bar">
        <span className="ei-bar__title" data-testid="node-config-name">
          <NodeTypeIcon type={node.type} size={14} />
          <span className="ei-trunc" title={node.name || node.id}>
            {node.name || node.id}
          </span>
          <span className="ei-bar__sub">{node.id}</span>
        </span>
        <span className="ei-bar__actions">
          <span title={hint} data-testid="node-config-hint">
            <Chip icon={Lock}>
              read only<span className="d-sr"> · {hint}</span>
            </Chip>
          </span>
        </span>
      </div>
      {node.on_demand && (
        <div className="ei-idline" title="The runtime never launches it; the ▶ on its card does" data-testid="node-config-on-demand">
          On demand
        </div>
      )}

      {!isScript && (
        <Section title="Skills" count={ownSkills.length}>
          <FormRow label="Skills">
            <SkillSelector
              tier="node"
              own={ownSkills}
              onChange={() => {}}
              inherited={inheritedSkillTiers}
              bank={skillBank}
              label="Skills"
              testId="node-config-skills"
              readOnly
            />
          </FormRow>
          <div className="ei-chips" data-testid="node-config-skill-list">
            {ownSkills.length === 0 ? (
              <span className="ei-meta">No skill of its own</span>
            ) : (
              ownSkills.map((skill) => (
                <Chip key={skill.id} mono>
                  {skill.name || skill.id}
                </Chip>
              ))
            )}
          </div>
          {runNode?.skills && (
            <FormRow label="At spawn">
              <span className="ei-meta d-mono ei-trunc">
                {runNode.skills.length === 0 ? "none" : runNode.skills.map((skill) => skill.name).join(", ")}
              </span>
            </FormRow>
          )}
        </Section>
      )}

      <Section title={isScript ? "Script (bash)" : "Prompt"} flush>
        <pre data-testid="node-config-prompt" className="ei-quote">
          {prompt || <span className="ei-dim">No prompt</span>}
        </pre>
        {invocations.length > 0 && (
          <div className="ei-amend" title="Added at spawn">
            <Lock size={10} strokeWidth={1.75} aria-hidden />
            <pre>{invocations.join("\n")}</pre>
          </div>
        )}
      </Section>
    </aside>
  );
}
