import { render, screen, fireEvent, act } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import type { PortIO } from "../../api/nodes";

const fetchArtifactMock = vi.fn();
vi.mock("../../api/artifacts", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../api/artifacts")>()),
  fetchArtifact: (...args: unknown[]) => fetchArtifactMock(...args),
}));

import PortRow from "./PortRow";

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

  it("an absent one stays quiet, with nothing to open or copy", () => {
    renderRow(preview(false));
    expect(screen.queryByText("worker/iter-1/preview/output.html")).not.toBeInTheDocument();
    expect(screen.getByTestId("port-type-badge")).toHaveTextContent("html");
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

describe("PortRow — a long name", () => {
  it("is whole in the tooltip, written or not", () => {
    const name = "a".repeat(200);
    renderRow({ ...preview(true), port: name });
    expect(screen.getByTestId("port-row-open")).toHaveAttribute("title", name);
  });

  it("is whole in the tooltip of a port not written yet", () => {
    const name = `${"long_output_name_".repeat(12)}end`;
    renderRow({ ...preview(false), port: name });
    expect(screen.getByText(name)).toHaveAttribute("title", name);
  });
});

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
    expect(screen.getByTestId("port-row")).toHaveAttribute("data-pending");
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

function review(frontmatter: Record<string, unknown> | null, exists = true): PortIO {
  return {
    port: "review",
    repeated: false,
    port_type: "markdown",
    files: [{ path: "worker/iter-2/review/output.md", exists, size: exists ? 6400 : null, frontmatter }],
  };
}

const PR = "https://github.com/acme/app/compare/main...feature/x";

function renderOutput(port: PortIO, onOpen = vi.fn()) {
  render(<PortRow port={port} kind="output" runId="run-1" nodeId="worker" showFrontmatter onOpen={onOpen} />);
  return onOpen;
}

describe("PortRow — an output card", () => {
  it("shows its whole frontmatter, its edge in its verdict's tone", () => {
    renderOutput(review({ verdict: "pass", blocking_findings: 0, PR_link: PR }));
    const row = screen.getByTestId("port-row");
    expect(row).toHaveAttribute("data-st", "done");
    const values = screen.getAllByTestId("frontmatter-value").map((v) => [v.dataset.field, v.textContent]);
    expect(values).toEqual([
      ["verdict", "pass"],
      ["blocking_findings", "0"],
      ["PR_link", PR],
    ]);
    expect(screen.getByTestId("frontmatter-open")).toHaveAttribute("href", PR);
    // Its tools are there at rest: copy, download, open.
    expect(screen.getByTestId("port-copy")).toBeInTheDocument();
    expect(screen.getByTestId("port-download")).toBeInTheDocument();
  });

  it("opens from anywhere on it, its controls keeping their own action", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    const onOpen = renderOutput(review({ verdict: "pass", PR_link: PR }));
    fireEvent.click(screen.getByText("worker/iter-2/review/output.md"));
    expect(onOpen).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByText("PR_link"));
    expect(onOpen).toHaveBeenCalledTimes(2);
    fireEvent.click(screen.getByTestId("port-row-open"));
    expect(onOpen).toHaveBeenCalledTimes(3);
    // A value copies, ↗ opens the address: neither opens the artifact.
    await act(async () => {
      fireEvent.click(screen.getByText(PR));
    });
    expect(writeText).toHaveBeenCalledWith(PR);
    fireEvent.click(screen.getByTestId("frontmatter-open"));
    expect(onOpen).toHaveBeenCalledTimes(3);
  });

  it("says « not produced yet » only on hover, and does not open", () => {
    const onOpen = renderOutput(review(null, false));
    const row = screen.getByTestId("port-row");
    expect(row).toHaveAttribute("data-pending");
    expect(screen.queryByText("not produced yet")).not.toBeInTheDocument();
    fireEvent.mouseEnter(row);
    expect(screen.getByTestId("port-not-produced")).toHaveTextContent("not produced yet");
    fireEvent.mouseLeave(row);
    expect(screen.queryByText("not produced yet")).not.toBeInTheDocument();
    fireEvent.click(row);
    expect(onOpen).not.toHaveBeenCalled();
    expect(row.querySelectorAll("button")).toHaveLength(0);
  });

  it("shows no frontmatter for a previous execution's file", () => {
    const port = review({ verdict: "pass" });
    port.files[0].inherited = true;
    renderOutput(port);
    expect(screen.queryByTestId("frontmatter-card")).not.toBeInTheDocument();
    expect(screen.getByTestId("port-row")).not.toHaveAttribute("data-st");
  });
});

describe("PortRow — an input row", () => {
  it("opens from anywhere on its line", () => {
    const onOpen = vi.fn();
    render(<PortRow port={review(null)} kind="input" runId="run-1" nodeId="worker" onOpen={onOpen} />);
    expect(screen.getByTestId("port-row")).not.toHaveClass("ri-port--card");
    fireEvent.click(screen.getByText("worker/iter-2/review/output.md"));
    expect(onOpen).toHaveBeenCalledTimes(1);
  });
});
