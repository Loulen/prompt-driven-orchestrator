import { describe, expect, it } from "vitest";
import type { Skill, SkillFolder } from "../types";
import {
  buildRows,
  descendantFolderIds,
  folderCounts,
  folderPathLabel,
  shortId,
} from "./skillTree";

const t = "2026-09-03T10:00:00Z";
const folder = (id: string, name: string, parent_id: string | null = null): SkillFolder => ({
  id,
  name,
  parent_id,
  created_at: t,
  updated_at: t,
});
const skill = (id: string, name: string, folder_id: string | null = null, description = ""): Skill => ({
  id,
  name,
  description,
  folder_id,
  created_at: t,
  updated_at: t,
});

const folders = [folder("f-m", "méthode"), folder("f-i", "ippon"), folder("f-j", "java", "f-i")];
const skills = [
  skill("s-tdd", "tdd", "f-m", "Test-driven development"),
  skill("s-gr", "grilling", "f-m"),
  skill("s-sp", "spring", "f-j"),
  skill("s-cr", "code-review", null, "Review staged changes"),
];

describe("skillTree", () => {
  it("labels a folder with its path", () => {
    expect(folderPathLabel("f-j", folders)).toBe("ippon / java");
    expect(folderPathLabel("f-m", folders)).toBe("méthode");
  });

  it("counts skills recursively", () => {
    const counts = folderCounts(folders, skills);
    expect(counts.get("f-m")).toBe(2);
    expect(counts.get("f-j")).toBe(1);
    expect(counts.get("f-i")).toBe(1);
  });

  it("collects descendant folder ids", () => {
    expect([...descendantFolderIds("f-i", folders)].sort()).toEqual(["f-i", "f-j"]);
  });

  it("renders folders first then root skills, collapsed by default", () => {
    const rows = buildRows(folders, skills, new Set());
    expect(rows.map((row) => `${row.ref.kind}:${row.ref.id}`)).toEqual([
      "folder:f-i",
      "folder:f-m",
      "skill:s-cr",
    ]);
    expect(rows[1].count).toBe(2);
  });

  it("expands a folder and indents its children", () => {
    const rows = buildRows(folders, skills, new Set(["f-m"]));
    const ids = rows.map((row) => row.ref.id);
    expect(ids).toEqual(["f-i", "f-m", "s-gr", "s-tdd", "s-cr"]);
    expect(rows.find((row) => row.ref.id === "s-tdd")?.depth).toBe(1);
  });

  it("filters by name or description, keeping ancestor folders expanded", () => {
    const rows = buildRows(folders, skills, new Set(), "spring");
    expect(rows.map((row) => row.ref.id)).toEqual(["f-i", "f-j", "s-sp"]);
    const byDesc = buildRows(folders, skills, new Set(), "staged");
    expect(byDesc.map((row) => row.ref.id)).toEqual(["s-cr"]);
  });

  it("puts a skill whose folder vanished back at the root", () => {
    const rows = buildRows(folders, [skill("s-x", "orphan", "f-gone")], new Set());
    expect(rows.map((row) => row.ref.id)).toContain("s-x");
  });

  it("shortens ids", () => {
    expect(shortId("7c3e1234-aaaa-bbbb-cccc-000000a91f")).toBe("7c3e…a91f");
    expect(shortId("short")).toBe("short");
  });
});
