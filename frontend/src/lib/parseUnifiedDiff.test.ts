import { describe, it, expect } from "vitest";
import { parseUnifiedDiff } from "./parseUnifiedDiff";

describe("parseUnifiedDiff", () => {
  it("returns [] for an empty string", () => {
    expect(parseUnifiedDiff("")).toEqual([]);
  });

  it("returns [] for whitespace without throwing", () => {
    expect(() => parseUnifiedDiff("   \n\t\n")).not.toThrow();
    expect(parseUnifiedDiff("   \n\t\n")).toEqual([]);
  });

  it("parses a modified file with +/- counts", () => {
    const raw = [
      "diff --git a/src/foo.rs b/src/foo.rs",
      "index abc1234..def5678 100644",
      "--- a/src/foo.rs",
      "+++ b/src/foo.rs",
      "@@ -1,3 +1,3 @@",
      " fn keep() {}",
      "-fn old() {}",
      "+fn new() {}",
      "",
    ].join("\n");
    const files = parseUnifiedDiff(raw);
    expect(files).toHaveLength(1);
    const f = files[0];
    expect(f.oldPath).toBe("src/foo.rs");
    expect(f.newPath).toBe("src/foo.rs");
    expect(f.displayPath).toBe("src/foo.rs");
    expect(f.status).toBe("modified");
    expect(f.isBinary).toBe(false);
    expect(f.additions).toBe(1);
    expect(f.deletions).toBe(1);
  });

  it("parses multiple files, preserving order, with a verbatim round-trip", () => {
    const raw = [
      "diff --git a/a.txt b/a.txt",
      "index 111..222 100644",
      "--- a/a.txt",
      "+++ b/a.txt",
      "@@ -1 +1 @@",
      "-one",
      "+ONE",
      "diff --git a/b.txt b/b.txt",
      "index 333..444 100644",
      "--- a/b.txt",
      "+++ b/b.txt",
      "@@ -1 +1 @@",
      "-two",
      "+TWO",
      "",
    ].join("\n");
    const files = parseUnifiedDiff(raw);
    expect(files.map((f) => f.newPath)).toEqual(["a.txt", "b.txt"]);
    // Bodies concatenate back to the original (no preamble before diff --git).
    expect(files.map((f) => f.body).join("")).toBe(raw);
  });

  it("parses an addition via /dev/null (oldPath null)", () => {
    const raw = [
      "diff --git a/new.txt b/new.txt",
      "new file mode 100644",
      "index 0000000..abc1234",
      "--- /dev/null",
      "+++ b/new.txt",
      "@@ -0,0 +1,2 @@",
      "+line one",
      "+line two",
      "",
    ].join("\n");
    const [f] = parseUnifiedDiff(raw);
    expect(f.oldPath).toBeNull();
    expect(f.newPath).toBe("new.txt");
    expect(f.displayPath).toBe("new.txt");
    expect(f.status).toBe("added");
    expect(f.additions).toBe(2);
    expect(f.deletions).toBe(0);
  });

  it("parses a deletion (newPath null)", () => {
    const raw = [
      "diff --git a/gone.txt b/gone.txt",
      "deleted file mode 100644",
      "index abc1234..0000000",
      "--- a/gone.txt",
      "+++ /dev/null",
      "@@ -1,2 +0,0 @@",
      "-old line one",
      "-old line two",
      "",
    ].join("\n");
    const [f] = parseUnifiedDiff(raw);
    expect(f.oldPath).toBe("gone.txt");
    expect(f.newPath).toBeNull();
    expect(f.displayPath).toBe("gone.txt");
    expect(f.status).toBe("deleted");
    expect(f.additions).toBe(0);
    expect(f.deletions).toBe(2);
  });

  it("parses a pure 100% rename (no hunks, 0/0)", () => {
    const raw = [
      "diff --git a/old/name.txt b/new/name.txt",
      "similarity index 100%",
      "rename from old/name.txt",
      "rename to new/name.txt",
      "",
    ].join("\n");
    const [f] = parseUnifiedDiff(raw);
    expect(f.oldPath).toBe("old/name.txt");
    expect(f.newPath).toBe("new/name.txt");
    expect(f.displayPath).toBe("old/name.txt → new/name.txt");
    expect(f.status).toBe("renamed");
    expect(f.additions).toBe(0);
    expect(f.deletions).toBe(0);
  });

  it("parses a rename with edits (renamed AND counts)", () => {
    const raw = [
      "diff --git a/old.txt b/renamed.txt",
      "similarity index 80%",
      "rename from old.txt",
      "rename to renamed.txt",
      "index abc..def 100644",
      "--- a/old.txt",
      "+++ b/renamed.txt",
      "@@ -1,2 +1,2 @@",
      " keep",
      "-old",
      "+new",
      "",
    ].join("\n");
    const [f] = parseUnifiedDiff(raw);
    expect(f.status).toBe("renamed");
    expect(f.oldPath).toBe("old.txt");
    expect(f.newPath).toBe("renamed.txt");
    expect(f.displayPath).toBe("old.txt → renamed.txt");
    expect(f.additions).toBe(1);
    expect(f.deletions).toBe(1);
  });

  it("parses a copy", () => {
    const raw = [
      "diff --git a/src.txt b/copy.txt",
      "similarity index 100%",
      "copy from src.txt",
      "copy to copy.txt",
      "",
    ].join("\n");
    const [f] = parseUnifiedDiff(raw);
    expect(f.status).toBe("copied");
    expect(f.oldPath).toBe("src.txt");
    expect(f.newPath).toBe("copy.txt");
    expect(f.displayPath).toBe("src.txt → copy.txt");
  });

  it("parses a modified binary file (isBinary, 0/0)", () => {
    const raw = [
      "diff --git a/logo.png b/logo.png",
      "index abc1234..def5678 100644",
      "Binary files a/logo.png and b/logo.png differ",
      "",
    ].join("\n");
    const [f] = parseUnifiedDiff(raw);
    expect(f.isBinary).toBe(true);
    expect(f.status).toBe("modified");
    expect(f.oldPath).toBe("logo.png");
    expect(f.newPath).toBe("logo.png");
    expect(f.additions).toBe(0);
    expect(f.deletions).toBe(0);
  });

  it("parses a new binary file (isBinary, added)", () => {
    const raw = [
      "diff --git a/img.png b/img.png",
      "new file mode 100644",
      "index 0000000..abc1234",
      "Binary files /dev/null and b/img.png differ",
      "",
    ].join("\n");
    const [f] = parseUnifiedDiff(raw);
    expect(f.isBinary).toBe(true);
    expect(f.status).toBe("added");
    expect(f.oldPath).toBeNull();
    expect(f.newPath).toBe("img.png");
  });

  it("handles a path with spaces (trailing tab on ---/+++)", () => {
    const raw = [
      "diff --git a/with space.txt b/with space.txt",
      "index abc..def 100644",
      "--- a/with space.txt\t",
      "+++ b/with space.txt\t",
      "@@ -1 +1 @@",
      "-old",
      "+new",
      "",
    ].join("\n");
    const [f] = parseUnifiedDiff(raw);
    expect(f.oldPath).toBe("with space.txt");
    expect(f.newPath).toBe("with space.txt");
    expect(f.displayPath).toBe("with space.txt");
    expect(f.additions).toBe(1);
    expect(f.deletions).toBe(1);
  });

  it("decodes a C-quoted unicode path", () => {
    // \303\251 = 0xC3 0xA9 = "é".
    const raw = [
      'diff --git "a/unicode-caf\\303\\251.txt" "b/unicode-caf\\303\\251.txt"',
      "index abc..def 100644",
      '--- "a/unicode-caf\\303\\251.txt"',
      '+++ "b/unicode-caf\\303\\251.txt"',
      "@@ -1 +1 @@",
      "-old",
      "+new",
      "",
    ].join("\n");
    const [f] = parseUnifiedDiff(raw);
    expect(f.oldPath).toBe("unicode-café.txt");
    expect(f.newPath).toBe("unicode-café.txt");
    expect(f.displayPath).toBe("unicode-café.txt");
  });

  it("ignores `\\ No newline at end of file` lines in the counts", () => {
    const raw = [
      "diff --git a/nonl.txt b/nonl.txt",
      "index abc..def 100644",
      "--- a/nonl.txt",
      "+++ b/nonl.txt",
      "@@ -1 +1 @@",
      "-old line",
      "\\ No newline at end of file",
      "+new line",
      "\\ No newline at end of file",
      "",
    ].join("\n");
    const [f] = parseUnifiedDiff(raw);
    expect(f.additions).toBe(1);
    expect(f.deletions).toBe(1);
  });

  it("is best-effort (never throws) on a malformed chunk and keeps the body verbatim", () => {
    const raw = "diff --git a/weird b/weird\n<garbage without markers>\n";
    expect(() => parseUnifiedDiff(raw)).not.toThrow();
    const files = parseUnifiedDiff(raw);
    expect(files).toHaveLength(1);
    expect(files[0].body).toBe(raw);
  });

  it("keeps each file's body verbatim (header + hunks)", () => {
    const raw = [
      "diff --git a/x.txt b/x.txt",
      "index 111..222 100644",
      "--- a/x.txt",
      "+++ b/x.txt",
      "@@ -1 +1 @@",
      "-a",
      "+b",
      "",
    ].join("\n");
    const [f] = parseUnifiedDiff(raw);
    expect(f.body).toBe(raw);
    expect(f.body).toContain("@@ -1 +1 @@");
    expect(f.body).toContain("+b");
  });
});
