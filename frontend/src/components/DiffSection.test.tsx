import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import DiffSection from "./DiffSection";
import type { RunState, NodeState } from "../types";

vi.mock("../api", () => ({
  fetchRunDiff: vi.fn(),
  fetchNodeDiff: vi.fn(),
}));

import { fetchRunDiff, fetchNodeDiff } from "../api";

const mockedFetchRunDiff = vi.mocked(fetchRunDiff);
const mockedFetchNodeDiff = vi.mocked(fetchNodeDiff);

function makeRun(overrides: Partial<RunState> = {}): RunState {
  return {
    run_id: "test-run-1",
    status: "running",
    pipeline_name: "test-pipe",
    input: "test input",
    started_at: "2026-05-14T00:00:00Z",
    completed_at: null,
    nodes: {},
    edges: [],
    node_defs: [],
    start_node: null,
    end_node: null,
    merge_resolver: null,
    loop_states: {},
    foreach_states: {},
    ...overrides,
  };
}

function makeNodeState(overrides: Partial<NodeState> = {}): NodeState {
  return {
    node_id: "impl-1",
    status: "completed",
    iter: 1,
    started_at: "2026-05-14T00:00:00Z",
    completed_at: "2026-05-14T00:01:00Z",
    failure_reason: null,
    iterations: [],
    ...overrides,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  mockedFetchRunDiff.mockResolvedValue("");
  mockedFetchNodeDiff.mockResolvedValue("");
});

describe("DiffSection", () => {
  it("is not rendered when run is null", () => {
    const { container } = render(<DiffSection run={null} />);
    expect(container.querySelector("[data-testid='diff-section']")).toBeNull();
  });

  it("renders a collapsible section header when run exists", () => {
    render(<DiffSection run={makeRun()} />);
    expect(screen.getByTestId("diff-section")).toBeInTheDocument();
    expect(screen.getByText("Diff")).toBeInTheDocument();
  });

  it("is collapsed by default", () => {
    render(<DiffSection run={makeRun()} />);
    expect(screen.queryByTestId("diff-content")).toBeNull();
  });

  it("expands when clicked and fetches aggregate diff", async () => {
    mockedFetchRunDiff.mockResolvedValue(
      "diff --git a/file.rs b/file.rs\n+fn hello() {}\n",
    );

    render(<DiffSection run={makeRun()} />);
    fireEvent.click(screen.getByText("Diff"));

    await waitFor(() => {
      expect(screen.getByTestId("diff-content")).toBeInTheDocument();
    });
    expect(mockedFetchRunDiff).toHaveBeenCalledWith("test-run-1");
  });

  it("renders diff text with syntax highlighting (additions green)", async () => {
    mockedFetchRunDiff.mockResolvedValue(
      "diff --git a/file.rs b/file.rs\n--- a/file.rs\n+++ b/file.rs\n@@ -0,0 +1 @@\n+fn hello() {}\n-fn old() {}\n",
    );

    render(<DiffSection run={makeRun()} />);
    fireEvent.click(screen.getByText("Diff"));

    await waitFor(() => {
      expect(screen.getByTestId("diff-content")).toBeInTheDocument();
    });

    const content = screen.getByTestId("diff-content");
    expect(content.textContent).toContain("+fn hello() {}");
    expect(content.textContent).toContain("-fn old() {}");
  });

  it("shows empty state when diff is empty", async () => {
    mockedFetchRunDiff.mockResolvedValue("");

    render(<DiffSection run={makeRun()} />);
    fireEvent.click(screen.getByText("Diff"));

    await waitFor(() => {
      expect(screen.getByTestId("diff-content")).toBeInTheDocument();
    });
    expect(screen.getByText("No changes")).toBeInTheDocument();
  });

  it("shows dropdown with completed code-mutating nodes", async () => {
    const run = makeRun({
      nodes: {
        "impl-1": makeNodeState({ node_id: "impl-1" }),
        "reviewer-1": makeNodeState({
          node_id: "reviewer-1",
          status: "completed",
        }),
      },
      node_defs: [
        {
          id: "impl-1",
          name: "Implementer",
          node_type: "code-mutating",
          view_x: 0,
          view_y: 0,
          inputs: [],
          outputs: [],
        },
        {
          id: "reviewer-1",
          name: "Reviewer",
          node_type: "doc-only",
          view_x: 0,
          view_y: 0,
          inputs: [],
          outputs: [],
        },
      ],
    });

    render(<DiffSection run={run} />);
    fireEvent.click(screen.getByText("Diff"));

    await waitFor(() => {
      expect(screen.getByTestId("diff-content")).toBeInTheDocument();
    });

    const select = screen.getByTestId("diff-node-select");
    expect(select).toBeInTheDocument();

    const options = select.querySelectorAll("option");
    expect(options.length).toBe(2);
    expect(options[0].textContent).toBe("Aggregate (all changes)");
    expect(options[1].textContent).toContain("Implementer");
  });

  it("fetches per-node diff when a node is selected", async () => {
    mockedFetchNodeDiff.mockResolvedValue(
      "diff --git a/node.rs b/node.rs\n+fn node_work() {}\n",
    );

    const run = makeRun({
      nodes: {
        "impl-1": makeNodeState({ node_id: "impl-1" }),
      },
      node_defs: [
        {
          id: "impl-1",
          name: "Implementer",
          node_type: "code-mutating",
          view_x: 0,
          view_y: 0,
          inputs: [],
          outputs: [],
        },
      ],
    });

    render(<DiffSection run={run} />);
    fireEvent.click(screen.getByText("Diff"));

    await waitFor(() => {
      expect(screen.getByTestId("diff-node-select")).toBeInTheDocument();
    });

    fireEvent.change(screen.getByTestId("diff-node-select"), {
      target: { value: "impl-1" },
    });

    await waitFor(() => {
      expect(mockedFetchNodeDiff).toHaveBeenCalledWith("test-run-1", "impl-1");
    });
  });

  it("shows an honest message for archived runs and does not fetch", async () => {
    render(<DiffSection run={makeRun({ status: "archived" })} />);
    fireEvent.click(screen.getByText("Diff"));

    await waitFor(() => {
      expect(screen.getByTestId("diff-content")).toBeInTheDocument();
    });

    expect(
      screen.getByText("Diff not preserved for archived runs."),
    ).toBeInTheDocument();
    // The branch is gone at cleanup — a fetch would be a lie.
    expect(mockedFetchRunDiff).not.toHaveBeenCalled();
    expect(mockedFetchNodeDiff).not.toHaveBeenCalled();
    expect(screen.queryByText("No changes")).toBeNull();
  });

  it("groups the diff by file with per-file paths and +/- badges", async () => {
    mockedFetchRunDiff.mockResolvedValue(
      [
        "diff --git a/src/a.rs b/src/a.rs",
        "index 111..222 100644",
        "--- a/src/a.rs",
        "+++ b/src/a.rs",
        "@@ -1 +1 @@",
        "-old a",
        "+new a",
        "diff --git a/src/b.rs b/src/b.rs",
        "new file mode 100644",
        "index 0000000..333",
        "--- /dev/null",
        "+++ b/src/b.rs",
        "@@ -0,0 +1,2 @@",
        "+line 1",
        "+line 2",
        "",
      ].join("\n"),
    );

    render(<DiffSection run={makeRun()} />);
    fireEvent.click(screen.getByText("Diff"));

    await waitFor(() => {
      expect(screen.getAllByTestId("diff-file")).toHaveLength(2);
    });

    const sections = screen.getAllByTestId("diff-file");
    // First file: modified src/a.rs, +1/-1.
    expect(sections[0].textContent).toContain("src/a.rs");
    expect(sections[0].textContent).toContain("+1");
    expect(sections[0].textContent).toContain("-1");
    // Second file: added src/b.rs, +2/-0.
    expect(sections[1].textContent).toContain("src/b.rs");
    expect(sections[1].textContent).toContain("+2");
    expect(sections[1].textContent).toContain("-0");
    // No monolithic "No changes" fallback when there is a diff.
    expect(screen.queryByText("No changes")).toBeNull();
  });
});
