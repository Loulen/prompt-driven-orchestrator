import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import InspectorTabs from "./InspectorTabs";

describe("InspectorTabs", () => {
  it("renders Run and Edit tab buttons", () => {
    render(
      <InspectorTabs activeTab="run" onTabChange={() => {}}>
        <div>content</div>
      </InspectorTabs>,
    );
    expect(screen.getByTestId("inspector-tab-run")).toBeInTheDocument();
    expect(screen.getByTestId("inspector-tab-edit")).toBeInTheDocument();
  });

  it("marks active tab with data-active attribute", () => {
    const { rerender } = render(
      <InspectorTabs activeTab="run" onTabChange={() => {}}>
        <div>content</div>
      </InspectorTabs>,
    );
    expect(screen.getByTestId("inspector-tab-run")).toHaveAttribute(
      "data-active",
      "true",
    );
    expect(screen.getByTestId("inspector-tab-edit")).toHaveAttribute(
      "data-active",
      "false",
    );

    rerender(
      <InspectorTabs activeTab="edit" onTabChange={() => {}}>
        <div>content</div>
      </InspectorTabs>,
    );
    expect(screen.getByTestId("inspector-tab-run")).toHaveAttribute(
      "data-active",
      "false",
    );
    expect(screen.getByTestId("inspector-tab-edit")).toHaveAttribute(
      "data-active",
      "true",
    );
  });

  it("calls onTabChange when a tab is clicked", () => {
    const onChange = vi.fn();
    render(
      <InspectorTabs activeTab="run" onTabChange={onChange}>
        <div>content</div>
      </InspectorTabs>,
    );
    fireEvent.click(screen.getByTestId("inspector-tab-edit"));
    expect(onChange).toHaveBeenCalledWith("edit");

    fireEvent.click(screen.getByTestId("inspector-tab-run"));
    expect(onChange).toHaveBeenCalledWith("run");
  });

  it("renders children", () => {
    render(
      <InspectorTabs activeTab="run" onTabChange={() => {}}>
        <div data-testid="tab-content">hello</div>
      </InspectorTabs>,
    );
    expect(screen.getByTestId("tab-content")).toBeInTheDocument();
  });
});
