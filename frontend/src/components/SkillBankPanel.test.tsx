import { render, screen, fireEvent, waitFor, within } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";

const fetchSkillMock = vi.fn();
const updateSkillMock = vi.fn();
const deleteSkillMock = vi.fn();
const fetchSkillReferentsMock = vi.fn();
const createSkillFolderMock = vi.fn();
const updateSkillFolderMock = vi.fn();
const deleteSkillFolderMock = vi.fn();
const createSkillMock = vi.fn();

vi.mock("../api", () => ({
  fetchSkill: (...args: unknown[]) => fetchSkillMock(...args),
  updateSkill: (...args: unknown[]) => updateSkillMock(...args),
  deleteSkill: (...args: unknown[]) => deleteSkillMock(...args),
  fetchSkillReferents: (...args: unknown[]) => fetchSkillReferentsMock(...args),
  createSkillFolder: (...args: unknown[]) => createSkillFolderMock(...args),
  updateSkillFolder: (...args: unknown[]) => updateSkillFolderMock(...args),
  deleteSkillFolder: (...args: unknown[]) => deleteSkillFolderMock(...args),
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

import SkillBankPanel from "./SkillBankPanel";
import type { Skill, SkillBank, SkillDetail, SkillFolder } from "../types";

const t = "2026-09-03T10:00:00Z";
const folder = (id: string, name: string, parent_id: string | null = null): SkillFolder => ({
  id,
  name,
  parent_id,
  created_at: t,
  updated_at: t,
});
const skill = (id: string, name: string, folder_id: string | null, description = "desc"): Skill => ({
  id,
  name,
  description,
  folder_id,
  created_at: t,
  updated_at: t,
});

const CONTENT = "---\nname: tdd\ndescription: Test-driven development.\nallowed-tools: Bash(npm:*)\n---\n\n# TDD\n\nRed, green, refactor.\n";

function detailOf(s: Skill): SkillDetail {
  return {
    ...s,
    content: CONTENT,
    frontmatter: { name: "tdd", description: "Test-driven development.", "allowed-tools": "Bash(npm:*)" },
    body: "# TDD\n\nRed, green, refactor.",
    files: [{ path: "checklist.md", size: 1229 }],
    path: `/home/user/.pdo/skills/${s.id}`,
  };
}

const EMPTY: SkillBank = { skills: [], folders: [], root_path: "/home/user/.pdo/skills" };
const POPULATED: SkillBank = {
  root_path: "/home/user/.pdo/skills",
  folders: [folder("f-m", "méthode"), folder("f-i", "ippon"), folder("f-j", "java", "f-i")],
  skills: [
    skill("s-tdd", "tdd", "f-m", "Test-driven development."),
    skill("s-gr", "grilling", null, "Grill relentlessly."),
    skill("s-cr", "code-review", null, "Review staged changes."),
  ],
};

function setup(bank: SkillBank, loaded = true) {
  const onChanged = vi.fn().mockResolvedValue(undefined);
  const view = render(<SkillBankPanel bank={bank} loaded={loaded} home="/home/user" onChanged={onChanged} />);
  return { onChanged, view };
}

describe("SkillBankPanel (#668)", () => {
  beforeEach(() => {
    for (const mock of [
      fetchSkillMock,
      updateSkillMock,
      deleteSkillMock,
      fetchSkillReferentsMock,
      createSkillFolderMock,
      updateSkillFolderMock,
      deleteSkillFolderMock,
      createSkillMock,
    ]) {
      mock.mockReset();
    }
    fetchSkillMock.mockImplementation(async (id: string) => {
      const found = POPULATED.skills.find((s) => s.id === id) ?? skill(id, id, null);
      return detailOf(found);
    });
  });

  it("FP step 1: an empty bank shows the empty state with one primary action and the disk path", () => {
    setup(EMPTY);
    expect(screen.getByTestId("skill-bank-empty")).toHaveTextContent("No skills yet");
    expect(screen.getByTestId("skill-paste-empty")).toBeInTheDocument();
    expect(screen.getByTestId("skill-bank-footer")).toHaveTextContent("0 skills · 0 folders");
    expect(screen.getByTestId("skill-bank-footer")).toHaveTextContent("~/.pdo/skills/<id>/");
    // Vocabulary visible but muted: filter + folder button are there.
    expect(screen.getByTestId("skill-filter")).toBeInTheDocument();
    expect(screen.getByTestId("skill-new-folder")).toBeInTheDocument();
  });

  it("opens the paste popup from the empty state and from the toolbar", () => {
    setup(EMPTY);
    fireEvent.click(screen.getByTestId("skill-paste-empty"));
    expect(screen.getByTestId("paste-skill-modal")).toBeInTheDocument();
    fireEvent.click(screen.getByLabelText("Close paste popup"));
    expect(screen.queryByTestId("paste-skill-modal")).toBeNull();
    fireEvent.click(screen.getByTestId("skill-paste"));
    expect(screen.getByTestId("paste-skill-modal")).toBeInTheDocument();
  });

  it("renders folders first with counts, root skills after, and the footer totals", () => {
    setup(POPULATED);
    const tree = screen.getByTestId("skill-tree");
    const items = within(tree).getAllByRole("treeitem");
    expect(items.map((item) => item.getAttribute("data-testid"))).toEqual([
      "tree-folder-f-i",
      "tree-folder-f-m",
      "tree-skill-s-cr",
      "tree-skill-s-gr",
    ]);
    expect(screen.getByTestId("tree-folder-f-m")).toHaveTextContent("1");
    expect(screen.getByTestId("skill-bank-footer")).toHaveTextContent("3 skills · 3 folders");
  });

  it("expands a folder and shows the skill beneath it (FP step 4, tree side)", () => {
    setup(POPULATED);
    fireEvent.click(screen.getByLabelText("Expand méthode"));
    expect(screen.getByTestId("tree-skill-s-tdd")).toBeInTheDocument();
    fireEvent.click(screen.getByLabelText("Collapse méthode"));
    expect(screen.queryByTestId("tree-skill-s-tdd")).toBeNull();
  });

  it("filters skills by name or description and keeps ancestor folders", () => {
    setup(POPULATED);
    fireEvent.change(screen.getByTestId("skill-filter"), { target: { value: "driven" } });
    expect(screen.getByTestId("tree-folder-f-m")).toBeInTheDocument();
    expect(screen.getByTestId("tree-skill-s-tdd")).toBeInTheDocument();
    expect(screen.queryByTestId("tree-skill-s-gr")).toBeNull();
    fireEvent.change(screen.getByTestId("skill-filter"), { target: { value: "zzz" } });
    expect(screen.getByTestId("skill-tree")).toHaveTextContent("No skill matches");
  });

  it("selecting a skill shows the read-only detail: header, id, frontmatter table, body, files tab", async () => {
    setup(POPULATED);
    fireEvent.click(screen.getByTestId("tree-skill-s-gr"));
    expect(screen.getByTestId("skill-detail-name")).toHaveTextContent("grilling");
    expect(screen.getByTestId("skill-detail-description")).toHaveTextContent("Grill relentlessly.");
    await waitFor(() => expect(fetchSkillMock).toHaveBeenCalledWith("s-gr"));
    await waitFor(() => expect(screen.getByTestId("skill-frontmatter")).toBeInTheDocument());
    const table = screen.getByTestId("skill-frontmatter");
    expect(table).toHaveTextContent("allowed-tools");
    expect(table).toHaveTextContent("Bash(npm:*)");
    expect(table).toHaveTextContent("Read-only");
    expect(screen.getByTestId("skill-body")).toHaveTextContent("Red, green, refactor.");
    // No editor anywhere in the detail.
    expect(within(screen.getByTestId("skill-detail")).queryByRole("textbox")).toBeNull();

    fireEvent.click(screen.getByRole("tab", { name: /Files/ }));
    const files = screen.getByTestId("skill-files");
    expect(files).toHaveTextContent("SKILL.md");
    expect(files).toHaveTextContent("checklist.md");
    expect(files).toHaveTextContent("1.2 kB");
    expect(within(files).getAllByText("read-only")).toHaveLength(2);
  });

  it("FP step 5: rename inline commits the label only and offers Undo", async () => {
    const { onChanged } = setup(POPULATED);
    updateSkillMock.mockResolvedValue({ ...POPULATED.skills[1], name: "grilling-hard" });
    fireEvent.click(screen.getByTestId("tree-skill-s-gr"));
    fireEvent.click(screen.getByTestId("skill-detail-rename"));
    const input = screen.getByTestId("rename-input") as HTMLInputElement;
    expect(input.value).toBe("grilling");
    fireEvent.change(input, { target: { value: "grilling-hard" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(updateSkillMock).toHaveBeenCalledWith("s-gr", { name: "grilling-hard" }));
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
    expect(screen.getByTestId("skill-toast")).toHaveTextContent("Renamed grilling to grilling-hard");
    fireEvent.click(screen.getByTestId("skill-toast-undo"));
    await waitFor(() => expect(updateSkillMock).toHaveBeenLastCalledWith("s-gr", { name: "grilling" }));
  });

  it("a rename collision (409) stays in edit and reads under the row", async () => {
    setup(POPULATED);
    updateSkillMock.mockRejectedValue(new ApiError("taken", { status: 409, body: { code: "duplicate_name" } }));
    fireEvent.click(screen.getByTestId("tree-skill-s-gr"));
    fireEvent.keyDown(screen.getByTestId("skill-tree"), { key: "F2" });
    const input = screen.getByTestId("rename-input");
    fireEvent.change(input, { target: { value: "Code-Review" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(screen.getByTestId("rename-error")).toHaveTextContent(/already taken/));
    expect(screen.getByTestId("rename-input")).toBeInTheDocument();
    fireEvent.keyDown(screen.getByTestId("rename-input"), { key: "Escape" });
    expect(screen.queryByTestId("rename-input")).toBeNull();
  });

  it("FP step 4: dropping a skill on a folder PUTs the move and offers Undo", async () => {
    const { onChanged } = setup(POPULATED);
    updateSkillMock.mockResolvedValue({ ...POPULATED.skills[1], folder_id: "f-m" });
    const dataTransfer = {
      data: {} as Record<string, string>,
      types: ["text/pdo-skill"],
      effectAllowed: "",
      dropEffect: "",
      setData(type: string, value: string) {
        this.data[type] = value;
      },
      getData(type: string) {
        return this.data[type];
      },
    };
    const row = screen.getByTestId("tree-skill-s-gr");
    const handle = row.querySelector("[draggable='true']") as HTMLElement;
    expect(handle).not.toBeNull();
    fireEvent.dragStart(handle, { dataTransfer });
    const target = screen.getByTestId("tree-folder-f-m");
    fireEvent.dragOver(target, { dataTransfer });
    expect(target).toHaveAttribute("data-drop-target", "true");
    fireEvent.drop(target, { dataTransfer });
    await waitFor(() => expect(updateSkillMock).toHaveBeenCalledWith("s-gr", { folder_id: "f-m" }));
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
    expect(screen.getByTestId("skill-toast")).toHaveTextContent("Moved grilling to méthode");
    fireEvent.click(screen.getByTestId("skill-toast-undo"));
    await waitFor(() => expect(updateSkillMock).toHaveBeenLastCalledWith("s-gr", { folder_id: null }));
  });

  it("Move to… in the detail header is the keyboard fallback to the same move", async () => {
    setup(POPULATED);
    updateSkillMock.mockResolvedValue({ ...POPULATED.skills[1], folder_id: "f-j" });
    fireEvent.click(screen.getByTestId("tree-skill-s-gr"));
    fireEvent.click(screen.getByTestId("skill-detail-move"));
    const picker = screen.getByTestId("move-picker");
    fireEvent.click(within(picker).getByRole("menuitem", { name: /ippon \/ java/ }));
    await waitFor(() => expect(updateSkillMock).toHaveBeenCalledWith("s-gr", { folder_id: "f-j" }));
  });

  it("FP step 6: Delete… lists the (empty) referents in place, then removes the skill", async () => {
    const { onChanged } = setup(POPULATED);
    fetchSkillReferentsMock.mockResolvedValue({
      skill_id: "s-gr",
      instance: false,
      projects: [],
      pipelines: [],
      runs: [],
    });
    deleteSkillMock.mockResolvedValue(undefined);
    fireEvent.click(screen.getByTestId("tree-skill-s-gr"));
    fireEvent.click(screen.getByTestId("skill-detail-delete"));
    await waitFor(() => expect(screen.getByTestId("skill-delete")).toBeInTheDocument());
    expect(screen.getByTestId("skill-delete")).toHaveTextContent("Delete grilling?");
    expect(screen.getByTestId("skill-referents")).toHaveTextContent("No live references.");
    expect(screen.getByTestId("skill-delete")).toHaveTextContent("~/.pdo/skills/s-gr/");
    // The tree stays: the operator keeps context.
    expect(screen.getByTestId("skill-tree")).toBeInTheDocument();
    expect(screen.getByTestId("skill-delete-confirm")).toHaveTextContent("Delete");
    fireEvent.click(screen.getByTestId("skill-delete-confirm"));
    await waitFor(() => expect(deleteSkillMock).toHaveBeenCalledWith("s-gr"));
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
    expect(screen.queryByTestId("skill-delete")).toBeNull();
  });

  it("renders populated referents grouped by tier (the shape the endpoint is built for)", async () => {
    setup(POPULATED);
    fetchSkillReferentsMock.mockResolvedValue({
      skill_id: "s-gr",
      instance: true,
      projects: [{ id: "p1", name: "acme-web-api" }],
      pipelines: [{ id: "l1", name: "feature-with-review", node_id: "reviewer" }],
      runs: [{ run_id: "run-e7a", name: null }],
    });
    fireEvent.click(screen.getByTestId("tree-skill-s-gr"));
    fireEvent.keyDown(screen.getByTestId("skill-tree"), { key: "Delete" });
    await waitFor(() => expect(screen.getByTestId("skill-delete")).toBeInTheDocument());
    const box = screen.getByTestId("skill-referents");
    expect(screen.getByTestId("skill-delete")).toHaveTextContent("4 live references");
    expect(box).toHaveTextContent("INSTANCE");
    expect(box).toHaveTextContent("PROJECT");
    expect(box).toHaveTextContent("acme-web-api");
    expect(box).toHaveTextContent("feature-with-review · node reviewer");
    expect(box).toHaveTextContent("run-e7a (not started)");
    expect(screen.getByTestId("skill-delete-confirm")).toHaveTextContent("Delete anyway");
  });

  it("creates a folder then enters rename; deleting a folder explains that its skills move up", async () => {
    setup(POPULATED);
    createSkillFolderMock.mockResolvedValue(folder("f-new", "New folder"));
    fireEvent.click(screen.getByTestId("skill-new-folder"));
    await waitFor(() => expect(createSkillFolderMock).toHaveBeenCalledWith({ name: "New folder", parent_id: null }));

    // Folder kebab → Delete folder.
    fireEvent.click(screen.getByTestId("tree-folder-f-m"));
    fireEvent.click(screen.getByTestId("folder-detail-delete"));
    expect(screen.getByTestId("folder-delete")).toHaveTextContent("Delete folder méthode?");
    expect(screen.getByTestId("folder-delete")).toHaveTextContent(/Its 1 skill and sub-folders move to the root/);
    expect(screen.getByTestId("folder-delete")).toHaveTextContent("No skill is deleted.");
    deleteSkillFolderMock.mockResolvedValue(undefined);
    fireEvent.click(screen.getByTestId("folder-delete-confirm"));
    await waitFor(() => expect(deleteSkillFolderMock).toHaveBeenCalledWith("f-m"));
  });

  it("the kebab offers the same verbs as the detail header", () => {
    setup(POPULATED);
    fireEvent.click(screen.getByTestId("kebab-skill-s-gr"));
    const menu = screen.getByTestId("tree-menu");
    expect(within(menu).getAllByRole("menuitem").map((m) => m.textContent)).toEqual([
      expect.stringContaining("Rename"),
      expect.stringContaining("Move to…"),
      expect.stringContaining("Copy id"),
      expect.stringContaining("Delete…"),
    ]);
    fireEvent.click(screen.getByTestId("kebab-folder-f-i"));
    const folderMenu = screen.getByTestId("tree-menu");
    expect(within(folderMenu).getAllByRole("menuitem").map((m) => m.textContent)).toEqual([
      expect.stringContaining("New subfolder"),
      expect.stringContaining("Rename"),
      expect.stringContaining("Delete folder"),
    ]);
  });

  it("keyboard: ↓ selects rows, → expands, F2 renames", () => {
    setup(POPULATED);
    const tree = screen.getByTestId("skill-tree");
    fireEvent.keyDown(tree, { key: "ArrowDown" });
    expect(screen.getByTestId("tree-folder-f-i")).toHaveAttribute("data-selected", "true");
    fireEvent.keyDown(tree, { key: "ArrowRight" });
    expect(screen.getByTestId("tree-folder-f-j")).toBeInTheDocument();
    fireEvent.keyDown(tree, { key: "ArrowDown" });
    expect(screen.getByTestId("tree-folder-f-j")).toHaveAttribute("data-selected", "true");
    fireEvent.keyDown(tree, { key: "F2" });
    expect(screen.getByTestId("rename-input")).toBeInTheDocument();
  });
});
