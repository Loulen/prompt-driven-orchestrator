import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import TriggerRow from "./TriggerRow";
import type { Trigger } from "../../../shared/generated/Trigger";

function trigger(overrides: Partial<Trigger> = {}): Trigger {
  return {
    id: "trg-1",
    name: "Weekly dependency bump",
    pipeline_id: "bump",
    pipeline_name: "Bump",
    target_repo: "/repos/foo",
    input_template: "bump",
    variables: {},
    cron: "0 7 * * 1",
    overlap_policy: "skip",
    auto_name: true,
    enabled: true,
    ...overrides,
  };
}

function renderRow(t: Trigger, onToggle = vi.fn()) {
  render(
    <TriggerRow
      trigger={t}
      isSelected={false}
      rowSelected={false}
      onSelect={() => {}}
      onSelectControl={() => {}}
      onDelete={() => {}}
      onToggle={onToggle}
    />,
  );
  return onToggle;
}

describe("TriggerRow — suspended by an extension (API 0.8)", () => {
  const suspension = { extension: "test", reason: "alice is on leave" };

  it("shows the switch off and an amber « suspended » pill naming the reason", () => {
    renderRow(trigger({ suspension }));
    const toggle = screen.getByTestId("trigger-toggle");
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(toggle.getAttribute("title")).toContain("alice is on leave");
    const pill = screen.getByTestId("trigger-suspended");
    expect(pill).toHaveTextContent("suspended");
    expect(pill).toHaveAccessibleName("Suspended: alice is on leave");
    // A suspended Trigger is not also labelled « disabled ».
    expect(screen.queryByText("disabled")).not.toBeInTheDocument();
  });

  it("shows the reason in the pill's tooltip, with the extension", async () => {
    renderRow(trigger({ suspension }));
    fireEvent.focus(screen.getByTestId("trigger-suspended"));
    const tip = await screen.findAllByTestId("tooltip-content");
    expect(tip[0]).toHaveTextContent("alice is on leave");
    expect(tip[0]).toHaveTextContent("Suspended by the extension test.");
  });

  it("keeps the toggle working: it switches the stored bit", () => {
    const onToggle = renderRow(trigger({ suspension }));
    fireEvent.click(screen.getByTestId("trigger-toggle"));
    expect(onToggle).toHaveBeenCalledOnce();
  });

  it("puts the pill after the name, on the name's line, both ending in an ellipsis", () => {
    renderRow(trigger({ suspension }));
    const name = screen.getByTestId("trigger-name");
    expect(name).toHaveClass("lp-name");
    expect(name.parentElement).toContainElement(screen.getByTestId("trigger-suspended"));
    expect(screen.getByTestId("trigger-suspended")).toHaveClass("lp-suspended");
  });

  it("says the next fire is off while suspended", () => {
    renderRow(trigger({ suspension, next_fire_at: "2099-01-01T00:00:00Z" }));
    expect(screen.getByTestId("trigger-schedule")).toHaveTextContent("—");
    expect(screen.getByTestId("trigger-schedule").getAttribute("title")).toContain("alice is on leave");
  });

  it("is a plain enabled row without a suspension", () => {
    renderRow(trigger());
    expect(screen.getByTestId("trigger-toggle")).toHaveAttribute("aria-checked", "true");
    expect(screen.queryByTestId("trigger-suspended")).not.toBeInTheDocument();
  });
});
