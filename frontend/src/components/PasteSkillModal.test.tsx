import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";

const createSkillMock = vi.fn();
const uploadSkillFilesMock = vi.fn();
const uploadSkillFileFromPathMock = vi.fn();
const browseFsMock = vi.fn();

vi.mock("../api", () => ({
  createSkill: (...args: unknown[]) => createSkillMock(...args),
  uploadSkillFiles: (...args: unknown[]) => uploadSkillFilesMock(...args),
  uploadSkillFileFromPath: (...args: unknown[]) => uploadSkillFileFromPathMock(...args),
  browseFs: (...args: unknown[]) => browseFsMock(...args),
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
    uploadSkillFilesMock.mockReset();
    uploadSkillFileFromPathMock.mockReset();
    browseFsMock.mockReset();
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
    await waitFor(() => expect(onCreated).toHaveBeenCalledWith(expect.objectContaining({ id: "s-1" }), 0));
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


// ---------------------------------------------------------------------------
// #671 — reference files in the popup: staged, dropped, uploaded after Create
// ---------------------------------------------------------------------------

function dropEvent(files: File[], dirs: string[] = []) {
  const items = [
    ...files.map((file) => ({ kind: "file", webkitGetAsEntry: () => ({ isDirectory: false, name: file.name }) })),
    ...dirs.map((name) => ({ kind: "file", webkitGetAsEntry: () => ({ isDirectory: true, name }) })),
  ];
  const allFiles = [...files, ...dirs.map((name) => new File([], name, { type: "" }))];
  return { dataTransfer: { files: allFiles, items, types: ["Files"], dropEffect: "none" } };
}

const SKILL = { id: "s-1", name: "tdd", description: "x", folder_id: "f-m", created_at: t, updated_at: t };

describe("PasteSkillModal reference files (#671)", () => {
  beforeEach(() => {
    createSkillMock.mockReset();
    uploadSkillFilesMock.mockReset();
    uploadSkillFileFromPathMock.mockReset();
    createSkillMock.mockResolvedValue(SKILL);
    uploadSkillFilesMock.mockImplementation(async (_id: string, files: { path: string; file: File }[]) => ({
      uploaded: files.map((f) => ({ path: f.path, size: f.file.size })),
      files: [],
    }));
  });

  it("rests with the drop bar and Browse…, no file rows", () => {
    setup();
    expect(screen.getByTestId("paste-skill-drop")).toHaveTextContent("drop files anywhere in this window");
    expect(screen.getByTestId("paste-skill-drop-browse")).toBeInTheDocument();
    expect(screen.queryByTestId("paste-skill-files")).toBeNull();
    expect(screen.getByTestId("paste-skill-create")).toHaveTextContent("Create skill");
  });

  it("a drag over the whole modal shows the overlay with the file count", () => {
    setup();
    fireEvent.dragEnter(screen.getByTestId("paste-skill-modal"), {
      dataTransfer: { types: ["Files"], items: [{ kind: "file" }, { kind: "file" }, { kind: "file" }], files: [] },
    });
    expect(screen.getByTestId("skill-drop-overlay")).toHaveTextContent("Drop to attach 3 files");
    fireEvent.dragLeave(screen.getByTestId("paste-skill-modal"), { dataTransfer: { types: ["Files"], items: [], files: [] } });
    expect(screen.queryByTestId("skill-drop-overlay")).toBeNull();
  });

  it("FP step 1: dropped files are staged (to upload) before validation, a folder is refused in place, nothing is uploaded", () => {
    setup();
    paste(VALID);
    fireEvent.drop(
      screen.getByTestId("paste-skill-modal"),
      dropEvent(
        [
          new File(["# cheatsheet"], "selectors-cheatsheet.md", { type: "text/markdown" }),
          Object.assign(new File(["spec"], "login.spec.ts", { type: "text/plain" }), { webkitRelativePath: "examples/login.spec.ts" }),
        ],
        ["fixtures"],
      ),
    );
    const rows = screen.getAllByTestId("paste-skill-file");
    expect(rows).toHaveLength(2);
    expect(rows[0]).toHaveAttribute("data-path", "selectors-cheatsheet.md");
    expect(rows[0]).toHaveTextContent("to upload");
    expect(rows[0]).toHaveTextContent("12 B");
    expect(rows[1]).toHaveAttribute("data-path", "examples/login.spec.ts");
    const refused = screen.getByTestId("paste-skill-refused");
    expect(refused).toHaveTextContent("fixtures/");
    expect(refused).toHaveTextContent("Drop files, not folders");
    expect(screen.getByTestId("paste-skill-create")).toHaveTextContent("Create skill + 2 files");
    expect(screen.getByTestId("paste-skill-preview")).toHaveTextContent("+ 2 files");
    expect(uploadSkillFilesMock).not.toHaveBeenCalled();
    expect(createSkillMock).not.toHaveBeenCalled();

    // Same-path drop replaces the row in place.
    fireEvent.drop(screen.getByTestId("paste-skill-modal"), dropEvent([new File(["# v2 longer"], "selectors-cheatsheet.md", { type: "text/markdown" })]));
    const after = screen.getAllByTestId("paste-skill-file");
    expect(after).toHaveLength(2);
    expect(after[0]).toHaveTextContent("replaces");

    // The ✕ removes a staged row.
    fireEvent.click(screen.getByLabelText("Remove examples/login.spec.ts"));
    expect(screen.getAllByTestId("paste-skill-file")).toHaveLength(1);
  });

  it("a dropped SKILL.md replaces the text (never a row), re-runs the checks, and ⌘Z undoes", async () => {
    setup();
    paste("draft");
    fireEvent.drop(screen.getByTestId("paste-skill-modal"), dropEvent([new File([VALID], "SKILL.md", { type: "text/markdown" })]));
    await waitFor(() => expect((screen.getByTestId("paste-skill-text") as HTMLTextAreaElement).value).toBe(VALID));
    expect(screen.queryByTestId("paste-skill-files")).toBeNull();
    expect(screen.getByTestId("paste-skill-replaced")).toHaveTextContent("Replaced by dropped SKILL.md");
    for (const id of ["frontmatter", "name", "description", "body", "unique"]) expect(checkState(id)).toBe("pass");

    fireEvent.keyDown(window, { key: "z", metaKey: true });
    expect((screen.getByTestId("paste-skill-text") as HTMLTextAreaElement).value).toBe("draft");
    expect(screen.queryByTestId("paste-skill-replaced")).toBeNull();
  });

  it("FP step 2: Create writes the skill, then uploads the staged files in order, and lands on the Files tab", async () => {
    const { onClose, onCreated } = setup();
    paste(VALID);
    fireEvent.drop(
      screen.getByTestId("paste-skill-modal"),
      dropEvent([new File(["a"], "a.md", { type: "text/markdown" }), new File(["bb"], "b.md", { type: "text/markdown" })]),
    );
    fireEvent.click(screen.getByTestId("paste-skill-create"));
    await waitFor(() => expect(createSkillMock).toHaveBeenCalledWith({ content: VALID, folder_id: "f-m" }));
    await waitFor(() => expect(uploadSkillFilesMock).toHaveBeenCalledTimes(2));
    expect((uploadSkillFilesMock.mock.calls[0] as [string, { path: string }[]])[1][0].path).toBe("a.md");
    expect((uploadSkillFilesMock.mock.calls[1] as [string, { path: string }[]])[1][0].path).toBe("b.md");
    await waitFor(() => expect(onCreated).toHaveBeenCalledWith(expect.objectContaining({ id: "s-1" }), 2));
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("degraded end: an upload failure keeps the popup open, says the skill was created, offers Retry / Skip, Done", async () => {
    uploadSkillFilesMock
      .mockResolvedValueOnce({ uploaded: [{ path: "a.md", size: 1 }], files: [] })
      .mockRejectedValueOnce(new ApiError("`b.md` is larger than the 10 MB limit", { status: 413, body: { code: "file_too_large" } }))
      .mockResolvedValueOnce({ uploaded: [{ path: "b.md", size: 2 }], files: [] });
    const { onClose, onCreated } = setup();
    paste(VALID);
    fireEvent.drop(
      screen.getByTestId("paste-skill-modal"),
      dropEvent([new File(["a"], "a.md", { type: "text/markdown" }), new File(["bb"], "b.md", { type: "text/markdown" })]),
    );
    fireEvent.click(screen.getByTestId("paste-skill-create"));
    await waitFor(() => expect(screen.getByTestId("paste-skill-upload-failed")).toBeInTheDocument());
    const callout = screen.getByTestId("paste-skill-upload-failed");
    expect(callout).toHaveTextContent("Skill created.");
    expect(callout).toHaveTextContent("1 of 2 files did not upload");
    const rows = screen.getAllByTestId("paste-skill-file");
    expect(rows[0]).toHaveAttribute("data-state", "uploaded");
    expect(rows[1]).toHaveAttribute("data-state", "failed");
    expect(rows[1]).toHaveTextContent("413");
    // Text and checks are frozen; the popup did not close.
    expect(screen.getByTestId("paste-skill-text")).toHaveAttribute("readonly");
    expect(screen.queryByTestId("paste-skill-checks")).toBeNull();
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByTestId("paste-skill-footer")).toHaveTextContent("already exists in the bank");

    fireEvent.click(screen.getByTestId("paste-skill-file-retry"));
    await waitFor(() => expect(screen.getAllByTestId("paste-skill-file")[1]).toHaveAttribute("data-state", "uploaded"));
    fireEvent.click(screen.getByTestId("paste-skill-done"));
    await waitFor(() => expect(onCreated).toHaveBeenCalledWith(expect.objectContaining({ id: "s-1" }), 2));
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("Skip settles a failed row and Done reports only what landed", async () => {
    uploadSkillFilesMock.mockRejectedValueOnce(new Error("boom"));
    const { onCreated } = setup();
    paste(VALID);
    fireEvent.drop(screen.getByTestId("paste-skill-modal"), dropEvent([new File(["a"], "a.md", { type: "text/markdown" })]));
    fireEvent.click(screen.getByTestId("paste-skill-create"));
    await waitFor(() => expect(screen.getByTestId("paste-skill-file-skip")).toBeInTheDocument());
    fireEvent.click(screen.getByTestId("paste-skill-file-skip"));
    expect(screen.getAllByTestId("paste-skill-file")[0]).toHaveAttribute("data-state", "skipped");
    fireEvent.click(screen.getByTestId("paste-skill-done"));
    await waitFor(() => expect(onCreated).toHaveBeenCalledWith(expect.objectContaining({ id: "s-1" }), 0));
  });

  it("Browse… stacks the explorer above the popup in multi-pick, and picks are staged as host copies", async () => {
    browseFsMock.mockResolvedValue({
      path: "/home/user/notes",
      parent: "/home/user",
      entries: [
        { name: "a.md", path: "/home/user/notes/a.md", is_dir: false, is_git_repo: false, is_symlink: false },
        { name: "b.md", path: "/home/user/notes/b.md", is_dir: false, is_git_repo: false, is_symlink: false },
      ],
      truncated: false,
      error: null,
    });
    const { onClose } = setup();
    paste(VALID);
    fireEvent.click(screen.getByTestId("paste-skill-drop-browse"));
    const modal = await screen.findByTestId("skill-file-browse-modal");
    expect(screen.getByTestId("skill-file-browse-backdrop").className).toContain("z-[70]");
    // Escape closes the explorer only, not the popup.
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByTestId("skill-file-browse-modal")).toBeNull();
    expect(onClose).not.toHaveBeenCalled();
    expect(modal).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId("paste-skill-drop-browse"));
    const entries = await screen.findAllByTestId("skill-file-browse-entry");
    fireEvent.click(entries[0]);
    fireEvent.click(entries[1]);
    fireEvent.click(screen.getByTestId("skill-file-browse-select"));
    const rows = screen.getAllByTestId("paste-skill-file");
    expect(rows.map((row) => row.getAttribute("data-path"))).toEqual(["a.md", "b.md"]);

    fireEvent.click(screen.getByTestId("paste-skill-create"));
    await waitFor(() => expect(uploadSkillFileFromPathMock).toHaveBeenCalledTimes(2));
    expect(uploadSkillFileFromPathMock).toHaveBeenCalledWith("s-1", "/home/user/notes/a.md", "a.md");
  });

  it("the drop bar is keyboard reachable: Enter opens the native picker, picked files are staged", () => {
    setup();
    const input = screen.getByTestId("paste-skill-drop-input") as HTMLInputElement;
    const click = vi.spyOn(input, "click");
    fireEvent.keyDown(screen.getByTestId("paste-skill-drop"), { key: "Enter" });
    expect(click).toHaveBeenCalled();
    fireEvent.change(input, { target: { files: [new File(["x"], "picked.md", { type: "text/markdown" })] } });
    expect(screen.getAllByTestId("paste-skill-file")[0]).toHaveAttribute("data-path", "picked.md");
  });
});
