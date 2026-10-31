/**
 * S3 — the extensions in the command field (API 0.18): the global Actions
 * of `palette` and those acting on the selection, refused ones absent; the
 * Sources de recherche asked on their own, capped, given up on after a delay,
 * a failing one kept to itself, an older query's answer dropped.
 */

import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ExtensionEntry } from "../generated/ExtensionEntry";
import type { UiCommand } from "../generated/UiCommand";
import type { UiContribution } from "../generated/UiContribution";
import type { EntityContext, PaletteContext, SearchResult } from "./contract";
import {
  MAX_RESULTS,
  PENDING_AFTER,
  readResults,
  SEARCH_DEBOUNCE,
  SEARCH_TIMEOUT,
  usePaletteActions,
  useSearch,
  useSearchSources,
} from "./palette";
import { hostReactMajor } from "./shared";
import { resetExtensions, setBundleLoader, useExtensionsStore } from "./store";

type Decision = { decision: "allow" } | { decision: "deny"; reason: string };

let decide: (operation: { resource: string; verb: unknown; target: string | null }) => Decision = () => ({
  decision: "allow",
});
let authorizeFails = false;
const asked: { resource: string; verb: unknown; target: string | null }[] = [];

function extension(id: string, name: string, contributions: UiContribution[], commands: UiCommand[] = []): ExtensionEntry {
  return {
    id,
    name,
    version: "1.0.0",
    ui: {
      bundle_url: `/ext/${id}/ui.js`,
      react_major: hostReactMajor(),
      ui_module_major: null,
      contributions,
      commands,
      locations: [],
    },
  };
}

const action = (location: string, id: string): UiContribution =>
  ({ location, id, label: `Do ${id}`, type: "action", command: `cmd.${id}` }) as UiContribution;
const command = (id: string, operation?: UiCommand["operation"]): UiCommand => ({
  id: `cmd.${id}`,
  title: `Do ${id}`,
  export: "noop",
  ...(operation ? { operation } : {}),
});
const source = (id: string, search: string): UiContribution =>
  ({ location: "palette", id, label: id, type: "search", search }) as UiContribution;

const hit = (id: string, title = `Result ${id}`): SearchResult => ({ id, title, link: `#/ext/a/topbar/end/${id}` });

const bundle: Record<string, unknown> = {
  noop: () => {},
  fast: (query: string) => [hit("1", `fast ${query}`)],
  slow: (query: string) => new Promise((resolve) => setTimeout(() => resolve([hit("2", `slow ${query}`)]), 2000)),
  failing: () => Promise.reject(new Error("index unavailable")),
  hanging: () => new Promise(() => {}),
  many: () => Array.from({ length: 200 }, (_, i) => hit(String(i))),
};

async function load(extensions: ExtensionEntry[]) {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      if (url === "/ext") return Response.json(extensions);
      if (url === "/authorize") {
        if (authorizeFails) return new Response("boom", { status: 500 });
        const { operations } = JSON.parse(String(init?.body));
        asked.push(...operations);
        return Response.json({ decisions: operations.map(decide) });
      }
      return new Response("not found", { status: 404 });
    }),
  );
  setBundleLoader(async () => bundle);
  await useExtensionsStore.getState().load();
}

beforeEach(() => {
  resetExtensions();
  decide = () => ({ decision: "allow" });
  authorizeFails = false;
  asked.length = 0;
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  resetExtensions();
});

const RUN: EntityContext = { kind: "run", id: "r1", attributes: {}, name: "Add search" };

describe("the Actions of the command field", () => {
  it("gathers each extension's global Actions, in registration order, and the selection's", async () => {
    await load([
      extension("a", "Alpha", [action("palette", "a1"), action("run/item/menu", "a-run")], [command("a1"), command("a-run")]),
      extension("b", "Beta", [action("palette", "b1"), action("palette", "b2"), action("topbar/end", "b-bar")], [
        command("b1"),
        command("b2"),
        command("b-bar"),
      ]),
    ]);
    const { result, rerender } = renderHook(({ selection }) => usePaletteActions(selection), {
      initialProps: { selection: null as EntityContext | null },
    });
    await waitFor(() => expect(result.current.global).toHaveLength(3));
    expect(result.current.global.map((a) => a.element.key)).toEqual(["a:a1", "b:b1", "b:b2"]);
    expect(result.current.contextual).toEqual([]);
    expect(result.current.context.location).toBe("palette");
    expect(result.current.context.selected).toBeNull();

    // A selected Run: the Actions of its row menu, handed the Run as `entity`.
    rerender({ selection: RUN });
    await waitFor(() => expect(result.current.contextual).toHaveLength(1));
    const [contextual] = result.current.contextual;
    expect(contextual.element.key).toBe("a:a-run");
    expect(contextual.entity).toBe(RUN);
    expect((contextual.context as unknown as { entity: EntityContext }).entity).toBe(RUN);
    expect(contextual.context.location).toBe("run/item/menu");
    expect(result.current.context.selected).toBe(RUN);
  });

  it("leaves out an Action whose Operation is refused, asked on the entity it acts on", async () => {
    decide = (op) => (op.verb === "delete" ? { decision: "deny", reason: "no" } : { decision: "allow" });
    await load([
      extension(
        "a",
        "Alpha",
        [action("palette", "create"), action("palette", "delete"), action("palette", "free"), action("run/item/menu", "archive")],
        [
          command("create", { resource: "a/note", verb: "create" }),
          command("delete", { resource: "a/note", verb: "delete" }),
          command("free"),
          command("archive", { resource: "run", verb: { action: "archive" } }),
        ],
      ),
    ]);
    const { result } = renderHook(() => usePaletteActions(RUN));
    await waitFor(() => expect(result.current.global.map((a) => a.element.key)).toEqual(["a:create", "a:free"]));
    expect(result.current.contextual.map((a) => a.element.key)).toEqual(["a:archive"]);
    expect(asked).toContainEqual({ resource: "a/note", verb: "create", target: null });
    expect(asked).toContainEqual({ resource: "run", verb: { action: "archive" }, target: "r1" });
  });

  it("hides a guarded Action until it is answered, and shows it when the question fails", async () => {
    authorizeFails = true;
    await load([extension("a", "Alpha", [action("palette", "create")], [command("create", { resource: "a/note", verb: "create" })])]);
    const { result } = renderHook(() => usePaletteActions(null));
    expect(result.current.global).toEqual([]);
    await waitFor(() => expect(result.current.global.map((a) => a.element.key)).toEqual(["a:create"]));
  });
});

describe("the Sources de recherche", () => {
  const context = { location: "palette", run: null, project: null, selected: null } as PaletteContext;

  async function sources(...searches: string[]) {
    await load(searches.map((search) => extension(search, search, [source(search, search)])));
    const found = renderHook(() => useSearchSources(context));
    await waitFor(() => expect(found.result.current).toHaveLength(searches.length));
    return found.result.current;
  }

  it("keeps the first results of a source, and only well-formed ones", () => {
    const many = Array.from({ length: 200 }, (_, i) => hit(String(i)));
    const read = readResults([...many, { id: "x" }, null, "nope"]);
    expect(read.total).toBe(200);
    expect(read.results).toHaveLength(MAX_RESULTS);
    expect(read.results[0].id).toBe("0");
    expect(readResults("not a list")).toEqual({ results: [], total: 0 });
    // One row per id.
    expect(readResults([hit("1", "a"), hit("1", "b"), hit("2")]).results.map((r) => r.title)).toEqual(["a", "Result 2"]);
  });

  it("keeps an avatar it can draw: a name, an Actor, an image; drops the rest of it", () => {
    const alice = { kind: "identified", namespace: "test", id: "alice" };
    const [named, odd, nameless] = readResults([
      { ...hit("1"), avatar: { name: "Alice Martin", actor: alice, avatar_url: "/a.svg" } },
      { ...hit("2"), avatar: { name: "Bob", actor: "bob", avatar_url: 3 } },
      { ...hit("3"), avatar: { actor: alice } },
    ]).results;
    expect(named.avatar).toEqual({ name: "Alice Martin", actor: alice, avatar_url: "/a.svg" });
    expect(odd.avatar).toEqual({ name: "Bob", actor: null, avatar_url: null });
    expect(nameless.avatar).toBeNull();
    expect(nameless.title).toBe("Result 3");
  });

  it("asks each source on its own: a fast one shows at once, a slow one later, a failing one in its group", async () => {
    const list = await sources("fast", "slow", "failing", "hanging", "many");
    vi.useFakeTimers();
    const { result } = renderHook(() => useSearch(list, "sea", context));
    expect(result.current.states).toEqual({});

    await act(() => vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE + 1));
    expect(result.current.states["fast:fast"]).toMatchObject({ status: "ok", results: [{ title: "fast sea" }] });
    expect(result.current.states["failing:failing"]).toMatchObject({ status: "error", error: "index unavailable" });
    expect(result.current.states["many:many"]).toMatchObject({ status: "ok", total: 200 });
    expect(result.current.states["many:many"].results).toHaveLength(MAX_RESULTS);
    // Not late yet: nothing to say about the slow ones.
    expect(result.current.states["slow:slow"].status).toBe("waiting");

    await act(() => vi.advanceTimersByTimeAsync(PENDING_AFTER));
    expect(result.current.states["slow:slow"].status).toBe("pending");
    expect(result.current.states["hanging:hanging"].status).toBe("pending");
    expect(result.current.states["fast:fast"].status).toBe("ok");

    await act(() => vi.advanceTimersByTimeAsync(2000));
    expect(result.current.states["slow:slow"]).toMatchObject({ status: "ok", results: [{ title: "slow sea" }] });
    expect(result.current.states["hanging:hanging"].status).toBe("pending");

    await act(() => vi.advanceTimersByTimeAsync(SEARCH_TIMEOUT));
    expect(result.current.states["hanging:hanging"]).toMatchObject({ status: "error", error: "Timed out" });
    expect(result.current.states["fast:fast"].status).toBe("ok");
  });

  it("drops the answer to an older query, and asks nothing for an empty one", async () => {
    const list = await sources("slow");
    const slow = vi.spyOn(list[0].search!, "search");
    vi.useFakeTimers();
    const { result, rerender } = renderHook(({ query }) => useSearch(list, query, context), {
      initialProps: { query: "a" },
    });
    await act(() => vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE + 1));
    rerender({ query: "ab" });
    await act(() => vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE + 1));
    // The first answer is due now: it is not this query's.
    await act(() => vi.advanceTimersByTimeAsync(2000 - SEARCH_DEBOUNCE));
    expect(result.current.states["slow:slow"]?.status).not.toBe("ok");
    await act(() => vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE + 1));
    expect(result.current.states["slow:slow"]).toMatchObject({ status: "ok", results: [{ title: "slow ab" }] });
    expect(slow.mock.calls.map(([query]) => query)).toEqual(["a", "ab"]);
    // The first request was aborted.
    expect((slow.mock.calls[0][2] as AbortSignal).aborted).toBe(true);

    rerender({ query: "  " });
    await act(() => vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE + 1));
    expect(result.current.states).toEqual({});
    expect(slow).toHaveBeenCalledTimes(2);
  });

  it("asks a failed source again on Retry", async () => {
    let fail = true;
    bundle.flaky = () => (fail ? Promise.reject(new Error("down")) : [hit("9", "back")]);
    const list = await sources("flaky");
    vi.useFakeTimers();
    const { result } = renderHook(() => useSearch(list, "x", context));
    await act(() => vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE + 1));
    expect(result.current.states["flaky:flaky"]).toMatchObject({ status: "error", error: "down" });
    fail = false;
    await act(async () => {
      result.current.retry("flaky:flaky");
      await vi.advanceTimersByTimeAsync(1);
    });
    expect(result.current.states["flaky:flaky"]).toMatchObject({ status: "ok", results: [{ title: "back" }] });
  });
});
