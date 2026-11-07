import { render, screen, fireEvent, waitFor, within } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";

const scanArchiveMock = vi.fn();
const importMock = vi.fn();

vi.mock("../api", () => ({
  scanSkillSource: vi.fn(),
  scanSkillArchive: (...args: unknown[]) => scanArchiveMock(...args),
  cancelSkillScan: vi.fn(),
  importSkills: (...args: unknown[]) => importMock(...args),
  fetchRecentSkillSources: vi.fn().mockResolvedValue({ sources: [] }),
  browseFs: vi.fn(),
  ApiError: class ApiError extends Error {},
}));

import ImportSkillsModal from "./ImportSkillsModal";
import type { Skill, SkillCandidate, SkillFolder, SkillScanResult } from "../types";

const t = "2026-11-05T10:00:00Z";
const folders: SkillFolder[] = [
  { id: "f-team", name: "team-review", parent_id: null, created_at: t, updated_at: t },
  { id: "f-ops", name: "ops", parent_id: null, created_at: t, updated_at: t },
];

const changed = (name: string, folderId: string, folderName: string): SkillCandidate => ({
  path: name,
  name,
  description: `The ${name} method.`,
  valid: true,
  file_count: 1,
  id: `id-${name}`,
  status: "id_changed",
  existing: { id: `id-${name}`, name, folder_id: folderId, folder_name: folderName },
});

const SCAN: SkillScanResult = {
  scan_id: "scan-c",
  source: { kind: "file", url: "team-review.skills.zip", ref: null, path: "", repo: "team-review", suggested_folder: "team-review" },
  commit: null,
  candidates: [
    changed("kept-one", "f-ops", "ops"),
    changed("replaced-one", "f-ops", "ops"),
    changed("moved-one", "f-ops", "ops"),
    changed("duplicated-one", "f-ops", "ops"),
    changed("code-review", "f-team", "team-review"),
    {
      path: "review-checklist",
      name: "review-checklist",
      description: "Checklist.",
      valid: true,
      file_count: 0,
      id: "id-rc",
      status: "id_up_to_date",
      existing: { id: "id-rc", name: "review-checklist", folder_id: "f-team", folder_name: "team-review" },
    },
    {
      path: "pdo-interactive",
      name: "pdo-interactive",
      description: "Talk.",
      valid: true,
      file_count: 0,
      id: "pdo-interactive",
      status: "locked",
    },
    {
      path: "draft-skill",
      name: "draft-skill",
      description: "",
      valid: false,
      reason: "missing `description`",
      code: "missing_description",
      file_count: 0,
      status: "invalid",
    },
  ],
  elsewhere: [],
  elsewhere_count: 0,
  archive: {
    file_name: "team-review.skills.zip",
    format: "pdo_archive",
    existing_folder: { id: "f-team", name: "team-review" },
    new_folder_name: "team-review 2",
  },
};

const NAMES = ["kept-one", "replaced-one", "moved-one", "duplicated-one", "duplicated-one-2", "code-review", "review-checklist"];

function setup() {
  const onClose = vi.fn();
  const onImported = vi.fn().mockResolvedValue(undefined);
  const onShowFolder = vi.fn();
  render(
    <ImportSkillsModal
      folders={folders}
      existingNames={NAMES}
      initialFolderId={null}
      onClose={onClose}
      onImported={onImported}
      onShowFolder={onShowFolder}
    />,
  );
  return { onClose, onImported, onShowFolder };
}

async function listed() {
  scanArchiveMock.mockResolvedValue(SCAN);
  fireEvent.change(screen.getByTestId("import-archive-drop-input"), {
    target: { files: [new File(["PK"], "team-review.skills.zip", { type: "application/zip" })] },
  });
  await waitFor(() => expect(screen.getByTestId("import-skills-modal")).toHaveAttribute("data-step", "results"));
}

const choose = (name: string, label: string) =>
  fireEvent.click(within(screen.getByTestId(`import-id-choice-${name}`)).getByRole("radio", { name: label }));
const explain = (name: string) => screen.getByTestId(`import-id-explain-${name}`);

const skill = (id: string, name: string, folder_id: string): Skill =>
  ({ id, name, description: "", folder_id, locked: false, created_at: t, updated_at: t }) as Skill;

describe("ImportSkillsModal — known id, different content (#1028)", () => {
  beforeEach(() => {
    scanArchiveMock.mockReset();
    importMock.mockReset();
  });

  it("offers Keep as is · Replace · Move · Duplicate on each changed row, Keep preselected, nothing to write", async () => {
    setup();
    await listed();
    for (const name of ["kept-one", "replaced-one", "moved-one", "duplicated-one", "code-review"]) {
      const group = within(screen.getByTestId(`import-id-choice-${name}`));
      expect(group.getAllByRole("radio").map((r) => r.textContent)).toEqual(["Keep as is", "Replace", "Move", "Duplicate"]);
      expect(group.getByRole("radio", { name: "Keep as is" })).toHaveAttribute("aria-checked", "true");
    }
    // An up-to-date row needs no overwrite choice.
    expect(screen.queryByTestId("import-id-choice-review-checklist")).toBeNull();
    expect(explain("kept-one")).toHaveTextContent("The bank's copy in “ops” stays untouched; the archive's version is ignored.");
    // Only « Keep as is »: nothing would be written.
    expect(screen.getByTestId("import-submit")).toBeDisabled();
    expect(screen.getByTestId("import-summary")).toHaveTextContent("nothing would be written");
    expect(screen.getByTestId("import-not-touched")).toHaveTextContent(
      "5 kept as is · 1 already up to date · 0 unchecked · 1 managed by PDO · 1 invalid.",
    );
  });

  it("explains each choice against the destination", async () => {
    setup();
    await listed();
    choose("replaced-one", "Replace");
    expect(explain("replaced-one")).toHaveTextContent("takes the archive's content and files; it keeps its id and stays in “ops”");
    choose("moved-one", "Move");
    expect(explain("moved-one")).toHaveTextContent("Replaced, then moved from “ops” to “team-review 2”");
    choose("duplicated-one", "Duplicate");
    expect(explain("duplicated-one")).toHaveTextContent("A copy under a new id lands in “team-review 2”");
    // Merging into the folder the skill already lives in: Move is Replace.
    choose("code-review", "Move");
    fireEvent.click(within(screen.getByTestId("import-dest-merge")).getByRole("radio"));
    expect(explain("code-review")).toHaveTextContent("Same as Replace here: the skill already lives in “team-review”.");
  });

  it("Duplicate pre-fills a free suffixed name, checks it live, and blocks Import while it is taken", async () => {
    setup();
    await listed();
    choose("duplicated-one", "Duplicate");
    const field = screen.getByTestId("import-duplicate-duplicated-one") as HTMLInputElement;
    expect(field.value).toBe("duplicated-one-3");
    expect(screen.getByTestId("import-duplicate-free-duplicated-one")).toHaveTextContent("free");
    expect(screen.getByTestId("import-submit")).toHaveTextContent("Import 1 skill");
    expect(screen.getByTestId("import-submit")).toBeEnabled();
    fireEvent.change(field, { target: { value: "kept-one" } });
    expect(screen.getByTestId("import-duplicate-taken-duplicated-one")).toHaveTextContent("taken");
    expect(screen.getByTestId("import-submit")).toBeDisabled();
    expect(screen.getByTestId("import-duplicate-unresolved")).toBeInTheDocument();
    fireEvent.change(field, { target: { value: "duplicated-one-team" } });
    expect(screen.getByTestId("import-submit")).toBeEnabled();
  });

  it("sends the four actions, then stays open on a per-line report", async () => {
    const { onImported, onClose, onShowFolder } = setup();
    await listed();
    choose("replaced-one", "Replace");
    choose("moved-one", "Move");
    choose("duplicated-one", "Duplicate");
    expect(screen.getByTestId("import-summary")).toHaveTextContent("1 replaced");
    expect(screen.getByTestId("import-summary")).toHaveTextContent("1 moved");
    expect(screen.getByTestId("import-summary")).toHaveTextContent("1 duplicated");
    expect(screen.getByTestId("import-submit")).toHaveTextContent("Import 3 skills");

    const target: SkillFolder = { id: "f-new", name: "team-review 2", parent_id: null, created_at: t, updated_at: t };
    importMock.mockResolvedValue({
      folder: target,
      imported: [
        { path: "replaced-one", action: "replaced", skill: skill("id-replaced-one", "replaced-one", "f-ops") },
        { path: "moved-one", action: "moved", previous_folder_id: "f-ops", skill: skill("id-moved-one", "moved-one", "f-new") },
        { path: "duplicated-one", action: "duplicated", skill: skill("id-copy", "duplicated-one-3", "f-new") },
      ],
      failed: [],
      commit: null,
    });
    fireEvent.click(screen.getByTestId("import-submit"));
    await waitFor(() => expect(screen.getByTestId("import-skills-modal")).toHaveAttribute("data-step", "report"));
    expect(importMock.mock.calls[0][0].items).toEqual([
      { path: "replaced-one", action: "replace" },
      { path: "moved-one", action: "move" },
      { path: "duplicated-one", action: "duplicate", name: "duplicated-one-3" },
      { path: "kept-one", action: "keep" },
      { path: "code-review", action: "keep" },
      { path: "review-checklist", action: "keep" },
    ]);
    expect(onImported).toHaveBeenCalledWith(expect.anything(), true, true);
    expect(onClose).not.toHaveBeenCalled();

    expect(screen.getByTestId("import-report-title")).toHaveTextContent("Import done · 3 written");
    const line = (name: string) => screen.getByTestId(`import-report-${name}`);
    expect(line("kept-one")).toHaveTextContent("kept as is in “ops”");
    expect(line("replaced-one")).toHaveTextContent("replaced, stays in “ops”");
    expect(line("moved-one")).toHaveTextContent("replaced, moved “ops” → “team-review 2”");
    expect(line("duplicated-one")).toHaveTextContent("duplicated as “duplicated-one-3” · new id");
    expect(line("review-checklist")).toHaveTextContent("already up to date in “team-review”");
    expect(line("pdo-interactive")).toHaveTextContent("managed by PDO here — not imported");
    expect(line("draft-skill")).toHaveTextContent("invalid: missing `description`");
    expect(line("draft-skill")).toHaveAttribute("data-tone", "failed");
    expect(screen.queryByTestId("import-change-source")).toBeNull();

    fireEvent.click(screen.getByTestId("import-report-show-folder"));
    expect(screen.getByTestId("import-report-show-folder")).toHaveTextContent("Show “team-review 2” in the bank");
    expect(onShowFolder).toHaveBeenCalledWith("f-new");
    expect(onClose).toHaveBeenCalled();
  });

  it("reports a failed row in red and a replace that kept its name", async () => {
    setup();
    await listed();
    choose("replaced-one", "Replace");
    choose("duplicated-one", "Duplicate");
    importMock.mockResolvedValue({
      folder: { id: "f-new", name: "team-review 2", parent_id: null, created_at: t, updated_at: t },
      imported: [
        {
          path: "replaced-one",
          action: "replaced",
          warning: "the name “x” is taken in the bank: kept “replaced-one”",
          skill: skill("id-replaced-one", "replaced-one", "f-ops"),
        },
      ],
      failed: [{ path: "duplicated-one", error: "`duplicated-one-3` is already taken", code: "duplicate_name" }],
      commit: null,
    });
    fireEvent.click(screen.getByTestId("import-submit"));
    await waitFor(() => expect(screen.getByTestId("import-report")).toBeInTheDocument());
    expect(screen.getByTestId("import-report-title")).toHaveTextContent("Import done · 1 written · 1 failed");
    expect(screen.getByTestId("import-report-duplicated-one")).toHaveTextContent("failed: `duplicated-one-3` is already taken");
    expect(screen.getByTestId("import-report-warning-replaced-one")).toHaveTextContent("kept “replaced-one”");
    fireEvent.click(screen.getByTestId("import-report-close"));
  });
});
