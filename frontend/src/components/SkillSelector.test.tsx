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

const check = (id: string) => screen.getByTestId(`sel-check-${id}`) as HTMLInputElement;

describe("SkillSelector — folded (#849)", () => {
  it("says the count alone, and never a name", () => {
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
    expect(screen.getByTestId("sel-count")).toHaveTextContent("3 active skills");
    // The permanent list under the button is gone — with it, every name it showed.
    const root = screen.getByTestId("sel-root");
    for (const name of ["tdd", "grilling", "code-review"]) {
      expect(root).not.toHaveTextContent(name);
    }
    expect(root.querySelector("[title*='tdd']")).toBeNull();
    // …and nothing is rendered under the button at all.
    expect(root.children).toHaveLength(1);
    expect(root.children[0]).toBe(screen.getByTestId("sel"));
  });

  it.each([
    [[], "No skill"],
    [[{ id: "c", name: "code-review" }], "1 active skill"],
    [[{ id: "a", name: "tdd" }, { id: "c", name: "code-review" }], "2 active skills"],
  ])("counts %j as %s", (own, label) => {
    render(<SkillSelector tier="run" own={own} bank={bank} onChange={vi.fn()} testId="sel" />);
    expect(screen.getByTestId("sel-count")).toHaveTextContent(label);
  });

  it("carries its label on the button itself, on one row with the count", () => {
    render(<SkillSelector tier="run" own={[]} bank={bank} onChange={vi.fn()} testId="sel" label="Skills — New Run" />);
    expect(screen.getByTestId("sel")).toHaveTextContent("Skills — New Run");
    expect(screen.getByTestId("sel")).toHaveTextContent("No skill");
  });

  /** The red paragraph became an icon: same sentence, one hover away. */
  it("shows an alert icon with the message in a tooltip when a selected skill is gone", () => {
    render(
      <SkillSelector
        tier="node"
        own={[{ id: "gone", name: "deleted-skill" }, { id: "c", name: "code-review" }]}
        bank={bank}
        onChange={vi.fn()}
        testId="sel"
      />,
    );
    // The missing one is NOT counted.
    expect(screen.getByTestId("sel-count")).toHaveTextContent("1 active skill");
    const alert = screen.getByTestId("sel-alert");
    expect(alert).toHaveAttribute(
      "title",
      "Skill deleted-skill no longer exists in the bank. It is skipped; runs still start.",
    );
    // No paragraph, no name in the folded control.
    expect(screen.getByTestId("sel-root")).not.toHaveTextContent("deleted-skill");
  });

  it("has no alert icon while every selected skill is in the bank", () => {
    render(<SkillSelector tier="node" own={[{ id: "c", name: "code-review" }]} bank={bank} onChange={vi.fn()} testId="sel" />);
    expect(screen.queryByTestId("sel-alert")).toBeNull();
  });

  it("says how many are gone when several are", () => {
    render(
      <SkillSelector
        tier="node"
        own={[{ id: "gone", name: "one" }, { id: "gone2", name: "two" }]}
        bank={bank}
        onChange={vi.fn()}
        testId="sel"
      />,
    );
    expect(screen.getByTestId("sel-alert")).toHaveAttribute(
      "title",
      "2 selected skills no longer exist in the bank. They are skipped; runs still start.",
    );
  });
});

describe("SkillSelector — unfolded", () => {
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
    // The inherited one is checked and disabled, with its origin tier beside it.
    expect(check("a").checked).toBe(true);
    expect(check("a").disabled).toBe(true);
    expect(screen.getByTestId("sel-option-a")).toHaveTextContent("Instance");
    const free = check("c");
    expect(free.checked).toBe(false);
    fireEvent.click(free);
    expect(onChange).toHaveBeenLastCalledWith([{ id: "c", name: "code-review" }]);
  });

  /**
   * A pick used to fold the picker away (#837) — the list under the button said
   * what had been taken. That list is gone (#849), so the popover IS the reading:
   * it stays open, and a second tick is one click away instead of one reopen.
   */
  it("stays open on a pick, so two skills are two clicks", () => {
    const onChange = vi.fn();
    const { rerender } = render(
      <SkillSelector tier="run" own={[]} bank={bank} onChange={onChange} testId="sel" />,
    );
    fireEvent.click(screen.getByTestId("sel"));
    fireEvent.click(check("a"));
    expect(screen.getByTestId("sel-popover")).toBeInTheDocument();
    rerender(<SkillSelector tier="run" own={[{ id: "a", name: "tdd" }]} bank={bank} onChange={onChange} testId="sel" />);
    fireEvent.click(check("b"));
    expect(onChange).toHaveBeenLastCalledWith([
      { id: "a", name: "tdd" },
      { id: "b", name: "grilling" },
    ]);
    expect(screen.getByTestId("sel-popover")).toBeInTheDocument();
    // The button still folds it away.
    fireEvent.click(screen.getByTestId("sel"));
    expect(screen.queryByTestId("sel-popover")).toBeNull();
  });

  it("unchecks an own skill from its box", () => {
    const onChange = vi.fn();
    render(
      <SkillSelector
        tier="run"
        own={[{ id: "a", name: "tdd" }, { id: "c", name: "code-review" }]}
        bank={bank}
        onChange={onChange}
        testId="sel"
      />,
    );
    fireEvent.click(screen.getByTestId("sel"));
    expect(check("a").checked).toBe(true);
    expect(check("a").disabled).toBe(false);
    fireEvent.click(check("a"));
    expect(onChange).toHaveBeenLastCalledWith([{ id: "c", name: "code-review" }]);
  });

  /**
   * The name is what people aim at, and a picker that only answers on its
   * 14-pixel checkbox reads as broken — the *First run* tour says "tick both
   * skills" and sends a first-time user straight at the row (#824).
   */
  it("toggles a skill from its name, not only from the checkbox", () => {
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
    fireEvent.click(screen.getByTestId("sel-label-c"));
    expect(onChange).toHaveBeenLastCalledWith([{ id: "c", name: "code-review" }]);

    // An inherited skill has no gesture to offer: the name is as locked as the box.
    onChange.mockClear();
    const inheritedLabel = screen.getByTestId("sel-label-a") as HTMLButtonElement;
    expect(inheritedLabel.disabled).toBe(true);
    fireEvent.click(inheritedLabel);
    expect(onChange).not.toHaveBeenCalled();
  });

  it("checking a folder checks its skills at this instant; unchecking removes them", () => {
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

  it("names a row from the bank even when the stored label is stale", () => {
    render(<SkillSelector tier="node" own={[{ id: "a", name: "old-label" }]} bank={bank} onChange={vi.fn()} testId="sel" />);
    fireEvent.click(screen.getByTestId("sel"));
    expect(screen.getByTestId("sel-option-a")).toHaveTextContent("tdd");
    expect(screen.getByTestId("sel-option-a")).not.toHaveTextContent("old-label");
  });

  it("says the bank is empty instead of an empty list", () => {
    render(<SkillSelector tier="instance" own={[]} bank={{ skills: [], folders: [], root_path: "" }} onChange={vi.fn()} testId="sel" />);
    expect(screen.getByTestId("sel-count")).toHaveTextContent("No skill");
    fireEvent.click(screen.getByTestId("sel"));
    expect(screen.getByTestId("sel-empty")).toHaveTextContent("The bank is empty");
  });
});

describe("SkillSelector — skills the bank lost", () => {
  it("lists them struck through at the head of the popover, and unchecking drops the reference", () => {
    const onChange = vi.fn();
    render(
      <SkillSelector
        tier="node"
        own={[{ id: "gone", name: "deleted-skill" }, { id: "c", name: "code-review" }]}
        bank={bank}
        onChange={onChange}
        testId="sel"
      />,
    );
    fireEvent.click(screen.getByTestId("sel"));
    const head = screen.getByTestId("sel-missing");
    const row = screen.getByTestId("sel-missing-gone");
    expect(head).toContainElement(row);
    expect(row).toHaveTextContent("deleted-skill");
    expect(row.querySelector(".line-through")).not.toBeNull();
    // At the head: before the bank's tree, which is where the eye lands first.
    expect(head.compareDocumentPosition(screen.getByTestId("sel-tree")))
      .toBe(Node.DOCUMENT_POSITION_FOLLOWING);

    const box = check("gone");
    expect(box.checked).toBe(true);
    expect(box.disabled).toBe(false);
    fireEvent.click(box);
    expect(onChange).toHaveBeenLastCalledWith([{ id: "c", name: "code-review" }]);
  });

  it("locks a missing skill that another tier selected", () => {
    render(
      <SkillSelector
        tier="node"
        own={[]}
        inherited={[{ tier: "project", skills: [{ id: "gone", name: "deleted-skill" }] }]}
        bank={bank}
        onChange={vi.fn()}
        testId="sel"
      />,
    );
    fireEvent.click(screen.getByTestId("sel"));
    const row = screen.getByTestId("sel-missing-gone");
    expect(row).toHaveAttribute("data-own", "false");
    expect(row).toHaveTextContent("Project");
    expect(check("gone").disabled).toBe(true);
  });

  it("has no missing head when nothing is missing", () => {
    render(<SkillSelector tier="node" own={[{ id: "c", name: "code-review" }]} bank={bank} onChange={vi.fn()} testId="sel" />);
    fireEvent.click(screen.getByTestId("sel"));
    expect(screen.queryByTestId("sel-missing")).toBeNull();
  });
});

describe("SkillSelector — read-only", () => {
  const readOnly = (onChange = vi.fn()) =>
    render(
      <SkillSelector
        tier="node"
        own={[{ id: "c", name: "code-review" }]}
        inherited={[{ tier: "run", skills: [{ id: "a", name: "tdd" }] }]}
        bank={bank}
        onChange={onChange}
        testId="sel"
        readOnly
      />,
    );

  it("opens the popover all the same, with every box frozen", () => {
    const onChange = vi.fn();
    readOnly(onChange);
    fireEvent.click(screen.getByTestId("sel"));
    expect(screen.getByTestId("sel-popover")).toBeInTheDocument();
    // Own, inherited, folder: nothing answers.
    expect(check("c").disabled).toBe(true);
    expect(check("a").disabled).toBe(true);
    expect((screen.getByTestId("sel-folder-check-f-method") as HTMLInputElement).disabled).toBe(true);
    expect((screen.getByTestId("sel-label-c") as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(check("c"));
    fireEvent.click(screen.getByTestId("sel-label-c"));
    fireEvent.click(screen.getByTestId("sel-folder-check-f-method"));
    expect(onChange).not.toHaveBeenCalled();
    // …and it says why the boxes do not answer.
    expect(screen.getByTestId("sel-popover")).toHaveTextContent("read-only");
  });

  it("still reads what is selected", () => {
    readOnly();
    expect(screen.getByTestId("sel-count")).toHaveTextContent("2 active skills");
    fireEvent.click(screen.getByTestId("sel"));
    expect(check("c").checked).toBe(true);
    expect(check("a").checked).toBe(true);
  });

  it("cannot uncheck a missing skill either", () => {
    const onChange = vi.fn();
    render(
      <SkillSelector
        tier="node"
        own={[{ id: "gone", name: "deleted-skill" }]}
        bank={bank}
        onChange={onChange}
        testId="sel"
        readOnly
      />,
    );
    fireEvent.click(screen.getByTestId("sel"));
    expect(check("gone").disabled).toBe(true);
    fireEvent.click(check("gone"));
    expect(onChange).not.toHaveBeenCalled();
  });
});

describe("SkillSelector — dismissal", () => {
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
