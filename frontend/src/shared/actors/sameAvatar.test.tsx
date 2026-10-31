// S4: one person, one avatar. The same Actor shows the same hue and the same
// initials on every surface that draws a person: the presence on a shared
// draft and on a Run, an Actor's label, a result of the command field, the
// author of a Review comment; the marks of its presence take that hue.
import { act, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { PaletteRow } from "../../app/palette/PaletteRows";
import { ParticipantStack } from "../../pipeline/components/ParticipantAvatar";
import { presenceColor } from "../../pipeline/model/presence";
import { AuthorIcon } from "../../review/components/commentBits";
import RunPresenceStack from "../../run/components/RunPresenceStack";
import type { WsMessage } from "../../run/model/liveStream";
import { applyPresenceMessage } from "../../run/store/presences";
import type { Actor } from "../generated/Actor";
import type { DraftParticipant } from "../generated/DraftParticipant";
import { personColor } from "../ui/kit";
import ActorLabel from "./ActorLabel";
import { resetActorsForTests, useActorsStore } from "./store";

const alice: Actor = { kind: "identified", namespace: "test", id: "alice" };
const bob: Actor = { kind: "identified", namespace: "test", id: "bob" };
const NAMES: Record<string, string> = { alice: "Alice Martin", bob: "Bob Stone" };

function participant(actor: Actor & { kind: "identified" }, connection: string): DraftParticipant {
  return {
    connection,
    actor,
    display: { name: NAMES[actor.id], avatar_url: null },
    can_write: true,
    assistant: false,
    selection: null,
    typing: null,
  };
}

function knowNames() {
  act(() => {
    useActorsStore.getState().remember(
      [alice, bob],
      [alice, bob].map((a) => ({ name: NAMES[(a as { id: string }).id], avatar_url: null })),
    );
  });
}

afterEach(() => {
  act(() => {
    applyPresenceMessage({ type: "presence_arrived", run_id: "r1", presences: [] } as unknown as WsMessage);
  });
  resetActorsForTests();
});

/** The hue and the initials of the avatar named `name` inside `root`. */
function face(root: HTMLElement, name: RegExp) {
  const avatar = within(root).getByRole("img", { name });
  return { hue: avatar.getAttribute("data-hue"), initials: avatar.textContent };
}

describe("one person, one avatar", () => {
  it("draws the same hue and initials on every surface, and paints its marks in that hue", () => {
    knowNames();
    act(() => {
      applyPresenceMessage({
        type: "presence_arrived",
        run_id: "r1",
        presences: [alice, bob].map((actor) => ({ actor, since: "2026-10-28T10:00:00Z" })),
      } as unknown as WsMessage);
    });
    render(
      <>
        <div data-testid="draft">
          <ParticipantStack participants={[participant(alice, "1"), participant(bob, "2")]} />
        </div>
        <div data-testid="run">
          <RunPresenceStack runId="r1" />
        </div>
        <div data-testid="label">
          <ActorLabel actor={alice} />
        </div>
        <div data-testid="palette">
          <PaletteRow
            row={{
              kind: "result",
              key: "people:alice",
              ranges: [],
              item: {
                key: "people:alice",
                extension: { id: "people", name: "People" },
                result: { id: "alice", title: "Alice Martin", link: "#/x", avatar: { name: "Alice Martin", actor: alice } },
              },
            }}
            active={false}
            section={{ key: "ext:people", title: "People", lead: null, rows: [] }}
            extensionIcons={new Map()}
            onPick={() => {}}
            onHover={() => {}}
          />
        </div>
        <div data-testid="review">
          <AuthorIcon author="user" actor={alice} />
        </div>
      </>,
    );

    const surfaces = ["draft", "run", "label", "palette", "review"].map((id) => face(screen.getByTestId(id), /^Alice Martin/));
    for (const seen of surfaces) expect(seen).toEqual(surfaces[0]);
    expect(surfaces[0].initials).toBe("AM");

    // Bob is another person: his hue is his own, the same on both presences.
    const bobDraft = face(screen.getByTestId("draft"), /^Bob Stone/);
    expect(face(screen.getByTestId("run"), /^Bob Stone/)).toEqual(bobDraft);

    // The marks of Alice's presence on the canvas take her avatar's hue.
    expect(presenceColor(participant(alice, "7"))).toBe(personColor({ actor: alice }));
    expect(presenceColor(participant(alice, "7"))).toContain(`--av-h${surfaces[0].hue}`);
  });
});
