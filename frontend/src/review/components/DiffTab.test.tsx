import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor, within } from "@testing-library/react";
import { useState } from "react";
import DiffTab from "./DiffTab";
import { LARGE_DIFF_FILES, TRUNCATE_LINES } from "../model/diffTab";
import type { CollapsedFiles } from "../model/diffTab";
import type { RunState } from "../../shared/generated/RunState";
import type { NodeState } from "../../shared/generated/NodeState";
import type { StructuredDiff } from "../../shared/generated/StructuredDiff";
import type { DiffFile } from "../../shared/generated/DiffFile";

// Migrated from DiffSection.test.tsx (#748): archived / no changes / per-file
// +/- / collapse-expand / binary, plus the tab's own additions (ledger, word
// highlight, large-diff collapse, truncation, "Diff changed · Reload").

vi.mock("../api/diff", () => ({
  fetchRunStructuredDiff: vi.fn(),
}));

import { fetchRunStructuredDiff } from "../api/diff";

const mockedFetch = vi.mocked(fetchRunStructuredDiff);

function makeRun(overrides: Partial<RunState> = {}): RunState {
  return {
    collection_states: {},
    switch_states: {},
    sandbox: "off",
    sessions_spawned: 0,
    run_id: "test-run-1",
    status: "running",
    pipeline_name: "test-pipe",
    input: "test input",
    started_at: "2026-05-14T00:00:00Z",
    completed_at: null,
    nodes: {},
    edges: [],
    node_defs: [],
    start_node: undefined,
    end_node: undefined,
    merge_resolver: undefined,
    loop_states: {},
    foreach_states: {},
    ...overrides,
  };
}

function makeNodeState(overrides: Partial<NodeState> = {}): NodeState {
  return { frontmatter_retries: 0,
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

function file(overrides: Partial<DiffFile> & { path: string }): DiffFile {
  const { path, ...rest } = overrides;
  return {
    old_path: path,
    new_path: path,
    status: "modified",
    binary: false,
    additions: 0,
    deletions: 0,
    hunks: [],
    ...rest,
  };
}

function diffOf(files: DiffFile[]): StructuredDiff {
  return {
    from_ref: "a3f21c0000000000000000000000000000004c2e",
    to_ref: "pdo/run-test-run-1",
    from_sha: "a3f21c0000000000000000000000000000004c2e",
    to_sha: "9b7d0e0000000000000000000000000000000000",
    three_dot: true,
    files,
    additions: files.reduce((n, f) => n + f.additions, 0),
    deletions: files.reduce((n, f) => n + f.deletions, 0),
    files_changed: files.length,
  };
}

const TWO_FILES = diffOf([
  file({
    path: "src/a.rs",
    additions: 1,
    deletions: 1,
    hunks: [
      {
        old_start: 1,
        old_lines: 1,
        new_start: 1,
        new_lines: 1,
        header: "",
        lines: [
          { kind: "del", content: "let a = 1;", old_no: 1, new_no: null },
          { kind: "add", content: "const a = 2;", old_no: null, new_no: 1 },
        ],
      },
    ],
  }),
  file({
    path: "src/b.rs",
    old_path: null,
    status: "added",
    additions: 2,
    deletions: 0,
    hunks: [
      {
        old_start: 0,
        old_lines: 0,
        new_start: 1,
        new_lines: 2,
        header: "",
        lines: [
          { kind: "add", content: "line 1", old_no: null, new_no: 1 },
          { kind: "add", content: "line 2", old_no: null, new_no: 2 },
        ],
      },
    ],
  }),
]);

/** Host that owns the collapsed state, like PipelineInfoPanel does. */
function Host({ run, initial = null }: { run: RunState; initial?: CollapsedFiles }) {
  const [collapsed, setCollapsed] = useState<CollapsedFiles>(initial);
  return <DiffTab run={run} collapsed={collapsed} onCollapsedChange={setCollapsed} />;
}

beforeEach(() => {
  vi.clearAllMocks();
  mockedFetch.mockResolvedValue(diffOf([]));
});

describe("DiffTab", () => {
  it("shows an honest message for archived runs and does not fetch", () => {
    render(<Host run={makeRun({ status: "archived" })} />);
    expect(screen.getByTestId("diff-archived")).toBeInTheDocument();
    expect(screen.getByText("Diff not preserved for archived runs")).toBeInTheDocument();
    expect(screen.getByText("The run branch was deleted at cleanup.")).toBeInTheDocument();
    // The branch is gone at cleanup — a fetch would be a lie.
    expect(mockedFetch).not.toHaveBeenCalled();
    expect(screen.queryByText("No changes")).toBeNull();
  });

  it("shows a skeleton while loading, then 'No changes' for an empty diff", async () => {
    render(<Host run={makeRun()} />);
    expect(screen.getByTestId("diff-loading")).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getByTestId("diff-empty")).toBeInTheDocument();
    });
    expect(screen.getByText("No changes")).toBeInTheDocument();
    expect(screen.getByText("Fork point and run tip are identical.")).toBeInTheDocument();
    expect(mockedFetch).toHaveBeenCalledWith("test-run-1");
  });

  it("renders the summary, the ledger and one block per file with +/- counts", async () => {
    mockedFetch.mockResolvedValue(TWO_FILES);
    render(<Host run={makeRun()} />);

    await waitFor(() => {
      expect(screen.getAllByTestId("diff-file")).toHaveLength(2);
    });

    const summary = screen.getByTestId("diff-summary");
    expect(summary).toHaveTextContent("2 files");
    expect(summary).toHaveTextContent("+3");
    expect(summary).toHaveTextContent("−1");
    // Short SHAs of the pair.
    expect(screen.getByTestId("diff-summary-refs")).toHaveTextContent("a3f21c0 → 9b7d0e0");

    const rows = screen.getAllByTestId("diff-ledger-row");
    expect(rows).toHaveLength(2);
    expect(rows[0]).toHaveTextContent("src/a.rs");
    expect(rows[0]).toHaveTextContent("+1");
    expect(rows[0]).toHaveTextContent("−1");
    expect(rows[1]).toHaveTextContent("src/b.rs");
    expect(rows[1]).toHaveTextContent("+2");
    expect(rows[1]).toHaveTextContent("−0");

    const blocks = screen.getAllByTestId("diff-file");
    expect(blocks[0]).toHaveTextContent("src/");
    expect(blocks[0]).toHaveTextContent("a.rs");
    expect(within(blocks[1]).getByTestId("diff-file-badge")).toHaveTextContent("new");
    expect(screen.queryByText("No changes")).toBeNull();
  });

  it("expands every file by default and colours added/deleted lines", async () => {
    mockedFetch.mockResolvedValue(TWO_FILES);
    render(<Host run={makeRun()} />);
    await waitFor(() => {
      expect(screen.getAllByTestId("diff-file-body")).toHaveLength(2);
    });
    const adds = screen.getAllByTestId("diff-line-add");
    const dels = screen.getAllByTestId("diff-line-del");
    expect(adds).toHaveLength(3);
    expect(dels).toHaveLength(1);
    expect(adds[0].className).toContain("bg-st-done-bg");
    expect(dels[0].className).toContain("bg-st-failed-bg");
    expect(dels[0]).toHaveTextContent("let a = 1;");
    expect(adds[0]).toHaveTextContent("const a = 2;");
    // Hunk header rendered as a thin separator.
    expect(screen.getAllByTestId("diff-hunk")[0]).toHaveTextContent("@@ -1,1 +1,1 @@");
  });

  it("highlights only the changed words of an adjacent −/+ pair", async () => {
    mockedFetch.mockResolvedValue(TWO_FILES);
    render(<Host run={makeRun()} />);
    await waitFor(() => {
      expect(screen.getAllByTestId("diff-file-body")).toHaveLength(2);
    });
    const del = screen.getAllByTestId("diff-line-del")[0];
    const words = within(del).getAllByTestId("diff-word").map((w) => w.textContent);
    expect(words).toEqual(["let", "1"]);
    const add = screen.getAllByTestId("diff-line-add")[0];
    expect(within(add).getAllByTestId("diff-word").map((w) => w.textContent)).toEqual([
      "const",
      "2",
    ]);
    // Pure additions (no `-` partner) carry no word highlight.
    expect(within(screen.getAllByTestId("diff-line-add")[1]).queryAllByTestId("diff-word")).toEqual([]);
  });

  it("collapses a file to its header and expands it back", async () => {
    mockedFetch.mockResolvedValue(TWO_FILES);
    render(<Host run={makeRun()} />);
    await waitFor(() => {
      expect(screen.getAllByTestId("diff-file-body")).toHaveLength(2);
    });
    const headers = screen.getAllByTestId("diff-file-header");
    fireEvent.click(headers[0]);
    expect(screen.getAllByTestId("diff-file-body")).toHaveLength(1);
    expect(screen.getAllByTestId("diff-file")[0]).toHaveAttribute("data-collapsed", "true");
    expect(headers[0]).toHaveAttribute("aria-expanded", "false");
    // The header stays, with its counts.
    expect(headers[0]).toHaveTextContent("a.rs");
    expect(headers[0]).toHaveTextContent("+1");

    fireEvent.click(headers[0]);
    expect(screen.getAllByTestId("diff-file-body")).toHaveLength(2);
    expect(screen.getAllByTestId("diff-line-del")[0]).toHaveTextContent("let a = 1;");
  });

  it("Collapse all / Expand all act on every file", async () => {
    mockedFetch.mockResolvedValue(TWO_FILES);
    render(<Host run={makeRun()} />);
    await waitFor(() => {
      expect(screen.getAllByTestId("diff-file-body")).toHaveLength(2);
    });
    fireEvent.click(screen.getByTestId("diff-collapse-all"));
    expect(screen.queryAllByTestId("diff-file-body")).toHaveLength(0);
    fireEvent.click(screen.getByTestId("diff-expand-all"));
    expect(screen.getAllByTestId("diff-file-body")).toHaveLength(2);
  });

  it("keeps the collapsed state the host hands it (survives Info ↔ Diff)", async () => {
    mockedFetch.mockResolvedValue(TWO_FILES);
    render(<Host run={makeRun()} initial={new Set(["src/b.rs"])} />);
    await waitFor(() => {
      expect(screen.getAllByTestId("diff-file")).toHaveLength(2);
    });
    const blocks = screen.getAllByTestId("diff-file");
    expect(blocks[0]).toHaveAttribute("data-collapsed", "false");
    expect(blocks[1]).toHaveAttribute("data-collapsed", "true");
  });

  it("renders a binary file collapsed, badged, with no body", async () => {
    mockedFetch.mockResolvedValue(
      diffOf([
        file({ path: "docs/pdo-ui.png", binary: true, additions: 0, deletions: 0 }),
        file({
          path: "a.txt",
          additions: 1,
          hunks: [
            {
              old_start: 0,
              old_lines: 0,
              new_start: 1,
              new_lines: 1,
              header: "",
              lines: [{ kind: "add", content: "x", old_no: null, new_no: 1 }],
            },
          ],
        }),
      ]),
    );
    render(<Host run={makeRun()} />);
    await waitFor(() => {
      expect(screen.getAllByTestId("diff-file")).toHaveLength(2);
    });
    const [png, txt] = screen.getAllByTestId("diff-file");
    expect(within(png).getByTestId("diff-file-badge")).toHaveTextContent("binary");
    expect(png).toHaveAttribute("data-collapsed", "true");
    expect(within(png).queryByTestId("diff-file-body")).toBeNull();
    expect(txt).toHaveAttribute("data-collapsed", "false");
    // A binary has no body even when "expanded".
    fireEvent.click(within(png).getByTestId("diff-file-header"));
    expect(within(png).queryByTestId("diff-file-body")).toBeNull();
  });

  it("shows a rename as old → new with a 'renamed' badge", async () => {
    mockedFetch.mockResolvedValue(
      diffOf([
        file({
          path: "src/DiffTab.tsx",
          old_path: "src/DiffSection.tsx",
          status: "renamed",
          additions: 2,
          deletions: 2,
        }),
      ]),
    );
    render(<Host run={makeRun()} />);
    await waitFor(() => {
      expect(screen.getAllByTestId("diff-file")).toHaveLength(1);
    });
    const header = screen.getByTestId("diff-file-header");
    expect(header).toHaveTextContent("src/DiffSection.tsx → src/DiffTab.tsx");
    expect(within(header).getByTestId("diff-file-badge")).toHaveTextContent("renamed");
  });

  it("starts collapsed with a banner past the large-diff threshold", async () => {
    const many = Array.from({ length: LARGE_DIFF_FILES + 1 }, (_, i) =>
      file({
        path: `f${i}.rs`,
        additions: 1,
        hunks: [
          {
            old_start: 0,
            old_lines: 0,
            new_start: 1,
            new_lines: 1,
            header: "",
            lines: [{ kind: "add", content: "x", old_no: null, new_no: 1 }],
          },
        ],
      }),
    );
    mockedFetch.mockResolvedValue(diffOf(many));
    render(<Host run={makeRun()} />);
    await waitFor(() => {
      expect(screen.getByTestId("diff-large-banner")).toBeInTheDocument();
    });
    expect(screen.getByTestId("diff-large-banner")).toHaveTextContent("Large diff — files collapsed");
    await waitFor(() => {
      expect(screen.queryAllByTestId("diff-file-body")).toHaveLength(0);
    });
    fireEvent.click(screen.getByTestId("diff-expand-all"));
    expect(screen.getAllByTestId("diff-file-body")).toHaveLength(LARGE_DIFF_FILES + 1);
  });

  it("truncates a very long file behind 'Show N more lines'", async () => {
    const n = TRUNCATE_LINES + 12;
    mockedFetch.mockResolvedValue(
      diffOf([
        file({
          path: "pnpm-lock.yaml",
          additions: n,
          hunks: [
            {
              old_start: 0,
              old_lines: 0,
              new_start: 1,
              new_lines: n,
              header: "",
              lines: Array.from({ length: n }, (_, i) => ({
                kind: "add" as const,
                content: `l${i}`,
                old_no: null,
                new_no: i + 1,
              })),
            },
          ],
        }),
      ]),
    );
    render(<Host run={makeRun()} />);
    await waitFor(() => {
      expect(screen.getByTestId("diff-show-more")).toBeInTheDocument();
    });
    expect(screen.getAllByTestId("diff-line-add")).toHaveLength(TRUNCATE_LINES);
    expect(screen.getByTestId("diff-show-more")).toHaveTextContent("Show 12 more lines");
    fireEvent.click(screen.getByTestId("diff-show-more"));
    expect(screen.getAllByTestId("diff-line-add")).toHaveLength(n);
    expect(screen.queryByTestId("diff-show-more")).toBeNull();
  });

  it("does not refetch on a Run push; offers 'Diff changed · Reload' when a node delivers", async () => {
    mockedFetch.mockResolvedValue(TWO_FILES);
    const run = makeRun({
      nodes: { "impl-1": makeNodeState({ delivery: { before: "aaa", after: "bbb" } }) },
    });
    const { rerender } = render(<Host run={run} />);
    await waitFor(() => {
      expect(screen.getAllByTestId("diff-file")).toHaveLength(2);
    });
    expect(mockedFetch).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId("diff-changed-reload")).toBeNull();

    // Collapse a file, then a sibling node delivers: the tip moved.
    fireEvent.click(screen.getAllByTestId("diff-file-header")[0]);
    const delivered = makeRun({
      nodes: {
        "impl-1": makeNodeState({ delivery: { before: "aaa", after: "bbb" } }),
        "impl-2": makeNodeState({ node_id: "impl-2", delivery: { before: "bbb", after: "ccc" } }),
      },
    });
    rerender(<Host run={delivered} />);
    // Nothing re-rendered under the reader: same fetch count, reload offered.
    expect(mockedFetch).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("diff-changed-reload")).toHaveTextContent("Diff changed · Reload");

    fireEvent.click(screen.getByTestId("diff-changed-reload"));
    expect(screen.getByTestId("diff-updating")).toBeInTheDocument();
    await waitFor(() => {
      expect(mockedFetch).toHaveBeenCalledTimes(2);
      expect(screen.queryByTestId("diff-updating")).toBeNull();
    });
    expect(screen.queryByTestId("diff-changed-reload")).toBeNull();
    // The collapsed state is kept by path across the reload.
    expect(screen.getAllByTestId("diff-file")[0]).toHaveAttribute("data-collapsed", "true");
  });

  it("links « Expand and comment » to the Run's Review page, fork → tip (#749)", async () => {
    mockedFetch.mockResolvedValue(TWO_FILES);
    render(<Host run={makeRun()} />);
    await waitFor(() => {
      expect(screen.getByTestId("diff-review-button")).toBeInTheDocument();
    });
    const link = screen.getByTestId("diff-review-button");
    expect(link.tagName).toBe("A");
    expect(link).toHaveTextContent("Expand and comment");
    // The default pair is the short canonical URL: no query.
    expect(link).toHaveAttribute("href", "/runs/test-run-1/review");
  });

  it("shows an error state with Retry when the endpoint fails", async () => {
    mockedFetch.mockRejectedValueOnce(new Error("run branch not found"));
    mockedFetch.mockResolvedValueOnce(TWO_FILES);
    render(<Host run={makeRun()} />);
    await waitFor(() => {
      expect(screen.getByTestId("diff-error")).toBeInTheDocument();
    });
    expect(screen.getByTestId("diff-error")).toHaveTextContent("run branch not found");
    fireEvent.click(screen.getByText("Retry"));
    await waitFor(() => {
      expect(screen.getAllByTestId("diff-file")).toHaveLength(2);
    });
  });
});
