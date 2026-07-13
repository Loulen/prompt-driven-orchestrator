import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import ConfirmCloseTabsModal from "./ConfirmCloseTabsModal";
import type { OpenPipeline } from "../stores/editStore";

function tab(id: string, over: Partial<OpenPipeline> = {}): OpenPipeline {
  return {
    id,
    scope: "repo",
    pipeline: { name: id, version: "1.0", variables: {}, nodes: [], edges: [] },
    prompts: {},
    diagnostics: [],
    dirty: false,
    externalDirty: false,
    ...over,
  };
}

describe("ConfirmCloseTabsModal (#342)", () => {
  it("renders nothing when closed", () => {
    const { container } = render(
      <ConfirmCloseTabsModal open={false} tabs={[]} onCancel={() => {}} onConfirm={() => {}} />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("names only the unsaved tabs (ignores clean and externalDirty ones)", () => {
    render(
      <ConfirmCloseTabsModal
        open
        tabs={[tab("clean"), tab("edited", { dirty: true }), tab("flash", { externalDirty: true })]}
        onCancel={() => {}}
        onConfirm={() => {}}
      />,
    );
    expect(screen.getByText("edited.yaml")).toBeInTheDocument();
    expect(screen.queryByText("clean.yaml")).not.toBeInTheDocument();
    expect(screen.queryByText("flash.yaml")).not.toBeInTheDocument();
  });

  it("counts a conflict / saveError tab as unsaved", () => {
    render(
      <ConfirmCloseTabsModal
        open
        tabs={[
          tab("conf", { dirty: true, conflict: { pipeline: tab("x").pipeline, prompts: {}, diagnostics: [] } }),
          tab("err", { dirty: true, saveError: { message: "boom" } }),
        ]}
        onCancel={() => {}}
        onConfirm={() => {}}
      />,
    );
    expect(screen.getByText("conf.yaml")).toBeInTheDocument();
    expect(screen.getByText("err.yaml")).toBeInTheDocument();
  });

  it("fires onConfirm / onCancel from the buttons", () => {
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    render(
      <ConfirmCloseTabsModal open tabs={[tab("a", { dirty: true })]} onCancel={onCancel} onConfirm={onConfirm} />,
    );
    fireEvent.click(screen.getByTestId("close-tabs-confirm"));
    expect(onConfirm).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByTestId("close-tabs-cancel"));
    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  it("cancels on a backdrop click", () => {
    const onCancel = vi.fn();
    render(<ConfirmCloseTabsModal open tabs={[tab("a", { dirty: true })]} onCancel={onCancel} onConfirm={() => {}} />);
    fireEvent.click(screen.getByTestId("close-tabs-backdrop"));
    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  it("cancels on Escape and does NOT confirm on Enter (destructive-action footgun)", () => {
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    render(<ConfirmCloseTabsModal open tabs={[tab("a", { dirty: true })]} onCancel={onCancel} onConfirm={onConfirm} />);
    fireEvent.keyDown(document, { key: "Enter" });
    expect(onConfirm).not.toHaveBeenCalled();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onCancel).toHaveBeenCalledTimes(1);
  });
});
