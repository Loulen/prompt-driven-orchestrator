import { render, screen, fireEvent, act } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import type { PortIO } from "../../api/nodes";

const fetchArtifactMock = vi.fn();
vi.mock("../../api/artifacts", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../api/artifacts")>()),
  fetchArtifact: (...args: unknown[]) => fetchArtifactMock(...args),
}));

import PortRow, { FrontmatterChips } from "./PortRow";

function preview(exists: boolean): PortIO {
  return {
    port: "preview",
    repeated: false,
    port_type: "html",
    files: [
      {
        path: "worker/iter-1/preview/output.html",
        exists,
        size: exists ? 120 : null,
        frontmatter: null,
      },
    ],
  };
}

function renderRow(port: PortIO, onOpen = vi.fn()) {
  render(
    <>
      <PortRow port={port} kind="output" runId="run-1" nodeId="worker" onOpen={onOpen} />
    </>,
  );
  return onOpen;
}

describe("PortRow — an html output", () => {
  beforeEach(() => {
    fetchArtifactMock.mockReset();
    fetchArtifactMock.mockResolvedValue("<p>step 2</p>");
  });

  it("an absent one keeps its path, with nothing to open or copy", () => {
    renderRow(preview(false));
    expect(screen.getByText("worker/iter-1/preview/output.html")).toBeInTheDocument();
    expect(screen.queryByTestId("port-row-open")).not.toBeInTheDocument();
    expect(screen.queryByTestId("port-copy")).not.toBeInTheDocument();
  });

  it("a written one opens in the viewer and copies its raw content", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    const onOpen = renderRow(preview(true));

    expect(screen.getByText("worker/iter-1/preview/output.html")).toBeInTheDocument();

    fireEvent.click(screen.getByTestId("port-row-open"));
    expect(onOpen).toHaveBeenCalledTimes(1);

    await act(async () => {
      fireEvent.click(screen.getByTestId("port-copy"));
    });
    expect(fetchArtifactMock).toHaveBeenCalledWith("run-1", "worker/iter-1/preview/output.html");
    expect(writeText).toHaveBeenCalledWith("<p>step 2</p>");
  });
});

function exports(paths: string[]): PortIO {
  return {
    port: "exports",
    repeated: false,
    port_type: "files",
    files: paths.map((path) => ({ path, exists: true, size: 2048, frontmatter: null })),
  };
}

describe("PortRow — downloads", () => {
  it("a files port lists its files, each one downloading, and zips them all", () => {
    renderRow(exports(["worker/iter-1/exports/report.pdf", "worker/iter-1/exports/rows.csv"]));
    expect(screen.getByTestId("port-type-badge")).toHaveTextContent("files");
    expect(screen.getByText("2 files · report.pdf, rows.csv")).toBeInTheDocument();

    const chips = screen.getAllByTestId("port-file-chip");
    expect(chips).toHaveLength(2);
    expect(chips[0]).toHaveTextContent("PDF");
    expect(chips[0]).toHaveTextContent("report.pdf");
    expect(chips[0]).toHaveAttribute("download");
    expect(chips[0].getAttribute("href")).toBe(
      "/runs/run-1/artifact?path=worker%2Fiter-1%2Fexports%2Freport.pdf&download=true",
    );

    const all = screen.getByTestId("port-download-all");
    expect(all).toHaveTextContent("Download all");
    expect(all.getAttribute("href")).toBe(
      "/runs/run-1/artifact/zip?path=worker%2Fiter-1%2Fexports",
    );
    // No single-file download on the row: the chips and « Download all » are it.
    expect(screen.queryByTestId("port-download")).not.toBeInTheDocument();
  });

  it("an empty files port has nothing to download", () => {
    renderRow(exports([]));
    expect(screen.getByText("no file yet")).toBeInTheDocument();
    expect(screen.queryByTestId("port-download-all")).not.toBeInTheDocument();
  });

  it("a markdown output downloads its file", () => {
    renderRow({ ...preview(true), port: "out", port_type: "markdown" });
    const link = screen.getByTestId("port-download");
    expect(link).toHaveAttribute("download");
    expect(link.getAttribute("href")).toBe(
      "/runs/run-1/artifact?path=worker%2Fiter-1%2Fpreview%2Foutput.html&download=true",
    );
  });

  it("an image list downloads as the zip of its folder", () => {
    renderRow({
      ...exports(["worker/iter-1/shots/a.png", "worker/iter-1/shots/b.png"]),
      port_type: "image_list",
    });
    expect(screen.getByTestId("port-download").getAttribute("href")).toBe(
      "/runs/run-1/artifact/zip?path=worker%2Fiter-1%2Fshots",
    );
  });

  it("a port with nothing on disk offers no download", () => {
    renderRow(preview(false));
    expect(screen.queryByTestId("port-download")).not.toBeInTheDocument();
  });
});

describe("FrontmatterChips", () => {
  it("shows the verdict alone with a count of the other keys, every key in the tooltip", () => {
    render(<FrontmatterChips values={{ score: 7, status: "draft", verdict: "PASS" }} />);
    const card = screen.getByTestId("frontmatter-card");
    const chips = card.querySelectorAll(".ri-fm__chip");
    expect(chips).toHaveLength(2);
    expect(chips[0]).toHaveTextContent("verdictPASS");
    expect(chips[0]).toHaveAttribute("data-st", "done");
    expect(chips[1]).toHaveTextContent("+2");
    expect(card).toHaveAttribute("title", "score: 7\nstatus: draft\nverdict: PASS");
  });

  it("a single key shows alone, without a count", () => {
    render(<FrontmatterChips values={{ score: 7 }} />);
    const chips = screen.getByTestId("frontmatter-card").querySelectorAll(".ri-fm__chip");
    expect(chips).toHaveLength(1);
    expect(chips[0]).toHaveTextContent("score7");
  });
});
