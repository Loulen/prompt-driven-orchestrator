import { describe, expect, it } from "vitest";
import {
  formatSize,
  isKebabCase,
  parseSimpleYaml,
  parseSkillMd,
  splitFrontmatter,
  timeAgo,
  validateSkillMd,
} from "./skillMd";

const VALID = `---
name: tdd
description: Test-driven development. Red-green-refactor at pre-agreed seams.
allowed-tools: Bash(npm:*) Bash(cargo:*)
---

# Test-driven development

Red, green, refactor.
`;

function states(text: string, names?: string[]) {
  return Object.fromEntries(validateSkillMd(text, names).checks.map((c) => [c.id, c.state]));
}

describe("skillMd — kebab-case", () => {
  it.each(["tdd", "code-review", "a1-b2"])("accepts %s", (name) => {
    expect(isKebabCase(name)).toBe(true);
  });
  it.each(["TDD", "code_review", "-lead", "trail-", "double--dash", "", "with space"])(
    "refuses %s",
    (name) => {
      expect(isKebabCase(name)).toBe(false);
    },
  );
});

describe("skillMd — frontmatter split and parse", () => {
  it("splits a closed block from its body", () => {
    const split = splitFrontmatter(VALID);
    expect(split?.yaml).toContain("name: tdd");
    expect(split?.body.trim().startsWith("# Test-driven development")).toBe(true);
  });

  it("returns null without an opening or closing fence", () => {
    expect(splitFrontmatter("# just markdown")).toBeNull();
    expect(splitFrontmatter("---\nname: tdd\nbody without closing")).toBeNull();
    expect(splitFrontmatter("--- not a fence\n---\n")).toBeNull();
  });

  it("parses scalars, quotes and block scalars", () => {
    const parsed = parseSimpleYaml(
      'name: "tdd"\ndescription: |\n  Line one\n  Line two\nother: \'x\'\n# comment\n',
    );
    expect(parsed.name).toBe("tdd");
    expect(parsed.description).toBe("Line one\nLine two");
    expect(parsed.other).toBe("x");
  });

  it("exposes name and description", () => {
    const parsed = parseSkillMd(VALID);
    expect(parsed.name).toBe("tdd");
    expect(parsed.description).toMatch(/^Test-driven/);
    expect(parsed.frontmatter?.["allowed-tools"]).toBe("Bash(npm:*) Bash(cargo:*)");
  });
});

describe("skillMd — the five checks", () => {
  it("passes a complete SKILL.md against an empty bank", () => {
    const result = validateSkillMd(VALID, []);
    expect(result.valid).toBe(true);
    expect(result.reason).toBeNull();
    expect(states(VALID, [])).toEqual({
      frontmatter: "pass",
      name: "pass",
      description: "pass",
      body: "pass",
      unique: "pass",
    });
  });

  it("leaves `unique` pending while the bank is unknown, so Create stays disabled", () => {
    const result = validateSkillMd(VALID);
    expect(result.valid).toBe(false);
    expect(states(VALID).unique).toBe("pending");
  });

  it("flags a missing description with reason and consequence (FP step 3)", () => {
    const text = VALID.replace(/description: .*\n/, "");
    const result = validateSkillMd(text, []);
    expect(result.valid).toBe(false);
    expect(states(text, []).description).toBe("fail");
    expect(result.reason).toMatch(/no `description`/);
    expect(result.reason).toMatch(/nothing was written/i);
  });

  it("flags a non-kebab name", () => {
    const text = VALID.replace("name: tdd", "name: TDD");
    const result = validateSkillMd(text, []);
    expect(states(text, []).name).toBe("fail");
    expect(result.reason).toMatch(/not kebab-case/);
  });

  it("flags an empty body", () => {
    const text = "---\nname: tdd\ndescription: x\n---\n\n   \n";
    expect(states(text, []).body).toBe("fail");
    expect(validateSkillMd(text, []).reason).toMatch(/body .* empty/i);
  });

  it("flags no frontmatter and leaves the field checks pending", () => {
    const result = validateSkillMd("# no frontmatter\n\nbody", []);
    expect(result.checks[0].state).toBe("fail");
    expect(states("# no frontmatter", []).name).toBe("pending");
    expect(result.reason).toMatch(/no frontmatter block/);
  });

  it("has no reason for blank text (nothing typed yet)", () => {
    expect(validateSkillMd("", []).reason).toBeNull();
  });

  it("detects a case-insensitive name collision locally", () => {
    const result = validateSkillMd(VALID, ["TDD", "grilling"]);
    expect(result.valid).toBe(false);
    expect(states(VALID, ["TDD"]).unique).toBe("fail");
    expect(result.reason).toMatch(/`TDD` exists/);
  });

  it("marks `unique` failed after a server 409 even if the local bank did not know", () => {
    const result = validateSkillMd(VALID, [], true);
    expect(result.valid).toBe(false);
    expect(result.checks.find((c) => c.id === "unique")?.state).toBe("fail");
    expect(result.reason).toMatch(/`tdd` exists/);
  });
});

describe("skillMd — display helpers", () => {
  it("formats sizes", () => {
    expect(formatSize(12)).toBe("12 B");
    expect(formatSize(1229)).toBe("1.2 kB");
    expect(formatSize(3 * 1024 * 1024)).toBe("3.0 MB");
  });

  it("formats relative times", () => {
    const now = Date.parse("2026-09-03T12:00:00Z");
    expect(timeAgo("2026-09-03T11:59:50Z", now)).toBe("just now");
    expect(timeAgo("2026-09-03T11:58:00Z", now)).toBe("2 min ago");
    expect(timeAgo("2026-09-03T09:00:00Z", now)).toBe("3 h ago");
    expect(timeAgo("2026-08-31T12:00:00Z", now)).toBe("3 days ago");
    expect(timeAgo("not a date", now)).toBe("not a date");
  });
});
