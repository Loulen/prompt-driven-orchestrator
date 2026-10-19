import { useMemo } from "react";
import TriggerFields from "./TriggerFields";
import AttachmentsField from "./AttachmentsField";
import type { NewRunForm } from "../../store/newRun/useNewRunForm";
import type { Skill } from "../../../shared/generated/Skill";
import { addRefs, resolveActiveSkills } from "../../../skill/model/skillSelection";
import { useSkillPromptField } from "../../../skill/store/useSkillPromptField";
import SkillCompletionList from "../../../skill/components/SkillCompletionList";
import SkillPromptHint from "../../../skill/components/SkillPromptHint";

/**
 * ── WHAT / WHEN ── the trigger fields, the prompt and the attachments. The
 * prompt completes skills: `/` lists the bank's skills, those the Run already
 * receives first; one it does not receive joins the Run tier.
 */
export default function WhatSection({ form }: { form: NewRunForm }) {
  const {
    input, setInput, mode, promptOptional, skills, setSkills, skillBank, inheritedSkillTiers,
    promptAddedSkills, setPromptAddedSkills,
  } = form;
  const received = useMemo(
    () =>
      new Set(
        resolveActiveSkills("run", skills, inheritedSkillTiers, skillBank)
          .rows.filter((row) => !row.missing)
          .map((row) => row.id),
      ),
    [skills, inheritedSkillTiers, skillBank],
  );
  const addSkill = (skill: Skill) => {
    const ref = { id: skill.id, name: skill.name };
    setSkills((prev) => addRefs(prev, [ref]));
    setPromptAddedSkills((prev) => addRefs(prev.filter((s) => s.id !== ref.id), [ref]));
  };
  const { fieldProps, completion, pick } = useSkillPromptField({
    value: input,
    onChange: setInput,
    bank: skillBank,
    received,
    onAdd: addSkill,
  });
  const added = promptAddedSkills.filter((a) => skills.some((s) => s.id === a.id));
  return (
    <div className="flex flex-col gap-3 py-4">
      <span className="text-fg-4 uppercase tracking-wider font-medium" style={{ fontSize: "10px" }}>
        {mode === "trigger" ? "When" : "What"}
      </span>

      {/* Schedule (Trigger mode only, #160) */}
      {mode === "trigger" && (
        <TriggerFields form={form} />
      )}

      <div className="flex flex-col gap-1.5">
        <label
          className="font-medium text-fg-2"
          style={{ fontSize: "11.5px" }}
        >
          {mode === "trigger"
            ? "Input template (optional)"
            : `Prompt${promptOptional ? " (optional)" : ""}`}
        </label>
        <textarea
          className="w-full resize-y rounded-md border border-line-strong bg-bg-3 px-2.5 py-2 font-mono text-fg placeholder:text-fg-4 focus:border-acc focus:outline-none"
          style={{ fontSize: "12px" }}
          rows={5}
          placeholder="Free-text prompt, a GitHub issue link, or a mix."
          {...fieldProps}
          data-testid="input-textarea"
        />
        <SkillCompletionList completion={completion} textarea={fieldProps.ref} onPick={pick} scope="run" />
        <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
          {mode === "trigger"
            ? "Passed as the Run's input each time the trigger fires. Required unless the pipeline is prompt-not-required."
            : promptOptional
            ? "This pipeline runs without a prompt — anything you enter is passed as additional info."
            : "Free-text prompt, an issue link, or a mix."}
        </span>
        <SkillPromptHint scope="run" lastAdded={added[added.length - 1]?.name ?? null} testId="input-skill-hint" />
      </div>

      {/* #779 Attachments — images as thumbnails, other files as chips, one
          wrapping row. One budget per run, read from Settings. */}
      <AttachmentsField form={form} />
    </div>
  );
}
