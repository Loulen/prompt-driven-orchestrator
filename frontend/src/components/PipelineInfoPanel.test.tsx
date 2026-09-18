import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { RunState, PipelineDef } from "../types";

// The badge/banner live in the InfoTab header. Mock the heavy
// children (network-fetching diff, tmux terminal) so the test stays focused on the
// #410 sandbox surface and never touches the network.
vi.mock("./DiffTab", () => ({ default: () => null }));
vi.mock("./TmuxTerminal", () => ({ default: vi.fn(() => null) }));

// #302 / ADR-0048: the Assistant tab drives create-if-absent / reap-on-leave
// against the daemon. Mock those two api helpers so the tests can assert the
// lifecycle without a network or a tmux session.
const {
  openLibraryAssistant,
  closeLibraryAssistant,
  fetchPipelineDocument,
  fetchRunPipelineDocument,
  fetchPipelineSkillsSidecar,
  startRunManager,
  stopRunManager,
  fetchSourceDrift,
  fetchRemotes,
} = vi.hoisted(() => ({
  openLibraryAssistant: vi.fn(),
  closeLibraryAssistant: vi.fn(),
  fetchPipelineDocument: vi.fn(),
  fetchRunPipelineDocument: vi.fn(),
  fetchPipelineSkillsSidecar: vi.fn(),
  startRunManager: vi.fn(),
  stopRunManager: vi.fn(),
  // #803: the Run view reads its drift on mount. Mocked so no test touches the
  // network, and so the ones that do not care about it see no Source block.
  fetchSourceDrift: vi.fn(),
  fetchRemotes: vi.fn(),
}));
vi.mock("../api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../api")>();
  return {
    ...actual,
    openLibraryAssistant,
    closeLibraryAssistant,
    fetchPipelineDocument,
    fetchRunPipelineDocument,
    fetchPipelineSkillsSidecar,
    startRunManager,
    stopRunManager,
    fetchSourceDrift,
    fetchRemotes,
  };
});

import PipelineInfoPanel from "./PipelineInfoPanel";
import TmuxTerminal from "./TmuxTerminal";
import type { TabId } from "./PipelineInfoPanel";

function makeRun(overrides: Partial<RunState> = {}): RunState {
  return {
    run_id: "run-abc1234567",
    status: "running",
    pipeline_name: "Test Pipeline",
    name: null,
    input: "do the thing",
    started_at: "2026-07-01T10:00:00.000Z",
    completed_at: null,
    nodes: {},
    edges: [],
    node_defs: [],
    start_node: null,
    end_node: null,
    merge_resolver: null,
    ...overrides,
  };
}

function renderPanel(run: RunState | null) {
  return render(
    <PipelineInfoPanel
      run={run}
      pipeline={null}
      libraryPipelines={[]}
      onLibraryChanged={() => {}}
      onClose={() => {}}
    />,
  );
}

beforeEach(() => {
  fetchSourceDrift.mockReset();
  fetchRemotes.mockReset();
  // Unless a test says otherwise, the drift is simply not readable: the block
  // disappears rather than claiming a zero.
  fetchSourceDrift.mockRejectedValue(new Error("no drift in this test"));
});

describe("PipelineInfoPanel — sandbox surface (#410)", () => {
  it("shows the sandbox badge for a sandboxed run (minimal)", () => {
    renderPanel(makeRun({ sandbox: "minimal" }));
    const badge = screen.getByTestId("sandbox-badge");
    expect(badge).toHaveTextContent(/sandbox:\s*minimal/i);
  });

  it("shows the sandbox badge for a full run", () => {
    renderPanel(makeRun({ sandbox: "full" }));
    expect(screen.getByTestId("sandbox-badge")).toHaveTextContent(/sandbox:\s*full/i);
  });

  it("omits the badge for an off/host run", () => {
    renderPanel(makeRun({ sandbox: "off" }));
    expect(screen.queryByTestId("sandbox-badge")).not.toBeInTheDocument();
  });

  it("omits the badge when sandbox is absent (historical/host run)", () => {
    renderPanel(makeRun());
    expect(screen.queryByTestId("sandbox-badge")).not.toBeInTheDocument();
  });

  it("shows the preparation banner while sandbox_prep is pending", () => {
    renderPanel(makeRun({ sandbox: "minimal", sandbox_prep: "pending" }));
    expect(screen.getByTestId("sandbox-prep-banner")).toHaveTextContent(/preparing the sandbox/i);
  });

  it("hides the preparation banner once sandbox_prep is ready", () => {
    renderPanel(makeRun({ sandbox: "minimal", sandbox_prep: "ready" }));
    expect(screen.queryByTestId("sandbox-prep-banner")).not.toBeInTheDocument();
    // The badge stays visible after prep completes.
    expect(screen.getByTestId("sandbox-badge")).toBeInTheDocument();
  });
});

// #397: the page-wide sweep that found the six anonymous toolbar buttons turned
// up a seventh here — this panel's close cross, an `X` icon with no label.
describe("PipelineInfoPanel — accessible names (#397)", () => {
  it("names the close button", () => {
    renderPanel(makeRun());
    expect(screen.getByTestId("info-panel-close")).toHaveAccessibleName(
      "Close pipeline info",
    );
  });

  it("still calls onClose when activated by that name", async () => {
    const onClose = vi.fn();
    render(
      <PipelineInfoPanel
        run={makeRun()}
        pipeline={null}
        libraryPipelines={[]}
        onLibraryChanged={() => {}}
        onClose={onClose}
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: "Close pipeline info" }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});

// Manager on demand: the Manager tab shows for EVERY live Run — an empty
// state with a Start button when no session exists, the terminal when one
// does — plus the transient starting state, the failure retry, and the
// confirmed Stop.
describe("PipelineInfoPanel — Manager tab (manager on demand)", () => {
  beforeEach(() => {
    startRunManager.mockReset();
    stopRunManager.mockReset();
    startRunManager.mockResolvedValue({ ok: true, session: "pdo-mgr-run-abc1234567", created: true });
    stopRunManager.mockResolvedValue({ ok: true, stopped: true });
  });

  it("shows the empty state with a Start button for a managerless run (the new default)", () => {
    renderPanel(makeRun());
    const tab = screen.getByTestId("info-tab-manager");
    expect(tab).toBeInTheDocument();
    expect(screen.queryByTestId("manager-empty-state")).not.toBeInTheDocument();
    fireEvent.click(tab);
    expect(screen.getByTestId("manager-empty-state")).toBeInTheDocument();
    expect(screen.getByTestId("manager-start")).toHaveTextContent("Start manager");
    expect(screen.getByTestId("manager-enable-settings")).toBeInTheDocument();
    expect(screen.queryByTestId("manager-live")).not.toBeInTheDocument();
  });

  it("shows the amber nudge dot only while the run waits on the user with no manager", () => {
    const { rerender } = render(
      <PipelineInfoPanel run={makeRun({ status: "awaiting_user" })} pipeline={null} onClose={() => {}} />,
    );
    expect(screen.getByTestId("manager-tab-dot")).toBeInTheDocument();

    rerender(
      <PipelineInfoPanel run={makeRun({ status: "running" })} pipeline={null} onClose={() => {}} />,
    );
    expect(screen.queryByTestId("manager-tab-dot")).not.toBeInTheDocument();
  });

  it("does not show the nudge dot once a manager is live", () => {
    render(
      <PipelineInfoPanel
        run={makeRun({ status: "awaiting_user", has_manager: true })}
        pipeline={null}
        onClose={() => {}}
      />,
    );
    expect(screen.queryByTestId("manager-tab-dot")).not.toBeInTheDocument();
  });

  it("mounts the terminal for a run whose manager exists, with a Stop control", () => {
    vi.mocked(TmuxTerminal).mockImplementation(() => <div data-testid="terminal-stub" />);
    renderPanel(makeRun({ has_manager: true }));
    fireEvent.click(screen.getByTestId("info-tab-manager"));
    expect(screen.getByTestId("manager-live")).toBeInTheDocument();
    expect(screen.getByTestId("terminal-stub")).toBeInTheDocument();
    expect(screen.getByTestId("manager-stop")).toBeInTheDocument();
    expect(screen.queryByTestId("manager-empty-state")).not.toBeInTheDocument();
  });

  it("starts the manager from the empty state and refetches the run", async () => {
    const onRefreshRun = vi.fn();
    const user = userEvent.setup();
    render(
      <PipelineInfoPanel run={makeRun()} pipeline={null} onClose={() => {}} onRefreshRun={onRefreshRun} />,
    );
    fireEvent.click(screen.getByTestId("info-tab-manager"));
    await user.click(screen.getByTestId("manager-start"));

    await waitFor(() => expect(startRunManager).toHaveBeenCalledWith("run-abc1234567"));
    expect(onRefreshRun).toHaveBeenCalled();
    // The transient state holds until the refreshed run state flips has_manager.
    expect(screen.getByTestId("manager-starting")).toBeInTheDocument();
  });

  it("swaps to the live terminal once has_manager flips true", async () => {
    vi.mocked(TmuxTerminal).mockImplementation(() => <div data-testid="terminal-stub" />);
    const { rerender } = render(
      <PipelineInfoPanel run={makeRun()} pipeline={null} onClose={() => {}} onRefreshRun={() => {}} />,
    );
    fireEvent.click(screen.getByTestId("info-tab-manager"));
    fireEvent.click(screen.getByTestId("manager-start"));
    await waitFor(() => expect(startRunManager).toHaveBeenCalled());
    expect(screen.getByTestId("manager-starting")).toBeInTheDocument();

    rerender(
      <PipelineInfoPanel
        run={makeRun({ has_manager: true })}
        pipeline={null}
        onClose={() => {}}
        onRefreshRun={() => {}}
      />,
    );
    expect(screen.getByTestId("manager-live")).toBeInTheDocument();
    expect(screen.getByTestId("terminal-stub")).toBeInTheDocument();
  });

  it("renders the failure with a Retry when the start refuses", async () => {
    startRunManager.mockRejectedValue(new Error("tmux server dead"));
    const user = userEvent.setup();
    renderPanel(makeRun());
    fireEvent.click(screen.getByTestId("info-tab-manager"));
    await user.click(screen.getByTestId("manager-start"));

    expect(await screen.findByTestId("manager-start-error")).toHaveTextContent(
      /failed to start the manager/i,
    );
    expect(screen.getByTestId("manager-retry")).toBeInTheDocument();
  });

  it("stops the manager only through the confirmation, then refetches", async () => {
    vi.mocked(TmuxTerminal).mockImplementation(() => <div data-testid="terminal-stub" />);
    const onRefreshRun = vi.fn();
    const user = userEvent.setup();
    render(
      <PipelineInfoPanel
        run={makeRun({ has_manager: true })}
        pipeline={null}
        onClose={() => {}}
        onRefreshRun={onRefreshRun}
      />,
    );
    fireEvent.click(screen.getByTestId("info-tab-manager"));

    // One click only asks — it must not kill anything yet.
    await user.click(screen.getByTestId("manager-stop"));
    expect(stopRunManager).not.toHaveBeenCalled();

    await user.click(screen.getByTestId("manager-stop-confirm"));
    await waitFor(() => expect(stopRunManager).toHaveBeenCalledWith("run-abc1234567"));
    expect(onRefreshRun).toHaveBeenCalled();
  });

  it("opens Settings from the empty state's enable-for-every-run link", async () => {
    const onOpenSettings = vi.fn();
    const user = userEvent.setup();
    render(
      <PipelineInfoPanel run={makeRun()} pipeline={null} onClose={() => {}} onOpenSettings={onOpenSettings} />,
    );
    fireEvent.click(screen.getByTestId("info-tab-manager"));
    await user.click(screen.getByTestId("manager-enable-settings"));
    expect(onOpenSettings).toHaveBeenCalledTimes(1);
  });
});

// #302 / ADR-0048: the Assistant tab is the mirror of the Manager tab — shown
// only for a library *template* (no live Run) with a resolvable pipeline id.
describe("PipelineInfoPanel — Assistant tab (#302)", () => {
  function makePipeline(overrides: Partial<PipelineDef> = {}): PipelineDef {
    return {
      name: "feature-with-review",
      version: "1.0",
      variables: {},
      nodes: [],
      edges: [],
      ...overrides,
    };
  }

  function renderTemplatePanel(
    props: {
      assistantId?: string | null;
      initialTab?: TabId;
      run?: RunState | null;
      pipeline?: PipelineDef;
    } = {},
  ) {
    // Honour an explicit `assistantId: null` (a template without a resolvable id)
    // rather than coalescing it back to the default.
    const assistantId = "assistantId" in props ? props.assistantId : "feature-with-review";
    return render(
      <PipelineInfoPanel
        run={props.run ?? null}
        pipeline={props.pipeline ?? makePipeline()}
        libraryPipelines={[]}
        onLibraryChanged={() => {}}
        onClose={() => {}}
        initialTab={props.initialTab}
        assistantId={assistantId}
      />,
    );
  }

  beforeEach(() => {
    openLibraryAssistant.mockReset();
    closeLibraryAssistant.mockReset();
    openLibraryAssistant.mockResolvedValue({
      session: "pdo-libassist-shared",
      created: true,
    });
    closeLibraryAssistant.mockResolvedValue({ ok: true, reaped: true });
    fetchPipelineDocument.mockResolvedValue("pdo_pipeline: 1\npipeline:\n  name: feature-with-review\n");
    fetchRunPipelineDocument.mockResolvedValue("pdo_pipeline: 1\npipeline:\n  name: run-snapshot\n");
  });

  it("shows the Assistant tab (not Manager) for a library template", () => {
    renderTemplatePanel();
    expect(screen.getByTestId("info-tab-assistant")).toBeInTheDocument();
    // Manager is a run-only tab — absent for a template.
    expect(screen.queryByTestId("info-tab-manager")).not.toBeInTheDocument();
  });

  it("hides the Assistant tab on a live run (Manager takes its place)", () => {
    // Even with an id supplied, the `!run` gate hides the Assistant on a run.
    renderTemplatePanel({ run: makeRun(), assistantId: "feature-with-review" });
    expect(screen.queryByTestId("info-tab-assistant")).not.toBeInTheDocument();
    expect(screen.getByTestId("info-tab-manager")).toBeInTheDocument();
  });

  it("hides the Assistant tab when no pipeline id is resolvable", () => {
    renderTemplatePanel({ assistantId: null });
    expect(screen.queryByTestId("info-tab-assistant")).not.toBeInTheDocument();
  });

  it("shows and copies the portable document from the daemon", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    renderTemplatePanel();

    await userEvent.click(screen.getByTestId("info-tab-yaml"));
    expect(await screen.findByTestId("portable-document-bar")).toHaveTextContent(
      "Portable document · v1",
    );
    await userEvent.click(screen.getByRole("button", { name: "Copy" }));

    expect(fetchPipelineDocument).toHaveBeenCalledWith("feature-with-review");
    expect(writeText).toHaveBeenCalledWith(expect.stringContaining("pdo_pipeline: 1"));
  });

  // #673 / ADR-0062: the skills a node selects travel beside the YAML, in a
  // sidecar zip the bar offers only when there is something to ship.
  it("offers the skills sidecar only when a node selects skills, and downloads it", async () => {
    fetchPipelineSkillsSidecar.mockReset();
    fetchPipelineSkillsSidecar.mockResolvedValue(new Blob(["PK"], { type: "application/zip" }));
    const createObjectURL = vi.fn().mockReturnValue("blob:sidecar");
    const revokeObjectURL = vi.fn();
    Object.defineProperty(URL, "createObjectURL", { configurable: true, value: createObjectURL });
    Object.defineProperty(URL, "revokeObjectURL", { configurable: true, value: revokeObjectURL });
    const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => {});

    const { unmount } = renderTemplatePanel();
    await userEvent.click(screen.getByTestId("info-tab-yaml"));
    await screen.findByTestId("portable-document-bar");
    expect(screen.queryByTestId("download-skills-sidecar")).not.toBeInTheDocument();
    unmount();

    renderTemplatePanel({
      pipeline: makePipeline({
        nodes: [
          {
            id: "worker",
            name: "Worker",
            type: "agent",
            inputs: [],
            outputs: [],
            interactive: false,
            skills: [
              { id: "11111111-1111-1111-1111-111111111111", name: "tdd" },
              { id: "22222222-2222-2222-2222-222222222222", name: "grilling" },
            ],
          },
          {
            id: "reviewer",
            name: "Reviewer",
            type: "agent",
            inputs: [],
            outputs: [],
            interactive: false,
            // The same id twice across nodes counts once in the sidecar.
            skills: [{ id: "11111111-1111-1111-1111-111111111111", name: "tdd" }],
          },
        ] as PipelineDef["nodes"],
      }),
    });
    await userEvent.click(screen.getByTestId("info-tab-yaml"));
    const button = await screen.findByTestId("download-skills-sidecar");
    expect(button).toHaveTextContent("Skills (2)");
    expect(screen.getByTestId("skills-sidecar-note")).toHaveTextContent(
      "feature-with-review.skills/",
    );

    await userEvent.click(button);

    await waitFor(() => expect(fetchPipelineSkillsSidecar).toHaveBeenCalledWith("feature-with-review"));
    await waitFor(() => expect(click).toHaveBeenCalled());
    expect(createObjectURL).toHaveBeenCalledWith(expect.any(Blob));
    click.mockRestore();
  });

  it("says so when the sidecar is empty (204: no referenced skill is in the bank)", async () => {
    fetchPipelineSkillsSidecar.mockReset();
    fetchPipelineSkillsSidecar.mockResolvedValue(null);
    renderTemplatePanel({
      pipeline: makePipeline({
        nodes: [
          {
            id: "worker",
            name: "Worker",
            type: "agent",
            inputs: [],
            outputs: [],
            interactive: false,
            skills: [{ id: "11111111-1111-1111-1111-111111111111", name: "tdd" }],
          },
        ] as PipelineDef["nodes"],
      }),
    });
    await userEvent.click(screen.getByTestId("info-tab-yaml"));
    await userEvent.click(await screen.findByTestId("download-skills-sidecar"));
    expect(await screen.findByTestId("skills-sidecar-error")).toHaveTextContent(
      "nothing to export",
    );
  });

  it("surfaces a rejected clipboard write", async () => {
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: vi.fn().mockRejectedValue(new Error("denied")) },
    });
    renderTemplatePanel();

    await userEvent.click(screen.getByTestId("info-tab-yaml"));
    await userEvent.click(await screen.findByRole("button", { name: "Copy" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Clipboard access was denied. Use Download instead.",
    );
  });

  it("ensures the shared session on open, with no pipeline id", async () => {
    renderTemplatePanel({ initialTab: "assistant" });

    // Create-on-open: mounting the Assistant tab ensures the session. No id and
    // no scope — one assistant serves every template (#594).
    await waitFor(() => expect(openLibraryAssistant).toHaveBeenCalledWith());
    // The resolved session name surfaces in the tab header.
    expect(await screen.findByText("pdo-libassist-shared")).toBeInTheDocument();
  });

  // **The assertion this test used to make, inverted on purpose** (#594). The
  // panel auto-closes on every edit-tab switch (#385), so a reap in the unmount
  // cleanup threw the conversation away each time the user looked at another
  // template. Reaping now lives at App level, keyed on leaving EVERY edit view.
  it("does NOT reap on unmount — closing the panel is not leaving the editor", async () => {
    const { unmount } = renderTemplatePanel({ initialTab: "assistant" });
    await waitFor(() => expect(openLibraryAssistant).toHaveBeenCalled());

    unmount();
    expect(closeLibraryAssistant).not.toHaveBeenCalled();
  });

  it("switching to the Assistant tab starts the session", async () => {
    renderTemplatePanel();
    expect(openLibraryAssistant).not.toHaveBeenCalled();
    await userEvent.click(screen.getByTestId("info-tab-assistant"));
    await waitFor(() => expect(openLibraryAssistant).toHaveBeenCalledWith());
  });

  // The sharing property, seen from the UI: changing the edited template must not
  // remount the tab, because a remount is a fresh `openLibraryAssistant` and a
  // torn-down terminal. Re-rendering with a different `assistantId` used to change
  // the subtree's `key`; it no longer does.
  it("changing the edited template does not restart the assistant", async () => {
    const { rerender } = render(
      <PipelineInfoPanel
        run={null}
        pipeline={makePipeline()}
        libraryPipelines={[]}
        onLibraryChanged={() => {}}
        onClose={() => {}}
        initialTab="assistant"
        assistantId="alpha"
      />,
    );
    await waitFor(() => expect(openLibraryAssistant).toHaveBeenCalledTimes(1));

    rerender(
      <PipelineInfoPanel
        run={null}
        pipeline={makePipeline()}
        libraryPipelines={[]}
        onLibraryChanged={() => {}}
        onClose={() => {}}
        initialTab="assistant"
        assistantId="beta"
      />,
    );

    expect(openLibraryAssistant).toHaveBeenCalledTimes(1);
    expect(closeLibraryAssistant).not.toHaveBeenCalled();
  });
});

describe("PipelineInfoPanel — Diff tab (#748)", () => {
  it("offers a Diff tab for a Run, between Info and Manager", () => {
    renderPanel(makeRun());
    const tabs = screen
      .getAllByTestId(/^info-tab-/)
      .map((t) => t.getAttribute("data-testid"));
    expect(tabs).toEqual(["info-tab-info", "info-tab-diff", "info-tab-repositories", "info-tab-manager", "info-tab-yaml"]);
  });

  it("hides the Diff tab for a template (no Run)", () => {
    renderPanel(null);
    expect(screen.queryByTestId("info-tab-diff")).toBeNull();
  });

  it("no longer renders the collapsible Diff section inside Info", () => {
    renderPanel(makeRun());
    expect(screen.queryByTestId("diff-section")).toBeNull();
    expect(screen.queryByTestId("diff-node-select")).toBeNull();
  });

  it("the Changes stat opens the Diff tab", () => {
    renderPanel(makeRun({ loc: { insertions: 38, deletions: 11, files_changed: 4 } }));
    const link = screen.getByTestId("stat-loc-open-diff");
    expect(link).toHaveTextContent("+38");
    expect(link).toHaveTextContent("−11");
    expect(link).toHaveTextContent("4 files");
    fireEvent.click(link);
    expect(screen.getByTestId("info-tab-diff").className).toContain("border-acc");
    // Info's body is gone: the Diff tab owns the panel now.
    expect(screen.queryByTestId("run-stats")).toBeNull();
  });

  it("counts unread review replies on the Diff tab (#751), replacing the dot, and clears once the Review marked them seen", () => {
    const replied = {
      id: "rc-001",
      path: "a.ts",
      side: "new" as const,
      line: 1,
      from_ref: "fork",
      to_ref: "tip",
      text: "t",
      author: "user",
      sent_at: "2026-07-01T10:00:00.000Z",
      status: "sent" as const,
      replies: [{ author: "manager", text: "done", at: "2026-07-01T10:05:00.000Z" }],
    };
    const delivered = {
      "impl-1": {
        node_id: "impl-1",
        status: "completed" as const,
        iter: 1,
        started_at: "2026-07-01T10:00:00.000Z",
        completed_at: "2026-07-01T10:01:00.000Z",
        failure_reason: null,
        iterations: [],
        delivery: { before: "aaa", after: "bbb" },
      },
    };
    const { rerender } = renderPanel(makeRun());
    rerender(
      <PipelineInfoPanel
        run={makeRun({
          nodes: delivered,
          review_comments: [replied, { ...replied, id: "rc-002", status: "resolved", resolved_by: "xuTJYLUa" }],
        })}
        pipeline={null}
        libraryPipelines={[]}
        onLibraryChanged={() => {}}
        onClose={() => {}}
      />,
    );
    // One open comment with an unread reply; the agent-resolved one does not count.
    // The count replaces the "tip moved" dot.
    expect(screen.getByTestId("diff-tab-unread")).toHaveTextContent("1");
    expect(screen.queryByTestId("diff-tab-dot")).toBeNull();
    // The Review page (another tab) marked it seen → storage event → badge gone, dot back.
    localStorage.setItem("pdo.review.seen.run-abc1234567", JSON.stringify({ "rc-001": 1, "rc-002": 1 }));
    fireEvent(window, new StorageEvent("storage", { key: "pdo.review.seen.run-abc1234567" }));
    expect(screen.queryByTestId("diff-tab-unread")).toBeNull();
    expect(screen.getByTestId("diff-tab-dot")).toBeInTheDocument();
  });

  it("dots the Diff tab when a node delivers while another tab is shown, and clears it on open", () => {
    const { rerender } = renderPanel(makeRun());
    expect(screen.queryByTestId("diff-tab-dot")).toBeNull();
    rerender(
      <PipelineInfoPanel
        run={makeRun({
          nodes: {
            "impl-1": {
              node_id: "impl-1",
              status: "completed",
              iter: 1,
              started_at: "2026-07-01T10:00:00.000Z",
              completed_at: "2026-07-01T10:01:00.000Z",
              failure_reason: null,
              iterations: [],
              delivery: { before: "aaa", after: "bbb" },
            },
          },
        })}
        pipeline={null}
        libraryPipelines={[]}
        onLibraryChanged={() => {}}
        onClose={() => {}}
      />,
    );
    expect(screen.getByTestId("diff-tab-dot")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("info-tab-diff"));
    expect(screen.queryByTestId("diff-tab-dot")).toBeNull();
  });
});

// #752: Repositories is a tab of the Run panel (the second half of #566), and
// what the deleted `RunInfoSidebar` carried besides it lives in the Info header.
describe("PipelineInfoPanel — Repositories tab and Info header (#752)", () => {
  it("offers Info | Diff | Repositories | Manager | YAML for a Run, and no Repositories tab on a template", () => {
    renderPanel(makeRun());
    expect(screen.getByTestId("info-tab-repositories")).toHaveTextContent("Repositories");
    expect(screen.queryByTestId("repositories-tab")).toBeNull();
    fireEvent.click(screen.getByTestId("info-tab-repositories"));
    expect(screen.getByTestId("repositories-tab")).toBeInTheDocument();
    expect(screen.queryByTestId("run-stats")).toBeNull();
  });

  it("hides the Repositories tab for a template (no Run)", () => {
    renderPanel(null);
    expect(screen.queryByTestId("info-tab-repositories")).toBeNull();
  });

  it("lists the Run's repositories in the tab — primary locked, secondaries with their mode — frozen on a terminal Run", () => {
    renderPanel(
      makeRun({
        status: "completed",
        target_repo: "/repos/primary",
        target_repos: [{ repo: "/repos/lib", alias: "lib", sha: "cafebabe1234", base_branch: "main", read_only: true }],
      }),
    );
    fireEvent.click(screen.getByTestId("info-tab-repositories"));
    expect(screen.getByTestId("run-repositories")).toBeInTheDocument();
    expect(screen.getByTestId("primary-repo-row")).toHaveTextContent("/repos/primary");
    expect(screen.getByTestId("secondary-repo-lib")).toHaveTextContent("/repos/lib");
    expect(screen.getByTestId("secondary-repo-mode-lib")).toHaveTextContent("READ-ONLY");
    expect(screen.queryByTestId("remove-secondary-repo-lib")).toBeNull();
    expect(screen.queryByTestId("add-secondary-repo")).toBeNull();
  });

  it("offers add on a live Run and says so when the Run recorded no repository", () => {
    const { unmount } = renderPanel(makeRun({ status: "running", target_repo: "/repos/primary", target_repos: [] }));
    fireEvent.click(screen.getByTestId("info-tab-repositories"));
    expect(screen.getByTestId("add-secondary-repo")).toBeInTheDocument();
    expect(screen.getByTestId("spawn-visibility-note")).toBeInTheDocument();
    unmount();
    renderPanel(makeRun({ status: "running" }));
    fireEvent.click(screen.getByTestId("info-tab-repositories"));
    expect(screen.getByTestId("repositories-tab-empty")).toBeInTheDocument();
    expect(screen.queryByTestId("run-repositories")).toBeNull();
  });

  it("Info states why a failed run failed, and names the terminal (#503)", () => {
    renderPanel(makeRun({ status: "failed", failure_reason: "merge conflict on ship: 20 conflicting file(s)" }));
    const box = screen.getByTestId("run-failure-reason");
    expect(box).toHaveTextContent("Failed");
    expect(box).toHaveTextContent("20 conflicting file(s)");
    expect(screen.queryByTestId("run-awaiting-reason")).toBeNull();
  });

  it("Info states why an incident-parked run awaits the user, not an interactive wait (#598)", () => {
    const { unmount } = renderPanel(
      makeRun({ status: "awaiting_user", awaiting_reason: "session_died: tmux session … no longer exists", awaiting_reason_code: "session_died" } as Partial<RunState>),
    );
    expect(screen.getByTestId("run-awaiting-reason")).toHaveTextContent("Interrupted");
    expect(screen.getByTestId("run-awaiting-reason")).toHaveTextContent("no longer exists");
    unmount();
    renderPanel(makeRun({ status: "awaiting_user" }));
    expect(screen.queryByTestId("run-awaiting-reason")).toBeNull();
    expect(screen.queryByTestId("run-failure-reason")).toBeNull();
  });

  it("Info shows the frozen harness when named (#551) and the editing / archived note (#315)", () => {
    const { unmount } = renderPanel(makeRun({ status: "running", harness: "opencode" } as Partial<RunState>));
    expect(screen.getByTestId("run-harness")).toHaveTextContent("opencode");
    expect(screen.getByTestId("run-info-note")).toHaveTextContent("changes sync to template");
    unmount();
    renderPanel(makeRun({ status: "archived" }));
    expect(screen.queryByTestId("run-harness")).toBeNull();
    expect(screen.getByTestId("run-info-note")).toHaveTextContent("Archived run");
    expect(screen.getByTestId("run-info-note")).toHaveTextContent("read-only");
  });

  it("shows no note, reason or harness on a template", () => {
    renderPanel(null);
    expect(screen.queryByTestId("run-info-note")).toBeNull();
    expect(screen.queryByTestId("run-harness")).toBeNull();
    expect(screen.queryByTestId("run-failure-reason")).toBeNull();
  });
});

/**
 * #803/ADR-0070 §4 — the Source block of the Info tab: where the Run was cut from,
 * its fork point, how far it has drifted, and the ONE explicit gesture that
 * refreshes the remote half.
 */
describe("PipelineInfoPanel — the Source block (#803)", () => {
  const DRIFT = {
    state: "available" as const,
    source_branch: "main",
    fork: "a1b2c3d",
    ahead: 1,
    behind: 3,
    local_behind: 1,
    upstream: "origin/main",
    upstream_behind: 3,
    last_fetch_at: "2026-09-18T10:00:00.000Z",
  };

  it("shows the source, the fork and the drift once the Run answers", async () => {
    fetchSourceDrift.mockResolvedValue(DRIFT);
    renderPanel(makeRun({ target_repo: "/home/me/pdo", source_branch: "main" }));

    const block = await screen.findByTestId("run-source-block");
    expect(block).toHaveTextContent("main");
    expect(block).toHaveTextContent("a1b2c3d");
    const chip = screen.getByTestId("source-drift-chip");
    expect(chip).toHaveTextContent("1↑");
    expect(chip).toHaveTextContent("3↓");
    // The tooltip is where the single `m` splits into its two sides.
    expect(chip.getAttribute("title")).toContain("main +1 · origin/main +3");
  });

  /**
   * ADR-0070 §1: opening a Run must not put traffic on someone's repository. The
   * READ of the drift is local; the fetch happens only when the button is pressed.
   */
  it("never fetches on display — only the button does", async () => {
    fetchSourceDrift.mockResolvedValue(DRIFT);
    fetchRemotes.mockResolvedValue({ branches: [], last_fetch_at: null, fetch_error: null });
    renderPanel(makeRun({ target_repo: "/home/me/pdo", source_branch: "main" }));

    await screen.findByTestId("run-source-block");
    expect(fetchRemotes).not.toHaveBeenCalled();

    fireEvent.click(screen.getByTestId("run-source-fetch"));
    await waitFor(() => expect(fetchRemotes).toHaveBeenCalledWith("/home/me/pdo"));
    // And it recomputes afterwards: a fetch nobody re-read would be a spinner.
    await waitFor(() => expect(fetchSourceDrift).toHaveBeenCalledTimes(2));
  });

  it("degrades the source side to `?` when the fetch fails, keeping the Run's own count", async () => {
    fetchSourceDrift.mockResolvedValue(DRIFT);
    fetchRemotes.mockResolvedValue({
      branches: [],
      last_fetch_at: null,
      fetch_error: { kind: "network", message: "could not resolve host" },
    });
    renderPanel(makeRun({ target_repo: "/home/me/pdo", source_branch: "main" }));
    await screen.findByTestId("run-source-block");

    fireEvent.click(screen.getByTestId("run-source-fetch"));
    await waitFor(() =>
      expect(screen.getByTestId("source-drift-chip")).toHaveAttribute("data-drift", "unknown"),
    );
    const chip = screen.getByTestId("source-drift-chip");
    expect(chip).toHaveTextContent("1↑");
    expect(chip).toHaveTextContent("?");
    expect(chip.getAttribute("title")).toContain("could not resolve host");
  });

  /**
   * An archived Run whose branch was cleaned up. Grey, named, and the fetch button
   * gone — there is nothing left to refresh, so offering the gesture would be a
   * button that cannot change anything.
   */
  it("says unavailable and drops the fetch button once the Run's branch is gone", async () => {
    fetchSourceDrift.mockResolvedValue({ state: "unavailable", reason: "branch deleted" });
    renderPanel(makeRun({ status: "archived", target_repo: "/home/me/pdo" }));

    await screen.findByTestId("run-source-block");
    expect(screen.getByTestId("source-drift-chip")).toHaveTextContent("unavailable");
    expect(screen.getByTestId("run-source-unavailable")).toHaveTextContent("branch deleted");
    expect(screen.queryByTestId("run-source-fetch")).not.toBeInTheDocument();
  });

  it("shows no Source block on a template", () => {
    renderPanel(null);
    expect(screen.queryByTestId("run-source-block")).not.toBeInTheDocument();
    expect(fetchSourceDrift).not.toHaveBeenCalled();
  });
});
