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
const rescanSkillFolderMock = vi.fn();
const updateSkillFolderFromSourceMock = vi.fn();
const fetchRecentSkillSourcesMock = vi.fn();
const fetchSkillFileMock = vi.fn();
const writeSkillFileMock = vi.fn();
const deleteSkillFileMock = vi.fn();
const uploadSkillFilesMock = vi.fn();
const uploadSkillFileFromPathMock = vi.fn();
const browseFsMock = vi.fn();

vi.mock("../api", () => ({
  rescanSkillFolder: (...args: unknown[]) => rescanSkillFolderMock(...args),
  updateSkillFolderFromSource: (...args: unknown[]) => updateSkillFolderFromSourceMock(...args),
  fetchRecentSkillSources: (...args: unknown[]) => fetchRecentSkillSourcesMock(...args),
  scanSkillSource: vi.fn(),
  cancelSkillScan: vi.fn(),
  importSkills: vi.fn(),
  fetchSkill: (...args: unknown[]) => fetchSkillMock(...args),
  updateSkill: (...args: unknown[]) => updateSkillMock(...args),
  deleteSkill: (...args: unknown[]) => deleteSkillMock(...args),
  fetchSkillReferents: (...args: unknown[]) => fetchSkillReferentsMock(...args),
  createSkillFolder: (...args: unknown[]) => createSkillFolderMock(...args),
  updateSkillFolder: (...args: unknown[]) => updateSkillFolderMock(...args),
  deleteSkillFolder: (...args: unknown[]) => deleteSkillFolderMock(...args),
  createSkill: (...args: unknown[]) => createSkillMock(...args),
  fetchSkillFile: (...args: unknown[]) => fetchSkillFileMock(...args),
  writeSkillFile: (...args: unknown[]) => writeSkillFileMock(...args),
  deleteSkillFile: (...args: unknown[]) => deleteSkillFileMock(...args),
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
const SOURCE_URL = "https://github.com/anthropics/skills";
const sourceFolder: SkillFolder = {
  ...folder("f-src", "anthropics/skills"),
  source: { url: SOURCE_URL, ref: "main", commit: "3f9c2e1deadbeef", path: "skills", imported_at: t, found: 14, invalid: 1 },
};
const importedSkill = (id: string, name: string, path: string): Skill => ({
  ...skill(id, name, "f-src", `${name} desc`),
  source: { url: SOURCE_URL, ref: "main", commit: "3f9c2e1deadbeef", path },
});
const WITH_SOURCE: SkillBank = {
  root_path: "/home/user/.pdo/skills",
  folders: [folder("f-m", "craft"), sourceFolder],
  skills: [
    skill("s-tdd", "tdd", "f-m", "Test-driven development."),
    importedSkill("s-pdf", "pdf", "skills/pdf"),
    importedSkill("s-fd", "frontend-design", "skills/frontend-design"),
  ],
};
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
    fetchSkillFileMock,
    writeSkillFileMock,
    deleteSkillFileMock,
    uploadSkillFilesMock,
    uploadSkillFileFromPathMock,
    browseFsMock,
    rescanSkillFolderMock,
    updateSkillFolderFromSourceMock,
    fetchRecentSkillSourcesMock,
  ]) {
    mock.mockReset();
  }
  fetchRecentSkillSourcesMock.mockResolvedValue({ sources: [] });
  fetchSkillFileMock.mockImplementation(async (_id: string, path: string) =>
    path === "SKILL.md"
      ? { path, size: CONTENT.length, binary: false, text: CONTENT }
      : { path, size: 12, binary: false, text: "- [ ] one\n" },
  );
  writeSkillFileMock.mockImplementation(async (_id: string, path: string, text: string) => ({ path, size: text.length }));
  deleteSkillFileMock.mockResolvedValue(undefined);
  uploadSkillFilesMock.mockImplementation(async (_id: string, files: { path: string; file: File }[]) => ({
    uploaded: files.map((f) => ({ path: f.path, size: f.file.size })),
    files: [],
  }));
  uploadSkillFileFromPathMock.mockImplementation(async (_id: string, fromPath: string) => {
    const name = fromPath.split("/").pop();
    return { uploaded: [{ path: name, size: 7 }], files: [] };
  });
  fetchSkillMock.mockImplementation(async (id: string) => {
    const found = POPULATED.skills.find((s) => s.id === id) ?? skill(id, id, null);
    return detailOf(found);
  });
});

describe("SkillBankPanel (#668)", () => {

  it("FP step 1: an empty bank shows the empty state with import primary, paste secondary, and the disk path", () => {
    setup(EMPTY);
    expect(screen.getByTestId("skill-bank-empty")).toHaveTextContent("No skills yet");
    expect(screen.getByTestId("skill-import-empty")).toBeInTheDocument();
    expect(screen.getByTestId("skill-paste-empty")).toBeInTheDocument();
    expect(screen.getByTestId("skill-bank-empty")).not.toHaveTextContent("later ticket");
    expect(screen.getByTestId("skill-bank-footer")).toHaveTextContent("0 skills · 0 folders");
    expect(screen.getByTestId("skill-bank-footer")).toHaveTextContent("~/.pdo/skills/<id>/");
    // Vocabulary visible but muted: filter + folder button are there.
    expect(screen.getByTestId("skill-filter")).toBeInTheDocument();
    expect(screen.getByTestId("skill-new-folder")).toBeInTheDocument();
  });

  it("opens the paste popup from the empty state and from the + Add menu", () => {
    setup(EMPTY);
    fireEvent.click(screen.getByTestId("skill-paste-empty"));
    expect(screen.getByTestId("paste-skill-modal")).toBeInTheDocument();
    fireEvent.click(screen.getByLabelText("Close paste popup"));
    expect(screen.queryByTestId("paste-skill-modal")).toBeNull();
    // The toolbar's primary is "+ Add ▾" (#670): two entries.
    fireEvent.click(screen.getByTestId("skill-add"));
    const menu = screen.getByTestId("skill-add-menu");
    expect(within(menu).getAllByRole("menuitem").map((m) => m.textContent)).toEqual([
      expect.stringContaining("Paste SKILL.md…"),
      expect.stringContaining("Import from a source…"),
    ]);
    fireEvent.click(screen.getByTestId("skill-paste"));
    expect(screen.getByTestId("paste-skill-modal")).toBeInTheDocument();
    expect(screen.queryByTestId("skill-add-menu")).toBeNull();
  });

  it("opens the import popup from the empty state and from the + Add menu, pre-filled with the selected folder", () => {
    setup(EMPTY);
    fireEvent.click(screen.getByTestId("skill-import-empty"));
    expect(screen.getByTestId("import-skills-modal")).toBeInTheDocument();
    fireEvent.click(screen.getByLabelText("Close import popup"));
    expect(screen.queryByTestId("import-skills-modal")).toBeNull();
    fireEvent.click(screen.getByTestId("skill-add"));
    fireEvent.click(screen.getByTestId("skill-import"));
    expect(screen.getByTestId("import-skills-modal")).toBeInTheDocument();
  });

  it("a Source folder shows a glyph in the tree, a badge and its provenance in the detail", () => {
    setup(WITH_SOURCE);
    expect(screen.getByTestId("tree-source-f-src")).toBeInTheDocument();
    expect(screen.queryByTestId("tree-source-f-m")).toBeNull();
    fireEvent.click(screen.getByTestId("tree-folder-f-src"));
    expect(screen.getByTestId("folder-source-badge")).toHaveTextContent("Source");
    const provenance = screen.getByTestId("folder-provenance");
    expect(provenance).toHaveTextContent(SOURCE_URL);
    expect(provenance).toHaveTextContent("main");
    expect(provenance).toHaveTextContent("3f9c2e1");
    expect(provenance).toHaveTextContent("skills/");
    expect(provenance).toHaveTextContent("2 of 14 skills found at the source");
    expect(provenance).toHaveTextContent("1 invalid");
    expect(screen.getByTestId("folder-update-from-source")).toBeInTheDocument();
    // A plain folder has none of it.
    fireEvent.click(screen.getByTestId("tree-folder-f-m"));
    expect(screen.queryByTestId("folder-provenance")).toBeNull();
    // Deleting a Source folder says the link is lost, the skills keep theirs.
    fireEvent.click(screen.getByTestId("tree-folder-f-src"));
    fireEvent.click(screen.getByTestId("folder-detail-delete"));
    expect(screen.getByTestId("folder-delete")).toHaveTextContent("The link to github.com/anthropics/skills is lost");
    expect(screen.getByTestId("folder-delete")).toHaveTextContent("the skills keep their own provenance");
  });

  it("an imported skill links its provenance to the Source folder", async () => {
    setup(WITH_SOURCE);
    fireEvent.click(screen.getByLabelText("Expand anthropics/skills"));
    fireEvent.click(screen.getByTestId("tree-skill-s-pdf"));
    const provenance = screen.getByTestId("skill-detail-provenance");
    expect(provenance).toHaveTextContent("github.com/anthropics/skills@3f9c2e1 · skills/pdf");
    fireEvent.click(within(provenance).getByRole("button"));
    await waitFor(() => expect(screen.getByTestId("folder-detail")).toBeInTheDocument());
    expect(screen.getByTestId("folder-source-badge")).toBeInTheDocument();
  });

  it("FP step 4: Update from source re-scans, shows the diff in the right panel, then updates", async () => {
    const { onChanged } = setup(WITH_SOURCE);
    let resolveRescan: (v: unknown) => void = () => undefined;
    rescanSkillFolderMock.mockImplementation(() => new Promise((r) => (resolveRescan = r)));
    fireEvent.click(screen.getByTestId("tree-folder-f-src"));
    fireEvent.click(screen.getByTestId("folder-update-from-source"));
    expect(screen.getByTestId("folder-rescan")).toHaveTextContent("Re-scanning github.com/anthropics/skills@main");
    expect(rescanSkillFolderMock).toHaveBeenCalledWith("f-src", expect.any(String));
    resolveRescan({
      scan_id: "scan-u",
      source: { kind: "git", url: SOURCE_URL, ref: "main", path: "skills", repo: "anthropics/skills", suggested_folder: "anthropics/skills · skills" },
      previous_commit: "3f9c2e1deadbeef",
      commit: "8a1d07bfeedface",
      entries: [
        { path: "skills/pdf", name: "pdf", description: "d", status: "updated", skill_id: "s-pdf", skill_md_changed: true, files_added: 1, files_removed: 0, files_changed: 1 },
        { path: "skills/frontend-design", name: "frontend-design", description: "d", status: "unchanged", skill_id: "s-fd", skill_md_changed: false, files_added: 0, files_removed: 0, files_changed: 0 },
        { path: "skills/webapp-testing", name: "webapp-testing", description: "d", status: "new", skill_md_changed: false, files_added: 0, files_removed: 0, files_changed: 0 },
        { path: "skills/code-review", name: "code-review-anthropic", description: "d", status: "skipped", skill_id: "s-x", reason: "already in “craft”", skill_md_changed: false, files_added: 0, files_removed: 0, files_changed: 0 },
        { path: "skills/old", name: "old", description: "d", status: "gone", skill_id: "s-old", skill_md_changed: false, files_added: 0, files_removed: 0, files_changed: 0 },
      ],
    });
    await waitFor(() => expect(screen.getByTestId("folder-update")).toBeInTheDocument());
    expect(screen.getByTestId("folder-update")).toHaveTextContent("Update anthropics/skills from its source?");
    expect(screen.getByTestId("folder-update-summary")).toHaveTextContent("3f9c2e1 → 8a1d07b");
    expect(screen.getByTestId("folder-update-summary")).toHaveTextContent("1 skill changed");
    const updated = screen.getByTestId("update-entry-pdf");
    expect(updated).toHaveTextContent("updated");
    expect(updated).toHaveTextContent("SKILL.md changed · +1 reference file · 1 reference file changed");
    expect(within(updated).getByRole("checkbox")).toBeChecked();
    expect(within(screen.getByTestId("update-entry-frontend-design")).getByRole("checkbox")).toBeDisabled();
    const fresh = screen.getByTestId("update-entry-webapp-testing");
    expect(fresh).toHaveTextContent("new at source · not imported");
    expect(within(fresh).getByRole("checkbox")).not.toBeChecked();
    expect(screen.getByTestId("update-entry-code-review-anthropic")).toHaveTextContent("skipped");
    expect(screen.getByTestId("update-entry-old")).toHaveTextContent("gone from source");
    expect(screen.getByTestId("folder-update")).toHaveTextContent("Runs already started keep their frozen copy.");
    expect(screen.getByTestId("folder-update-confirm")).toHaveTextContent("Update 1 skill");
    // Nothing was written yet.
    expect(updateSkillFolderFromSourceMock).not.toHaveBeenCalled();

    // Opt the new one in, confirm.
    fireEvent.click(within(fresh).getByRole("checkbox"));
    expect(screen.getByTestId("folder-update-confirm")).toHaveTextContent("Update 2 skills");
    updateSkillFolderFromSourceMock.mockResolvedValue({
      folder: { ...sourceFolder, source: { ...sourceFolder.source!, commit: "8a1d07bfeedface" } },
      imported: [{ path: "skills/pdf", action: "updated", skill: WITH_SOURCE.skills[1] }, { path: "skills/webapp-testing", action: "imported", skill: skill("s-wt", "webapp-testing", "f-src") }],
      failed: [],
      commit: "8a1d07bfeedface",
    });
    fireEvent.click(screen.getByTestId("folder-update-confirm"));
    await waitFor(() =>
      expect(updateSkillFolderFromSourceMock).toHaveBeenCalledWith("f-src", {
        scan_id: "scan-u",
        items: [
          { path: "skills/pdf", action: "update" },
          { path: "skills/webapp-testing", action: "import" },
        ],
      }),
    );
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
    expect(screen.queryByTestId("folder-update")).toBeNull();
    expect(screen.getByTestId("skill-toast")).toHaveTextContent("Updated 2 skills from github.com/anthropics/skills");
    expect(screen.queryByTestId("skill-toast-undo")).toBeNull();
  });

  it("a failed re-scan reads in place and drops back to the folder detail", async () => {
    setup(WITH_SOURCE);
    rescanSkillFolderMock.mockRejectedValue(new ApiError("fatal: could not read Username", { status: 502, body: { code: "clone_failed" } }));
    fireEvent.click(screen.getByTestId("tree-folder-f-src"));
    fireEvent.click(screen.getByTestId("folder-update-from-source"));
    await waitFor(() => expect(screen.getByTestId("skill-bank-error")).toHaveTextContent("could not read Username"));
    expect(screen.getByTestId("folder-detail")).toBeInTheDocument();
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
    expect(files).toHaveTextContent("1.2 KB");
    // #671: files are editable, the read-only badge is gone; the editor opens on click only.
    expect(within(files).queryByText("read-only")).toBeNull();
    expect(within(files).queryByTestId("skill-file-editor")).toBeNull();
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


// ---------------------------------------------------------------------------
// #671 — reference files: Files tab, drop, delete, edit
// ---------------------------------------------------------------------------

function dropEvent(files: File[], dirs: string[] = []) {
  const items = [
    ...files.map((file) => ({ kind: "file", webkitGetAsEntry: () => ({ isDirectory: false, name: file.name }) })),
    ...dirs.map((name) => ({ kind: "file", webkitGetAsEntry: () => ({ isDirectory: true, name }) })),
  ];
  const allFiles = [...files, ...dirs.map((name) => new File([], name, { type: "" }))];
  return {
    dataTransfer: {
      files: allFiles,
      items,
      types: ["Files"],
      dropEffect: "none",
    },
  };
}

async function openFilesTab() {
  fireEvent.click(screen.getByTestId("tree-skill-s-gr"));
  await waitFor(() => expect(fetchSkillMock).toHaveBeenCalledWith("s-gr"));
  fireEvent.click(screen.getByRole("tab", { name: /Files/ }));
  await waitFor(() => expect(screen.getByTestId("skill-files")).toHaveTextContent("checklist.md"));
}

describe("SkillBankPanel Files tab (#671)", () => {
  it("lists SKILL.md first then the files with sizes, a trash on each reference file, and the disk path", async () => {
    setup(POPULATED);
    await openFilesTab();
    const rows = screen.getAllByTestId("skill-file-row");
    expect(rows[0]).toHaveAttribute("data-path", "SKILL.md");
    expect(rows[1]).toHaveAttribute("data-path", "checklist.md");
    expect(rows[1]).toHaveTextContent("1.2 KB");
    expect(within(rows[0]).queryByTestId("skill-file-delete")).toBeNull();
    expect(within(rows[1]).getByTestId("skill-file-delete")).toBeInTheDocument();
    expect(screen.getByTestId("skill-files-footer")).toHaveTextContent("~/.pdo/skills/");
    expect(screen.getByTestId("skill-files-drop")).toBeInTheDocument();
  });

  it("FP step 3: a drop on the detail uploads immediately, refreshes the list, and refuses a folder in place", async () => {
    setup(POPULATED);
    await openFilesTab();
    const before = fetchSkillMock.mock.calls.length;
    fireEvent.drop(screen.getByTestId("skill-detail"), dropEvent([new File(["# n"], "notes.md", { type: "text/markdown" })], ["fixtures"]));
    await waitFor(() => expect(uploadSkillFilesMock).toHaveBeenCalledTimes(1));
    const [id, files] = uploadSkillFilesMock.mock.calls[0] as [string, { path: string; file: File }[]];
    expect(id).toBe("s-gr");
    expect(files[0].path).toBe("notes.md");
    // The list is re-read and a toast confirms.
    await waitFor(() => expect(fetchSkillMock.mock.calls.length).toBeGreaterThan(before));
    await waitFor(() => expect(screen.getByTestId("skill-toast")).toHaveTextContent("Added notes.md"));
    const refused = screen.getByTestId("skill-file-refused");
    expect(refused).toHaveTextContent("fixtures/");
    expect(refused).toHaveTextContent("Drop files, not folders");
  });

  it("a file drag over the SKILL.md tab switches to Files and shows the overlay", async () => {
    setup(POPULATED);
    fireEvent.click(screen.getByTestId("tree-skill-s-gr"));
    await waitFor(() => expect(screen.getByTestId("skill-body")).toBeInTheDocument());
    fireEvent.dragEnter(screen.getByTestId("skill-detail"), {
      dataTransfer: { types: ["Files"], items: [{ kind: "file" }, { kind: "file" }], files: [] },
    });
    expect(screen.getByTestId("skill-drop-overlay")).toHaveTextContent("Drop to attach 2 files");
    expect(screen.getByRole("tab", { name: /Files/ })).toHaveAttribute("aria-selected", "true");
  });

  it("FP step 3: the trash asks inline (Keep / Delete) and Delete removes the file", async () => {
    setup(POPULATED);
    await openFilesTab();
    fireEvent.click(screen.getByTestId("skill-file-delete"));
    const confirm = screen.getByTestId("skill-file-delete-confirm");
    expect(confirm).toHaveTextContent("Delete this file?");
    fireEvent.click(within(confirm).getByText("Keep"));
    expect(screen.queryByTestId("skill-file-delete-confirm")).toBeNull();
    expect(deleteSkillFileMock).not.toHaveBeenCalled();

    fireEvent.click(screen.getByTestId("skill-file-delete"));
    fireEvent.click(screen.getByTestId("skill-file-delete-yes"));
    await waitFor(() => expect(deleteSkillFileMock).toHaveBeenCalledWith("s-gr", "checklist.md"));
    await waitFor(() => expect(screen.getByTestId("skill-toast")).toHaveTextContent("Deleted checklist.md"));
  });

  it("clicking a file opens the plain-text editor; Save PUTs, Revert restores, the dot tracks unsaved", async () => {
    setup(POPULATED);
    await openFilesTab();
    fireEvent.click(within(screen.getAllByTestId("skill-file-row")[1]).getByTestId("skill-file-open"));
    await waitFor(() => expect(fetchSkillFileMock).toHaveBeenCalledWith("s-gr", "checklist.md"));
    const text = (await screen.findByTestId("skill-file-text")) as HTMLTextAreaElement;
    expect(text.value).toBe("- [ ] one\n");
    expect(screen.getByTestId("skill-file-save")).toBeDisabled();

    fireEvent.change(text, { target: { value: "- [x] one\n" } });
    expect(screen.getByTestId("skill-file-editor")).toHaveAttribute("data-dirty", "true");
    expect(screen.getByTestId("skill-file-editor-dot")).toHaveAttribute("aria-label", "unsaved");
    fireEvent.click(screen.getByTestId("skill-file-revert"));
    expect(text.value).toBe("- [ ] one\n");

    fireEvent.change(text, { target: { value: "- [x] done\n" } });
    fireEvent.keyDown(text, { key: "s", metaKey: true });
    await waitFor(() => expect(writeSkillFileMock).toHaveBeenCalledWith("s-gr", "checklist.md", "- [x] done\n"));
    await waitFor(() => expect(screen.getByTestId("skill-file-editor")).not.toHaveAttribute("data-dirty"));
  });

  it("a binary file shows its size instead of a textarea", async () => {
    fetchSkillFileMock.mockResolvedValueOnce({ path: "checklist.md", size: 2048, binary: true, text: null });
    setup(POPULATED);
    await openFilesTab();
    fireEvent.click(within(screen.getAllByTestId("skill-file-row")[1]).getByTestId("skill-file-open"));
    await waitFor(() => expect(screen.getByTestId("skill-file-binary")).toHaveTextContent("binary file · 2.0 KB"));
    expect(screen.queryByTestId("skill-file-text")).toBeNull();
  });

  it("switching skill with unsaved changes asks first: Stay keeps the editor, Discard leaves", async () => {
    setup(POPULATED);
    await openFilesTab();
    fireEvent.click(within(screen.getAllByTestId("skill-file-row")[1]).getByTestId("skill-file-open"));
    const text = (await screen.findByTestId("skill-file-text")) as HTMLTextAreaElement;
    fireEvent.change(text, { target: { value: "dirty" } });

    fireEvent.click(screen.getByTestId("tree-skill-s-cr"));
    const prompt = screen.getByTestId("skill-file-leave-prompt");
    expect(prompt).toHaveTextContent("Unsaved changes.");
    // Still on grilling.
    expect(screen.getByTestId("skill-detail-name")).toHaveTextContent("grilling");
    fireEvent.click(screen.getByTestId("skill-file-leave-stay"));
    expect(screen.queryByTestId("skill-file-leave-prompt")).toBeNull();
    expect(screen.getByTestId("skill-detail-name")).toHaveTextContent("grilling");

    fireEvent.click(screen.getByTestId("tree-skill-s-cr"));
    fireEvent.click(screen.getByTestId("skill-file-leave-discard"));
    await waitFor(() => expect(screen.getByTestId("skill-detail-name")).toHaveTextContent("code-review"));
    expect(writeSkillFileMock).not.toHaveBeenCalled();
  });

  it("a dropped SKILL.md that passes the five checks is saved on the spot and badged", async () => {
    setup(POPULATED);
    await openFilesTab();
    // Named after the skill being edited: another skill's name would trip the `unique` check.
    const replacement = CONTENT.replace("name: tdd", "name: grilling").replace("Red, green, refactor.", "Replaced by drop.");
    fireEvent.drop(screen.getByTestId("skill-detail"), dropEvent([new File([replacement], "SKILL.md", { type: "text/markdown" })]));
    await waitFor(() => expect(writeSkillFileMock).toHaveBeenCalledWith("s-gr", "SKILL.md", replacement));
    expect(uploadSkillFilesMock).not.toHaveBeenCalled();
    await waitFor(() => expect(screen.getAllByTestId("skill-file-row")[0]).toHaveTextContent("replaced by drop"));
    expect(screen.getAllByTestId("skill-file-row")[0]).toHaveTextContent("5 / 5 checks pass · saved");
  });

  it("a dropped SKILL.md that fails a check opens the editor unsaved with the reason, nothing written", async () => {
    setup(POPULATED);
    await openFilesTab();
    const broken = CONTENT.replace("name: tdd", "name: grilling").replace(/description: .*\n/, "");
    fireEvent.drop(screen.getByTestId("skill-detail"), dropEvent([new File([broken], "SKILL.md", { type: "text/markdown" })]));
    const editor = await screen.findByTestId("skill-file-editor");
    expect(editor).toHaveAttribute("data-path", "SKILL.md");
    await waitFor(() => expect((screen.getByTestId("skill-file-text") as HTMLTextAreaElement).value).toBe(broken));
    expect(screen.getByTestId("skill-file-error")).toHaveTextContent(/no `description`/);
    expect(editor).toHaveAttribute("data-dirty", "true");
    expect(writeSkillFileMock).not.toHaveBeenCalled();
  });

  it("Browse… opens the explorer in multi-pick and copies the picks from the host", async () => {
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
    setup(POPULATED);
    await openFilesTab();
    fireEvent.click(screen.getByTestId("skill-files-drop-browse"));
    const entries = await screen.findAllByTestId("skill-file-browse-entry");
    fireEvent.click(entries[0]);
    fireEvent.click(entries[1]);
    expect(screen.getByTestId("skill-file-browse-select")).toHaveTextContent("Add 2 files");
    fireEvent.click(screen.getByTestId("skill-file-browse-select"));
    await waitFor(() => expect(uploadSkillFileFromPathMock).toHaveBeenCalledTimes(2));
    expect(uploadSkillFileFromPathMock).toHaveBeenCalledWith("s-gr", "/home/user/notes/a.md", "a.md");
    expect(uploadSkillFileFromPathMock).toHaveBeenCalledWith("s-gr", "/home/user/notes/b.md", "b.md");
  });
});
