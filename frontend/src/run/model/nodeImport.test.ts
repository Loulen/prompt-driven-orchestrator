import { describe, expect, it } from "vitest";
import { importBlockedReason, importedFilesText, notPilotRefusal, stageImportFiles } from "./nodeImport";
import { ApiError } from "../../shared/http/client";

const file = (name: string, body = "x") => new File([body], name);

describe("importedFilesText", () => {
  it("names one file by its path relative to the worktree", () => {
    expect(importedFilesText([".pdo/artifacts/_attachments/grill/contrat.pdf"])).toBe(
      "I imported a file for you: `.pdo/artifacts/_attachments/grill/contrat.pdf` (path relative to the worktree root).",
    );
  });

  it("gives each file of a multi-file import its own phrase, on one line", () => {
    const text = importedFilesText(["a/x.pdf", "a/y.csv"]);
    expect(text).toBe(
      "I imported a file for you: `a/x.pdf` (path relative to the worktree root). " +
        "I imported a file for you: `a/y.csv` (path relative to the worktree root).",
    );
    expect(text).not.toContain("\n");
  });
});

describe("stageImportFiles", () => {
  it("adds new files and replaces one picked again under the same name", () => {
    const first = file("contrat.pdf", "v1");
    const again = file("contrat.pdf", "v22");
    const staged = stageImportFiles([first], [file("annexe.csv"), again]);
    expect(staged.map((f) => f.name)).toEqual(["contrat.pdf", "annexe.csv"]);
    expect(staged[0]).toBe(again);
  });
});

describe("importBlockedReason", () => {
  const live = { isArchived: false, selectedIter: 2, currentIter: 2, status: "running" as const };

  it("lets a running or awaiting current iteration receive files", () => {
    expect(importBlockedReason(live)).toBeNull();
    expect(importBlockedReason({ ...live, status: "awaiting_user" })).toBeNull();
  });

  it("says why an archived Run, an older iteration or a settled node cannot", () => {
    expect(importBlockedReason({ ...live, isArchived: true })).toContain("archived");
    expect(importBlockedReason({ ...live, selectedIter: 1 })).toContain("iter 2");
    expect(importBlockedReason({ ...live, status: "completed" })).toContain("no live session");
  });
});

describe("notPilotRefusal", () => {
  it("reads the pilot of a 409 not_pilot, and nothing else", () => {
    const alice = { kind: "identified", namespace: "test", id: "alice" } as const;
    expect(
      notPilotRefusal(new ApiError("x", { status: 409, body: { error: "not_pilot", pilot: alice } })),
    ).toEqual({ pilot: alice });
    expect(notPilotRefusal(new ApiError("x", { status: 409, body: { error: "not_pilot" } }))).toEqual({
      pilot: null,
    });
    expect(notPilotRefusal(new ApiError("x", { status: 409, body: { error: "node_session_not_live" } }))).toBeNull();
    expect(notPilotRefusal(new ApiError("x", { status: 502 }))).toBeNull();
    expect(notPilotRefusal(new Error("x"))).toBeNull();
  });
});
