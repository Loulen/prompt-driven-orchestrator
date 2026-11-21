import { act, render } from "@testing-library/react";
import { describe, it, expect, beforeEach } from "vitest";
import EditorModals from "./EditorModals";
import { useEditStore } from "../pipeline/store/editStore";
import type { OpenPipeline } from "../pipeline/store/editState";

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

function unload(): Event {
  const e = new Event("beforeunload", { cancelable: true });
  window.dispatchEvent(e);
  return e;
}

describe("EditorModals: leaving the page with unsaved work (CORE-281)", () => {
  beforeEach(() => {
    useEditStore.setState({ openTabs: [], activeTabId: null, pendingSingleTab: null });
  });

  it("asks the browser's alert while an open tab has unsaved work", () => {
    useEditStore.setState({ openTabs: [tab("a"), tab("b", { dirty: true })], activeTabId: "a" });
    render(<EditorModals saveErrorTab={undefined} onViewYaml={() => {}} />);
    expect(unload().defaultPrevented).toBe(true);
  });

  it("lets the page go when every open tab is clean", () => {
    useEditStore.setState({ openTabs: [tab("a"), tab("b")], activeTabId: "a" });
    render(<EditorModals saveErrorTab={undefined} onViewYaml={() => {}} />);
    expect(unload().defaultPrevented).toBe(false);
  });

  it("drops the alert once the unsaved tab is closed", () => {
    useEditStore.setState({ openTabs: [tab("a", { dirty: true })], activeTabId: "a" });
    render(<EditorModals saveErrorTab={undefined} onViewYaml={() => {}} />);
    act(() => useEditStore.getState().closeTabs(["a"]));
    expect(unload().defaultPrevented).toBe(false);
  });
});
