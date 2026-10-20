import { describe, expect, it } from "vitest";
import type { DraftNotice } from "../../shared/generated/DraftNotice";
import { assistantOwner, fieldsLabel, myActor } from "./draftNotice";

describe("draftNotice", () => {
  it("names a few fields and counts the others", () => {
    expect(fieldsLabel([])).toBe("");
    expect(fieldsLabel(["prompt of reviewer"])).toBe("prompt of reviewer");
    expect(fieldsLabel(["a", "b"])).toBe("a and b");
    expect(fieldsLabel(["a", "b", "c", "d", "e", "f"])).toBe("a, b, c, d and 2 more");
  });

  it("says whose assistant changed the draft", () => {
    const notice: Extract<DraftNotice, { kind: "assistant" }> = {
      kind: "assistant",
      actor: { kind: "identified", namespace: "t", id: "alice" },
      display: { name: "Alice Martin", avatar_url: null },
      fields: [],
    };
    expect(assistantOwner(notice, { kind: "identified", namespace: "t", id: "alice" })).toBe("Your");
    expect(assistantOwner(notice, { kind: "local_operator" })).toBe("Alice Martin's");
    expect(assistantOwner({ ...notice, display: null }, null)).toBe("alice (t)'s");
  });

  it("finds the Actor of this tab's connection", () => {
    const me = { kind: "local_operator" } as const;
    const participant = {
      connection: "4",
      actor: me,
      display: null,
      color: 0,
      can_write: true,
      assistant: false,
      selection: null,
      typing: null,
    };
    expect(myActor([participant], "4")).toEqual(me);
    expect(myActor([participant], "5")).toBeNull();
  });
});
