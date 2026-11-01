import { useMemo } from "react";
import { Info, Lock } from "lucide-react";
import { Section } from "../../../shared/ui/kit";
import { firstName, presenceStyle } from "../../model/presence";
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
  const typingCue =
    typing.length > 0 ? (
      <span
        className="ei-keep pi-typing"
        style={presenceStyle(typing[0])}
        title={`${typing.map(firstName).join(", ")} ${typing.length > 1 ? "are" : "is"} typing`}
        data-testid="prompt-typing"
      >
        <ParticipantAvatar participant={typing[0]} size="sm" />
        <span className="pi-typing__dots" aria-hidden>
          <i />
          <i />
          <i />
        </span>
        <span className="ei-trunc">
          {typing.map(firstName).join(", ")} {typing.length > 1 ? "are" : "is"} typing
        </span>
      </span>
    ) : null;
  // #248: a script node's body is bash, run deterministically (no LLM); its I/O
  // arrives as PDO_* env vars, not a prose preamble. The variables are one hover away.
  const scriptHelp = isScript ? (
    <span
      className="ei-keep ei-dim"
      role="img"
      aria-label="Script environment"
      title={SCRIPT_HELP}
      data-testid="script-help"
    >
      <Info size={12} strokeWidth={1.5} aria-hidden />
    </span>
  ) : null;
  return (
    <Section
      title={isScript ? "Script" : "Prompt"}
      flush
      aside={
        typingCue || scriptHelp ? (
          <>
            {typingCue}
            {scriptHelp}
          </>
        ) : undefined
      }
    >
      <div className="ei-prompt">
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
          aria-label={isScript ? "Script" : "Prompt"}
          title={isScript ? undefined : "/ inserts a skill; one this node does not receive joins its skills"}
          wrap={isScript ? "off" : undefined}
          rows={10}
          placeholder={isScript ? "#!/usr/bin/env bash" : "Role prompt · {{inputs.<port>}}"}
        />
        {!isScript && (
          <SkillCompletionList completion={completion} textarea={fieldProps.ref} onPick={pick} scope="node" />
        )}
      </div>

      {/* #723 / #588 — the invocation lines PDO files at spawn for the toggles
          that are on, in spawn order (interactive, then orchestrate). Rendered
          under the field, outside it, never editable: toggle and text cannot
          drift. Byte-for-byte the daemon's `toggle_invocations`. */}
      {!isScript && (isInteractive || isOrchestrator) && (
        <div
          className="ei-launch"
          title={`Added at launch (${[isInteractive && "Interactive", isOrchestrator && "Orchestrator"].filter(Boolean).join(", ")})`}
          data-testid="node-toggle-prompt-amendment"
        >
          <Lock size={11} strokeWidth={1.75} aria-hidden />
          <span>Added at launch</span>
          {[isInteractive && "/pdo-interactive", isOrchestrator && "/pdo-orchestrate"]
            .filter((line): line is string => typeof line === "string")
            .map((line) => (
              <code key={line} className="ei-launch__line">
                {line}
              </code>
            ))}
        </div>
      )}
      {/* The completion says it once it added a skill; how to call it is in the prompt's tooltip. */}
      {!isScript && lastAddedSkill && (
        <div className="ei-pad">
          <SkillPromptHint scope="node" lastAdded={lastAddedSkill} testId="node-prompt-skill-hint" />
        </div>
      )}
    </Section>
  );
}

const SCRIPT_HELP =
  "Bash, run deterministically. $PDO_INPUT_<PORT>, $PDO_OUTPUT_<PORT>, $PDO_ARTIFACTS_DIR, $PDO_VAR_<NAME>. " +
  "Write output.md to $PDO_OUTPUT_<PORT> (frontmatter drives a when: edge). Exit 0 completes; non-zero or timeout fails.";
