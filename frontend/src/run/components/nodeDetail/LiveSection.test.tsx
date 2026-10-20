import { render, screen, fireEvent, within } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import type { LiveArtifactIO } from "../../../shared/generated/LiveArtifactIO";

vi.mock("../../api/artifacts", () => ({
  artifactUrl: (runId: string, path: string) => `/runs/${runId}/artifact?path=${encodeURIComponent(path)}`,
  artifactDownloadUrl: (runId: string, path: string) =>
    `/runs/${runId}/artifact?path=${encodeURIComponent(path)}&download=true`,
  portZipUrl: (runId: string, folder: string) =>
    `/runs/${runId}/artifact/zip?path=${encodeURIComponent(folder)}`,
}));

import LiveSection from "./LiveSection";

const progress: LiveArtifactIO = {
  name: "progress",
  port_type: "markdown",
  declared: true,
  path: "designer/iter-1/live/progress.md",
  files: [
    {
      path: "designer/iter-1/live/progress.md",
      exists: true,
      size: 3400,
      frontmatter: { status: "drafting", step: "three of five" },
    },
  ],
  schema_warnings: [{ field: "step", reason: "expected int, got string" }],
  updated_at: new Date(Date.now() - 20_000).toISOString(),
};

const screens: LiveArtifactIO = {
  name: "draft-screens",
  port_type: "image_list",
  declared: true,
  path: "designer/iter-1/live/draft-screens/",
  files: [
    { path: "designer/iter-1/live/draft-screens/a.png", exists: true, size: 200, frontmatter: null },
    { path: "designer/iter-1/live/draft-screens/b.png", exists: true, size: 300, frontmatter: null },
  ],
  schema_warnings: [],
  updated_at: new Date(Date.now() - 60_000).toISOString(),
};

const notes: LiveArtifactIO = {
  name: "notes-for-builder",
  port_type: "markdown",
  declared: false,
  path: "designer/iter-1/live/notes-for-builder.md",
  files: [
    { path: "designer/iter-1/live/notes-for-builder.md", exists: true, size: 600, frontmatter: null },
  ],
  schema_warnings: [],
  updated_at: new Date(Date.now() - 90_000).toISOString(),
};

const risks: LiveArtifactIO = {
  name: "risks",
  port_type: "markdown",
  declared: true,
  path: "designer/iter-1/live/risks.md",
  files: [],
  schema_warnings: [],
  updated_at: null,
};

function row(name: string) {
  return screen.getAllByTestId("live-row").find((r) => r.dataset.live === name)!;
}

describe("LiveSection (ADR-0089)", () => {
  it("lists the live artifacts with their count and the newest update", () => {
    render(<LiveSection live={[progress, screens, notes, risks]} runId="run-1" onOpen={vi.fn()} />);
    expect(screen.getByText("Live artifacts")).toBeInTheDocument();
    expect(screen.getByText("4")).toBeInTheDocument();
    expect(screen.getByTestId("live-updated")).toHaveTextContent("updated 20 s ago");
    expect(within(row("progress")).getByText("live/progress.md")).toBeInTheDocument();
    expect(within(row("draft-screens")).getByText("live/draft-screens/ · 2 images")).toBeInTheDocument();
    expect(within(row("draft-screens")).getByTestId("live-thumbnails").children).toHaveLength(2);
  });

  it("shows the frontmatter as tags, the field off schema in amber with its rule", () => {
    render(<LiveSection live={[progress]} runId="run-1" onOpen={vi.fn()} />);
    const tags = within(row("progress")).getAllByTestId("live-tag");
    expect(tags).toHaveLength(1);
    expect(tags[0]).toHaveTextContent("statusdrafting");
    const warning = within(row("progress")).getByTestId("live-tag-warning");
    expect(warning).toHaveTextContent("step three of five");
    expect(warning).toHaveAttribute("title", "Schema: expected int, got string");
  });

  it("names a declared field that is absent", () => {
    const missing = {
      ...progress,
      files: [{ ...progress.files[0], frontmatter: { step: 3 } }],
      schema_warnings: [{ field: "status", reason: "missing required field" }],
    };
    render(<LiveSection live={[missing]} runId="run-1" onOpen={vi.fn()} />);
    expect(screen.getByTestId("live-tag-warning")).toHaveTextContent("status missing");
  });

  it("badges a free artifact, greys a declared one not produced", () => {
    render(<LiveSection live={[notes, risks]} runId="run-1" onOpen={vi.fn()} />);
    expect(within(row("notes-for-builder")).getByTestId("live-free-badge")).toHaveTextContent("free");
    expect(row("risks").dataset.produced).toBe("false");
    expect(within(row("risks")).getByTestId("live-not-produced")).toHaveTextContent("not produced");
    expect(within(row("risks")).queryByTestId("live-free-badge")).not.toBeInTheDocument();
    expect(within(row("risks")).queryByTestId("live-row-open")).not.toBeInTheDocument();
  });

  it("opens a produced artifact and downloads its file", () => {
    const onOpen = vi.fn();
    render(<LiveSection live={[progress]} runId="run-1" onOpen={onOpen} />);
    fireEvent.click(screen.getByTestId("live-row-open"));
    expect(onOpen).toHaveBeenCalledWith(progress);
    expect(screen.getByTestId("live-download")).toHaveAttribute(
      "href",
      `/runs/run-1/artifact?path=${encodeURIComponent("designer/iter-1/live/progress.md")}&download=true`,
    );
  });

  it("downloads a folder artifact as the zip of its folder", () => {
    render(<LiveSection live={[screens]} runId="run-1" onOpen={vi.fn()} />);
    expect(screen.getByTestId("live-download")).toHaveAttribute(
      "href",
      `/runs/run-1/artifact/zip?path=${encodeURIComponent("designer/iter-1/live/draft-screens")}`,
    );
  });

  it("lists the files of a files artifact by name", () => {
    const exports: LiveArtifactIO = {
      ...screens,
      name: "exports",
      port_type: "files",
      path: "designer/iter-1/live/exports/",
      files: [
        { path: "designer/iter-1/live/exports/report.pdf", exists: true, size: 200, frontmatter: null },
        { path: "designer/iter-1/live/exports/rows.csv", exists: true, size: 300, frontmatter: null },
      ],
    };
    render(<LiveSection live={[exports]} runId="run-1" onOpen={vi.fn()} />);
    expect(screen.getByText(/2 files · report\.pdf, rows\.csv/)).toBeInTheDocument();
    expect(screen.queryByTestId("live-thumbnails")).not.toBeInTheDocument();
  });
});

describe("LiveSection — inherited files (#796)", () => {
  it("reads a declared artifact whose only file predates the execution as not produced", () => {
    const stale = {
      ...progress,
      files: [{ ...progress.files[0], inherited: true }],
    };
    render(<LiveSection live={[stale]} runId="run-1" onOpen={vi.fn()} />);
    expect(row("progress").dataset.produced).toBe("false");
  });
});
