import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import DraftNoticeBanner from "./DraftNoticeBanner";
import { ParticipantAvatar, SelectedByLabel } from "./ParticipantAvatar";
import type { DraftNoticeEntry } from "../store/editState";
import type { DraftParticipant } from "../../shared/generated/DraftParticipant";

const external: DraftNoticeEntry = {
  id: 1,
  notice: {
    kind: "external_edit",
    file: "feature-with-review.yaml",
    version: { number: 8, title: "external edit" },
    fields: ["prompt of reviewer", "edge reviewer → fixer"],
  },
};

const assistant: DraftNoticeEntry = {
  id: 2,
  notice: { kind: "assistant", actor: { kind: "local_operator" }, display: null, fields: ["name of fixer"] },
};

describe("DraftNoticeBanner", () => {
  it("renders nothing without a notice", () => {
    const { container } = render(<DraftNoticeBanner notices={[]} me={null} onSeeVersion={vi.fn()} onDismiss={vi.fn()} />);
    expect(container.firstChild).toBeNull();
  });

  it("says a file changed outside PDO, its version, and what merged; See vN and × act", () => {
    const onSeeVersion = vi.fn();
    const onDismiss = vi.fn();
    render(<DraftNoticeBanner notices={[external]} me={null} onSeeVersion={onSeeVersion} onDismiss={onDismiss} />);
    const notice = screen.getByTestId("draft-notice");
    expect(notice).toHaveTextContent(
      "feature-with-review.yaml changed outside · kept as v8 · external edit · merged: prompt of reviewer and edge reviewer → fixer",
    );
    fireEvent.click(screen.getByTestId("draft-notice-see-version"));
    expect(onSeeVersion).toHaveBeenCalledWith(8);
    fireEvent.click(screen.getByTestId("draft-notice-dismiss"));
    expect(onDismiss).toHaveBeenCalledWith(1);
  });

  it("names whose assistant changed the draft, with no version to see", () => {
    render(
      <DraftNoticeBanner notices={[assistant]} me={{ kind: "local_operator" }} onSeeVersion={vi.fn()} onDismiss={vi.fn()} />,
    );
    expect(screen.getByTestId("draft-notice")).toHaveTextContent("Your library assistant · name of fixer");
    expect(screen.queryByTestId("draft-notice-see-version")).toBeNull();
  });
});

describe("a library assistant among the participants", () => {
  const participant: DraftParticipant = {
    connection: "9",
    actor: { kind: "local_operator" },
    display: { name: "Alice Martin", avatar_url: null },
    can_write: true,
    assistant: true,
    selection: { kind: "node", id: "fixer" },
    typing: null,
  };

  it("shows as a robot named after its Actor, labelled « Library » on the canvas", () => {
    render(<ParticipantAvatar participant={participant} />);
    const avatar = screen.getByTestId("draft-participant");
    expect(avatar).toHaveAttribute("data-assistant", "true");
    expect(avatar).toHaveAttribute("title", "Alice Martin's library assistant");
    render(<SelectedByLabel participant={participant} />);
    expect(screen.getByTestId("selected-by")).toHaveTextContent("Library");
  });
});
