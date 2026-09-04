import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { RunState, PipelineDef } from "../types";

// The badge/banner live in the InfoTab header, above DiffSection. Mock the heavy
// children (network-fetching diff, tmux terminal) so the test stays focused on the
// #410 sandbox surface and never touches the network.
vi.mock("./DiffSection", () => ({ default: () => null }));
vi.mock("./TmuxTerminal", () => ({ default: () => null }));

// #302 / ADR-0048: the Assistant tab drives create-if-absent / reap-on-leave
// against the daemon. Mock those two api helpers so the tests can assert the
// lifecycle without a network or a tmux session.
const {
  openLibraryAssistant,
  closeLibraryAssistant,
  fetchPipelineDocument,
  fetchRunPipelineDocument,
  fetchPipelineSkillsSidecar,
} = vi.hoisted(() => ({
  openLibraryAssistant: vi.fn(),
  closeLibraryAssistant: vi.fn(),
  fetchPipelineDocument: vi.fn(),
  fetchRunPipelineDocument: vi.fn(),
  fetchPipelineSkillsSidecar: vi.fn(),
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
  };
});

import PipelineInfoPanel from "./PipelineInfoPanel";
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
