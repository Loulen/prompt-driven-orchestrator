import { fireEvent, render, screen } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";

// « Fichier importé en cours de Run »: the node panel opens the import modal
// from its terminal (a drop, or the Import button) and says when the node on
// screen cannot receive files.

globalThis.ResizeObserver = class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

vi.mock("../../settings/api/settings", () => ({
  artifactUrl: (runId: string, path: string) => `/runs/${runId}/artifact?path=${encodeURIComponent(path)}`,
}));
vi.mock("../../settings/api/agentProfiles", async () => await import("../../settings/api/settings"));
vi.mock("../api/artifacts", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  ...(await import("../../settings/api/settings")),
}));
vi.mock("../api/commands", () => ({ killNode: vi.fn(), restartNode: vi.fn() }));
vi.mock("../api/completion", () => ({
  markNodeDone: vi.fn().mockResolvedValue({ kind: "completed" }),
}));
vi.mock("../api/nodes", () => ({
  fetchPrompt: vi.fn().mockResolvedValue("prompt"),
  fetchNodeIO: vi.fn().mockResolvedValue({ inputs: [], outputs: [] }),
  stopNode: vi.fn(),
  retryNode: vi.fn(),
  retryNodePreview: vi.fn().mockResolvedValue({ downstream: [], affected_count: 0, with_artifacts: [] }),
  startNode: vi.fn(),
}));
vi.mock("../../workspace/api/provisioning", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../workspace/api/provisioning")>()),
  previewProvisioning: vi.fn().mockResolvedValue({ entries: [], rules: [], conflicts: [] }),
}));

const PDF = new File(["%PDF"], "contrat.pdf", { type: "application/pdf" });

// The terminal stands in for the real one: a drop and the Import button are
// the two calls of `onImportFiles`.
vi.mock("../../session/components/TmuxTerminal", () => ({
  default: ({
    onImportFiles,
    importDisabledReason,
  }: {
    onImportFiles?: (files: File[]) => void;
    importDisabledReason?: string | null;
  }) => (
    <div data-testid="tmux-terminal" data-import-reason={importDisabledReason ?? ""}>
      {onImportFiles && (
        <>
          <button data-testid="fake-drop" onClick={() => onImportFiles([PDF])} />
          <button data-testid="fake-import" onClick={() => onImportFiles([])} />
        </>
      )}
    </div>
  ),
}));
vi.mock("./nodeDetail/ImportFilesModal", () => ({
  default: ({
    nodeId,
    iter,
    initialFiles,
    blockedReason,
    onClose,
  }: {
    nodeId: string;
    iter: number;
    initialFiles: File[];
    blockedReason?: string | null;
    onClose: () => void;
  }) => (
    <div data-testid="import-modal" data-reason={blockedReason ?? ""}>
      {nodeId} iter {iter}: {initialFiles.map((f) => f.name).join(",") || "none"}
      <button data-testid="close-import" onClick={onClose} />
    </div>
  ),
}));
vi.mock("../../shared/ui/resizable", () => ({
  ResizablePanelGroup: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  ResizablePanel: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  ResizableHandle: () => <div />,
}));
vi.mock("./MarkdownArtifactModal", () => ({ default: () => null }));

import NodeDetailPanel from "./NodeDetailPanel";
import type { NodeState } from "../../shared/generated/NodeState";

function runningNode(overrides?: Partial<NodeState>): NodeState {
  return {
    frontmatter_retries: 0,
    node_id: "grill",
    status: "running",
    iter: 2,
    started_at: "2026-10-08T00:00:00Z",
    completed_at: null,
    failure_reason: null,
    iterations: [
      { iter: 1, status: "completed", started_at: null, completed_at: null },
      { iter: 2, status: "running", started_at: null, completed_at: null },
    ],
    ...overrides,
  };
}

describe("NodeDetailPanel — file import", () => {
  it("a drop on the terminal opens the import modal with the files, for the current iteration", () => {
    render(<NodeDetailPanel node={runningNode()} runId="run-1" />);
    expect(screen.getByTestId("tmux-terminal")).toHaveAttribute("data-import-reason", "");
    fireEvent.click(screen.getByTestId("fake-drop"));
    expect(screen.getByTestId("import-modal")).toHaveTextContent("grill iter 2: contrat.pdf");
    fireEvent.click(screen.getByTestId("close-import"));
    expect(screen.queryByTestId("import-modal")).toBeNull();
  });

  it("the Import button opens it empty", () => {
    render(<NodeDetailPanel node={runningNode({ status: "awaiting_user" })} runId="run-1" />);
    fireEvent.click(screen.getByTestId("fake-import"));
    expect(screen.getByTestId("import-modal")).toHaveTextContent("grill iter 2: none");
  });

  it("an archived Run offers no import at all", () => {
    render(<NodeDetailPanel node={runningNode()} runId="run-1" isArchived />);
    expect(screen.queryByTestId("fake-drop")).toBeNull();
  });

  it("a settled node says it has no live session", () => {
    render(
      <NodeDetailPanel
        node={runningNode({
          status: "failed",
          iterations: [{ iter: 2, status: "failed", started_at: null, completed_at: null }],
        })}
        runId="run-1"
        initialTerminalExpanded
      />,
    );
    expect(screen.getByTestId("tmux-terminal").dataset.importReason).toContain("no live session");
  });
});
