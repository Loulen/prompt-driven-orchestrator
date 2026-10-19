import { useMemo } from "react";
import { Lock } from "lucide-react";
import { SectionHead } from "../InspectorPrimitives";
import { firstName, presenceColor } from "../../model/presence";
import { useTypingInPrompt } from "../../store/useDraftPresence";
import { ParticipantAvatar } from "../ParticipantAvatar";
import SharedPromptField from "./SharedPromptField";
import type { Skill } from "../../../shared/generated/Skill";
import type { SkillRef } from "../../../shared/generated/SkillRef";
import type { SkillBank } from "../../../skill/model/skill";
import { resolveActiveSkills, type InheritedTier } from "../../../skill/model/skillSelection";
import { useSkillPromptField } from "../../../skill/store/useSkillPromptField";
import SkillCompletionList from "../../../skill/components/SkillCompletionList";
import SkillPromptHint from "../../../skill/components/SkillPromptHint";

/**
 * The node's prompt — or, for a script node (#248), its bash body — and the
 * invocation lines PDO files under it at spawn for the toggles that are on.
 *
 * An agentic prompt completes skills: `/` at the start of a word lists the
 * bank's skills at the caret, those the node receives first; picking one the
 * node does not receive adds it to the node's Skills (`onAddSkill`), and the
 * line under the prompt says so.
 */
export default function PromptSection({
  nodeId,
  isScript,
  isInteractive,
  isOrchestrator,
  promptContent,
  updatePrompt,
  skillBank,
  ownSkills,
  inheritedSkillTiers,
  onAddSkill,
  lastAddedSkill,
}: {
  nodeId: string;
  isScript: boolean;
  isInteractive: boolean;
  isOrchestrator: boolean;
  promptContent: string;
  updatePrompt: (nodeId: string, content: string) => void;
  skillBank: SkillBank;
  ownSkills: SkillRef[];
  inheritedSkillTiers: InheritedTier[];
  /** Absent when the node's skills cannot change (frozen at spawn, archived). */
  onAddSkill?: (skill: Skill) => void;
  /** The last skill the completion added to this node, for the line under the prompt. */
  lastAddedSkill?: string | null;
}) {
  // ADR-0088: who else types in this prompt of the shared draft.
  const typing = useTypingInPrompt(nodeId);
  const received = useMemo(
    () =>
      new Set(
        resolveActiveSkills("node", ownSkills, inheritedSkillTiers, skillBank)
          .rows.filter((row) => !row.missing)
          .map((row) => row.id),
      ),
    [ownSkills, inheritedSkillTiers, skillBank],
  );
  const { fieldProps, completion, pick } = useSkillPromptField({
    value: promptContent,
    onChange: (content) => updatePrompt(nodeId, content),
    bank: skillBank,
    received,
    onAdd: onAddSkill,
  });
  return (
    <>
        {/* Prompt / Script body. For a script node (#248) this textarea holds the
            bash body; its I/O arrives as PDO_* env vars, not a prose preamble. */}
        <SectionHead
          title={isScript ? "Script (bash)" : "Prompt"}
          aside={
            typing.length > 0 && (
              <span className="flex items-center gap-1 text-fg-4" style={{ fontSize: "10px" }} data-testid="prompt-typing">
                <ParticipantAvatar participant={typing[0]} size={11} />
                <span style={{ color: presenceColor(typing[0]) }}>
                  {typing.map(firstName).join(", ")} {typing.length > 1 ? "are" : "is"} typing
                </span>
              </span>
            )
          }
        />
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
        <SharedPromptField
          nodeId={nodeId}
          typing={typing}
          // #822: a script node's body keeps `script-body` (what it has always been
          // called, and what the script tests point at); the agentic prompt gets the
          // stable target the First pipeline tour types into.
          data-testid={isScript ? "script-body" : "node-prompt-input"}
          {...(isScript
            ? { value: promptContent, onChange: (e: React.ChangeEvent<HTMLTextAreaElement>) => updatePrompt(nodeId, e.target.value) }
            : fieldProps)}
          className={`min-h-[120px] w-full resize-y rounded border border-line-strong bg-bg-3 px-2 py-1.5 font-mono text-fg outline-none focus:border-acc ${
            !isScript && (isOrchestrator || isInteractive) ? "rounded-b-none border-b-0 border-dashed" : ""
          }`}
          style={{ fontSize: "11px", lineHeight: "1.5" }}
          placeholder={isScript ? "#!/usr/bin/env bash\n# e.g. curl -X POST \"$DISCORD_WEBHOOK\" ..." : "Enter the node's role prompt..."}
        />
        {!isScript && (
          <SkillCompletionList completion={completion} textarea={fieldProps.ref} onPick={pick} scope="node" />
        )}

        {/* #723 / #588 — the invocation lines PDO files at spawn for the toggles
            that are on, in spawn order (interactive, then orchestrate). Rendered,
            never editable, never in the textarea: toggle and text cannot drift.
            Byte-for-byte the daemon's `toggle_invocations` (no prose any more —
            the seeded skills carry the conduct). Dashed block attached under the
            textarea. */}
        {!isScript && (isInteractive || isOrchestrator) && (
          <div
            data-testid="node-toggle-prompt-amendment"
            className="rounded-b border border-t-0 border-dashed border-line-strong bg-bg-2 px-2 py-1.5"
          >
            <div className="mb-1 flex items-center gap-1 text-fg-4" style={{ fontSize: "9.5px" }}>
              <Lock size={9} />
              Added by PDO at spawn (
              {[isInteractive && "Interactive", isOrchestrator && "Orchestrator"].filter(Boolean).join(", ")})
            </div>
            <pre className="whitespace-pre-wrap font-mono text-fg-3" style={{ fontSize: "10.5px", lineHeight: 1.5 }}>
              {[isInteractive && "/pdo-interactive", isOrchestrator && "/pdo-orchestrate"].filter(Boolean).join("\n")}
            </pre>
          </div>
        )}
        {!isScript && <SkillPromptHint scope="node" lastAdded={lastAddedSkill ?? null} testId="node-prompt-skill-hint" />}
    </>
  );
}
