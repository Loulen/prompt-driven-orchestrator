import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { ParticipantStack } from "./ParticipantAvatar";
import type { DraftParticipant } from "../../shared/generated/DraftParticipant";

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

describe("ParticipantStack", () => {
  it("shows a person in two sessions once", () => {
    render(<ParticipantStack participants={[participant("1", "Alice"), participant("2", "Alice"), participant("3", "Bob")]} />);
    expect(screen.getByTestId("draft-participants").children).toHaveLength(2);
  });
});
