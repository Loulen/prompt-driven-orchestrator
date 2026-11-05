import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";

const exportSkillArchiveMock = vi.fn();
const fetchSkillMock = vi.fn();

vi.mock("../api", () => ({
  exportSkillArchive: (...args: unknown[]) => exportSkillArchiveMock(...args),
  fetchSkill: (...args: unknown[]) => fetchSkillMock(...args),
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
import ExportSkillsModal from "./ExportSkillsModal";
import type { Skill, SkillFolder } from "../types";

const t = "2026-11-05T10:00:00Z";
const folder = (id: string, name: string, parent_id: string | null = null): SkillFolder => ({
  id,
  name,
  parent_id,
  created_at: t,
  updated_at: t,
});
const skill = (id: string, name: string, folder_id: string | null, locked = false): Skill => ({
  id,
  name,
  description: `${name} desc`,
  folder_id,
  locked,
  created_at: t,
  updated_at: t,
});

// The bank of the design (v1-04): ops (2), pdo (2 seeded), team-review (4), one at the root.
const folders = [folder("f-ops", "ops"), folder("f-pdo", "pdo"), folder("f-team", "team-review")];
const skills = [
  skill("s-deploy", "deploy-check", "f-ops"),
  skill("s-runbook", "runbook", "f-ops"),
  skill("pdo-orchestrate", "git-flow", "f-pdo", true),
  skill("pdo-interactive", "pdo-interactive", "f-pdo", true),
  skill("s-review", "code-review", "f-team"),
  skill("s-check", "review-checklist", "f-team"),
  skill("s-scan", "security-scan", "f-team"),
  skill("s-spec", "spec-reader", "f-team"),
  skill("s-domain", "domain-modeling", null),
];

const NOW = new Date(2026, 10, 5, 9, 7);

function renderModal(props: Partial<React.ComponentProps<typeof ExportSkillsModal>> = {}) {
  const onClose = vi.fn();
  const onExported = vi.fn();
  const save = vi.fn();
  render(
    <ExportSkillsModal
      folders={folders}
      skills={skills}
      onClose={onClose}
      onExported={onExported}
      save={save}
      now={() => NOW}
      {...props}
    />,
  );
  return { onClose, onExported, save };
}

const checkbox = (testId: string) => screen.getByTestId(testId) as HTMLInputElement;
const fileName = () => screen.getByTestId("export-file-name") as HTMLInputElement;
const submit = () => screen.getByTestId("export-submit") as HTMLButtonElement;

beforeEach(() => {
  exportSkillArchiveMock.mockReset();
  fetchSkillMock.mockReset();
  fetchSkillMock.mockImplementation((id: string) =>
    Promise.resolve({ id, files: id === "s-review" ? [{ path: "a.md", size: 1 }, { path: "b.md", size: 1 }] : [] }),
  );
});

describe("ExportSkillsModal (#1026)", () => {
  it("opened from the toolbar: the whole tree, nothing checked, seeded greyed, Export disabled", () => {
    renderModal();
    expect(screen.getByRole("dialog", { name: "Export skills to an archive" })).toBeTruthy();
    expect(screen.getByTestId("export-count").textContent).toBe("0 of 7 skills checked");
    expect(screen.getByTestId("export-seeded-count").textContent).toContain("2 managed by PDO, not exportable");
    for (const id of ["s-deploy", "s-runbook", "s-review", "s-domain"]) {
      expect(checkbox(`export-check-${id}`).checked).toBe(false);
      expect(checkbox(`export-check-${id}`).disabled).toBe(false);
    }
    // Seeded: disabled, « Managed by PDO », and the folder holding only them too.
    expect(checkbox("export-check-pdo-orchestrate").disabled).toBe(true);
    expect(checkbox("export-check-pdo-interactive").disabled).toBe(true);
    expect(screen.getByTestId("export-locked-pdo-orchestrate").textContent).toContain("Managed by PDO");
    expect(checkbox("export-folder-check-f-pdo").disabled).toBe(true);
    expect(submit().disabled).toBe(true);
    expect(submit().textContent).toContain("Export 0 skills");
    expect(screen.getByTestId("export-not-exported").textContent).toContain(
      "2 skills managed by PDO (git-flow, pdo-interactive)",
    );
    expect(fileName().value).toBe("pdo-skills-20261105-0907.zip");
  });

  it("from a skill's detail: that skill pre-checked and `<name>.skills.zip` proposed", async () => {
    renderModal({ initialChecked: ["s-review"] });
    expect(checkbox("export-check-s-review").checked).toBe(true);
    expect(checkbox("export-folder-check-f-team").indeterminate).toBe(true);
    expect(screen.getByTestId("export-count").textContent).toBe("1 of 7 skills checked");
    expect(fileName().value).toBe("code-review.skills.zip");
    expect(submit().textContent).toContain("Export 1 skill");
    await waitFor(() =>
      expect(screen.getByTestId("export-summary").textContent).toContain("SKILL.md + 2 reference files, verbatim"),
    );
  });

  it("a seeded id passed as pre-checked is ignored", () => {
    renderModal({ initialChecked: ["pdo-orchestrate"] });
    expect(screen.getByTestId("export-count").textContent).toBe("0 of 7 skills checked");
    expect(submit().disabled).toBe(true);
  });

  it("FP step 2: checking a folder checks its skills now; unchecking one updates the counter", () => {
    renderModal();
    fireEvent.click(checkbox("export-folder-check-f-team"));
    for (const id of ["s-review", "s-check", "s-scan", "s-spec"]) {
      expect(checkbox(`export-check-${id}`).checked).toBe(true);
    }
    expect(checkbox("export-folder-check-f-team").checked).toBe(true);
    expect(screen.getByTestId("export-count").textContent).toBe("4 of 7 skills checked");
    // Exactly one folder: named after it.
    expect(fileName().value).toBe("team-review.skills.zip");
    expect(submit().textContent).toContain("Export 4 skills");

    fireEvent.click(checkbox("export-check-s-scan"));
    expect(screen.getByTestId("export-count").textContent).toBe("3 of 7 skills checked");
    expect(checkbox("export-folder-check-f-team").checked).toBe(false);
    expect(checkbox("export-folder-check-f-team").indeterminate).toBe(true);
    expect(fileName().value).toBe("pdo-skills-20261105-0907.zip");
    expect(screen.getByTestId("export-summary").textContent).toContain(
      "code-review · review-checklist · spec-reader",
    );

    // The folder box again checks every skill of the folder; once more unchecks them.
    fireEvent.click(checkbox("export-folder-check-f-team"));
    expect(screen.getByTestId("export-count").textContent).toBe("4 of 7 skills checked");
    fireEvent.click(checkbox("export-folder-check-f-team"));
    expect(screen.getByTestId("export-count").textContent).toBe("0 of 7 skills checked");
  });

  it("clicking a skill's name toggles it, like its box", () => {
    renderModal();
    fireEvent.mouseDown(screen.getByText("runbook"));
    fireEvent.click(screen.getByText("runbook"));
    expect(checkbox("export-check-s-runbook").checked).toBe(true);
  });

  it("Select all checks every exportable skill, never a seeded one; Deselect all clears", () => {
    renderModal();
    fireEvent.click(screen.getByTestId("export-select-all"));
    expect(screen.getByTestId("export-count").textContent).toBe("7 of 7 skills checked");
    expect(checkbox("export-check-pdo-orchestrate").checked).toBe(false);
    fireEvent.click(screen.getByTestId("export-deselect-all"));
    expect(screen.getByTestId("export-count").textContent).toBe("0 of 7 skills checked");
  });

  it("the file name follows the selection until typed, then keeps it; .zip appended; ↺ resets; empty is refused", () => {
    renderModal({ initialChecked: ["s-review"] });
    fireEvent.change(fileName(), { target: { value: "team-skills" } });
    expect(screen.getByTestId("export-file-name-hint").textContent).toContain("team-skills.zip");
    expect(screen.getByTestId("export-file-name-hint").textContent).toContain(".zip is appended");
    // The selection changes: the typed name stays.
    fireEvent.click(checkbox("export-check-s-check"));
    expect(fileName().value).toBe("team-skills");

    fireEvent.change(fileName(), { target: { value: "  " } });
    expect(screen.getByTestId("export-file-name-hint").textContent).toContain("name required");
    expect(submit().disabled).toBe(true);

    fireEvent.click(screen.getByTestId("export-file-name-reset"));
    expect(fileName().value).toBe("pdo-skills-20261105-0907.zip");
    expect(submit().disabled).toBe(false);
  });

  it("FP step 3: Export sends the ids and the .zip-completed name, saves the download, reports it", async () => {
    const blob = new Blob(["zip"]);
    exportSkillArchiveMock.mockResolvedValue({ blob, fileName: "team-skills.zip", count: 3 });
    const { save, onExported } = renderModal();
    fireEvent.click(checkbox("export-folder-check-f-team"));
    fireEvent.click(checkbox("export-check-s-scan"));
    fireEvent.change(fileName(), { target: { value: "team-skills" } });
    fireEvent.click(submit());
    await waitFor(() => expect(onExported).toHaveBeenCalled());
    const [ids, name] = exportSkillArchiveMock.mock.calls[0];
    expect([...ids].sort()).toEqual(["s-check", "s-review", "s-spec"]);
    expect(name).toBe("team-skills.zip");
    expect(save).toHaveBeenCalledWith(blob, "team-skills.zip");
    expect(onExported).toHaveBeenCalledWith({ blob, fileName: "team-skills.zip", count: 3 });
  });

  it("a refused export reads in place and keeps the modal open", async () => {
    exportSkillArchiveMock.mockRejectedValue(
      new ApiError("unknown skill id `s-review`: it is not in the bank", { status: 404 }),
    );
    const { onExported } = renderModal({ initialChecked: ["s-review"] });
    fireEvent.click(submit());
    await waitFor(() => expect(screen.getByTestId("export-error").textContent).toContain("s-review"));
    expect(onExported).not.toHaveBeenCalled();
    expect(submit().disabled).toBe(false);
  });

  it("keyboard: Esc closes, ⌘A selects all, ⌘↵ exports, Space toggles the selected row", async () => {
    exportSkillArchiveMock.mockResolvedValue({ blob: new Blob([]), fileName: "x.zip", count: 7 });
    const { onClose, onExported } = renderModal();
    fireEvent.keyDown(window, { key: "a", metaKey: true });
    expect(screen.getByTestId("export-count").textContent).toBe("7 of 7 skills checked");

    // Select a row with the keyboard (no pointer): Space toggles it.
    const tree = screen.getByTestId("skill-tree");
    fireEvent.keyDown(tree, { key: "ArrowDown" });
    fireEvent.keyDown(tree, { key: " " });
    expect(screen.getByTestId("export-count").textContent).toBe("5 of 7 skills checked");

    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalled();

    fireEvent.keyDown(window, { key: "Enter", ctrlKey: true });
    await waitFor(() => expect(onExported).toHaveBeenCalled());
  });
});
