import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { PipelineVersionEntry } from "../../shared/generated/PipelineVersionEntry";
import VersionsTab from "./VersionsTab";
import VersionBanner, { VersionLine } from "./VersionBanner";
import { INITIAL_DRAFT_STATE } from "../store/draftSession";
import type { OpenPipeline } from "../store/editState";
import type { RunState } from "../../shared/generated/RunState";
import { useEditStore } from "../store/editStore";

vi.mock("../api/versions", () => ({ fetchPipelineVersions: vi.fn(), fetchPipelineVersion: vi.fn() }));
vi.mock("../../shared/actors/store", () => ({ useActorDisplay: () => ({ name: "Théo Bernard", avatar_url: null }) }));
const { fetchPipelineVersions } = await import("../api/versions");

const alice = { kind: "identified" as const, namespace: "test", id: "theo" };

function version(number: number, title: string, patch: Partial<PipelineVersionEntry> = {}): PipelineVersionEntry {
  return { number, title, author: alice, saved_at: "2026-10-14T10:00:00Z", external: false, runs: 0, ...patch };
}

beforeEach(() => {
  vi.mocked(fetchPipelineVersions).mockResolvedValue([
    version(3, "external edit", { author: null, external: true }),
    version(2, "Add reviewer loop", { runs: 4 }),
    version(1, "v1", { runs: 1 }),
  ]);
  useEditStore.setState({
    pipelines: [
      {
        id: "flow",
        name: "flow",
        scope: "instance",
        path: "/p/flow.yaml",
        node_count: 3,
        modified: null,
        variables: {},
        prompt_required: false,
        validation: { kind: "all" },
        version: { number: 3, title: "external edit" },
        draft: { changes: 2, nodes: 3, participants: [] },
      },
    ],
  });
});

afterEach(() => useEditStore.setState({ pipelines: [], openTabs: [], activeTabId: null }));

describe("VersionsTab (ADR-0088)", () => {
  it("lists the draft, then the versions newest first, the last one marked", async () => {
    render(<VersionsTab pipelineId="flow" />);
    expect(screen.getByTestId("versions-draft")).toHaveTextContent("Draft2 changes since v3");
    const last = await screen.findByTestId("version-row-3");
    expect(last).toHaveTextContent("external edit");
    expect(last).toHaveTextContent("last");
    expect(last).toHaveTextContent("file");
    expect(last.querySelector("[data-ext]")).toHaveTextContent("external edit");
    expect(screen.getByTestId("version-runs-2")).toHaveTextContent("4");
    expect(screen.getByTestId("version-runs-2")).toHaveAttribute("title", "4 runs");
    expect(screen.getByTestId("version-runs-1")).toHaveAttribute("title", "1 run");
    expect(screen.getByTestId("version-runs-3")).toHaveTextContent("");
    expect(screen.getAllByTestId("version-last")).toHaveLength(1);
  });

  it("views a version read only and restores one into the draft", async () => {
    const openPipelineVersion = vi.fn();
    const restoreVersion = vi.fn().mockResolvedValue(undefined);
    useEditStore.setState({ openPipelineVersion, restoreVersion });
    render(<VersionsTab pipelineId="flow" />);
    fireEvent.click(await screen.findByTestId("version-view-2"));
    expect(openPipelineVersion).toHaveBeenCalledWith("flow", 2);
    fireEvent.click(screen.getByTestId("version-restore-1"));
    await waitFor(() => expect(restoreVersion).toHaveBeenCalledWith("flow", 1));
  });
});

describe("VersionBanner (ADR-0088)", () => {
  it("says which version is shown, by whom, and restores it", async () => {
    const restoreVersion = vi.fn().mockResolvedValue(undefined);
    useEditStore.setState({ restoreVersion });
    render(
      <VersionBanner
        version={{ pipelineId: "flow", number: 5, title: "Split designer prompt", author: alice, savedAt: "2026-10-14T10:00:00Z" }}
      />,
    );
    expect(screen.getByTestId("version-banner")).toHaveTextContent(
      "v5Split designer promptread onlyTBThéo BernardOct 14Restore into draft",
    );
    fireEvent.click(screen.getByTestId("version-banner-restore"));
    await waitFor(() => expect(restoreVersion).toHaveBeenCalledWith("flow", 5));
  });
});

describe("VersionsTab — a Replace", () => {
  it("reads the history again when a Save lands under the same number and title", async () => {
    render(<VersionsTab pipelineId="flow" />);
    await screen.findByTestId("version-row-3");
    const before = vi.mocked(fetchPipelineVersions).mock.calls.length;
    act(() => useEditStore.setState({ lastSavedAt: { flow: Date.now() } }));
    await waitFor(() => expect(vi.mocked(fetchPipelineVersions).mock.calls.length).toBe(before + 1));
  });
});

describe("VersionsTab — states", () => {
  it("says it loads, then offers a retry when the history does not come", async () => {
    vi.mocked(fetchPipelineVersions).mockRejectedValueOnce(new Error("boom"));
    render(<VersionsTab pipelineId="flow" />);
    expect(screen.getByText("Loading versions")).toBeInTheDocument();
    expect(await screen.findByTestId("versions-error")).toHaveTextContent("Versions unavailable");
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(screen.queryByTestId("versions-error")).toBeNull();
    expect(screen.getByText("Loading versions")).toBeInTheDocument();
    expect(await screen.findByTestId("version-row-3")).toBeInTheDocument();
  });
});

describe("VersionLine (ADR-0088)", () => {
  function tab(patch: Partial<OpenPipeline>): OpenPipeline {
    return {
      id: "flow",
      scope: "instance",
      pipeline: { name: "flow", version: "1", variables: {}, nodes: [], edges: [] },
      prompts: {},
      diagnostics: [],
      dirty: false,
      externalDirty: false,
      ...patch,
    };
  }

  it("says the draft's changes since the last version, nothing when there are none", () => {
    const draft = { ...INITIAL_DRAFT_STATE, synced: true, canWrite: true, changes: 3 };
    const { unmount } = render(<VersionLine tab={tab({ draft })} run={null} />);
    expect(screen.getByTestId("version-line-draft")).toHaveTextContent("Draft3 changes since v3Versions");
    unmount();
    const { container } = render(<VersionLine tab={tab({ draft: { ...draft, changes: 0 } })} run={null} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("says a Run launched an older version than the pipeline's last", () => {
    const run = { run_id: "r1", pipeline_name: "flow", pipeline_id: "flow", pipeline_version: { number: 2, title: "Add reviewer loop" } } as RunState;
    render(<VersionLine tab={tab({ id: "__run__r1", runId: "r1" })} run={run} />);
    expect(screen.getByTestId("version-line-launched")).toHaveTextContent("Launchedv2 · Add reviewer loopv3 is outOpen pipeline");
  });
});
