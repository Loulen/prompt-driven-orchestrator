import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Skill } from "../../shared/generated/Skill";
import type { SkillFolder } from "../../shared/generated/SkillFolder";
import { useSkillPromptField } from "../store/useSkillPromptField";
import SkillCompletionList from "./SkillCompletionList";

const t = "2026-10-19T10:00:00Z";
const skill = (id: string, name: string, folder_id: string | null = null): Skill => ({
  id,
  name,
  description: "",
  folder_id,
  locked: false,
  created_at: t,
  updated_at: t,
});
const folders: SkillFolder[] = [{ id: "f-t", name: "Team", parent_id: null, created_at: t, updated_at: t }];
const bank = {
  folders,
  skills: [skill("s-cr", "code-review", "f-t"), skill("s-rc", "review-checklist", "f-t"), skill("s-tdd", "tdd")],
};

function Prompt({ onAdd, initial = "" }: { onAdd: (skill: Skill) => void; initial?: string }) {
  const [value, setValue] = useState(initial);
  const { fieldProps, completion, pick } = useSkillPromptField({
    value,
    onChange: setValue,
    bank,
    received: new Set(["s-cr"]),
    onAdd,
  });
  return (
    <>
      <textarea data-testid="prompt" {...fieldProps} />
      <SkillCompletionList completion={completion} textarea={fieldProps.ref} onPick={pick} scope="node" />
    </>
  );
}

describe("the skills completion of a prompt", () => {
  it("opens on /, lists the received skills first, filters as you type", async () => {
    render(<Prompt onAdd={vi.fn()} />);
    await userEvent.type(screen.getByTestId("prompt"), "Use /");
    const list = screen.getByTestId("skill-completion-list");
    expect(list).toHaveTextContent("On this node");
    expect(list).toHaveTextContent("Skill bank");
    const names = screen.getAllByRole("option").map((option) => option.getAttribute("data-testid"));
    expect(names).toEqual([
      "skill-completion-option-code-review",
      "skill-completion-option-review-checklist",
      "skill-completion-option-tdd",
    ]);
    expect(screen.getByTestId("skill-completion-option-review-checklist")).toHaveTextContent("Team/");
    expect(screen.getByTestId("skill-completion-option-review-checklist")).toHaveTextContent("adds it to the node");

    await userEvent.type(screen.getByTestId("prompt"), "td");
    expect(screen.getAllByRole("option")).toHaveLength(1);
  });

  it("inserts the skill picked with the arrows and Enter, and adds one the node lacks", async () => {
    const onAdd = vi.fn();
    render(<Prompt onAdd={onAdd} />);
    const prompt = screen.getByTestId("prompt");
    await userEvent.type(prompt, "then /rev");
    await userEvent.keyboard("{ArrowDown}{Enter}");
    expect(prompt).toHaveValue("then /review-checklist ");
    expect(onAdd).toHaveBeenCalledWith(expect.objectContaining({ id: "s-rc" }));
    expect(screen.queryByTestId("skill-completion-list")).toBeNull();
  });

  it("adds nothing for a skill the node already receives", async () => {
    const onAdd = vi.fn();
    render(<Prompt onAdd={onAdd} />);
    await userEvent.type(screen.getByTestId("prompt"), "/code");
    await userEvent.keyboard("{Tab}");
    expect(screen.getByTestId("prompt")).toHaveValue("/code-review ");
    expect(onAdd).not.toHaveBeenCalled();
  });

  it("closes on Escape without inserting", async () => {
    const onAdd = vi.fn();
    render(<Prompt onAdd={onAdd} />);
    await userEvent.type(screen.getByTestId("prompt"), "/re");
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByTestId("skill-completion-list")).toBeNull();
    expect(screen.getByTestId("prompt")).toHaveValue("/re");
    expect(onAdd).not.toHaveBeenCalled();
  });

  it("stays closed in the middle of a path or a URL", async () => {
    render(<Prompt onAdd={vi.fn()} />);
    await userEvent.type(screen.getByTestId("prompt"), "src/re https://x.io/tdd");
    expect(screen.queryByTestId("skill-completion-list")).toBeNull();
  });

  it("opens above the caret when the prompt sits at the bottom of the screen", async () => {
    render(<Prompt onAdd={vi.fn()} />);
    const prompt = screen.getByTestId("prompt");
    const bottom = window.innerHeight - 2;
    vi.spyOn(prompt, "getBoundingClientRect").mockReturnValue(
      DOMRect.fromRect({ x: 10, y: bottom, width: 300, height: 40 }),
    );
    await userEvent.type(prompt, "/");
    const list = screen.getByTestId("skill-completion-list");
    expect(list).toHaveAttribute("data-placement", "above");
    expect(list.style.bottom).toBe(`${window.innerHeight - bottom + 2}px`);
    expect(list.style.top).toBe("");
  });
});
