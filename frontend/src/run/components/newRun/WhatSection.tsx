import { useMemo } from "react";
import AttachmentsField from "./AttachmentsField";
import VariableOverrides from "./VariableOverrides";
import FormRow from "../../../shared/ui/FormRow";
import type { NewRunForm } from "../../store/newRun/useNewRunForm";
import type { Skill } from "../../../shared/generated/Skill";
import { addRefs, resolveActiveSkills } from "../../../skill/model/skillSelection";
import { useSkillPromptField } from "../../../skill/store/useSkillPromptField";
import SkillCompletionList from "../../../skill/components/SkillCompletionList";
import SkillPromptHint from "../../../skill/components/SkillPromptHint";

/**
 * ── WHAT ── the prompt (a Trigger's input template), the attachments and the
 * pipeline's variables. The prompt completes skills: `/` lists the bank's
 * skills, those the Run already receives first; one it does not receive joins
 * the Run tier.
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
  const trigger = mode === "trigger";
  return (
    <>
      <FormRow
        label={trigger ? "Input" : "Prompt"}
        top
        htmlFor="new-run-input"
        title={
          trigger
            ? "The Run's input at each fire; {{guard.stdout}} is the guard's output"
            : promptOptional
              ? "Optional for this pipeline"
              : "Free text, an issue link, or both; / completes a skill"
        }
      >
        <div className="ln-col ln-grow">
          <textarea
            id="new-run-input"
            className="d-input d-area ln-area"
            rows={4}
            placeholder={trigger ? "{{guard.stdout}}" : "Free-text prompt or issue URL"}
            {...fieldProps}
            data-testid="input-textarea"
          />
          <SkillCompletionList completion={completion} textarea={fieldProps.ref} onPick={pick} scope="run" />
          {added.length > 0 && (
            <SkillPromptHint scope="run" lastAdded={added[added.length - 1].name} testId="input-skill-hint" />
          )}
        </div>
      </FormRow>

      {/* #779 Attachments — images as thumbnails, other files as chips, one
          wrapping row. One budget per run, read from Settings. */}
      {mode === "run" && <AttachmentsField form={form} />}

      <VariableOverrides form={form} />
    </>
  );
}
