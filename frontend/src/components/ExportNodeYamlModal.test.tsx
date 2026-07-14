import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import ExportNodeYamlModal from "./ExportNodeYamlModal";
import type { NodeDef } from "../types";

function node(overrides: Partial<NodeDef> = {}): NodeDef {
  return {
    id: "n1abc",
    name: "Writer",
    type: "doc-only",
    inputs: [],
    outputs: [{ name: "adr", repeated: false, side: "right" }],
    interactive: false,
    view: { x: 0, y: 0 },
    ...overrides,
  };
}

describe("ExportNodeYamlModal (#345)", () => {
  it("renders the node's YAML with a block-scalar prompt and no id", () => {
    render(
      <ExportNodeYamlModal node={node()} prompt={"Do the thing.\nCarefully."} onClose={() => {}} />,
    );
    const pre = screen.getByTestId("export-node-yaml");
    expect(pre.textContent).toContain("name: Writer");
    expect(pre.textContent).toContain("prompt: |");
    expect(pre.textContent).toContain("Do the thing.");
    expect(pre.textContent).not.toContain("n1abc"); // id omitted
  });

  it("Copy writes the YAML to the clipboard and flashes Copied!", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    render(<ExportNodeYamlModal node={node()} prompt="Hello." onClose={() => {}} />);
    fireEvent.click(screen.getByTestId("export-node-copy"));
    await waitFor(() => expect(writeText).toHaveBeenCalledTimes(1));
    expect(writeText.mock.calls[0][0]).toContain("name: Writer");
    expect(await screen.findByText("Copied!")).toBeInTheDocument();
  });

  it("Download builds a blob and clicks an anchor with a slugified .yaml name", () => {
    const createURL = vi.fn().mockReturnValue("blob:fake");
    const revoke = vi.fn();
    (URL as unknown as { createObjectURL: unknown }).createObjectURL = createURL;
    (URL as unknown as { revokeObjectURL: unknown }).revokeObjectURL = revoke;
    let downloadName = "";
    const clickSpy = vi
      .spyOn(HTMLAnchorElement.prototype, "click")
      .mockImplementation(function (this: HTMLAnchorElement) {
        downloadName = this.download;
      });
    render(<ExportNodeYamlModal node={node({ name: "My Node" })} prompt="p" onClose={() => {}} />);
    fireEvent.click(screen.getByTestId("export-node-download"));
    expect(createURL).toHaveBeenCalledTimes(1);
    expect(clickSpy).toHaveBeenCalledTimes(1);
    expect(downloadName).toBe("my-node.yaml");
    clickSpy.mockRestore();
  });

  it("closes on the Close button and on Escape", () => {
    const onClose = vi.fn();
    render(<ExportNodeYamlModal node={node()} prompt="p" onClose={onClose} />);
    fireEvent.click(screen.getByTestId("export-node-close"));
    expect(onClose).toHaveBeenCalledTimes(1);
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(2);
  });
});
