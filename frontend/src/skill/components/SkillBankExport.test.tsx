import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";

const exportSkillsMock = vi.fn();
const uploadSkillsZipMock = vi.fn();
const saveBlobMock = vi.fn();

vi.mock("../api/bank", () => ({
  fetchSkill: vi.fn(async (id: string) => ({ id, name: id, content: null, files: [], path: "" })),
  updateSkill: vi.fn(),
  deleteSkill: vi.fn(),
  fetchSkillReferents: vi.fn(),
  createSkillFolder: vi.fn(),
  updateSkillFolder: vi.fn(),
  deleteSkillFolder: vi.fn(),
  createSkill: vi.fn(),
}));

vi.mock("../api/bundle", () => ({
  exportSkills: (...args: unknown[]) => exportSkillsMock(...args),
  uploadSkillsZip: (...args: unknown[]) => uploadSkillsZipMock(...args),
}));

vi.mock("../../shared/lib/download", () => ({
  saveBlob: (...args: unknown[]) => saveBlobMock(...args),
}));

import SkillBankPanel from "./SkillBankPanel";
import type { Skill } from "../../shared/generated/Skill";
import type { SkillFolder } from "../../shared/generated/SkillFolder";
import type { SkillBank } from "../model/skill";

const t = "2026-10-19T10:00:00Z";
const folder = (id: string, name: string, parent_id: string | null = null): SkillFolder => ({
  id,
  name,
  parent_id,
  created_at: t,
  updated_at: t,
});
const skill = (id: string, name: string, folder_id: string | null): Skill => ({
  locked: false,
  id,
  name,
  description: "desc",
  folder_id,
  created_at: t,
  updated_at: t,
});

const BANK: SkillBank = {
  root_path: "/home/user/.pdo/skills",
  folders: [folder("f-t", "Team")],
  skills: [skill("s-tdd", "tdd", "f-t"), skill("s-cr", "code-review", "f-t"), skill("s-rn", "release-notes", null)],
};

function setup() {
  const onChanged = vi.fn().mockResolvedValue(undefined);
  render(<SkillBankPanel bank={BANK} loaded home="/home/user" onChanged={onChanged} />);
  return { onChanged };
}

beforeEach(() => {
  exportSkillsMock.mockReset();
  uploadSkillsZipMock.mockReset();
  saveBlobMock.mockReset();
  exportSkillsMock.mockResolvedValue({ blob: new Blob(["PK"]), fileName: "Team.skills.zip", count: 2 });
});

describe("the bank's export and upload", () => {
  it("checks a folder and a skill, counts them at the foot, exports them as one zip", async () => {
    setup();
    expect(screen.queryByTestId("skill-export-bar")).toBeNull();
    fireEvent.click(screen.getByTestId("tree-check-folder-f-t"));
    expect(screen.getByTestId("skill-export-count")).toHaveTextContent("2 skills selected");
    expect(screen.queryByTestId("skill-bank-footer")).toBeNull();

    fireEvent.click(screen.getByTestId("tree-check-skill-s-rn"));
    expect(screen.getByTestId("skill-export-count")).toHaveTextContent("3 skills selected");
    fireEvent.click(screen.getByTestId("skill-export-selection"));
    await waitFor(() => expect(exportSkillsMock).toHaveBeenCalledTimes(1));
    expect(exportSkillsMock.mock.calls[0][0]).toEqual({ skill_ids: ["s-tdd", "s-cr", "s-rn"], folder_ids: [] });
    await waitFor(() => expect(saveBlobMock).toHaveBeenCalledWith(expect.any(Blob), "Team.skills.zip"));
    expect(screen.getByTestId("skill-toast")).toHaveTextContent("Exported 2 skills to Team.skills.zip");

    fireEvent.click(screen.getByTestId("skill-export-clear"));
    expect(screen.queryByTestId("skill-export-bar")).toBeNull();
    expect(screen.getByTestId("skill-bank-footer")).toBeInTheDocument();
  });

  it("a selection that is exactly one folder exports the folder", async () => {
    setup();
    fireEvent.click(screen.getByTestId("tree-check-folder-f-t"));
    fireEvent.click(screen.getByTestId("skill-export-selection"));
    await waitFor(() => expect(exportSkillsMock).toHaveBeenCalledWith({ skill_ids: [], folder_ids: ["f-t"] }));
  });

  it("⇧-click checks the rows between the last box and this one", () => {
    setup();
    // Rows: Team (folded), release-notes.
    fireEvent.click(screen.getByTestId("tree-check-folder-f-t"));
    fireEvent.click(screen.getByTestId("tree-check-folder-f-t"));
    expect(screen.queryByTestId("skill-export-bar")).toBeNull();
    fireEvent.click(screen.getByTestId("tree-check-skill-s-rn"), { shiftKey: true });
    expect(screen.getByTestId("skill-export-count")).toHaveTextContent("3 skills selected");
  });

  it("Export folder in the folder's detail downloads its zip", async () => {
    setup();
    fireEvent.click(screen.getByTestId("tree-folder-f-t"));
    expect(screen.getByTestId("folder-export-hint")).toHaveTextContent("Upload a zip");
    fireEvent.click(screen.getByTestId("folder-detail-export"));
    await waitFor(() => expect(exportSkillsMock).toHaveBeenCalledWith({ skill_ids: [], folder_ids: ["f-t"] }));
  });

  it("Add › Upload a zip sends the file, refreshes the bank and opens the folder it filled", async () => {
    uploadSkillsZipMock.mockResolvedValue({
      created: [{ id: "s-x", name: "x" }],
      kept: [{ id: "s-tdd", name: "tdd" }],
      renamed: [],
      missing: [],
      folder: { id: "f-t", name: "Team" },
      warnings: [],
    });
    const { onChanged } = setup();
    fireEvent.click(screen.getByTestId("skill-add"));
    expect(screen.getByTestId("skill-upload-zip")).toHaveTextContent("Upload a zip");
    const file = new File([new Uint8Array([0x50, 0x4b])], "Team.skills.zip", { type: "application/zip" });
    fireEvent.change(screen.getByTestId("skill-upload-zip-input"), { target: { files: [file] } });
    await waitFor(() => expect(uploadSkillsZipMock).toHaveBeenCalledWith(file));
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
    expect(await screen.findByTestId("skill-toast")).toHaveTextContent(
      "Uploaded 1 skill into Team · 1 already in the bank",
    );
    expect(screen.getByTestId("folder-detail")).toBeInTheDocument();
  });
});
