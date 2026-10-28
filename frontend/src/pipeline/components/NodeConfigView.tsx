import { useMemo } from "react";
import { Lock } from "lucide-react";
import type { NodeState } from "../../shared/generated/NodeState";
import type { SkillRef } from "../../shared/generated/SkillRef";
import { useEditStore } from "../store/editStore";
import { SectionHead } from "./InspectorPrimitives";
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

  return (
    <aside className="flex h-full flex-col overflow-y-auto bg-surface-1" data-testid="node-config-view">
      <div
        className="flex h-8 shrink-0 items-center justify-between border-b border-line px-3 font-medium text-text-2"
        style={{ fontSize: "11.5px" }}
      >
        <span>Node config</span>
        <span className="flex items-center gap-1 font-normal text-text-3" style={{ fontSize: "10px" }}>
          <Lock size={10} />
          Read-only
        </span>
      </div>
      <div className="flex flex-col gap-3 p-3" style={{ fontSize: "11.5px" }}>
        <p className="text-text-3" style={{ fontSize: "10px" }} data-testid="node-config-hint">
          {archived
            ? "This run is archived and read-only."
            : "You are following this run. To change this node, use the Edit pencil › Edit for this run in the canvas toolbar."}
        </p>
        <SectionHead title="Identity" />
        <div className="flex items-center gap-2 text-text" data-testid="node-config-name">
          <NodeTypeIcon type={node.type} size={11} className="shrink-0 text-text-3" />
          <span className="font-medium">{node.name || node.id}</span>
          <span className="font-mono text-text-3" style={{ fontSize: "10px" }}>{node.id}</span>
        </div>
        {node.on_demand && (
          <p className="text-text-3" style={{ fontSize: "10.5px" }} data-testid="node-config-on-demand">
            On demand — the runtime never launches it; the ▶ on its card does.
          </p>
        )}

        {!isScript && (
          <>
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
            <ul className="flex flex-wrap gap-1" data-testid="node-config-skill-list">
              {ownSkills.length === 0 ? (
                <li className="text-text-3" style={{ fontSize: "10px" }}>No skill of its own.</li>
              ) : (
                ownSkills.map((skill) => (
                  <li
                    key={skill.id}
                    className="rounded border border-line bg-surface-2 px-1.5 py-0.5 font-mono text-text-2"
                    style={{ fontSize: "10px" }}
                  >
                    {skill.name || skill.id}
                  </li>
                ))
              )}
            </ul>
            {runNode?.skills && (
              <p className="text-text-3" style={{ fontSize: 9.5 }}>
                Frozen at spawn:{" "}
                {runNode.skills.length === 0 ? "none" : runNode.skills.map((skill) => skill.name).join(", ")}
              </p>
            )}
          </>
        )}

        <SectionHead title={isScript ? "Script (bash)" : "Prompt"} />
        <pre
          data-testid="node-config-prompt"
          className={`min-h-[80px] whitespace-pre-wrap break-words rounded border border-line bg-surface-2 px-2 py-1.5 font-mono text-text-2 ${
            invocations.length > 0 ? "rounded-b-none border-b-0" : ""
          }`}
          style={{ fontSize: "11px", lineHeight: "1.5" }}
        >
          {prompt || <span className="text-text-3">No prompt.</span>}
        </pre>
        {invocations.length > 0 && (
          <div className="-mt-3 rounded-b border border-t-0 border-dashed border-line-strong bg-surface-1 px-2 py-1.5">
            <div className="mb-1 flex items-center gap-1 text-text-3" style={{ fontSize: "9.5px" }}>
              <Lock size={9} />
              Added by PDO at spawn
            </div>
            <pre className="whitespace-pre-wrap font-mono text-text-3" style={{ fontSize: "10.5px", lineHeight: 1.5 }}>
              {invocations.join("\n")}
            </pre>
          </div>
        )}
      </div>
    </aside>
  );
}
