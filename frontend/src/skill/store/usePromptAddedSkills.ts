import { useState } from "react";
import type { Skill } from "../../shared/generated/Skill";
import type { SkillRef } from "../../shared/generated/SkillRef";
import { addRefs } from "../model/skillSelection";

/**
 * The skills a prompt's `/` completion added to a tier's own selection (a
 * node's): `add` writes the skill into `own` through `onChange` and remembers
 * it for `owner`; `added` is what is still selected, for the « added » chips of
 * the selector and the line under the prompt. Another owner starts afresh.
 */
export function usePromptAddedSkills(owner: string, own: SkillRef[], onChange: (skills: SkillRef[]) => void) {
  const [memo, setMemo] = useState<{ owner: string; skills: SkillRef[] }>({ owner: "", skills: [] });
  const added = memo.owner === owner ? memo.skills.filter((a) => own.some((s) => s.id === a.id)) : [];
  const add = (skill: Skill) => {
    const ref = { id: skill.id, name: skill.name };
    onChange(addRefs(own, [ref]));
    setMemo((prev) => ({
      owner,
      skills: addRefs(prev.owner === owner ? prev.skills.filter((s) => s.id !== ref.id) : [], [ref]),
    }));
  };
  return { added, add };
}
