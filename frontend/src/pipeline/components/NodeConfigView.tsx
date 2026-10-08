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
    <aside className="flex h-full flex-col overflow-y-auto bg-bg-2" data-testid="node-config-view">
      <div
        className="flex h-[36px] items-center justify-between border-b border-line px-3 font-medium text-fg-2"
        style={{ fontSize: "11.5px" }}
      >
        <span>Node config</span>
        <span className="flex items-center gap-1 font-normal text-fg-4" style={{ fontSize: "10px" }}>
          <Lock size={10} />
          Read-only
        </span>
      </div>
      <div className="flex flex-col gap-3 p-3" style={{ fontSize: "11.5px" }}>
        <p className="text-fg-4" style={{ fontSize: "10px" }} data-testid="node-config-hint">
          {archived
            ? "This run is archived and read-only."
            : "You are following this run. To change this node, use the Edit pencil › Edit for this run in the canvas toolbar."}
        </p>
        <SectionHead title="Identity" />
        <div className="flex items-center gap-2 text-fg" data-testid="node-config-name">
          <NodeTypeIcon type={node.type} size={11} className="shrink-0 text-fg-3" />
          <span className="font-medium">{node.name || node.id}</span>
          <span className="font-mono text-fg-4" style={{ fontSize: "10px" }}>{node.id}</span>
        </div>

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
                <li className="text-fg-4" style={{ fontSize: "10px" }}>No skill of its own.</li>
              ) : (
                ownSkills.map((skill) => (
                  <li
                    key={skill.id}
                    className="rounded border border-line bg-bg-3 px-1.5 py-0.5 font-mono text-fg-2"
                    style={{ fontSize: "10px" }}
                  >
                    {skill.name || skill.id}
                  </li>
                ))
              )}
            </ul>
            {runNode?.skills && (
              <p className="text-fg-4" style={{ fontSize: 9.5 }}>
                Frozen at spawn:{" "}
                {runNode.skills.length === 0 ? "none" : runNode.skills.map((skill) => skill.name).join(", ")}
              </p>
            )}
          </>
        )}

        <SectionHead title={isScript ? "Script (bash)" : "Prompt"} />
        <pre
          data-testid="node-config-prompt"
          className={`min-h-[80px] whitespace-pre-wrap break-words rounded border border-line bg-bg-3 px-2 py-1.5 font-mono text-fg-2 ${
            invocations.length > 0 ? "rounded-b-none border-b-0" : ""
          }`}
          style={{ fontSize: "11px", lineHeight: "1.5" }}
        >
          {prompt || <span className="text-fg-4">No prompt.</span>}
        </pre>
        {invocations.length > 0 && (
          <div className="-mt-3 rounded-b border border-t-0 border-dashed border-line-strong bg-bg-2 px-2 py-1.5">
            <div className="mb-1 flex items-center gap-1 text-fg-4" style={{ fontSize: "9.5px" }}>
              <Lock size={9} />
              Added by PDO at spawn
            </div>
            <pre className="whitespace-pre-wrap font-mono text-fg-3" style={{ fontSize: "10.5px", lineHeight: 1.5 }}>
              {invocations.join("\n")}
            </pre>
          </div>
        )}
      </div>
    </aside>
  );
}
