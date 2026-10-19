import { describe, expect, it } from "vitest";
import type { Skill } from "../../shared/generated/Skill";
import type { SkillFolder } from "../../shared/generated/SkillFolder";
import { buildRows } from "./skillTree";
import { checkState, clickCheck, exportRequest, skillIdsOf } from "./skillExport";

const t = "2026-10-19T10:00:00Z";
const folder = (id: string, name: string, parent_id: string | null = null): SkillFolder => ({
  id,
  name,
  parent_id,
  created_at: t,
  updated_at: t,
});
const skill = (id: string, name: string, folder_id: string | null = null): Skill => ({
  id,
  name,
  description: "",
  folder_id,
  locked: false,
  created_at: t,
  updated_at: t,
});

const bank = {
  folders: [folder("f-a", "a"), folder("f-b", "b", "f-a"), folder("f-c", "c")],
  skills: [skill("s1", "one", "f-a"), skill("s2", "two", "f-b"), skill("s3", "three", "f-c"), skill("s4", "four")],
};

describe("skillExport", () => {
  it("a folder stands for every skill below it", () => {
    expect(skillIdsOf({ kind: "folder", id: "f-a" }, bank).sort()).toEqual(["s1", "s2"]);
    expect(skillIdsOf({ kind: "skill", id: "s4" }, bank)).toEqual(["s4"]);
  });

  it("a click checks a row's skills, a second one unchecks them", () => {
    const once = clickCheck(new Set(), { kind: "folder", id: "f-a" }, bank);
    expect([...once].sort()).toEqual(["s1", "s2"]);
    expect(checkState(once, ["s1", "s2"])).toBe("all");
    expect(checkState(once, ["s1", "s3"])).toBe("some");
    expect(checkState(once, [])).toBe("none");
    const twice = clickCheck(once, { kind: "folder", id: "f-a" }, bank);
    expect(twice.size).toBe(0);
    const partly = clickCheck(new Set(["s1"]), { kind: "folder", id: "f-a" }, bank);
    expect([...partly].sort()).toEqual(["s1", "s2"]);
  });

  it("shift checks the visible rows between the anchor and the click", () => {
    const rows = buildRows(bank.folders, bank.skills, new Set(["f-a", "f-b", "f-c"]), "");
    const next = clickCheck(new Set(), { kind: "skill", id: "s3" }, bank, {
      anchor: { kind: "skill", id: "s2" },
      rows,
    });
    // Rows: a › b › two, one, c › three, four: from `two` to `three`.
    expect([...next].sort()).toEqual(["s1", "s2", "s3"]);
    expect(next.has("s4")).toBe(false);
  });

  it("asks for the folder when the selection is exactly its skills", () => {
    expect(exportRequest(new Set(["s1", "s2"]), bank)).toEqual({ skill_ids: [], folder_ids: ["f-a"] });
    expect(exportRequest(new Set(["s2"]), bank)).toEqual({ skill_ids: [], folder_ids: ["f-b"] });
    const parentOfOne = { ...bank, skills: bank.skills.filter((s) => s.id !== "s1") };
    expect(exportRequest(new Set(["s2"]), parentOfOne)).toEqual({ skill_ids: [], folder_ids: ["f-b"] });
    expect(exportRequest(new Set(["s1", "s4", "gone"]), bank)).toEqual({ skill_ids: ["s1", "s4"], folder_ids: [] });
  });
});
