import { describe, it, expect } from "vitest";
import {
  MAX_FILE_BYTES,
  formatBytes,
  isSkillMd,
  mergeStaged,
  normaliseRelativePath,
  sortDroppedFiles,
  sortHostPicks,
  totalStagedBytes,
} from "./skillFiles";

function file(name: string, content = "x", type = "text/plain"): File {
  return new File([content], name, { type });
}

describe("skillFiles (#671)", () => {
  it("recognises SKILL.md by its file name, at any depth", () => {
    expect(isSkillMd("SKILL.md")).toBe(true);
    expect(isSkillMd("my-skill/SKILL.md")).toBe(true);
    expect(isSkillMd("skill.md")).toBe(false);
    expect(isSkillMd("SKILL.md.bak")).toBe(false);
  });

  it("normalises relative paths and refuses anything leaving the folder", () => {
    expect(normaliseRelativePath(" examples/login.spec.ts ")).toBe("examples/login.spec.ts");
    expect(normaliseRelativePath("a\\b.md")).toBe("a/b.md");
    for (const bad of ["", "/etc/passwd", "../x", "a/../b", "a//b", "./a"]) {
      expect(normaliseRelativePath(bad)).toBeNull();
    }
  });

  it("sorts a drop into staged files, a SKILL.md and refused folders", () => {
    const folder = new File([], "fixtures", { type: "" });
    const items = [
      { kind: "file", webkitGetAsEntry: () => ({ isDirectory: false, name: "notes.md" }) },
      { kind: "file", webkitGetAsEntry: () => ({ isDirectory: true, name: "fixtures" }) },
      { kind: "file", webkitGetAsEntry: () => ({ isDirectory: false, name: "SKILL.md" }) },
    ] as unknown as DataTransferItem[];
    const sorted = sortDroppedFiles([file("notes.md", "# n"), folder, file("SKILL.md", "---")], items);
    expect(sorted.files.map((f) => f.path)).toEqual(["notes.md"]);
    expect(sorted.files[0].size).toBe(3);
    expect(sorted.files[0].status).toEqual({ state: "staged" });
    expect(sorted.skillMd?.name).toBe("SKILL.md");
    expect(sorted.refused).toEqual([{ name: "fixtures/", reason: "Drop files, not folders" }]);
  });

  it("refuses an oversize file in place and keeps a folder drop's sub-path", () => {
    const big = { name: "big.bin", size: MAX_FILE_BYTES + 1, type: "application/octet-stream" } as File;
    const nested = Object.assign(file("login.spec.ts"), { webkitRelativePath: "examples/login.spec.ts" });
    const sorted = sortDroppedFiles([big, nested]);
    expect(sorted.refused).toHaveLength(1);
    expect(sorted.refused[0].name).toBe("big.bin");
    expect(sorted.refused[0].reason).toMatch(/10 MB limit/);
    expect(sorted.files.map((f) => f.path)).toEqual(["examples/login.spec.ts"]);
  });

  it("sorts explorer picks: SKILL.md is a replacement, the rest are host copies", () => {
    const picks = sortHostPicks(["/home/u/notes/cheatsheet.md", "/home/u/skill/SKILL.md"]);
    expect(picks.skillMdPath).toBe("/home/u/skill/SKILL.md");
    expect(picks.files).toEqual([
      {
        path: "cheatsheet.md",
        size: null,
        source: { kind: "host", fromPath: "/home/u/notes/cheatsheet.md" },
        status: { state: "staged" },
      },
    ]);
  });

  it("merges a same-path row in place with the `replaces` badge, and sums sizes", () => {
    const a = sortDroppedFiles([file("a.md", "aa"), file("b.md", "bbb")]).files;
    const a2 = sortDroppedFiles([file("a.md", "aaaa")]).files;
    const merged = mergeStaged(a, a2);
    expect(merged.map((f) => f.path)).toEqual(["a.md", "b.md"]);
    expect(merged[0].replaces).toBe(true);
    expect(merged[0].size).toBe(4);
    expect(totalStagedBytes(merged)).toBe(7);
  });

  it("formats sizes in the row grammar", () => {
    expect(formatBytes(80)).toBe("80 B");
    expect(formatBytes(4198)).toBe("4.1 KB");
    expect(formatBytes(12 * 1024 * 1024)).toBe("12.0 MB");
  });
});
