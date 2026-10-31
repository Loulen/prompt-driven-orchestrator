import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Actor } from "../generated/Actor";
import ActorLabel from "./ActorLabel";
import { actorKey, requestDisplays, resetActorsForTests, useActorsStore } from "./store";

const alice: Actor = { kind: "identified", namespace: "test", id: "alice" };
const operator: Actor = { kind: "local_operator" };

let fetchMock: ReturnType<typeof vi.fn>;

beforeEach(() => {
  resetActorsForTests();
  fetchMock = vi.fn(async (_url: string, init?: RequestInit) => {
    const { actors } = JSON.parse(String(init?.body)) as { actors: Actor[] };
    const displays = actors.map((a) =>
      a.kind === "identified" ? { name: `${a.id[0].toUpperCase()}${a.id.slice(1)} Martin`, avatar_url: null } : null,
    );
    return new Response(JSON.stringify({ displays }), { status: 200 });
  });
  vi.stubGlobal("fetch", fetchMock);
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("the Actors' display names", () => {
  it("keys an Actor by what identifies it", () => {
    expect(actorKey(alice)).toBe("identified:test/alice");
    expect(actorKey({ kind: "agent", run_id: "r", node_id: "n" })).toBe("agent:r/n");
    expect(actorKey(operator)).toBe("local_operator");
  });

  it("asks once per tick for every Actor not known yet", async () => {
    await Promise.all([requestDisplays([alice]), requestDisplays([alice, operator])]);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(fetchMock.mock.calls[0][0]).toBe("/actors/display");
    expect(JSON.parse(String(fetchMock.mock.calls[0][1]?.body))).toEqual({ actors: [alice, operator] });
    await requestDisplays([alice, operator]);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(useActorsStore.getState().displays[actorKey(alice)]?.name).toBe("Alice Martin");
    expect(useActorsStore.getState().displays[actorKey(operator)]).toBeNull();
  });

  it("an unreachable daemon names nobody", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => Promise.reject(new TypeError("down"))));
    await requestDisplays([alice]);
    expect(useActorsStore.getState().displays[actorKey(alice)]).toBeNull();
  });

  it("a label names the Actor, or falls back to the core's label", async () => {
    render(
      <>
        <p data-testid="named"><ActorLabel actor={alice} /></p>
        <p data-testid="unnamed"><ActorLabel actor={operator} /></p>
      </>,
    );
    await waitFor(() => expect(screen.getByTestId("named").textContent).toBe("AMAlice Martin"));
    expect(screen.getByTestId("unnamed").textContent).toBe("Local operator");
  });
});
