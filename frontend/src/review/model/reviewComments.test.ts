import { describe, it, expect } from "vitest";
import { authorKind, authorLabel, commentState, firstWords, footerStatus, sortForSidebar, stateCounts, stateCountsByPath, pendingCount, relativeTime, sendDisabledReason, pendingTone, reviewQuickAccessTitle } from "./commentStatus";
import { addDraft, anchorLabel, draftsForPair, readDrafts, removeDrafts, toSendInputs, updateDraft, wipKey, writeDrafts } from "./drafts";
import { countsByPath, entryAt, mergeEntries, homeIds, mappingsOf, outdatedCountByPath, parseExcerpt, remapSummary, sentEntriesForPair } from "./entries";
import { allSeen, markSeen, readSeen, unreadCommentCount, unreadReplies, writeSeen } from "./seen";
import type { ReviewComment } from "../../shared/generated/ReviewComment";
import type { RunRefs } from "../../shared/generated/RunRefs";

const PAIR = { from: "fork", to: "tip" };

function sent(overrides: Partial<ReviewComment> = {}): ReviewComment {
  return { excerpt: "",
    id: "rc-001",
    path: "b.ts",
    side: "new",
    line: 5,
    from_ref: "fork",
    to_ref: "tip",
    text: "t",
    author: "user",
    sent_at: "2026-09-09T10:00:00Z",
    status: "sent",
    ...overrides,
  };
}

describe("reviewComments (#750)", () => {
  it("round-trips drafts through storage and drops garbage", () => {
    localStorage.clear();
    let drafts = addDraft([], { path: "a.ts", side: "new", line: 3 }, PAIR, "hello");
    drafts = addDraft(drafts, { path: "a.ts", side: "old", line: 1 }, PAIR, "old side");
    writeDrafts("r1", drafts);
    expect(readDrafts("r1")).toEqual(drafts);
    expect(readDrafts("r2")).toEqual([]);
    localStorage.setItem("pdo.review.drafts.r3", JSON.stringify([{ nope: 1 }, drafts[0]]));
    expect(readDrafts("r3")).toEqual([drafts[0]]);
    localStorage.setItem("pdo.review.drafts.r4", "{not json");
    expect(readDrafts("r4")).toEqual([]);
    writeDrafts("r1", []);
    expect(localStorage.getItem("pdo.review.drafts.r1")).toBeNull();
  });

  it("updates and removes by key; keys are unique", () => {
    let drafts = addDraft([], { path: "a.ts", side: "new", line: 3 }, PAIR, "a");
    drafts = addDraft(drafts, { path: "a.ts", side: "new", line: 4 }, PAIR, "b");
    expect(drafts[0].key).not.toBe(drafts[1].key);
    const upd = updateDraft(drafts, drafts[0].key, "A");
    expect(upd[0].text).toBe("A");
    expect(upd[1].text).toBe("b");
    expect(removeDrafts(upd, [drafts[1].key])).toHaveLength(1);
  });

  it("merges drafts and sent comments of the pair in diff order, sent first on a shared line", () => {
    const drafts = [
      ...addDraft([], { path: "b.ts", side: "new", line: 9 }, PAIR, "late"),
      ...addDraft([], { path: "a.ts", side: "new", line: 2 }, PAIR, "early"),
      ...addDraft([], { path: "a.ts", side: "old", line: 2 }, PAIR, "left"),
      ...addDraft([], { path: "a.ts", side: "new", line: 1 }, { from: "fork", to: "node:x:1:after" }, "other pair"),
      ...addDraft([], { path: "b.ts", side: "new", line: 5 }, PAIR, "stale"),
    ];
    const entries = mergeEntries(drafts, [sent()], PAIR, ["a.ts", "b.ts"]);
    expect(entries.map((e) => `${e.kind}:${anchorLabel(e.anchor)}`)).toEqual([
      "draft:a.ts:L2",
      "draft:a.ts:R2",
      "sent:b.ts:R5",
      "draft:b.ts:R5",
      "draft:b.ts:R9",
    ]);
    expect(entryAt(entries, { path: "b.ts", side: "new", line: 5 })?.kind).toBe("sent");
    expect(entryAt(entries, { path: "zz", side: "new", line: 5 })).toBeUndefined();
    expect(draftsForPair(drafts, PAIR)).toHaveLength(4);
    const counts = countsByPath(entries);
    expect(counts.get("a.ts")).toEqual({ drafts: 2, sent: 0 });
    expect(counts.get("b.ts")).toEqual({ drafts: 2, sent: 1 });
  });

  it("labels anchors GitHub-style and builds the wire inputs", () => {
    expect(anchorLabel({ path: "frontend/src/pages/ReviewPage.tsx", side: "new", line: 48 })).toBe("ReviewPage.tsx:R48");
    expect(anchorLabel({ path: "review.rs", side: "old", line: 123 })).toBe("review.rs:L123");
    const drafts = addDraft([], { path: "a.ts", side: "new", line: 3 }, PAIR, "x");
    expect(toSendInputs(drafts)).toEqual([{ path: "a.ts", side: "new", line: 3, from: "fork", to: "tip", text: "x" }]);
    expect(wipKey("r1", { path: "a.ts", side: "new", line: 3 }, PAIR)).toBe("pdo.review.wip.r1|a.ts|new|3|fork|tip");
  });

  it("explains why sending is disabled: archived, or a tip that no longer resolves", () => {
    const refs: RunRefs = {
      default_from: "fork",
      default_to: "tip",
      deliveries: [],
      refs: [
        { id: "fork", kind: "fork", label: "Fork point", git_ref: "a", sha: "a" },
        { id: "tip", kind: "tip", label: "Run tip", git_ref: "pdo/run-r1", sha: "b" },
      ],
    };
    expect(sendDisabledReason(null, refs)).toBeNull();
    expect(sendDisabledReason({ run_id: "r1", status: "completed" }, refs)).toBeNull();
    expect(sendDisabledReason({ run_id: "r1", status: "completed" }, null)).toBeNull();
    expect(sendDisabledReason({ run_id: "r1", status: "archived" }, refs)).toContain("pdo/run-r1");
    const gone = { ...refs, refs: refs.refs.map((r) => (r.id === "tip" ? { ...r, sha: null } : r)) };
    expect(sendDisabledReason({ run_id: "r1", status: "completed" }, gone)).toContain("no longer exists");
  });

  it("counts pending (sent, unresolved) comments and formats relative time", () => {
    expect(pendingCount(undefined)).toBe(0);
    expect(pendingCount([sent(), sent({ id: "rc-002", status: "resolved" })])).toBe(1);
    const now = new Date("2026-09-09T10:10:00Z");
    expect(relativeTime("2026-09-09T10:09:50Z", now)).toBe("just now");
    expect(relativeTime("2026-09-09T10:05:00Z", now)).toBe("5 min ago");
    expect(relativeTime("garbage", now)).toBe("");
    expect(relativeTime("2026-09-09T08:00:00Z", now)).toMatch(/\d/);
  });
});

describe("reviewComments — the conversation (#751)", () => {
  const replied = (over: Partial<ReviewComment> = {}) =>
    sent({ id: "rc-002", line: 7, replies: [{ author: "manager", text: "done", at: "2026-09-09T10:05:00Z" }], ...over });
  const proposed = (over: Partial<ReviewComment> = {}) =>
    sent({
      id: "rc-003",
      line: 9,
      proposal_pending: true,
      replies: [{ author: "xuTJYLUa", text: "fixed", at: "2026-09-09T10:06:00Z", proposes_resolution: true }],
      ...over,
    });
  const resolved = (over: Partial<ReviewComment> = {}) =>
    sent({ id: "rc-004", line: 11, status: "resolved", resolved_by: "user", resolved_at: "2026-09-09T10:07:00Z", ...over });

  it("reads a comment's state: open, proposed, resolved — and counts them", () => {
    expect(commentState(sent())).toBe("open");
    expect(commentState(replied())).toBe("open");
    expect(commentState(proposed())).toBe("proposed");
    expect(commentState(resolved())).toBe("resolved");
    expect(stateCounts([sent(), replied(), proposed(), resolved()])).toEqual({ open: 2, proposed: 1, resolved: 1 });
    const entries = mergeEntries([], [sent(), proposed({ path: "a.ts" }), resolved()], PAIR, ["a.ts", "b.ts"]);
    expect(stateCountsByPath(entries).get("a.ts")).toEqual({ open: 0, proposed: 1, resolved: 0 });
    expect(stateCountsByPath(entries).get("b.ts")).toEqual({ open: 1, proposed: 0, resolved: 1 });
  });

  it("sorts the sidebar drafts → proposed → open → resolved, keeping diff order inside a group", () => {
    const drafts = addDraft([], { path: "b.ts", side: "new", line: 20 }, PAIR, "d");
    const entries = mergeEntries(drafts, [resolved(), sent(), proposed(), replied()], PAIR, ["b.ts"]);
    const order = sortForSidebar(entries).map((e) => (e.kind === "draft" ? "draft" : e.comment.id));
    expect(order).toEqual(["draft", "rc-003", "rc-001", "rc-002", "rc-004"]);
  });

  it("names authors by kind and truncates the collapsed summary", () => {
    expect(authorKind("user")).toBe("user");
    expect(authorKind("manager")).toBe("manager");
    expect(authorKind("xuTJYLUa")).toBe("node");
    expect(authorLabel("user")).toBe("you");
    expect(authorLabel("xuTJYLUa")).toBe("xuTJYLUa");
    expect(firstWords("\n  Missing early return on an empty event slice, the loop allocates.\nmore", 44)).toBe(
      "Missing early return on an empty event slice…",
    );
    expect(firstWords("short")).toBe("short");
  });

  it("describes the footer: awaiting → replied → proposed → resolved → reopened / declined", () => {
    expect(footerStatus(sent())).toEqual({ kind: "awaiting" });
    expect(footerStatus(replied())).toEqual({ kind: "replied", author: "manager", at: "2026-09-09T10:05:00Z" });
    expect(footerStatus(proposed())).toEqual({ kind: "proposed", author: "xuTJYLUa", at: "2026-09-09T10:06:00Z" });
    expect(footerStatus(resolved())).toEqual({ kind: "resolved", by: "user", at: "2026-09-09T10:07:00Z" });
    // Reopened after the last reply → reopened; declined proposal → declined.
    expect(footerStatus(replied({ reopened_by: "user", reopened_at: "2026-09-09T10:08:00Z" }))).toEqual({
      kind: "reopened",
      by: "user",
      at: "2026-09-09T10:08:00Z",
    });
    expect(
      footerStatus(replied({ reopened_by: "user", reopened_at: "2026-09-09T10:08:00Z", proposal_declined: true })),
    ).toEqual({ kind: "declined", by: "user", at: "2026-09-09T10:08:00Z" });
    // A reply after the reopen wins.
    expect(
      footerStatus(
        replied({
          reopened_at: "2026-09-09T10:04:00Z",
          reopened_by: "user",
        }),
      ).kind,
    ).toBe("replied");
  });

  it("tracks unread replies per browser and counts the Diff tab badge", () => {
    const store = new Map<string, string>();
    const storage = {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => void store.set(k, v),
      removeItem: (k: string) => void store.delete(k),
    };
    expect(readSeen("r1", storage)).toEqual({});
    const comments = [replied(), proposed(), resolved({ replies: [{ author: "manager", text: "x", at: "t" }] })];
    // Nothing seen: both open comments with replies count; the resolved one does not.
    expect(unreadReplies(comments[0], {})).toBe(1);
    expect(unreadCommentCount(comments, {})).toBe(2);
    // Opening the Review: everything seen.
    const seen = allSeen(comments);
    writeSeen("r1", seen, storage);
    expect(readSeen("r1", storage)).toEqual({ "rc-002": 1, "rc-003": 1, "rc-004": 1 });
    expect(unreadCommentCount(comments, seen)).toBe(0);
    // A second reply lands: one unread again, cleared per card.
    const grown = replied({ replies: [...replied().replies!, { author: "manager", text: "again", at: "t2" }] });
    expect(unreadReplies(grown, seen)).toBe(1);
    expect(unreadCommentCount([grown], seen)).toBe(1);
    const after = markSeen(seen, grown);
    expect(unreadReplies(grown, after)).toBe(0);
    expect(markSeen(after, grown)).toBe(after);
    // Garbage in storage reads as nothing seen.
    storage.setItem("pdo.review.seen.r1", "[1,2]");
    expect(readSeen("r1", storage)).toEqual({});
    writeSeen("r1", {}, storage);
    expect(storage.getItem("pdo.review.seen.r1")).toBeNull();
  });
});

// #752 (ADR-0067 §5): reported / outdated comments, and the toolbar pill's tone.
describe("reviewComments — re-mapped comments and the Review quick access (#752)", () => {
  it("places mapped comments at their reported line, groups outdated ones, and keeps the rest as 'other'", () => {
    const a = sent({ id: "rc-001", line: 212 });
    const b = sent({ id: "rc-002", line: 239, path: "c.ts" });
    const c = sent({ id: "rc-003", line: 7, path: "gone.ts" });
    const d = sent({ id: "rc-004", line: 9, from_ref: "node:x:1:before", to_ref: "node:x:1:after" });
    const mappings = mappingsOf([
      { ...a, outdated: false, mapped_line: 214, moved: true },
      { ...b, outdated: true },
      { ...c, outdated: false, mapped_line: 7 },
      { ...d, outdated: false, mapped_line: 9 },
    ]);
    const entries = sentEntriesForPair([a, b, c, d], PAIR, mappings, ["b.ts", "c.ts"]);
    expect(entries.map((e) => e.kind === "sent" && [e.comment.id, e.anchor.line, e.outdated ?? false, e.movedFrom])).toEqual([
      ["rc-001", 214, false, 212],
      ["rc-002", 239, true, undefined],
      // rc-003: its file is not in the displayed diff → not at home.
      ["rc-004", 9, false, undefined], // written on another pair, but its line is here
    ]);
    expect(homeIds(entries)).toEqual(new Set(["rc-001", "rc-002", "rc-004"]));
    expect(remapSummary(entries)).toBe("1 comment moved · 1 outdated");
    expect(outdatedCountByPath(entries)).toEqual(new Map([["c.ts", 1]]));
    // No mapping yet: exact-pair comments show where written, others wait.
    const plain = sentEntriesForPair([a, d], PAIR, undefined, ["b.ts"]);
    expect(plain).toHaveLength(1);
    expect(plain[0].anchor.line).toBe(212);
    expect(remapSummary(plain)).toBeNull();
    // mergeEntries threads the mapping through and sorts by the reported line.
    const merged = mergeEntries([], [a, b], PAIR, ["b.ts", "c.ts"], mappings);
    expect(merged.map((e) => e.anchor.line)).toEqual([214, 239]);
  });

  it("parses the stored excerpt into numbered lines with the commented one marked", () => {
    expect(parseExcerpt("  1 | a\n> 2 | b\n  3 | ")).toEqual([
      { no: 1, text: "a", marked: false },
      { no: 2, text: "b", marked: true },
      { no: 3, text: "", marked: false },
    ]);
    expect(parseExcerpt(undefined)).toEqual([]);
    expect(parseExcerpt("garbage")).toEqual([]);
  });

  it("colours the toolbar pill: pending → unread (blue) → proposed (amber wins), and titles it", () => {
    const open = sent({ id: "rc-001" });
    const replied = sent({ id: "rc-002", replies: [{ author: "manager", text: "ok", at: "t" }] });
    const proposed = sent({ id: "rc-003", proposal_pending: true, replies: [{ author: "manager", text: "done", at: "t", proposes_resolution: true }] });
    const resolved = sent({ id: "rc-004", status: "resolved" });
    expect(pendingTone([open, resolved], {})).toBe("pending");
    expect(reviewQuickAccessTitle([open, resolved], {})).toBe("Review · 1 pending");
    expect(pendingTone([open, replied], {})).toBe("unread");
    expect(reviewQuickAccessTitle([open, replied], {})).toBe("Review · 2 pending, 1 unread reply");
    expect(pendingTone([open, replied], { "rc-002": 1 })).toBe("pending");
    expect(pendingTone([open, replied, proposed], {})).toBe("proposed");
    expect(reviewQuickAccessTitle([open, replied, proposed], {})).toBe("Review · 3 pending, 2 unread replies, 1 resolution proposed");
    expect(reviewQuickAccessTitle([resolved], {})).toBe("Review");
    expect(reviewQuickAccessTitle(undefined, {})).toBe("Review");
  });
});
