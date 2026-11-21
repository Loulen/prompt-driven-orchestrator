import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import type { SupportStatusResponse } from "../../../shared/generated/SupportStatusResponse";
import SupportAgentEntry from "./SupportAgentEntry";
import { useSupportAgent } from "../../store/supportAgentStore";

const closed: SupportStatusResponse = {
  session: "pdo-support-operator",
  alive: false,
  agent: null,
  last_choice: null,
  last_input_at: null,
  last_output_at: null,
  still_there: null,
};
const live: SupportStatusResponse = {
  ...closed,
  alive: true,
  agent: { choice: { mode: "profile", profile_id: "deep" }, label: "Deep", harness: "claude", model: "opus", effort: null, opened_at: "2027-01-15T08:00:00.000Z" },
  last_output_at: "2027-01-15T08:00:05.000Z",
};

beforeEach(() => {
  useSupportAgent.setState({ status: null, dialog: null, expanded: false, pending: false, stillThere: null, seenAnswer: null, collapsedAt: 0 });
});

describe("the support agent's entry in the status bar", () => {
  it("without an agent: « ? », which opens the window", () => {
    useSupportAgent.setState({ status: closed });
    render(<SupportAgentEntry />);
    const entry = screen.getByTestId("support-agent-entry");
    expect(entry).toHaveAttribute("aria-label", "Ask the support agent");
    expect(entry).toHaveAttribute("data-state", "none");
    expect(entry.querySelector("svg.lucide-circle-question-mark, svg.lucide-circle-help")).not.toBeNull();
    fireEvent.click(entry);
    expect(useSupportAgent.getState().dialog).toEqual({ question: "" });
    expect(useSupportAgent.getState().expanded).toBe(false);
  });

  it("with an agent: its harness's logo, which brings its terminal back", () => {
    useSupportAgent.setState({ status: live });
    render(<SupportAgentEntry />);
    const entry = screen.getByTestId("support-agent-entry");
    expect(entry).toHaveAttribute("aria-label", "Support agent");
    expect(entry.querySelector('[data-harness-logo="claude"]')).not.toBeNull();
    expect(screen.queryByTestId("support-agent-unread")).toBeNull();
    fireEvent.click(entry);
    expect(useSupportAgent.getState().expanded).toBe(true);
    expect(useSupportAgent.getState().dialog).toBeNull();
  });

  it("an answer since the minimize: an amber dot, and the tooltip says it", () => {
    useSupportAgent.setState({ status: live, seenAnswer: "2027-01-15T08:00:01.000Z" });
    render(<SupportAgentEntry />);
    expect(screen.getByTestId("support-agent-unread")).toBeInTheDocument();
    expect(screen.getByTestId("support-agent-entry")).toHaveAttribute("aria-label", "Support agent: new answer");
    act(() => useSupportAgent.getState().expand());
    expect(screen.queryByTestId("support-agent-unread")).toBeNull();
  });
});
