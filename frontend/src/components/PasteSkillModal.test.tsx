import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";

const createSkillMock = vi.fn();

vi.mock("../api", () => ({
  createSkill: (...args: unknown[]) => createSkillMock(...args),
  // Defined INSIDE the hoisted factory (a top-level class would not exist yet),
  // with the real constructor signature so tests read like production code.
  ApiError: class ApiError extends Error {
    status?: number;
    body?: unknown;
    constructor(message: string, opts: { status?: number; body?: unknown } = {}) {
      super(message);
      this.status = opts.status;
      this.body = opts.body;
    }
  },
}));

import { ApiError } from "../api";

import PasteSkillModal from "./PasteSkillModal";
import type { SkillFolder } from "../types";

const VALID = `---
name: tdd
description: Test-driven development. Red-green-refactor at pre-agreed seams.
allowed-tools: Bash(npm:*) Bash(cargo:*)
---

# Test-driven development

Red, green, refactor.
`;

const t = "2026-09-03T10:00:00Z";
const folders: SkillFolder[] = [
  { id: "f-m", name: "méthode", parent_id: null, created_at: t, updated_at: t },
  { id: "f-i", name: "ippon", parent_id: null, created_at: t, updated_at: t },
  { id: "f-j", name: "java", parent_id: "f-i", created_at: t, updated_at: t },
];

function setup(overrides: Partial<React.ComponentProps<typeof PasteSkillModal>> = {}) {
  const onClose = vi.fn();
  const onCreated = vi.fn().mockResolvedValue(undefined);
  render(
    <PasteSkillModal
      folders={folders}
      existingNames={["grilling"]}
      initialFolderId="f-m"
      onClose={onClose}
      onCreated={onCreated}
      {...overrides}
    />,
  );
  return { onClose, onCreated };
}

function checkState(id: string) {
  return screen.getByTestId(`check-${id}`).getAttribute("data-state");
}

function paste(text: string) {
  fireEvent.change(screen.getByTestId("paste-skill-text"), { target: { value: text } });
}

describe("PasteSkillModal (#668)", () => {
  beforeEach(() => {
    createSkillMock.mockReset();
  });

  it("opens with Create disabled, the five checks pending/failed, and the tree's folder pre-selected", () => {
    setup();
    expect(screen.getByTestId("paste-skill-create")).toBeDisabled();
    expect(screen.getByTestId("paste-skill-checks").children).toHaveLength(5);
    expect(checkState("frontmatter")).toBe("fail");
    expect(checkState("unique")).toBe("pending");
    // Blank text: no reason yet (nothing typed).
    expect(screen.queryByTestId("paste-skill-reason")).toBeNull();
    expect((screen.getByTestId("paste-skill-folder") as HTMLSelectElement).value).toBe("f-m");
    // Nested folders read as a path.
    expect(screen.getByRole("option", { name: "ippon / java" })).toBeInTheDocument();
  });

  it("FP step 2: a valid SKILL.md passes all five checks, previews the row, and Create posts it", async () => {
    const { onClose, onCreated } = setup();
    createSkillMock.mockResolvedValue({ id: "s-1", name: "tdd", description: "x", folder_id: "f-m" });
    paste(VALID);

    for (const id of ["frontmatter", "name", "description", "body", "unique"]) {
      expect(checkState(id)).toBe("pass");
    }
    const preview = screen.getByTestId("paste-skill-preview");
    expect(preview).toHaveTextContent("tdd");
    expect(preview).toHaveTextContent("Test-driven development. Red-green-refactor at pre-agreed seams.");
    expect(screen.queryByTestId("paste-skill-reason")).toBeNull();

    const create = screen.getByTestId("paste-skill-create");
    expect(create).toBeEnabled();
    fireEvent.click(create);

    await waitFor(() => expect(createSkillMock).toHaveBeenCalledTimes(1));
    expect(createSkillMock).toHaveBeenCalledWith({ content: VALID, folder_id: "f-m" });
    await waitFor(() => expect(onCreated).toHaveBeenCalledWith(expect.objectContaining({ id: "s-1" })));
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("FP step 3: a SKILL.md without description turns the check red, explains, and keeps Create disabled", () => {
    setup();
    paste(VALID.replace(/description: .*\n/, ""));

    expect(checkState("description")).toBe("fail");
    expect(screen.getByTestId("check-description")).toHaveTextContent("description missing");
    expect(screen.getByTestId("paste-skill-text")).toHaveAttribute("aria-invalid", "true");
    const reason = screen.getByTestId("paste-skill-reason");
    expect(reason).toHaveTextContent("Cannot create.");
    expect(reason).toHaveTextContent(/no `description`/);
    expect(reason).toHaveTextContent(/nothing was written/i);
    expect(screen.getByTestId("paste-skill-create")).toBeDisabled();
    expect(createSkillMock).not.toHaveBeenCalled();
  });

  it("detects a local case-insensitive collision before any request", () => {
    setup({ existingNames: ["TDD"] });
    paste(VALID);
    expect(checkState("unique")).toBe("fail");
    expect(screen.getByTestId("paste-skill-reason")).toHaveTextContent("Name already taken.");
    expect(screen.getByTestId("paste-skill-create")).toBeDisabled();
  });

  it("renders a daemon 409 as the failing `unique` check, in place, and re-enables once the name changes", async () => {
    setup({ existingNames: [] });
    createSkillMock.mockRejectedValueOnce(
      new ApiError("a skill named `tdd` already exists", { status: 409, body: { code: "duplicate_name" } }),
    );
    paste(VALID);
    fireEvent.click(screen.getByTestId("paste-skill-create"));

    await waitFor(() => expect(checkState("unique")).toBe("fail"));
    expect(screen.getByTestId("paste-skill-reason")).toHaveTextContent("Name already taken.");
    expect(screen.getByTestId("paste-skill-create")).toBeDisabled();
    // Still one dialog, no modal on the modal.
    expect(screen.getAllByRole("dialog")).toHaveLength(1);

    paste(VALID.replace("name: tdd", "name: tdd-strict"));
    expect(checkState("unique")).toBe("pass");
    expect(screen.getByTestId("paste-skill-create")).toBeEnabled();
  });

  it("renders a daemon 400 like a local failure, on the check its code names", async () => {
    setup({ existingNames: [] });
    createSkillMock.mockRejectedValueOnce(
      new ApiError("the frontmatter has no `description`", { status: 400, body: { code: "missing_description" } }),
    );
    paste(VALID);
    fireEvent.click(screen.getByTestId("paste-skill-create"));
    await waitFor(() => expect(checkState("description")).toBe("fail"));
    expect(screen.getByTestId("paste-skill-reason")).toHaveTextContent("the frontmatter has no `description`");
  });

  it("asks before discarding typed text; closes at once when empty", () => {
    const { onClose } = setup();
    fireEvent.click(screen.getByLabelText("Close paste popup"));
    expect(onClose).toHaveBeenCalledTimes(1);

    onClose.mockClear();
    paste("some text");
    fireEvent.click(screen.getByLabelText("Close paste popup"));
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByTestId("paste-skill-discard-prompt")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("paste-skill-discard"));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("Escape goes through the same discard guard", () => {
    const { onClose } = setup();
    paste("draft");
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByTestId("paste-skill-discard-prompt")).toBeInTheDocument();
  });

  it("lets the operator change the target folder, root included", async () => {
    setup({ existingNames: [] });
    createSkillMock.mockResolvedValue({ id: "s-2", name: "tdd", description: "x", folder_id: null });
    paste(VALID);
    fireEvent.change(screen.getByTestId("paste-skill-folder"), { target: { value: "" } });
    fireEvent.click(screen.getByTestId("paste-skill-create"));
    await waitFor(() => expect(createSkillMock).toHaveBeenCalledWith({ content: VALID, folder_id: null }));
  });
});
