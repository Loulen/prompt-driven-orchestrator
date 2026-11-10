import { act, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import RunPresenceStack from "./RunPresenceStack";
import { applyPresenceMessage } from "../store/presences";
import { resetActorsForTests, useActorsStore } from "../../shared/actors/store";
import type { Actor } from "../../shared/generated/Actor";
import type { WsMessage } from "../model/liveStream";

const person = (id: string): Actor => ({ kind: "identified", namespace: "test", id });

function arrive(runId: string, ...actors: Actor[]) {
  act(() => {
    applyPresenceMessage({
      type: "presence_arrived",
      run_id: runId,
      presences: actors.map((actor) => ({ actor, since: "2026-10-28T10:00:00Z" })),
    } as unknown as WsMessage);
  });
}

function knowNames(...pairs: [string, string][]) {
  act(() => {
    useActorsStore.getState().remember(
      pairs.map(([id]) => person(id)),
      pairs.map(([, name]) => ({ name, avatar_url: null })),
    );
  });
}

afterEach(() => {
  arrive("r1");
  resetActorsForTests();
});

describe("RunPresenceStack", () => {
  it("draws nothing when nobody else has the Run open", () => {
    act(() => {
      useActorsStore.getState().setCurrent({ actor: person("me"), display: { name: "Me Myself", avatar_url: null } } as never);
    });
    arrive("r1", person("me"));
    const { container } = render(<RunPresenceStack runId="r1" />);
    expect(container.firstChild).toBeNull();
  });

  it("stacks the others' initials, past three as a count", () => {
    act(() => {
      useActorsStore.getState().setCurrent({ actor: person("me"), display: { name: "Me Myself", avatar_url: null } } as never);
    });
    knowNames(["a", "Ada Lovelace"], ["b", "Bob Marley"], ["c", "Cleo Ray"], ["d", "Dan Brown"]);
    arrive("r1", person("me"), person("a"), person("b"), person("c"), person("d"));
    render(<RunPresenceStack runId="r1" />);
    const stack = screen.getByTestId("run-presence");
    expect(stack.textContent).toContain("AL");
    expect(stack.textContent).toContain("BM");
    expect(stack.textContent).toContain("CR");
    expect(stack.textContent).not.toContain("DB");
    expect(stack.textContent).toContain("+1");
    expect(stack.textContent).not.toContain("MM");
    expect(screen.getByLabelText("Ada Lovelace · Viewing now")).toBeInTheDocument();
    expect(screen.getByLabelText("1 more: Dan Brown")).toHaveAttribute("title", "Dan Brown · Viewing now");
  });
});
