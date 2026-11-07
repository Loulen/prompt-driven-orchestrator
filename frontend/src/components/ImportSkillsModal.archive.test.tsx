import { render, screen, fireEvent, waitFor, within } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";

const scanMock = vi.fn();
const scanArchiveMock = vi.fn();
const cancelMock = vi.fn();
const importMock = vi.fn();
const recentMock = vi.fn();

vi.mock("../api", () => ({
  scanSkillSource: (...args: unknown[]) => scanMock(...args),
  scanSkillArchive: (...args: unknown[]) => scanArchiveMock(...args),
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

const t = "2026-11-05T10:00:00Z";
const folders: SkillFolder[] = [
  { id: "f-team", name: "team-review", parent_id: null, created_at: t, updated_at: t },
  { id: "f-ops", name: "ops", parent_id: null, created_at: t, updated_at: t },
];

const ARCHIVE_SCAN: SkillScanResult = {
  scan_id: "scan-a",
  source: {
    kind: "file",
    url: "team-review.skills.zip",
    ref: null,
    path: "",
    repo: "team-review",
    suggested_folder: "team-review",
  },
  commit: null,
  candidates: [
    { path: "security-scan", name: "security-scan", description: "Scan a diff for secrets.", valid: true, file_count: 1, id: "id-sec", status: "new" },
    {
      path: "code-review",
      name: "code-review",
      description: "Two-axis review.",
      valid: true,
      file_count: 2,
      id: "id-cr",
      status: "id_changed",
      existing: { id: "id-cr", name: "code-review", folder_id: "f-team", folder_name: "team-review" },
    },
    {
      path: "review-checklist",
      name: "review-checklist",
      description: "The checklist a reviewer walks.",
      valid: true,
      file_count: 0,
      id: "id-rc",
      status: "id_up_to_date",
      existing: { id: "id-rc", name: "review-checklist", folder_id: "f-team", folder_name: "team-review" },
    },
    {
      path: "spec-reader",
      name: "spec-reader",
      description: "Read a spec.",
      valid: true,
      file_count: 0,
      id: "id-sr",
      status: "name_taken",
      existing: { id: "other", name: "spec-reader", folder_id: "f-inbox", folder_name: "inbox" },
    },
    {
      path: "pdo-interactive",
      name: "pdo-interactive",
      description: "Talk with a human inside a PDO node.",
      valid: true,
      file_count: 0,
      id: "pdo-interactive",
      status: "locked",
      existing: { id: "pdo-interactive", name: "pdo-interactive", folder_id: "skf-pdo", folder_name: "PDO" },
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

function setup() {
  const onClose = vi.fn();
  const onImported = vi.fn().mockResolvedValue(undefined);
  render(
    <ImportSkillsModal
      folders={folders}
      existingNames={["code-review", "review-checklist", "spec-reader", "pdo-interactive"]}
      initialFolderId={null}
      home="/home/user"
      onClose={onClose}
      onImported={onImported}
    />,
  );
  return { onClose, onImported };
}

const zip = (name = "team-review.skills.zip") => new File(["PK\u0003\u0004"], name, { type: "application/zip" });

function pick(file: File) {
  fireEvent.change(screen.getByTestId("import-archive-drop-input"), { target: { files: [file] } });
}

async function listed(scan: SkillScanResult = ARCHIVE_SCAN) {
  scanArchiveMock.mockResolvedValue(scan);
  pick(zip(scan.source.url));
  await waitFor(() => expect(screen.getByTestId("import-skills-modal")).toHaveAttribute("data-step", "results"));
}

const fileDrag = (files: File[]) => ({
  dataTransfer: {
    types: ["Files"],
    items: files.map(() => ({ kind: "file" })),
    files,
    dropEffect: "none",
  },
});

describe("ImportSkillsModal — Import d'archive (#1027)", () => {
  beforeEach(() => {
    for (const mock of [scanMock, scanArchiveMock, cancelMock, importMock, recentMock]) mock.mockReset();
    recentMock.mockResolvedValue({ sources: [] });
  });

  it("offers a .zip drop bar under the source field, and says which files it accepts", () => {
    setup();
    const bar = screen.getByTestId("import-archive-drop");
    expect(bar).toHaveTextContent(".zip · drop it anywhere in this popup, or");
    expect(within(bar).getByTestId("import-archive-drop-browse")).toHaveTextContent("Browse…");
    const input = screen.getByTestId("import-archive-drop-input") as HTMLInputElement;
    expect(input.accept).toContain(".zip");
    expect(input.multiple).toBe(false);
    expect(screen.getByTestId("import-accepted")).toHaveTextContent("an archive exported from a PDO bank (ids kept)");
    expect(screen.getByTestId("import-how-it-works")).toHaveTextContent("Point at a source, or drop a .zip");
  });

  it("reads a picked .zip and lists it with the format chip and the id statuses", async () => {
    setup();
    await listed();
    const [, file, fileName] = scanArchiveMock.mock.calls[0];
    expect((file as File).name).toBe("team-review.skills.zip");
    expect(fileName).toBe("team-review.skills.zip");
    expect(scanMock).not.toHaveBeenCalled();

    expect(screen.getByTestId("import-archive-name")).toHaveTextContent("team-review.skills.zip");
    expect(screen.getByTestId("import-archive-format")).toHaveTextContent("PDO archive · ids kept");
    expect(screen.getByTestId("import-found-count")).toHaveTextContent("5 skills found");
    expect(screen.getByTestId("import-found-check")).toBeInTheDocument();
    expect(screen.getByTestId("import-found-breakdown")).toHaveTextContent(
      "· 1 new · 1 changed · 1 up to date · 1 name taken · 1 not importable",
    );

    const row = (name: string) => screen.getByTestId(`import-candidate-${name}`);
    expect(row("security-scan")).toHaveTextContent("new · id kept");
    expect(within(row("security-scan")).getByRole("checkbox")).toBeChecked();
    expect(row("review-checklist")).toHaveTextContent("already up to date · in “team-review”");
    expect(row("code-review")).toHaveTextContent("changed · in “team-review”");
    // #1028: a changed row is checked, with « Keep as is » preselected.
    expect(within(row("code-review")).getByRole("checkbox")).toBeChecked();
    expect(within(row("code-review")).getByRole("radio", { name: "Keep as is" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByTestId("import-id-explain-code-review")).toHaveTextContent("stays untouched");
    expect(row("pdo-interactive")).toHaveTextContent("managed by PDO");
    for (const name of ["review-checklist", "pdo-interactive"]) {
      const box = within(row(name)).getByRole("checkbox");
      expect(box).not.toBeChecked();
      expect(box).toBeDisabled();
    }
    // The name rules are unchanged for an unknown id whose name is taken.
    expect(row("spec-reader")).toHaveTextContent("name taken · in “inbox”");
    expect(screen.getByTestId("import-resolution-spec-reader")).toBeInTheDocument();
    expect(screen.getByTestId("import-not-touched")).toHaveTextContent(
      "1 kept as is · 1 already up to date · 0 unchecked · 1 managed by PDO · 0 invalid.",
    );

    // Select all never checks a row the bank already has by id.
    fireEvent.click(screen.getByTestId("import-select-all"));
    expect(within(row("review-checklist")).getByRole("checkbox")).not.toBeChecked();
  });

  it("an archive the bank already has entirely leaves nothing to import", async () => {
    setup();
    await listed({
      ...ARCHIVE_SCAN,
      candidates: ARCHIVE_SCAN.candidates.filter((c) => c.status === "id_up_to_date"),
      archive: { ...ARCHIVE_SCAN.archive!, existing_folder: null, new_folder_name: "team-review" },
    });
    expect(screen.getByTestId("import-submit")).toBeDisabled();
    expect(screen.getByTestId("import-submit")).toHaveTextContent("Import 0 skills");
    // Nothing is ticked: the header does not show a check.
    expect(screen.queryByTestId("import-found-check")).toBeNull();
  });

  it("defaults to a new suffixed folder when one exists, imports into it with the file as source, and stays on the report", async () => {
    const { onImported, onClose } = setup();
    await listed();
    const dest = screen.getByTestId("import-archive-destination");
    expect(dest).toHaveTextContent("A folder team-review already exists at the root of the bank.");
    expect(screen.getByTestId("import-dest-new")).toHaveAttribute("data-selected", "true");
    expect(within(screen.getByTestId("import-dest-new")).getByTestId("import-folder-name")).toHaveTextContent(
      "team-review 2",
    );
    expect(screen.getByTestId("import-dest-merge")).toHaveTextContent("Merge into “team-review”");
    expect(screen.getByTestId("import-archive-note")).toHaveTextContent(
      "Tagged “imported from team-review.skills.zip” — a file has no update",
    );
    expect(screen.queryByTestId("import-archive-homonym")).toBeNull();
    // Renaming the new folder onto an existing one warns, it never merges silently.
    fireEvent.click(screen.getByLabelText("Rename destination folder"));
    fireEvent.change(screen.getByTestId("import-folder-name-input"), { target: { value: "ops" } });
    expect(screen.getByTestId("import-archive-homonym")).toHaveTextContent("A folder named “ops” already exists here");
    fireEvent.change(screen.getByTestId("import-folder-name-input"), { target: { value: "team-review 2" } });
    fireEvent.blur(screen.getByTestId("import-folder-name-input"));
    expect(screen.queryByTestId("import-archive-homonym")).toBeNull();
    // The name collision still has to be resolved before Import enables.
    expect(screen.getByTestId("import-submit")).toBeDisabled();
    fireEvent.click(within(screen.getByTestId("import-resolution-spec-reader")).getByRole("radio", { name: "skip" }));
    expect(screen.getByTestId("import-submit")).toHaveTextContent("Import 1 skill");

    importMock.mockResolvedValue({
      folder: { id: "f-new", name: "team-review 2", parent_id: null, created_at: t, updated_at: t },
      imported: [{ path: "security-scan", action: "imported", skill: { id: "id-sec", name: "security-scan" } }],
      failed: [],
      commit: null,
    });
    fireEvent.click(screen.getByTestId("import-submit"));
    await waitFor(() => expect(importMock).toHaveBeenCalledTimes(1));
    expect(importMock).toHaveBeenCalledWith({
      scan_id: "scan-a",
      source: "team-review.skills.zip",
      kind: "file",
      folder: { name: "team-review 2", parent_id: null },
      items: [
        { path: "security-scan", action: "import" },
        { path: "code-review", action: "keep" },
        { path: "review-checklist", action: "keep" },
      ],
    });
    await waitFor(() => expect(onImported).toHaveBeenCalledWith(expect.anything(), true, true));
    // #1028: the popup stays open on the per-line report.
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByTestId("import-skills-modal")).toHaveAttribute("data-step", "report");
  });

  it("merging sends the existing folder's id", async () => {
    setup();
    await listed();
    fireEvent.click(within(screen.getByTestId("import-dest-merge")).getByRole("radio"));
    expect(screen.getByTestId("import-dest-merge")).toHaveAttribute("data-selected", "true");
    expect(within(screen.getByTestId("import-dest-new")).queryByTestId("import-folder-name")).toBeNull();
    expect(screen.getByTestId("import-archive-note")).toHaveTextContent("the folder keeps its own provenance");
    fireEvent.click(within(screen.getByTestId("import-resolution-spec-reader")).getByRole("radio", { name: "skip" }));
    importMock.mockResolvedValue({
      folder: folders[0],
      imported: [{ path: "security-scan", action: "imported", skill: { id: "id-sec", name: "security-scan" } }],
      failed: [],
      commit: null,
    });
    fireEvent.click(screen.getByTestId("import-submit"));
    await waitFor(() => expect(importMock).toHaveBeenCalledTimes(1));
    expect(importMock.mock.calls[0][0]).toMatchObject({ kind: "file", folder: { id: "f-team" } });
  });

  it("with no folder of that name, the destination is simply the new folder named after the file", async () => {
    setup();
    await listed({
      ...ARCHIVE_SCAN,
      source: { ...ARCHIVE_SCAN.source, url: "squad.zip", suggested_folder: "squad", repo: "squad" },
      candidates: [ARCHIVE_SCAN.candidates[0]],
      archive: { file_name: "squad.zip", format: "plain_zip", existing_folder: null, new_folder_name: "squad" },
    });
    expect(screen.getByTestId("import-archive-format")).toHaveTextContent("zip without manifest · by name");
    expect(screen.queryByTestId("import-dest-merge")).toBeNull();
    expect(screen.getByTestId("import-folder-name")).toHaveTextContent("squad");
  });

  it("the whole popup takes the drop, with an overlay naming the accepted formats", async () => {
    setup();
    const modal = screen.getByTestId("import-skills-modal");
    const file = zip();
    fireEvent.dragEnter(modal, fileDrag([file]));
    const overlay = screen.getByTestId("skill-drop-overlay");
    expect(overlay).toHaveTextContent("Drop to import team-review.skills.zip");
    expect(overlay).toHaveTextContent("A .zip only — a PDO skills archive, a pipeline's skills sidecar");
    scanArchiveMock.mockResolvedValue(ARCHIVE_SCAN);
    fireEvent.drop(modal, fileDrag([file]));
    expect(screen.queryByTestId("skill-drop-overlay")).toBeNull();
    await waitFor(() => expect(scanArchiveMock).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(modal).toHaveAttribute("data-step", "results"));
  });

  it("refuses a file that is not a .zip in place, without uploading it", () => {
    setup();
    fireEvent.drop(screen.getByTestId("import-skills-modal"), fileDrag([new File(["%PDF"], "notes.pdf")]));
    const alert = screen.getByTestId("import-archive-error");
    expect(alert).toHaveTextContent("Not a skills archive. notes.pdf is not a .zip.");
    expect(alert).toHaveTextContent("Drop a PDO archive, a pipeline's skills sidecar, or a zip of folders holding a SKILL.md.");
    expect(scanArchiveMock).not.toHaveBeenCalled();
    expect(screen.queryByTestId("import-accepted")).toBeNull();
    expect(screen.getByTestId("import-skills-modal")).toHaveAttribute("data-step", "source");
  });

  it("shows the daemon's refusal of a corrupted archive in place", async () => {
    setup();
    scanArchiveMock.mockRejectedValue(
      new ApiError("The archive is corrupted or truncated (invalid Zip archive).", {
        status: 422,
        body: { code: "corrupt_archive" },
      }),
    );
    pick(zip("broken.zip"));
    await waitFor(() => expect(screen.getByTestId("import-archive-error")).toBeInTheDocument());
    expect(screen.getByTestId("import-archive-error")).toHaveTextContent(
      "Corrupted archive. The archive is corrupted or truncated (invalid Zip archive). Drop a PDO archive",
    );
    expect(screen.getByTestId("import-skills-modal")).toHaveAttribute("data-step", "source");
  });

  it("tells a .zip that is not a zip apart, as a sentence before the advice", async () => {
    setup();
    scanArchiveMock.mockRejectedValue(
      new ApiError("notes.zip is not a .zip", { status: 422, body: { code: "not_a_zip" } }),
    );
    pick(zip("notes.zip"));
    await waitFor(() => expect(screen.getByTestId("import-archive-error")).toBeInTheDocument());
    const alert = screen.getByTestId("import-archive-error");
    expect(alert).toHaveTextContent("Not a skills archive. notes.zip is not a .zip. Drop a PDO archive");
    expect(alert.textContent).not.toContain("`");
  });

  it("marks the archive path as new on the source step", () => {
    setup();
    expect(screen.getByTestId("import-archive-new")).toHaveTextContent("new");
  });
});
