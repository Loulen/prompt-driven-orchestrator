import { describe, expect, it } from "vitest";
import { importedFilesText, stageImportFiles } from "./attachments";

describe("importedFilesText (#971)", () => {
  it("names one file by its path relative to the worktree root, on one line", () => {
    const text = importedFilesText([".pdo/artifacts/_attachments/grill/contrat.pdf"]);
    expect(text).toBe(
      "I imported a file for you: `.pdo/artifacts/_attachments/grill/contrat.pdf` (path relative to the worktree root).",
    );
    expect(text).not.toContain("\n");
  });

  it("lists several files", () => {
    expect(importedFilesText(["a/x.pdf", "a/y.csv"])).toBe(
      "I imported 2 files for you: `a/x.pdf`, `a/y.csv` (paths relative to the worktree root).",
    );
  });
});

describe("stageImportFiles (#971)", () => {
  it("adds new names and replaces a re-picked one in place", () => {
    const a = new File(["1"], "a.pdf");
    const b = new File(["2"], "b.pdf");
    const a2 = new File(["22"], "a.pdf");
    const next = stageImportFiles(stageImportFiles([], [a, b]), [a2]);
    expect(next).toEqual([a2, b]);
  });
});
