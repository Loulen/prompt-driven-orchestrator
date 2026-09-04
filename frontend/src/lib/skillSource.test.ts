import { describe, expect, it } from "vitest";
import { displaySourceUrl, parseSkillSource, shortCommit } from "./skillSource";

describe("parseSkillSource (#670)", () => {
  it("reads a GitHub root URL", () => {
    expect(parseSkillSource("https://github.com/anthropics/skills")).toEqual({
      kind: "git",
      url: "https://github.com/anthropics/skills",
      ref: null,
      path: "",
      repo: "anthropics/skills",
      suggestedFolder: "anthropics/skills",
    });
  });

  it("reads a /tree/<branch>/<path> URL and proposes `repo · last segment`", () => {
    const parsed = parseSkillSource("https://github.com/anthropics/skills/tree/main/skills/engineering");
    expect(parsed?.ref).toBe("main");
    expect(parsed?.path).toBe("skills/engineering");
    expect(parsed?.suggestedFolder).toBe("anthropics/skills · engineering");
    expect(parseSkillSource("https://github.com/o/r.git/tree/dev")?.ref).toBe("dev");
    expect(parseSkillSource("https://gitlab.com/o/r/-/tree/main/skills")?.path).toBe("skills");
  });

  it("reads SSH, file:// and local folders", () => {
    expect(parseSkillSource("git@github.com:ippon/private-skills.git")?.repo).toBe("ippon/private-skills");
    expect(parseSkillSource("file:///tmp/fixture.git")?.kind).toBe("git");
    const local = parseSkillSource("~/Documents/skills-repo/");
    expect(local?.kind).toBe("local");
    expect(local?.url).toBe("~/Documents/skills-repo");
    expect(local?.suggestedFolder).toBe("skills-repo");
  });

  it("returns null for anything else", () => {
    expect(parseSkillSource("")).toBeNull();
    expect(parseSkillSource("not a source")).toBeNull();
    expect(parseSkillSource("https://github.com/owner-only")).toBeNull();
    expect(parseSkillSource("ftp://x/y/z")).toBeNull();
  });

  it("helpers shorten commits and URLs", () => {
    expect(shortCommit("3f9c2e1abcdef")).toBe("3f9c2e1");
    expect(shortCommit(null)).toBe("");
    expect(displaySourceUrl("https://github.com/anthropics/skills.git")).toBe("github.com/anthropics/skills");
  });
});
