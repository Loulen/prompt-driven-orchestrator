import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { PipelineVersionEntry } from "../../shared/generated/PipelineVersionEntry";
import VersionsTab from "./VersionsTab";
import VersionBanner from "./VersionBanner";
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
    expect(last.querySelector(".italic")).not.toBeNull();
    expect(screen.getByTestId("version-row-2")).toHaveTextContent("4 runs");
    expect(screen.getByTestId("version-row-1")).toHaveTextContent("1 run");
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
      "Viewing v5 · Split designer prompt, read-only · saved by Théo Bernard on Oct 14",
    );
    fireEvent.click(screen.getByTestId("version-banner-restore"));
    await waitFor(() => expect(restoreVersion).toHaveBeenCalledWith("flow", 5));
  });
});
