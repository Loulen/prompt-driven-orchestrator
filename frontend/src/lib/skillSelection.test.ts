import { describe, expect, it } from "vitest";
import type { Skill, SkillBank, SkillFolder } from "../types";
import {
  addRefs,
  allSelected,
  activeCountLabel,
  missingIds,
  removeRef,
  resolveActiveSkills,
  skillsInFolder,
  toRefs,
} from "./skillSelection";

const skill = (id: string, name: string, folder_id: string | null = null): Skill => ({
  id,
  name,
  description: `${name} description`,
  folder_id,
  created_at: "2026-09-03T00:00:00.000Z",
  updated_at: "2026-09-03T00:00:00.000Z",
});
const folder = (id: string, name: string, parent_id: string | null = null): SkillFolder => ({
  id,
  name,
  parent_id,
  created_at: "2026-09-03T00:00:00.000Z",
  updated_at: "2026-09-03T00:00:00.000Z",
});

const bank: SkillBank = {
  root_path: "/tmp/.pdo/skills",
  folders: [folder("f-method", "method"), folder("f-java", "java", "f-method")],
  skills: [
    skill("a", "tdd", "f-method"),
    skill("b", "grilling", "f-method"),
    skill("c", "code-review"),
    skill("d", "spring", "f-java"),
  ],
};

describe("resolveActiveSkills", () => {
  it("unions the tiers coarsest first and attributes each row to its origin", () => {
    const resolved = resolveActiveSkills(
      "node",
      [{ id: "c", name: "code-review" }],
      [
        { tier: "project", skills: [{ id: "b", name: "grilling" }] },
        { tier: "instance", skills: [{ id: "a", name: "tdd" }] },
      ],
      bank,
    );
    expect(resolved.rows.map((row) => [row.id, row.tiers, row.own, row.inherited])).toEqual([
      ["a", ["instance"], false, true],
      ["b", ["project"], false, true],
      ["c", ["node"], true, false],
    ]);
    expect(resolved.activeCount).toBe(3);
    expect(resolved.missing).toEqual([]);
  });

  it("delivers a skill picked at two tiers once, own and inherited at the same time", () => {
    const resolved = resolveActiveSkills(
      "run",
      [{ id: "a", name: "tdd" }, { id: "d", name: "spring" }],
      [{ tier: "instance", skills: [{ id: "a", name: "tdd" }] }],
      bank,
    );
    expect(resolved.rows).toHaveLength(2);
    expect(resolved.rows[0]).toMatchObject({ id: "a", tiers: ["instance", "run"], own: true, inherited: true });
    expect(resolved.rows[1]).toMatchObject({ id: "d", tiers: ["run"], own: true, inherited: false });
    expect(resolved.activeCount).toBe(2);
  });

  it("names a row from the bank, not from the stored label", () => {
    const resolved = resolveActiveSkills("node", [{ id: "a", name: "old-label" }], [], bank);
    expect(resolved.rows[0].name).toBe("tdd");
  });

  it("flags an id the bank no longer knows as missing, excluded from the active count", () => {
    const resolved = resolveActiveSkills(
      "node",
      [{ id: "gone", name: "deleted" }],
      [{ tier: "instance", skills: [{ id: "a", name: "tdd" }] }],
      bank,
    );
    expect(resolved.rows.map((row) => row.id)).toEqual(["a", "gone"]);
    expect(resolved.missing).toEqual([
      expect.objectContaining({ id: "gone", name: "deleted", missing: true, tiers: ["node"] }),
    ]);
    expect(resolved.activeCount).toBe(1);
  });

  it("ignores blank ids and handles empty tiers", () => {
    const resolved = resolveActiveSkills("node", [{ id: "  ", name: "x" }], [], bank);
    expect(resolved.rows).toEqual([]);
    expect(activeCountLabel(resolved.activeCount)).toBe("No skill");
    expect(activeCountLabel(1)).toBe("1 active skill");
    expect(activeCountLabel(3)).toBe("3 active skills");
  });
});

describe("folder gesture", () => {
  it("checking a folder yields its skills at this instant, recursively", () => {
    expect(skillsInFolder("f-method", bank).map((skill) => skill.id)).toEqual(["a", "b", "d"]);
    expect(skillsInFolder("f-java", bank).map((skill) => skill.id)).toEqual(["d"]);
    expect(skillsInFolder("f-unknown", bank)).toEqual([]);
  });

  it("addRefs keeps order and de-duplicates; removeRef drops by id", () => {
    const own = addRefs([{ id: "c", name: "code-review" }], toRefs(skillsInFolder("f-method", bank)));
    expect(own.map((skill) => skill.id)).toEqual(["c", "a", "b", "d"]);
    expect(addRefs(own, [{ id: "a", name: "tdd" }])).toHaveLength(4);
    expect(removeRef(own, "a").map((skill) => skill.id)).toEqual(["c", "b", "d"]);
  });

  it("allSelected says whether every skill of a folder is already own", () => {
    const own = toRefs(skillsInFolder("f-method", bank));
    expect(allSelected(own, skillsInFolder("f-method", bank))).toBe(true);
    expect(allSelected(removeRef(own, "d"), skillsInFolder("f-method", bank))).toBe(false);
    expect(allSelected(own, [])).toBe(false);
  });

  it("missingIds lists own references the bank lost", () => {
    expect(missingIds([{ id: "a", name: "tdd" }, { id: "zz", name: "lost" }], bank)).toEqual([
      { id: "zz", name: "lost" },
    ]);
  });
});
