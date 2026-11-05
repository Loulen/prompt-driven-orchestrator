import { describe, it, expect } from "vitest";
import {
  archiveStamp,
  defaultArchiveName,
  exportableInFolder,
  folderOfSelection,
  sanitiseArchiveName,
  withZip,
} from "./skillArchive";
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
  description: "",
  folder_id,
  locked,
  created_at: t,
  updated_at: t,
});

const folders = [folder("a", "ops"), folder("a1", "deploy", "a"), folder("b", "pdo")];
const skills = [
  skill("s1", "deploy-check", "a1"),
  skill("s2", "rollback", "a1"),
  skill("p1", "pdo-orchestrate", "b", true),
  skill("r1", "root-skill", null),
];
const NOW = new Date(2026, 0, 2, 3, 4);

describe("skillArchive (#1026)", () => {
  it("names one skill after it, one folder after the outermost matching one, else a dated name", () => {
    expect(defaultArchiveName(new Set(["s1"]), folders, skills, NOW)).toBe("deploy-check.skills.zip");
    // `ops` and `deploy` both hold exactly {s1, s2}: the outermost wins.
    expect(defaultArchiveName(new Set(["s1", "s2"]), folders, skills, NOW)).toBe("ops.skills.zip");
    expect(defaultArchiveName(new Set(["s1", "r1"]), folders, skills, NOW)).toBe("pdo-skills-20260102-0304.zip");
    expect(defaultArchiveName(new Set(), folders, skills, NOW)).toBe("pdo-skills-20260102-0304.zip");
  });

  it("a folder's exportable skills leave the seeded ones out, sub-folders included", () => {
    expect(exportableInFolder("a", folders, skills).sort()).toEqual(["s1", "s2"]);
    expect(exportableInFolder("b", folders, skills)).toEqual([]);
    expect(folderOfSelection(new Set(["s1"]), folders, skills)).toBeNull();
  });

  it("appends .zip, sanitises like the daemon, stamps local time", () => {
    expect(withZip("team-skills")).toBe("team-skills.zip");
    expect(withZip("x.ZIP")).toBe("x.ZIP");
    expect(withZip("   ")).toBe("");
    expect(sanitiseArchiveName("a/b:c")).toBe("a-b-c");
    expect(sanitiseArchiveName(" .. ")).toBe("skill");
    expect(archiveStamp(new Date(2026, 10, 5, 9, 7))).toBe("20261105-0907");
  });
});
