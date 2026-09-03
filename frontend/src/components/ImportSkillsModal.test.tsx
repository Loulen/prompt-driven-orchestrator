import { render, screen, fireEvent, waitFor, within } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";

const scanMock = vi.fn();
const cancelMock = vi.fn();
const importMock = vi.fn();
const recentMock = vi.fn();

vi.mock("../api", () => ({
  scanSkillSource: (...args: unknown[]) => scanMock(...args),
  cancelSkillScan: (...args: unknown[]) => cancelMock(...args),
  importSkills: (...args: unknown[]) => importMock(...args),
  fetchRecentSkillSources: (...args: unknown[]) => recentMock(...args),
  browseFs: vi.fn().mockResolvedValue({ path: "/home/user", parent: null, entries: [], truncated: false, error: null }),
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
import ImportSkillsModal from "./ImportSkillsModal";
import type { SkillFolder, SkillScanResult } from "../types";

const t = "2026-09-03T10:00:00Z";
const folders: SkillFolder[] = [
  { id: "f-craft", name: "craft", parent_id: null, created_at: t, updated_at: t },
];
const URL = "https://github.com/anthropics/skills/tree/main/skills/engineering";

const SCAN: SkillScanResult = {
  scan_id: "scan-x",
  source: {
    kind: "git",
    url: "https://github.com/anthropics/skills",
    ref: "main",
    path: "skills/engineering",
    repo: "anthropics/skills",
    suggested_folder: "anthropics/skills · engineering",
  },
  commit: "3f9c2e1deadbeef",
  candidates: [
    { path: "skills/engineering/pdf", name: "pdf", description: "Extract text and tables from PDFs.", valid: true, file_count: 3, status: "new" },
    { path: "skills/engineering/webapp-testing", name: "webapp-testing", description: "Drive a local web app.", valid: true, file_count: 0, status: "new" },
    {
      path: "skills/engineering/code-review",
      name: "code-review",
      description: "Structured review of a diff.",
      valid: true,
      file_count: 0,
      status: "name_taken",
      existing: { id: "s-cr", name: "code-review", folder_id: "f-craft", folder_name: "craft" },
    },
    { path: "skills/engineering/mcp-builder", name: "mcp-builder", description: "Build an MCP server.", valid: true, file_count: 0, status: "new" },
    {
      path: "skills/engineering/skill-creator",
      name: "skill-creator",
      description: "",
      valid: false,
      reason: "the frontmatter has no `description`; the harness would ignore this skill",
      code: "missing_description",
      file_count: 0,
      status: "invalid",
    },
    {
      path: "skills/engineering/frontend-design",
      name: "frontend-design",
      description: "Opinionated UI generation.",
      valid: true,
      file_count: 0,
      status: "same_commit",
      existing: { id: "s-fd", name: "frontend-design", folder_id: "f-src", folder_name: "anthropics/skills" },
    },
  ],
  elsewhere: [],
  elsewhere_count: 0,
};

function setup(existingNames = ["code-review", "tdd"]) {
  const onClose = vi.fn();
  const onImported = vi.fn().mockResolvedValue(undefined);
  render(
    <ImportSkillsModal
      folders={folders}
      existingNames={existingNames}
      initialFolderId={null}
      home="/home/user"
      onClose={onClose}
      onImported={onImported}
    />,
  );
  return { onClose, onImported };
}

async function scanned() {
  scanMock.mockResolvedValue(SCAN);
  const input = screen.getByTestId("import-source-input");
  fireEvent.change(input, { target: { value: URL } });
  fireEvent.keyDown(input, { key: "Enter" });
  await waitFor(() => expect(screen.getByTestId("import-skills-modal")).toHaveAttribute("data-step", "results"));
}

describe("ImportSkillsModal (#670)", () => {
  beforeEach(() => {
    for (const mock of [scanMock, cancelMock, importMock, recentMock]) mock.mockReset();
    recentMock.mockResolvedValue({
      sources: [
        { url: "https://github.com/anthropics/skills", ref: "main", path: "", last_used_at: t, folder_id: "f-src", folder_name: "anthropics/skills" },
        { url: "/home/user/Documents/skills-repo", ref: null, path: "", last_used_at: t },
      ],
    });
  });

  it("parses the URL live into chips and a proposed folder name; Enter scans", async () => {
    setup();
    expect(screen.getByTestId("import-scan")).toBeDisabled();
    fireEvent.change(screen.getByTestId("import-source-input"), { target: { value: URL } });
    const chips = screen.getByTestId("import-source-chips");
    expect(chips).toHaveTextContent("repoanthropics/skills");
    expect(chips).toHaveTextContent("refmain");
    expect(chips).toHaveTextContent("pathskills/engineering");
    expect(chips).toHaveTextContent("folderanthropics/skills · engineering");
    expect(screen.getByTestId("import-scan")).toBeEnabled();
    expect(screen.getByTestId("import-how-it-works")).toBeInTheDocument();
    // Recent sources are listed with where they live in the bank.
    await waitFor(() => expect(screen.getByTestId("import-recent-sources")).toBeInTheDocument());
    expect(screen.getByTestId("import-recent-sources")).toHaveTextContent("github.com/anthropics/skills");
    expect(screen.getByTestId("import-recent-sources")).toHaveTextContent("in bank as “anthropics/skills”");
    expect(screen.getByTestId("import-recent-sources")).toHaveTextContent("~/Documents/skills-repo");

    scanMock.mockResolvedValue(SCAN);
    fireEvent.keyDown(screen.getByTestId("import-source-input"), { key: "Enter" });
    expect(scanMock).toHaveBeenCalledWith(expect.any(String), URL);
    await waitFor(() => expect(screen.getByTestId("import-skills-modal")).toHaveAttribute("data-step", "results"));
  });

  it("shows the scanning state with a cancel that tells the daemon", async () => {
    setup();
    let resolve: (value: SkillScanResult) => void = () => undefined;
    scanMock.mockImplementation(() => new Promise<SkillScanResult>((r) => (resolve = r)));
    cancelMock.mockResolvedValue({ cancelled: true });
    fireEvent.change(screen.getByTestId("import-source-input"), { target: { value: URL } });
    fireEvent.click(screen.getByTestId("import-scan"));
    expect(screen.getByTestId("import-scanning")).toHaveTextContent("Cloning anthropics/skills@main");
    expect(screen.getByTestId("import-source-input")).toBeDisabled();
    expect(screen.getByTestId("import-scan")).toHaveTextContent("Scanning…");
    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => expect(cancelMock).toHaveBeenCalledTimes(1));
    expect(screen.getByTestId("import-skills-modal")).toHaveAttribute("data-step", "source");
    // A late result of the cancelled scan is ignored.
    resolve(SCAN);
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.getByTestId("import-skills-modal")).toHaveAttribute("data-step", "source");
  });

  it("FP step 1: lists the skills found with name + description, the invalid one greyed with its reason", async () => {
    setup();
    await scanned();
    expect(screen.getByTestId("import-found-count")).toHaveTextContent("6 skills found");
    expect(screen.getByTestId("import-commit")).toHaveTextContent("@ 3f9c2e1");
    const pdf = screen.getByTestId("import-candidate-pdf");
    expect(pdf).toHaveTextContent("Extract text and tables from PDFs.");
    expect(pdf).toHaveTextContent("skills/engineering/pdf/SKILL.md · 3 reference files");
    expect(pdf).toHaveTextContent("new");
    expect(within(pdf).getByRole("checkbox")).toBeChecked();
    const bad = screen.getByTestId("import-candidate-skill-creator");
    expect(bad).toHaveTextContent("not importable");
    expect(bad).toHaveTextContent("Invalid frontmatter: the frontmatter has no `description`");
    expect(within(bad).getByRole("checkbox")).toBeDisabled();
    const same = screen.getByTestId("import-candidate-frontend-design");
    expect(same).toHaveTextContent("same commit · already in “anthropics/skills”");
    expect(within(same).getByRole("checkbox")).not.toBeChecked();
    // The "how it works" column is gone; the destination shows the proposed name.
    expect(screen.queryByTestId("import-how-it-works")).toBeNull();
    expect(screen.getByTestId("import-folder-name")).toHaveTextContent("anthropics/skills · engineering");
  });

  it("FP step 3: a taken name blocks Import until replace / rename / skip is chosen", async () => {
    const { onImported, onClose } = setup();
    await scanned();
    const taken = screen.getByTestId("import-candidate-code-review");
    expect(taken).toHaveTextContent("name taken · in “craft”");
    expect(screen.getByTestId("import-unresolved")).toBeInTheDocument();
    expect(screen.getByTestId("import-submit")).toBeDisabled();
    // Rename: pre-filled `<name>-<owner>`, checked live.
    fireEvent.click(within(taken).getByRole("radio", { name: "rename" }));
    const rename = screen.getByTestId("import-rename-code-review") as HTMLInputElement;
    expect(rename.value).toBe("code-review-anthropics");
    expect(taken).toHaveTextContent("free");
    fireEvent.change(rename, { target: { value: "TDD" } });
    expect(taken).toHaveTextContent("taken");
    expect(screen.getByTestId("import-submit")).toBeDisabled();
    fireEvent.change(rename, { target: { value: "code-review-anthropic" } });
    expect(screen.queryByTestId("import-unresolved")).toBeNull();
    expect(screen.getByTestId("import-submit")).toHaveTextContent("Import 4 skills");
    expect(screen.getByTestId("import-summary")).toHaveTextContent("code-review → code-review-anthropic");

    // Uncheck one; skip is also a resolution.
    fireEvent.click(within(screen.getByTestId("import-candidate-mcp-builder")).getByRole("checkbox"));
    expect(screen.getByTestId("import-submit")).toHaveTextContent("Import 3 skills");
    expect(screen.getByTestId("import-not-touched")).toHaveTextContent("1 unchecked · 1 invalid · 1 already present");

    importMock.mockResolvedValue({
      folder: { id: "f-new", name: "anthropics/skills · engineering", parent_id: null, created_at: t, updated_at: t },
      imported: [],
      failed: [],
      commit: "3f9c2e1deadbeef",
    });
    fireEvent.click(screen.getByTestId("import-submit"));
    await waitFor(() => expect(importMock).toHaveBeenCalledTimes(1));
    expect(importMock.mock.calls[0][0]).toEqual({
      scan_id: "scan-x",
      source: URL,
      folder: { name: "anthropics/skills · engineering", parent_id: null },
      items: [
        { path: "skills/engineering/pdf", action: "import" },
        { path: "skills/engineering/webapp-testing", action: "import" },
        { path: "skills/engineering/code-review", action: "rename", name: "code-review-anthropic" },
      ],
    });
    await waitFor(() => expect(onImported).toHaveBeenCalledWith(expect.anything(), true));
    expect(onClose).toHaveBeenCalled();
  });

  it("replace sends the existing skill's path with action replace; skip leaves it out", async () => {
    setup();
    await scanned();
    const taken = screen.getByTestId("import-candidate-code-review");
    fireEvent.click(within(taken).getByRole("radio", { name: "replace" }));
    expect(screen.getByTestId("import-summary")).toHaveTextContent("1 replaced");
    fireEvent.click(within(taken).getByRole("radio", { name: "skip" }));
    expect(screen.getByTestId("import-submit")).toHaveTextContent("Import 3 skills");
    expect(screen.getByTestId("import-not-touched")).toHaveTextContent("The existing “code-review” in “craft” stays as is.");
  });

  it("Select all valid checks every valid row; the folder name is editable; Esc asks before leaving", async () => {
    const { onClose } = setup();
    await scanned();
    fireEvent.click(screen.getByTestId("import-select-all"));
    expect(within(screen.getByTestId("import-candidate-frontend-design")).getByRole("checkbox")).toBeChecked();
    expect(within(screen.getByTestId("import-candidate-skill-creator")).getByRole("checkbox")).not.toBeChecked();
    fireEvent.click(screen.getByLabelText("Rename destination folder"));
    fireEvent.change(screen.getByTestId("import-folder-name-input"), { target: { value: "engineering" } });
    fireEvent.keyDown(screen.getByTestId("import-folder-name-input"), { key: "Enter" });
    expect(screen.getByTestId("import-folder-name")).toHaveTextContent("engineering");
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.getByTestId("import-discard-prompt")).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.click(screen.getByTestId("import-discard"));
    expect(onClose).toHaveBeenCalled();
  });

  it("a clone refusal reads in place with git's message, the credential hint and Retry", async () => {
    setup();
    scanMock.mockRejectedValueOnce(
      new ApiError("fatal: could not read Username for 'https://github.com'", {
        status: 502,
        body: { code: "clone_failed" },
      }),
    );
    fireEvent.change(screen.getByTestId("import-source-input"), { target: { value: "https://github.com/ippon/private-skills" } });
    fireEvent.click(screen.getByTestId("import-scan"));
    await waitFor(() => expect(screen.getByTestId("import-scan-error")).toBeInTheDocument());
    const error = screen.getByTestId("import-scan-error");
    expect(error).toHaveTextContent("Clone refused.");
    expect(error).toHaveTextContent("could not read Username");
    expect(error).toHaveTextContent("gh auth login");
    expect(error).toHaveTextContent("git@github.com:ippon/private-skills.git");
    expect(screen.getByTestId("import-source-input")).toHaveAttribute("aria-invalid", "true");
    scanMock.mockResolvedValueOnce(SCAN);
    fireEvent.click(screen.getByTestId("import-retry"));
    await waitFor(() => expect(scanMock).toHaveBeenCalledTimes(2));
  });

  it("an empty sub-path proposes the whole repo or the folders that hold skills", async () => {
    setup();
    scanMock.mockResolvedValueOnce({
      ...SCAN,
      source: { ...SCAN.source, path: "docs" },
      candidates: [],
      elsewhere: ["skills/engineering", "skills/data"],
      elsewhere_count: 14,
    });
    fireEvent.change(screen.getByTestId("import-source-input"), {
      target: { value: "https://github.com/anthropics/skills/tree/main/docs" },
    });
    fireEvent.click(screen.getByTestId("import-scan"));
    await waitFor(() => expect(screen.getByTestId("import-empty")).toBeInTheDocument());
    expect(screen.getByTestId("import-empty")).toHaveTextContent("No SKILL.md under docs/.");
    expect(screen.getByTestId("import-empty")).toHaveTextContent("Found 14 in the repo elsewhere");
    scanMock.mockResolvedValueOnce(SCAN);
    fireEvent.click(screen.getByText("skills/engineering/"));
    await waitFor(() => expect(scanMock).toHaveBeenLastCalledWith(expect.any(String), "https://github.com/anthropics/skills/tree/main/skills/engineering"));
  });

  it("a partial failure keeps the popup open, marks failed rows red and successes ticked", async () => {
    const { onImported, onClose } = setup(["tdd"]);
    scanMock.mockResolvedValue({ ...SCAN, candidates: SCAN.candidates.filter((c) => c.status === "new") });
    fireEvent.change(screen.getByTestId("import-source-input"), { target: { value: URL } });
    fireEvent.keyDown(screen.getByTestId("import-source-input"), { key: "Enter" });
    await waitFor(() => expect(screen.getByTestId("import-skills-modal")).toHaveAttribute("data-step", "results"));
    importMock.mockResolvedValue({
      folder: { id: "f-new", name: "anthropics/skills · engineering", parent_id: null, created_at: t, updated_at: t },
      imported: [
        { path: "skills/engineering/pdf", action: "imported", skill: { id: "s1", name: "pdf", description: "", folder_id: "f-new", created_at: t, updated_at: t } },
        { path: "skills/engineering/mcp-builder", action: "imported", skill: { id: "s2", name: "mcp-builder", description: "", folder_id: "f-new", created_at: t, updated_at: t } },
      ],
      failed: [{ path: "skills/engineering/webapp-testing", error: "failed to copy: disk full", code: "storage" }],
      commit: "3f9c2e1deadbeef",
    });
    fireEvent.click(screen.getByTestId("import-submit"));
    await waitFor(() => expect(screen.getByTestId("import-partial")).toBeInTheDocument());
    expect(onClose).not.toHaveBeenCalled();
    expect(onImported).toHaveBeenCalledWith(expect.anything(), false);
    expect(screen.getByTestId("import-candidate-webapp-testing")).toHaveTextContent("failed to copy: disk full");
    expect(screen.getByTestId("import-candidate-pdf")).toHaveTextContent("imported");
    expect(within(screen.getByTestId("import-candidate-pdf")).getByRole("checkbox")).not.toBeChecked();
    expect(within(screen.getByTestId("import-candidate-pdf")).getByRole("checkbox")).toBeDisabled();
    expect(screen.getByTestId("import-submit")).toHaveTextContent("Import 1 skill");
    // The import already wrote: Esc closes without a discard prompt.
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByTestId("import-discard-prompt")).toBeNull();
    expect(onClose).toHaveBeenCalled();
  });

  it("a same-commit duplicate, once checked, needs replace / rename / skip like a taken name", async () => {
    setup(["code-review", "tdd", "frontend-design"]);
    await scanned();
    const same = screen.getByTestId("import-candidate-frontend-design");
    expect(screen.queryByTestId("import-resolution-frontend-design")).toBeNull();
    fireEvent.click(within(same).getByRole("checkbox"));
    expect(screen.getByTestId("import-resolution-frontend-design")).toBeInTheDocument();
    expect(screen.getByTestId("import-submit")).toBeDisabled();
    fireEvent.click(within(same).getByRole("radio", { name: "rename" }));
    expect((screen.getByTestId("import-rename-frontend-design") as HTMLInputElement).value).toBe("frontend-design-anthropics");
    expect(same).toHaveTextContent("free");
    // Resolve the other collision too: Import enables with both renames.
    fireEvent.click(within(screen.getByTestId("import-candidate-code-review")).getByRole("radio", { name: "skip" }));
    expect(screen.getByTestId("import-submit")).toHaveTextContent("Import 4 skills");
    expect(screen.getByTestId("import-summary")).toHaveTextContent("frontend-design → frontend-design-anthropics");
  });

  it("warns when a folder of the destination name already exists here and can import into it", async () => {
    const onClose = vi.fn();
    const onImported = vi.fn().mockResolvedValue(undefined);
    const withSource: SkillFolder[] = [
      ...folders,
      {
        id: "f-src",
        name: "anthropics/skills · engineering",
        parent_id: null,
        source: { url: "https://github.com/anthropics/skills", ref: "main", commit: "0000000", path: "skills/engineering", imported_at: t, found: 3, invalid: 0 },
        created_at: t,
        updated_at: t,
      },
    ];
    render(
      <ImportSkillsModal folders={withSource} existingNames={["tdd"]} initialFolderId={null} home="/home/user" onClose={onClose} onImported={onImported} />,
    );
    scanMock.mockResolvedValue({ ...SCAN, candidates: SCAN.candidates.filter((c) => c.status === "new") });
    fireEvent.change(screen.getByTestId("import-source-input"), { target: { value: URL } });
    fireEvent.keyDown(screen.getByTestId("import-source-input"), { key: "Enter" });
    await waitFor(() => expect(screen.getByTestId("import-skills-modal")).toHaveAttribute("data-step", "results"));
    expect(screen.getByTestId("import-folder-homonym")).toHaveTextContent("A folder named “anthropics/skills · engineering” already exists here");
    fireEvent.click(screen.getByTestId("import-use-existing"));
    expect(screen.queryByTestId("import-folder-homonym")).toBeNull();
    expect(screen.getByTestId("import-into-existing")).toHaveTextContent("existing");
    importMock.mockResolvedValue({ folder: withSource[1], imported: [], failed: [], commit: "3f9c2e1deadbeef" });
    fireEvent.click(screen.getByTestId("import-submit"));
    await waitFor(() => expect(importMock).toHaveBeenCalledTimes(1));
    expect(importMock.mock.calls[0][0].folder).toEqual({ id: "f-src" });
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("Browse local… opens the folder explorer and fills the field with the pick", async () => {
    setup();
    fireEvent.click(screen.getByTestId("import-browse-local"));
    await waitFor(() => expect(screen.getByRole("dialog", { name: "Choose a local folder of skills" })).toBeInTheDocument());
    fireEvent.click(screen.getByTestId("import-browse-select"));
    await waitFor(() => expect((screen.getByTestId("import-source-input") as HTMLInputElement).value).toBe("/home/user"));
    expect(screen.getByTestId("import-source-chips")).toHaveTextContent("folderuser");
  });
});
