import { afterEach, describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import DraftPresence from "./DraftPresence";
import { useEditStore } from "../store/editStore";
import { INITIAL_DRAFT_STATE } from "../store/draftSession";
import type { DraftParticipant } from "../../shared/generated/DraftParticipant";
import type { DraftTabState } from "../store/editState";

function participant(connection: string, name: string): DraftParticipant {
  return {
    connection,
    actor: { kind: "identified", namespace: "test", id: name },
    display: { name, avatar_url: null },
    color: Number(connection),
    can_write: true,
    assistant: false,
    selection: null,
    typing: null,
  };
}

function show(draft: Partial<DraftTabState>) {
  useEditStore.setState({
    activeTabId: "flow",
    openTabs: [
      {
        id: "flow",
        scope: "instance",
        pipeline: { name: "flow", version: "1", variables: {}, nodes: [], edges: [] },
        prompts: {},
        diagnostics: [],
        dirty: false,
        externalDirty: false,
        draft: { ...INITIAL_DRAFT_STATE, synced: true, canWrite: true, connection: "1", ...draft },
      },
    ],
  });
}

afterEach(() => useEditStore.setState({ openTabs: [], activeTabId: null }));

describe("DraftPresence", () => {
  it("shows the others, not me; the changes are on the line under the tabs", () => {
    show({ participants: [participant("1", "Me"), participant("2", "Alice Martin")], changes: 3 });
    render(<DraftPresence />);
    const avatars = screen.getAllByTestId("draft-participant");
    expect(avatars).toHaveLength(1);
    expect(avatars[0]).toHaveTextContent("AM");
    expect(avatars[0]).toHaveAttribute("title", "Alice Martin");
    expect(screen.queryByTestId("draft-changes")).toBeNull();
  });

  it("says when the tab only reads or is offline, and draws nothing alone", () => {
    show({ canWrite: false, changes: 0 });
    const { unmount } = render(<DraftPresence />);
    expect(screen.getByTestId("draft-read-only")).toHaveTextContent("read only");
    unmount();
    show({ status: "offline" });
    const offline = render(<DraftPresence />);
    expect(screen.getByTestId("draft-offline")).toHaveTextContent("offline");
    offline.unmount();
    show({ participants: [participant("1", "Me")] });
    const { container } = render(<DraftPresence />);
    expect(container).toBeEmptyDOMElement();
  });
});
