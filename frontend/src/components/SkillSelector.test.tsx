import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import SkillSelector from "./SkillSelector";
import type { Skill, SkillBank, SkillFolder } from "../types";

const skill = (id: string, name: string, folder_id: string | null = null): Skill => ({
  id,
  name,
  description: `${name} description`,
  folder_id,
  created_at: "2026-09-03T00:00:00.000Z",
  updated_at: "2026-09-03T00:00:00.000Z",
});
const folder = (id: string, name: string, parent_id: string | null = null): SkillFolder => ({
  id,
  name,
  parent_id,
  created_at: "2026-09-03T00:00:00.000Z",
  updated_at: "2026-09-03T00:00:00.000Z",
});

const bank: SkillBank = {
  root_path: "/tmp/.pdo/skills",
  folders: [folder("f-method", "method")],
  skills: [
    skill("a", "tdd", "f-method"),
    skill("b", "grilling", "f-method"),
    skill("c", "code-review"),
  ],
};

describe("SkillSelector", () => {
  it("shows own + inherited with their origin tier and the effective total (FP step 2)", () => {
    render(
      <SkillSelector
        tier="node"
        own={[{ id: "c", name: "code-review" }]}
        inherited={[
          { tier: "instance", skills: [{ id: "a", name: "tdd" }] },
          { tier: "project", skills: [{ id: "b", name: "grilling" }], label: "Product" },
        ]}
        bank={bank}
        onChange={vi.fn()}
        testId="sel"
      />,
    );
    expect(screen.getByTestId("sel-count")).toHaveTextContent("3 effective skills");
    const a = screen.getByTestId("sel-row-a");
    expect(a).toHaveTextContent("tdd");
    expect(a).toHaveTextContent("Instance");
    expect(a).toHaveAttribute("data-inherited", "true");
    expect(a).toHaveAttribute("data-own", "false");
    expect(screen.getByTestId("sel-row-b")).toHaveTextContent("Project");
    const c = screen.getByTestId("sel-row-c");
    expect(c).toHaveAttribute("data-own", "true");
    expect(c).toHaveTextContent("Node");
    // Only an own skill can be removed here: inherited ones stay (additive union).
    expect(screen.getByTestId("sel-remove-c")).toBeInTheDocument();
    expect(screen.queryByTestId("sel-remove-a")).toBeNull();
  });

  it("checks and unchecks a skill of the bank, keeping inherited ones greyed and locked", () => {
    const onChange = vi.fn();
    render(
      <SkillSelector
        tier="run"
        own={[]}
        inherited={[{ tier: "instance", skills: [{ id: "a", name: "tdd" }] }]}
        bank={bank}
        onChange={onChange}
        testId="sel"
      />,
    );
    fireEvent.click(screen.getByTestId("sel"));
    expect(screen.getByTestId("sel-popover")).toBeInTheDocument();
    // The inherited one is checked and disabled; the free one is unchecked.
    const inherited = screen.getByTestId("sel-check-a") as HTMLInputElement;
    expect(inherited.checked).toBe(true);
    expect(inherited.disabled).toBe(true);
    const free = screen.getByTestId("sel-check-c") as HTMLInputElement;
    expect(free.checked).toBe(false);
    fireEvent.click(free);
    expect(onChange).toHaveBeenLastCalledWith([{ id: "c", name: "code-review" }]);
  });

  it("checking a folder checks its skills at this instant; unchecking removes them (FP step 3)", () => {
    const onChange = vi.fn();
    const { rerender } = render(
      <SkillSelector tier="run" own={[{ id: "c", name: "code-review" }]} bank={bank} onChange={onChange} testId="sel" />,
    );
    fireEvent.click(screen.getByTestId("sel"));
    fireEvent.click(screen.getByTestId("sel-folder-check-f-method"));
    expect(onChange).toHaveBeenLastCalledWith([
      { id: "c", name: "code-review" },
      { id: "a", name: "tdd" },
      { id: "b", name: "grilling" },
    ]);
    // With every skill of the folder own, the folder reads checked and unchecks them all.
    rerender(
      <SkillSelector
        tier="run"
        own={[{ id: "c", name: "code-review" }, { id: "a", name: "tdd" }, { id: "b", name: "grilling" }]}
        bank={bank}
        onChange={onChange}
        testId="sel"
      />,
    );
    const box = screen.getByTestId("sel-folder-check-f-method") as HTMLInputElement;
    expect(box.checked).toBe(true);
    fireEvent.click(box);
    expect(onChange).toHaveBeenLastCalledWith([{ id: "c", name: "code-review" }]);
  });

  it("warns on an id the bank no longer has, without dropping it (FP step 4)", () => {
    render(
      <SkillSelector
        tier="node"
        own={[{ id: "gone", name: "deleted-skill" }, { id: "c", name: "code-review" }]}
        bank={bank}
        onChange={vi.fn()}
        testId="sel"
      />,
    );
    expect(screen.getByTestId("sel-count")).toHaveTextContent("1 effective skill");
    const row = screen.getByTestId("sel-row-gone");
    expect(row).toHaveAttribute("data-missing", "true");
    expect(row).toHaveTextContent("deleted-skill");
    expect(screen.getByTestId("sel-missing")).toHaveTextContent("no longer exists in the bank");
    expect(screen.getByTestId("sel-missing")).toHaveTextContent("runs still start");
  });

  it("names a row from the bank even when the stored label is stale", () => {
    render(<SkillSelector tier="node" own={[{ id: "a", name: "old-label" }]} bank={bank} onChange={vi.fn()} testId="sel" />);
    expect(screen.getByTestId("sel-row-a")).toHaveTextContent("tdd");
    expect(screen.getByTestId("sel-row-a")).not.toHaveTextContent("old-label");
  });

  it("says the bank is empty instead of an empty list", () => {
    render(<SkillSelector tier="instance" own={[]} bank={{ skills: [], folders: [], root_path: "" }} onChange={vi.fn()} testId="sel" />);
    expect(screen.getByTestId("sel-count")).toHaveTextContent("No skill");
    fireEvent.click(screen.getByTestId("sel"));
    expect(screen.getByTestId("sel-empty")).toHaveTextContent("The bank is empty");
  });

  it("closes on a mousedown outside the picker (#686)", () => {
    render(<SkillSelector tier="node" own={[]} bank={bank} onChange={vi.fn()} testId="sel" />);
    fireEvent.click(screen.getByTestId("sel"));
    expect(screen.getByTestId("sel-popover")).toBeInTheDocument();
    fireEvent.mouseDown(document.body);
    expect(screen.queryByTestId("sel-popover")).toBeNull();
    expect(screen.getByTestId("sel")).toHaveAttribute("aria-expanded", "false");
  });

  it("stays open on a mousedown inside the picker (#686)", () => {
    render(<SkillSelector tier="node" own={[]} bank={bank} onChange={vi.fn()} testId="sel" />);
    fireEvent.click(screen.getByTestId("sel"));
    fireEvent.mouseDown(screen.getByTestId("sel-option-c"));
    expect(screen.getByTestId("sel-popover")).toBeInTheDocument();
  });

  it("closes on Escape (#686)", () => {
    render(<SkillSelector tier="node" own={[]} bank={bank} onChange={vi.fn()} testId="sel" />);
    fireEvent.click(screen.getByTestId("sel"));
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByTestId("sel-popover")).toBeNull();
    expect(screen.getByTestId("sel")).toHaveAttribute("aria-expanded", "false");
  });
});
