import { describe, expect, it } from "vitest";
import type { Skill } from "../../shared/generated/Skill";
import type { SkillFolder } from "../../shared/generated/SkillFolder";
import { skillOptions, skillQuery } from "./skillCompletion";

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
  folders: [folder("f-p", "Team"), folder("f-s", "Sub", "f-p")],
  skills: [
    skill("s-rn", "release-notes"),
    skill("s-rc", "review-checklist", "f-p"),
    skill("s-cr", "code-review", "f-p"),
    skill("s-rs", "revert-safely", "f-s"),
    skill("s-tdd", "tdd", "f-p"),
  ],
};

describe("skillQuery", () => {
  it("opens on a / at the start of the text or of a word", () => {
    expect(skillQuery("/", 1)).toEqual({ trigger: "/", query: "", start: 0, end: 1 });
    expect(skillQuery("Use /rev", 8)).toEqual({ trigger: "/", query: "rev", start: 4, end: 8 });
    expect(skillQuery("then\n/co", 8)?.query).toBe("co");
  });

  it("stays closed inside a path, a URL or after the word", () => {
    expect(skillQuery("src/lib", 7)).toBeNull();
    expect(skillQuery("see https://x.io/a", 18)).toBeNull();
    expect(skillQuery("/usr/bin", 8)).toBeNull();
    expect(skillQuery("/tdd now", 8)).toBeNull();
    expect(skillQuery("plain", 5)).toBeNull();
  });
});

describe("skillOptions", () => {
  it("puts the received skills first, then the bank, each filtered by the query", () => {
    const options = skillOptions(bank, new Set(["s-cr"]), "rev");
    expect(options.map((o) => [o.skill.name, o.received])).toEqual([
      ["code-review", true],
      ["revert-safely", false],
      ["review-checklist", false],
    ]);
  });

  it("matches any case and names each skill's folder path", () => {
    const options = skillOptions(bank, new Set(), "SAFE");
    expect(options).toHaveLength(1);
    expect(options[0].folder).toBe("Team/Sub/");
    expect(skillOptions(bank, new Set(), "notes")[0].folder).toBe("");
  });

  it("lists the whole bank on an empty query, received first", () => {
    const names = skillOptions(bank, new Set(["s-tdd"]), "").map((o) => o.skill.name);
    expect(names).toEqual(["tdd", "code-review", "release-notes", "revert-safely", "review-checklist"]);
  });
});
