import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import ConfirmCloseTabsModal from "./ConfirmCloseTabsModal";
import type { OpenPipeline } from "../store/editState";
import { INITIAL_DRAFT_STATE } from "../store/draftSession";

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

  it("counts a saveError tab as unsaved", () => {
    render(
      <ConfirmCloseTabsModal
        open
        tabs={[
          tab("err", { dirty: true, saveError: { message: "boom" } }),
        ]}
        onCancel={() => {}}
        onConfirm={() => {}}
      />,
    );
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
    // The backdrop of a modal <dialog> is the dialog itself.
    fireEvent.click(screen.getByTestId("close-tabs-backdrop"));
    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  it("cancels on Escape and does NOT confirm on Enter (destructive-action footgun)", () => {
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    render(<ConfirmCloseTabsModal open tabs={[tab("a", { dirty: true })]} onCancel={onCancel} onConfirm={onConfirm} />);
    const dialog = screen.getByTestId("close-tabs-backdrop");
    fireEvent.keyDown(dialog, { key: "Enter" });
    fireEvent.keyDown(dialog, { key: "Enter", ctrlKey: true });
    expect(onConfirm).not.toHaveBeenCalled();
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(onCancel).toHaveBeenCalledTimes(1);
  });
});

describe("ConfirmCloseTabsModal — a shared draft keeps its changes (ADR-0088)", () => {
  const draft = {
    ...INITIAL_DRAFT_STATE,
    status: "offline" as const,
    synced: true,
    canWrite: true,
    connection: "1",
    changes: 1,
  };

  it("says the shared draft keeps what reached it, not that all is lost", () => {
    render(<ConfirmCloseTabsModal open tabs={[tab("flow", { dirty: true, draft })]} onCancel={() => {}} onConfirm={() => {}} />);
    expect(screen.getByRole("dialog")).toHaveTextContent("Close tab?");
    expect(screen.getByTestId("close-tabs-drafts")).toHaveTextContent("Its draft keeps the sent changes · unsent edits are lost");
    expect(screen.getByTestId("close-tabs-body")).not.toHaveTextContent("Loses its changes");
    expect(screen.getByTestId("close-tabs-confirm")).toHaveTextContent("Close tab");
  });

  it("names the tabs that lose their changes apart from the drafts", () => {
    render(
      <ConfirmCloseTabsModal
        open
        tabs={[tab("flow", { dirty: true, draft }), tab("local", { dirty: true })]}
        onCancel={() => {}}
        onConfirm={() => {}}
      />,
    );
    expect(screen.getByRole("dialog")).toHaveTextContent("Discard unsaved changes?");
    expect(screen.getByTestId("close-tabs-body")).toHaveTextContent("Loses its changeslocal.yamlIts draft keeps the sent changes · unsent edits are lostflow.yaml");
  });
});

describe("ConfirmCloseTabsModal — bounded to the window (#940)", () => {
  it("scrolls a long tab list in its body; the actions stay outside it", () => {
    const tabs = Array.from({ length: 60 }, (_, i) => tab(`tab_${i}`, { dirty: true }));
    render(<ConfirmCloseTabsModal open tabs={tabs} onCancel={() => {}} onConfirm={() => {}} />);
    const body = screen.getByTestId("close-tabs-body");
    // The modal's body scrolls (`d-frame__body`), bounded to the window (`d-frame`).
    expect(body.parentElement).toHaveClass("d-frame__body");
    expect(body.parentElement?.parentElement).toHaveClass("d-frame");
    expect(body).toHaveTextContent("tab_59.yaml");
    expect(body).not.toContainElement(screen.getByTestId("close-tabs-confirm"));
  });
});
