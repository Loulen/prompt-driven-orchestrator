import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useState } from "react";
import type { ProvisioningRules } from "../types";
import { previewProvisioning } from "../api";
import ProvisioningRulesEditor from "./ProvisioningRulesEditor";

vi.mock("../api", async () => {
  const actual = await vi.importActual<typeof import("../api")>("../api");
  return { ...actual, previewProvisioning: vi.fn() };
});

describe("ProvisioningRulesEditor", () => {
  beforeEach(() => {
    vi.mocked(previewProvisioning).mockResolvedValue({
      entries: [],
      rules: [],
      conflicts: [
        { scope: "run", relative_path: ".env", modes: ["copy", "symlink"] },
      ],
    });
  });

  it("edits three mode lists and surfaces a start-blocking conflict", async () => {
    const onChange = vi.fn();
    const onValidityChange = vi.fn();
    function Harness() {
      const [rules, setRules] = useState<ProvisioningRules>({
        copy: [],
        hardlink: [],
        symlink: [],
      });
      return (
        <ProvisioningRulesEditor
          level="run"
          repository="/repo"
          rules={rules}
          onChange={(next) => {
            onChange(next);
            setRules(next);
          }}
          onValidityChange={onValidityChange}
        />
      );
    }
    render(
      <Harness />,
    );

    await userEvent.type(screen.getByLabelText("Copy patterns"), ".env");

    expect(onChange).toHaveBeenCalledWith({
      copy: [".env"],
      hardlink: [],
      symlink: [],
    });
    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent(
        "Mode conflict in Run — .env",
      ),
    );
    expect(onValidityChange).toHaveBeenLastCalledWith(false);
  });

  it("shows inherited rules, grouped exclusions, and the frozen state", async () => {
    vi.mocked(previewProvisioning).mockResolvedValue({
      entries: [
        {
          relative_path: "fixtures/a.bin",
          mode: "copy",
          origin_scope: "instance",
          pattern: "fixtures/",
          provided_by_git: false,
        },
      ],
      rules: [
        {
          scope: "instance",
          mode: "copy",
          pattern: "fixtures/",
          paths: ["fixtures/a.bin"],
          excluded_paths: [
            {
              relative_path: "fixtures/private.bin",
              excluded_by_scope: "isolated_node",
            },
          ],
          unmatched: false,
        },
      ],
      conflicts: [],
    });

    render(
      <ProvisioningRulesEditor
        level="isolated_node"
        repository="/repo"
        rules={{ copy: [], hardlink: [], symlink: [] }}
        onChange={() => {}}
        readOnly
        frozenAt="09:12"
      />,
    );

    await waitFor(() => expect(screen.getByText("fixtures/")).toBeInTheDocument());
    expect(screen.getByText("Instance · 2")).toBeInTheDocument();
    expect(screen.getByText(/frozen at 09:12 · reused on restart/)).toBeInTheDocument();
    expect(screen.getByLabelText("Copy patterns")).toHaveAttribute("readonly");
    await userEvent.click(screen.getByText(/fixtures\/ · Instance · copy · 2/));
    expect(screen.getByText(/fixtures\/private.bin · excluded by Node/)).toHaveClass(
      "line-through",
    );
  });

  it("stacks mode lists until its own container is wide enough for three columns", () => {
    render(
      <ProvisioningRulesEditor
        level="run"
        repository=""
        rules={{ copy: [], hardlink: [], symlink: [] }}
        onChange={() => {}}
      />,
    );

    expect(screen.getByTestId("provisioning-mode-grid")).toHaveClass(
      "grid-cols-1",
      "@[520px]:grid-cols-3",
    );
  });
});
