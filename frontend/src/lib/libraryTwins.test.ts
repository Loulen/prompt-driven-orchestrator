import { describe, it, expect } from "vitest";
import { cascadableTwin, isStarred, libraryOnly, libraryTwins } from "./libraryTwins";
import type { LibraryPipelineEntry, LibraryPipelineScope } from "../api";
import type { PipelineListEntry, PipelineScope } from "../types";

function lib(
  id: string,
  name: string,
  scope: LibraryPipelineScope = "user",
): LibraryPipelineEntry {
  return {
    id,
    name,
    scope,
    node_count: 3,
    modified: null,
    yaml: `name: ${name}\n`,
    pipeline: { name, version: "1.0", variables: {}, nodes: [], edges: [] },
    prompts: {},
  };
}

function working(
  id: string,
  name: string,
  scope: PipelineScope = "repo",
): PipelineListEntry {
  return {
    id,
    name,
    scope,
    path: `/repo/.pdo/pipelines/${id}.yaml`,
    node_count: 3,
    modified: null,
    variables: {},
  };
}

describe("libraryTwins", () => {
  it("finds no twin in an empty library", () => {
    expect(libraryTwins(working("fixture-repo-id", "fixture"), [])).toEqual([]);
  });

  it("finds the twin by NAME even when the ids diverge (#227)", () => {
    // The Library copy's id is an independently derived slug: matching on id
    // would find nothing here, which is the whole bug the name key prevents.
    const twin = lib("fixture-lib-slug", "fixture");
    const found = libraryTwins(working("fixture-repo-id", "fixture"), [twin]);
    expect(found).toEqual([twin]);
  });

  it("never joins on id: a same-id, different-name entry is not a twin", () => {
    const sameIdOtherName = lib("fixture", "something-else");
    expect(libraryTwins(working("fixture", "fixture"), [sameIdOtherName])).toEqual([]);
  });

  it("returns both copies of a double-star (same name in repo and user scope)", () => {
    const repoCopy = lib("fixture-repo-copy", "fixture", "repo");
    const userCopy = lib("fixture-user-copy", "fixture", "user");
    expect(
      libraryTwins(working("fixture-repo-id", "fixture"), [
        repoCopy,
        lib("other", "other"),
        userCopy,
      ]),
    ).toEqual([repoCopy, userCopy]);
  });
});

describe("cascadableTwin", () => {
  const twin = lib("fixture-lib-slug", "fixture");

  it("offers nothing when no row is targeted (the pre-open modal state)", () => {
    expect(cascadableTwin(null, [twin])).toBeNull();
  });

  it("offers the unique same-name twin, so the caller deletes it by ITS id", () => {
    const got = cascadableTwin(working("fixture-repo-id", "fixture"), [twin]);
    expect(got).toBe(twin);
    expect(got!.id).toBe("fixture-lib-slug");
  });

  it("offers nothing when the library has no same-name copy", () => {
    expect(cascadableTwin(working("fixture-repo-id", "fixture"), [])).toBeNull();
    expect(
      cascadableTwin(working("fixture-repo-id", "fixture"), [lib("other", "other")]),
    ).toBeNull();
  });

  it("refuses an ambiguous double-star — never guesses which copy to destroy", () => {
    expect(
      cascadableTwin(working("fixture-repo-id", "fixture"), [
        lib("fixture-repo-copy", "fixture", "repo"),
        lib("fixture-user-copy", "fixture", "user"),
      ]),
    ).toBeNull();
  });

  it("refuses a scope:'library' target: the row IS the copy, nothing to cascade to", () => {
    // Same name, exactly one twin — only the scope guard stops the cascade (#216).
    expect(cascadableTwin(working("fixture", "fixture", "library"), [twin])).toBeNull();
  });

  it("offers the cascade for a user-scoped working pipeline too, not just repo", () => {
    expect(cascadableTwin(working("fixture-user-id", "fixture", "user"), [twin])).toBe(twin);
  });
});

describe("libraryOnly", () => {
  it("keeps every entry when /pipelines is empty", () => {
    const entries = [lib("a", "alpha"), lib("b", "beta")];
    expect(libraryOnly(entries, [])).toEqual(entries);
  });

  it("drops the entries whose name already sits in /pipelines", () => {
    const alpha = lib("alpha-lib-slug", "alpha");
    const beta = lib("beta-lib-slug", "beta");
    expect(libraryOnly([alpha, beta], [working("alpha-repo-id", "alpha")])).toEqual([beta]);
  });

  it("drops on name, not id: a divergent-id same-name row still hides the entry", () => {
    // The mirror of the #227 join: matching on id would leave a phantom
    // library-only row next to the working pipeline it belongs to.
    const alpha = lib("alpha-lib-slug", "alpha");
    expect(libraryOnly([alpha], [working("alpha-repo-id", "alpha")])).toEqual([]);
  });

  it("keeps an entry whose id collides with a /pipelines row of another name", () => {
    const entry = lib("shared-id", "alpha");
    expect(libraryOnly([entry], [working("shared-id", "beta")])).toEqual([entry]);
  });

  it("preserves library order", () => {
    const a = lib("a", "alpha");
    const b = lib("b", "beta");
    const c = lib("c", "gamma");
    expect(libraryOnly([a, b, c], [working("beta-id", "beta")])).toEqual([a, c]);
  });
});

describe("isStarred", () => {
  it("is false against an empty library", () => {
    expect(isStarred(working("fixture-repo-id", "fixture"), [])).toBe(false);
  });

  it("is true on a name match with a divergent id", () => {
    expect(
      isStarred(working("fixture-repo-id", "fixture"), [lib("fixture-lib-slug", "fixture")]),
    ).toBe(true);
  });

  it("is false when only the id matches", () => {
    expect(isStarred(working("fixture", "fixture"), [lib("fixture", "other")])).toBe(false);
  });

  it("stays true on a double-star (the star shows even when no cascade is offered)", () => {
    const library = [
      lib("fixture-repo-copy", "fixture", "repo"),
      lib("fixture-user-copy", "fixture", "user"),
    ];
    const row = working("fixture-repo-id", "fixture");
    expect(isStarred(row, library)).toBe(true);
    // …and the cascade is still refused: the two predicates are independent.
    expect(cascadableTwin(row, library)).toBeNull();
  });
});
