import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import AddNodeFromYamlModal from "./AddNodeFromYamlModal";
import { useEditStore } from "../pipeline/store/editStore";
import type { ParseNodeResult } from "../pipeline/model/library";

// Keep the real `libraryPortToPortDef`; only `parseNodeYaml` is stubbed per-test.
vi.mock("../settings/api/settings", async (orig) => {
  const actual = await orig<typeof import("../settings/api/settings")>();
  return { ...actual, parseNodeYaml: vi.fn() };
});
// The agentic profiles' API answers with the mock of `../settings/api/settings` too.
vi.mock("../settings/api/agentProfiles", async () => await import("../settings/api/settings"));
// The pipeline context's API answers with the mock of `../settings/api/settings` above.
vi.mock("../pipeline/api/library", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  ...(await import("../settings/api/settings")),
}));
vi.mock("../lib/nanoid", () => ({ generateNodeId: () => "fresh-node-id" }));

import { parseNodeYaml } from "../pipeline/api/library";
const mockParse = vi.mocked(parseNodeYaml);

function seedTab() {
  useEditStore.setState({
    openTabs: [
      {
        id: "t1",
        scope: "repo",
        pipeline: { name: "p", version: "1.0", variables: {}, nodes: [], edges: [] },
        prompts: {},
        diagnostics: [],
        dirty: false,
        externalDirty: false,
      },
    ],
    activeTabId: "t1",
    selection: { kind: "none", id: null },
    history: {},
  });
}

function okResult(overrides: Partial<ParseNodeResult> = {}): ParseNodeResult {
  return {
    spec: {
      name: "Imported",
      type: "agent",
      inputs: [],
      outputs: [{ name: "out", repeated: false, side: "right" }],
      interactive: false,
      model: null,
      ...(overrides.spec ?? {}),
    },
    prompt: "the prompt",
    warnings: [],
    ...overrides,
  } as ParseNodeResult;
}

const drop = () => ({ x: 100, y: 200 });

beforeEach(() => {
  seedTab();
  mockParse.mockReset();
});

describe("AddNodeFromYamlModal (#345)", () => {
  it("paste + submit creates a fresh, selected node and closes (no warnings)", async () => {
    mockParse.mockResolvedValue(okResult());
    const onClose = vi.fn();
    render(<AddNodeFromYamlModal getDropPosition={drop} onClose={onClose} />);

    fireEvent.change(screen.getByTestId("add-node-yaml-textarea"), {
      target: { value: "name: Imported\ntype: agent\nisolated_worktree: false\n" },
    });
    fireEvent.click(screen.getByTestId("add-node-yaml-submit"));

    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
    const st = useEditStore.getState();
    const nodes = st.openTabs[0].pipeline.nodes;
    expect(nodes).toHaveLength(1);
    expect(nodes[0].id).toBe("fresh-node-id");
    expect(nodes[0].name).toBe("Imported");
    expect(nodes[0].view).toEqual({ x: 100, y: 200 });
    expect(st.openTabs[0].prompts["fresh-node-id"]).toBe("the prompt");
    expect(st.selection).toEqual({ kind: "node", id: "fresh-node-id" });
  });

  it("carries the pasted node's workspace onto the canvas (#653)", async () => {
    // A node that opted OUT of isolation must not silently fork a sub-worktree
    // once instantiated — the isolation line travels with the spec.
    mockParse.mockResolvedValue(
      okResult({ spec: { ...okResult().spec, isolated_worktree: false } }),
    );
    render(<AddNodeFromYamlModal getDropPosition={drop} onClose={() => {}} />);
    fireEvent.change(screen.getByTestId("add-node-yaml-textarea"), {
      target: { value: "name: Imported\ntype: agent\nisolated_worktree: false\n" },
    });
    fireEvent.click(screen.getByTestId("add-node-yaml-submit"));

    await waitFor(() =>
      expect(useEditStore.getState().openTabs[0].pipeline.nodes).toHaveLength(1),
    );
    expect(useEditStore.getState().openTabs[0].pipeline.nodes[0].isolated_worktree).toBe(false);
  });

  it("the created node is undoable in one step", async () => {
    mockParse.mockResolvedValue(okResult());
    render(<AddNodeFromYamlModal getDropPosition={drop} onClose={() => {}} />);
    fireEvent.change(screen.getByTestId("add-node-yaml-textarea"), {
      target: { value: "name: Imported\ntype: agent\nisolated_worktree: false\n" },
    });
    fireEvent.click(screen.getByTestId("add-node-yaml-submit"));
    await waitFor(() => expect(useEditStore.getState().openTabs[0].pipeline.nodes).toHaveLength(1));
    useEditStore.getState().undo();
    expect(useEditStore.getState().openTabs[0].pipeline.nodes).toHaveLength(0);
  });

  it("a hard error shows a red box, creates no node, and keeps the modal open", async () => {
    mockParse.mockRejectedValue(new Error("this is a whole pipeline, not a node"));
    const onClose = vi.fn();
    render(<AddNodeFromYamlModal getDropPosition={drop} onClose={onClose} />);
    fireEvent.change(screen.getByTestId("add-node-yaml-textarea"), {
      target: { value: "name: p\nnodes: []\n" },
    });
    fireEvent.click(screen.getByTestId("add-node-yaml-submit"));

    expect(await screen.findByTestId("add-node-yaml-error")).toHaveTextContent(/whole pipeline/);
    expect(onClose).not.toHaveBeenCalled();
    expect(useEditStore.getState().openTabs[0].pipeline.nodes).toHaveLength(0);
  });

  it("warnings create the node AND show the amber list (does not auto-close)", async () => {
    mockParse.mockResolvedValue(
      okResult({ warnings: ["node 'x': unknown node type 'bogus', defaulting to 'agent'"] }),
    );
    const onClose = vi.fn();
    render(<AddNodeFromYamlModal getDropPosition={drop} onClose={onClose} />);
    fireEvent.change(screen.getByTestId("add-node-yaml-textarea"), {
      target: { value: "name: x\ntype: bogus\n" },
    });
    fireEvent.click(screen.getByTestId("add-node-yaml-submit"));

    expect(await screen.findByTestId("add-node-yaml-warnings")).toHaveTextContent(/bogus/);
    expect(onClose).not.toHaveBeenCalled();
    expect(useEditStore.getState().openTabs[0].pipeline.nodes).toHaveLength(1);
  });

  it("a .yaml upload fills the textarea from the file text", async () => {
    render(<AddNodeFromYamlModal getDropPosition={drop} onClose={() => {}} />);
    const file = new File(["name: FromFile\ntype: agent\nisolated_worktree: false\n"], "node.yaml", {
      type: "text/yaml",
    });
    fireEvent.change(screen.getByTestId("add-node-yaml-file"), { target: { files: [file] } });
    await waitFor(() =>
      expect((screen.getByTestId("add-node-yaml-textarea") as HTMLTextAreaElement).value).toContain(
        "name: FromFile",
      ),
    );
  });
});
