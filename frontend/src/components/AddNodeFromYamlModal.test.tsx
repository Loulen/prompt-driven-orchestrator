import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import AddNodeFromYamlModal from "./AddNodeFromYamlModal";
import { useEditStore } from "../stores/editStore";
import type { ParseNodeResult } from "../api";

// Keep the real `libraryPortToPortDef`; only `parseNodeYaml` is stubbed per-test.
vi.mock("../api", async (orig) => {
  const actual = await orig<typeof import("../api")>();
  return { ...actual, parseNodeYaml: vi.fn() };
});
vi.mock("../lib/nanoid", () => ({ generateNodeId: () => "fresh-node-id" }));

import { parseNodeYaml } from "../api";
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
      type: "doc-only",
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
      target: { value: "name: Imported\ntype: doc-only\n" },
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

  it("the created node is undoable in one step", async () => {
    mockParse.mockResolvedValue(okResult());
    render(<AddNodeFromYamlModal getDropPosition={drop} onClose={() => {}} />);
    fireEvent.change(screen.getByTestId("add-node-yaml-textarea"), {
      target: { value: "name: Imported\ntype: doc-only\n" },
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
      okResult({ warnings: ["node 'x': unknown node type 'bogus', defaulting to 'doc-only'"] }),
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
    const file = new File(["name: FromFile\ntype: doc-only\n"], "node.yaml", {
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
