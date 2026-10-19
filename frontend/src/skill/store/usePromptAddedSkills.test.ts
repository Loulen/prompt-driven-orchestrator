import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { Skill } from "../../shared/generated/Skill";
import type { SkillRef } from "../../shared/generated/SkillRef";
import { usePromptAddedSkills } from "./usePromptAddedSkills";

const t = "2026-10-19T10:00:00Z";
const skill = (id: string, name: string): Skill => ({
  id,
  name,
  description: "",
  folder_id: null,
  locked: false,
  created_at: t,
  updated_at: t,
});

describe("usePromptAddedSkills", () => {
  it("writes the skill into the selection and remembers it for its owner only", () => {
    const onChange = vi.fn();
    const { result, rerender } = renderHook(
      ({ owner, own }: { owner: string; own: SkillRef[] }) => usePromptAddedSkills(owner, own, onChange),
      { initialProps: { owner: "reviewer", own: [{ id: "s-cr", name: "code-review" }] } },
    );
    act(() => result.current.add(skill("s-rc", "review-checklist")));
    expect(onChange).toHaveBeenCalledWith([
      { id: "s-cr", name: "code-review" },
      { id: "s-rc", name: "review-checklist" },
    ]);
    const own = onChange.mock.calls[0][0] as SkillRef[];
    rerender({ owner: "reviewer", own });
    expect(result.current.added).toEqual([{ id: "s-rc", name: "review-checklist" }]);

    // Unchecked in the selector: the chip goes.
    rerender({ owner: "reviewer", own: [own[0]] });
    expect(result.current.added).toEqual([]);
    // Another node: nothing was added there.
    rerender({ owner: "designer", own });
    expect(result.current.added).toEqual([]);
  });
});
