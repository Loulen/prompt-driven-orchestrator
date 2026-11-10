/**
 * CORE-63: a sent review comment shows its author by the name an extension
 * gives its Actor; a comment without Actor shows as before.
 */

import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Actor } from "../../shared/generated/Actor";
import type { ReviewComment } from "../../shared/generated/ReviewComment";
import { resetActorsForTests } from "../../shared/actors/store";
import { resetExtensions } from "../../shared/extensions/store";
import { authorLabel, footerStatus } from "../model/commentStatus";
import CommentCard from "./CommentCard";

const alice: Actor = { kind: "identified", namespace: "test", id: "alice" };
const bob: Actor = { kind: "identified", namespace: "test", id: "bob" };

function comment(over: Partial<ReviewComment> = {}): ReviewComment {
  return {
    id: "rc-001",
    path: "src/a.rs",
    side: "new",
    line: 3,
    from_ref: "fork",
    to_ref: "tip",
    text: "Why?",
    excerpt: "",
    author: "user",
    sent_at: "2026-10-09T10:00:00.000Z",
    status: "sent",
    ...over,
  } as ReviewComment;
}

function card(c: ReviewComment) {
  render(
    <CommentCard
      entry={{ kind: "sent", anchor: { path: c.path, side: c.side, line: c.line }, comment: c }}
      sending={false}
      startingManager={false}
      pairLabel="Fork point → Run tip"
      sendDisabledReason={null}
      onEdit={vi.fn()}
      onDelete={vi.fn()}
      onSend={vi.fn()}
    />,
  );
}

beforeEach(() => {
  resetActorsForTests();
  resetExtensions();
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      if (url === "/actors/display") {
        const { actors } = JSON.parse(String(init?.body)) as { actors: Actor[] };
        const displays = actors.map((a) =>
          a.kind === "identified" ? { name: `${a.id[0].toUpperCase()}${a.id.slice(1)} Martin`, avatar_url: null } : null,
        );
        return new Response(JSON.stringify({ displays }), { status: 200 });
      }
      return new Response("[]", { status: 200, headers: { "Content-Type": "application/json" } });
    }),
  );
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("the author of a sent review comment", () => {
  it("shows the name an extension gives its Actor, not « you »", async () => {
    card(comment({ actor: alice }));
    expect((await screen.findByTestId("review-author-name")).textContent).toBe("Alice Martin");
    expect(screen.getByTestId("review-author").getAttribute("title")).toBe("Alice Martin");
  });

  it("shows an older comment without Actor as before", async () => {
    card(comment());
    await new Promise((r) => setTimeout(r, 20));
    expect(screen.queryByTestId("review-author-name")).toBeNull();
    expect(screen.getByTestId("review-author").getAttribute("title")).toBe("you");
  });

  it("shows an unnamed Actor as before", async () => {
    card(comment({ actor: { kind: "local_operator" } }));
    await new Promise((r) => setTimeout(r, 20));
    expect(screen.queryByTestId("review-author-name")).toBeNull();
  });

  it("names the person an agent's reply acts for, and who resolved", async () => {
    card(
      comment({
        actor: alice,
        status: "resolved",
        resolved_by: "user",
        resolved_by_actor: bob,
        resolved_at: "2026-10-09T10:05:00.000Z",
        replies: [{ author: "manager", actor: alice, text: "Done.", at: "2026-10-09T10:01:00.000Z" }],
      }),
    );
    screen.getByTestId("review-comment-header").click();
    expect((await screen.findByTestId("review-reply-actor")).textContent).toBe("Alice Martin (agent)");
    await waitFor(() =>
      expect(screen.getByTestId("review-comment-resolved-by").getAttribute("title")).toBe("Resolved by Bob Martin"),
    );
  });
});

describe("the anchor of a folded thread", () => {
  it("keeps its line number out of the part that gives way", () => {
    const file = "a-file-name-long-enough-to-be-cut-in-a-half-width-split-column.rs";
    card(comment({ path: `src/deep/${file}`, status: "resolved", resolved_at: "2026-10-09T10:05:00.000Z" }));
    expect(screen.getByTestId("review-comment-header")).toHaveAttribute("aria-expanded", "false");
    const anchor = screen.getByTestId("review-comment-anchor");
    expect(anchor).toHaveAttribute("title", `${file}:R3`);
    // The file name ends in an ellipsis; the line number never shrinks with it.
    expect(screen.getByTestId("review-comment-anchor-file")).toHaveTextContent(file);
    expect(screen.getByTestId("review-comment-anchor-line")).toHaveTextContent(":R3");
  });
});

describe("the label of an author", () => {
  it("reads the name of a person, and the person an agent acts for", () => {
    expect(authorLabel("user")).toBe("you");
    expect(authorLabel("user", "Alice")).toBe("Alice");
    expect(authorLabel("manager", "Alice")).toBe("Alice (agent) · manager");
    expect(authorLabel("xuTJYLUa", "Alice")).toBe("Alice (agent) · xuTJYLUa");
    expect(authorLabel("xuTJYLUa")).toBe("xuTJYLUa");
  });

  it("the footer carries the Actor who acted last", () => {
    const c = comment({ reopened_by: "user", reopened_by_actor: bob, reopened_at: "2026-10-09T10:09:00.000Z" });
    expect(footerStatus(c)).toMatchObject({ kind: "reopened", actor: bob });
  });
});
