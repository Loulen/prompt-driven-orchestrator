import { act, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ExtensionEntry } from "../../../shared/generated/ExtensionEntry";
import type { UiContribution } from "../../../shared/generated/UiContribution";
import type { RunListEntry } from "../../../shared/generated/RunListEntry";
import type { Presence } from "../../../shared/generated/Presence";
import type { DecorationProps } from "../../../shared/extensions/contract";
import type { RunPeopleContext } from "../../../shared/extensions/runPeople";
import { hostReactMajor } from "../../../shared/extensions/shared";
import { resetExtensions, setBundleLoader } from "../../../shared/extensions/store";
import { applyPresenceMessage, reloadPresences } from "../../store/presences";
import RunRowPeople from "./RunRowPeople";

const alice = { kind: "identified", namespace: "test", id: "alice" } as const;
const bob = { kind: "identified", namespace: "test", id: "bob" } as const;
const run = { run_id: "r1", name: "Nightly", attributes: {}, created_by: alice } as unknown as RunListEntry;

function served(contributions: UiContribution[], presences: unknown = { runs: [] }) {
  const extensions: ExtensionEntry[] = [
    {
      id: "people",
      name: "People",
      version: "1.0.0",
      ui: {
        bundle_url: "/ext/people/ui.js",
        react_major: hostReactMajor(),
        ui_module_major: null,
        contributions,
        commands: [],
        locations: [],
      },
    },
  ];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) => {
      const body = url === "/ext" ? extensions : url === "/runs/presences" ? presences : null;
      return body === null
        ? new Response("not found", { status: 404 })
        : new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
    }),
  );
}

const PEOPLE = [
  { location: "run/item/people", id: "marks", label: "People", type: "decoration", component: "Marks" },
] as UiContribution[];

function presence(actor: typeof alice | typeof bob): Presence {
  return { actor, since: "2026-10-09T10:00:00.000Z" };
}

beforeEach(() => {
  resetExtensions();
  setBundleLoader(async () => ({
    Marks: ({ context }: DecorationProps<RunPeopleContext>) => (
      <span data-testid="marks">
        {context.entity.id} by {context.entity.created_by?.kind}:{" "}
        {context.presences.map((p) => (p.actor.kind === "identified" ? p.actor.id : p.actor.kind)).join(",")}
      </span>
    ),
  }));
  applyPresenceMessage({ type: "presence_left", run_id: "r1", presences: [] });
});

afterEach(() => {
  vi.unstubAllGlobals();
  resetExtensions();
});

describe("the people of a Run's row (run/item/people)", () => {
  it("draws no person without a contribution: the core shows nobody", async () => {
    served([]);
    const { container } = render(
      <div data-testid="row">
        <RunRowPeople run={run} />
      </div>,
    );
    await act(async () => {});
    expect(screen.getByTestId("row")).toBeEmptyDOMElement();
    expect(container.textContent).toBe("");
  });

  it("hands the contribution the Run and its Presences, live", async () => {
    served(PEOPLE);
    render(<RunRowPeople run={run} />);
    await waitFor(() => expect(screen.getByTestId("marks")).toHaveTextContent("r1 by identified:"));
    act(() => {
      applyPresenceMessage({
        type: "presence_arrived",
        run_id: "r1",
        actor: bob,
        presences: [presence(alice), presence(bob)],
      });
    });
    expect(screen.getByTestId("marks")).toHaveTextContent("r1 by identified: alice,bob");
    act(() => {
      applyPresenceMessage({ type: "presence_left", run_id: "r1", actor: alice, presences: [presence(bob)] });
    });
    expect(screen.getByTestId("marks")).toHaveTextContent("r1 by identified: bob");
  });

  it("reads every open Run once, at a resync of the stream", async () => {
    served(PEOPLE, { runs: [{ run_id: "r1", presences: [presence(alice)] }] });
    render(<RunRowPeople run={run} />);
    await act(async () => {
      await reloadPresences();
    });
    await waitFor(() => expect(screen.getByTestId("marks")).toHaveTextContent("alice"));
  });

  it("keeps an arrival announced while the reload was in flight", async () => {
    served(PEOPLE, { runs: [{ run_id: "r1", presences: [presence(alice)] }] });
    render(<RunRowPeople run={run} />);
    await act(async () => {
      const reload = reloadPresences();
      applyPresenceMessage({
        type: "presence_arrived",
        run_id: "r1",
        actor: bob,
        presences: [presence(alice), presence(bob)],
      });
      await reload;
    });
    await waitFor(() => expect(screen.getByTestId("marks")).toHaveTextContent("alice,bob"));
  });

  it("ignores the stream's other messages", () => {
    expect(applyPresenceMessage({ type: "pipeline_changed" })).toBe(false);
    expect(applyPresenceMessage({ type: "presence_arrived", run_id: "r9", presences: [] })).toBe(true);
  });
});
