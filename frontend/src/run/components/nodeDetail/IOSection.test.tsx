/**
 * The I/O tab's sections: the Outputs as cards, the node's deliverables first;
 * the Inputs as compact rows. Either opens its port from anywhere on it.
 */
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { PortIO } from "../../api/nodes";
import IOSection from "./IOSection";

const report: PortIO = {
  port: "report",
  repeated: false,
  port_type: "markdown",
  files: [{ path: "worker/iter-1/report/output.md", exists: true, size: 900, frontmatter: { verdict: "fail", url: "https://example.com/pr/1" } }],
};
const notes: PortIO = {
  port: "notes",
  repeated: false,
  port_type: "markdown",
  files: [{ path: "worker/iter-1/notes/output.md", exists: false, size: null, frontmatter: null }],
};

describe("IOSection", () => {
  it("shows the outputs as cards with their whole frontmatter, and opens one from anywhere", () => {
    const onOpenFile = vi.fn();
    render(<IOSection title="Outputs" kind="output" ports={[report, notes]} runId="r1" nodeId="worker" showFrontmatter onOpenFile={onOpenFile} />);
    const rows = screen.getAllByTestId("port-row");
    expect(rows.map((r) => r.classList.contains("ri-port--card"))).toEqual([true, true]);
    expect(rows[0]).toHaveAttribute("data-st", "failed");
    expect(rows[1]).toHaveAttribute("data-pending");
    expect(screen.getAllByTestId("frontmatter-value").map((v) => v.textContent)).toEqual(["fail", "https://example.com/pr/1"]);
    fireEvent.click(screen.getByText("verdict"));
    expect(onOpenFile).toHaveBeenCalledWith("report", report.files, "markdown");
  });

  it("keeps the inputs compact", () => {
    const onOpenFile = vi.fn();
    render(<IOSection title="Inputs" kind="input" ports={[report]} runId="r1" nodeId="worker" onOpenFile={onOpenFile} />);
    const row = screen.getByTestId("port-row");
    expect(row).not.toHaveClass("ri-port--card");
    expect(screen.queryByTestId("frontmatter-card")).toBeNull();
    fireEvent.click(screen.getByText("worker/iter-1/report/output.md"));
    expect(onOpenFile).toHaveBeenCalledWith("report", report.files, "markdown");
  });
});
