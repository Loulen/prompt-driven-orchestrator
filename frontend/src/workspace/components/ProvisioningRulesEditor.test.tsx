import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useState } from "react";
import type { ProvisioningPlan } from "../../shared/generated/ProvisioningPlan";
import type { ProvisioningRules } from "../../shared/generated/ProvisioningRules";
import { previewProvisioning } from "../api/provisioning";
import ProvisioningRulesEditor from "./ProvisioningRulesEditor";

vi.mock("../api/provisioning", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api/provisioning")>()),
  previewProvisioning: vi.fn(),
}));

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
          defaultExpanded
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
    expect(onValidityChange).toHaveBeenLastCalledWith(
      false,
      "Provisioning has a mode conflict",
    );
  });

  it("shows inherited rules, grouped exclusions, and the frozen state", async () => {
    const frozenPlan: ProvisioningPlan = {
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
    };
    vi.mocked(previewProvisioning).mockClear();

    render(
      <ProvisioningRulesEditor
        level="isolated_node"
        repository="/repo"
        rules={{ copy: [], hardlink: [], symlink: [] }}
        onChange={() => {}}
        readOnly
        frozenAt="09:12"
        frozenPlan={frozenPlan}
      />,
    );

    // Collapsed: the frozen marker and the summary still show.
    expect(screen.getByText(/frozen at 09:12 · reused on restart/)).toBeInTheDocument();
    expect(screen.getByTestId("provisioning-summary")).toHaveTextContent("1 inherited · 0 at this level");
    await userEvent.click(screen.getByRole("button", { name: /^Provisioning/ }));

    await waitFor(() => expect(screen.getByText("fixtures/")).toBeInTheDocument());
    expect(screen.getByText("Instance · 2")).toBeInTheDocument();
    expect(screen.getByText(/frozen at 09:12 · reused on restart/)).toBeInTheDocument();
    expect(screen.getByText("Resolved plan · frozen")).toBeInTheDocument();
    expect(screen.queryByText("Resolved plan · live")).not.toBeInTheDocument();
    expect(previewProvisioning).not.toHaveBeenCalled();
    expect(screen.getByLabelText("Copy patterns")).toHaveAttribute("readonly");
    await userEvent.click(screen.getByText(/fixtures\/ · Instance · copy · 2/));
    expect(screen.getByText(/fixtures\/private.bin · excluded by Node/)).toHaveClass(
      "line-through",
    );
  });

  it("makes overrides visible and lets a conflict jump to either rule", async () => {
    vi.mocked(previewProvisioning).mockResolvedValue({
      entries: [],
      rules: [
        {
          scope: "instance",
          mode: "copy",
          pattern: ".env",
          paths: [".env"],
          excluded_paths: [],
          unmatched: false,
        },
        {
          scope: "run",
          mode: "symlink",
          pattern: ".env",
          paths: [".env"],
          excluded_paths: [],
          unmatched: false,
        },
        {
          scope: "isolated_node",
          mode: "symlink",
          pattern: "secrets/*",
          paths: ["secrets/token"],
          excluded_paths: [],
          unmatched: false,
        },
        {
          scope: "isolated_node",
          mode: "copy",
          pattern: "secrets/*",
          paths: ["secrets/token"],
          excluded_paths: [],
          unmatched: false,
        },
      ],
      conflicts: [
        {
          scope: "isolated_node",
          relative_path: "secrets/token",
          modes: ["copy", "symlink"],
        },
      ],
    });

    render(
      <ProvisioningRulesEditor
        level="isolated_node"
        repository="/repo"
        rules={{
          copy: ["ordinary", "secrets/*"],
          hardlink: [],
          symlink: ["other", "secrets/*"],
        }}
        onChange={() => {}}
      />,
    );

    expect(await screen.findByText(/overrides Instance copy/)).toBeInTheDocument();

    await userEvent.click(
      screen.getByRole("button", { name: "Jump to copy rule for secrets/token" }),
    );
    expect(screen.getByLabelText("Copy patterns")).toHaveFocus();
    expect(screen.getByLabelText("Copy patterns")).toHaveProperty(
      "selectionStart",
      "ordinary\n".length,
    );
    expect(screen.getByLabelText("Copy patterns")).toHaveProperty(
      "selectionEnd",
      "ordinary\nsecrets/*".length,
    );

    await userEvent.click(
      screen.getByRole("button", { name: "Jump to symlink rule for secrets/token" }),
    );
    expect(screen.getByLabelText("Symlink patterns")).toHaveFocus();
    expect(screen.getByLabelText("Symlink patterns")).toHaveProperty(
      "selectionStart",
      "other\n".length,
    );
    expect(screen.getByLabelText("Symlink patterns")).toHaveProperty(
      "selectionEnd",
      "other\nsecrets/*".length,
    );
    expect(screen.getByText(/\.env · Instance · copy · 1 · overridden by Run symlink/))
      .toBeInTheDocument();
  });

  it("previews Instance rules without folding in a Project owner", async () => {
    render(
      <ProvisioningRulesEditor
        level="instance"
        repository="/repo"
        rules={{ copy: [".env"], hardlink: [], symlink: [] }}
        onChange={() => {}}
      />,
    );

    await waitFor(() =>
      expect(previewProvisioning).toHaveBeenCalledWith(
        "/repo",
        "instance",
        { copy: [".env"], hardlink: [], symlink: [] },
        [],
        "HEAD",
        null,
      ),
    );
  });

  it("always isolates an Instance preview from supplied narrower scopes", async () => {
    render(
      <ProvisioningRulesEditor
        level="instance"
        repository="/repo"
        rules={{ copy: [".env"], hardlink: [], symlink: [] }}
        inherited={[
          {
            scope: "project",
            rules: { copy: ["project.env"], hardlink: [], symlink: [] },
          },
        ]}
        onChange={() => {}}
      />,
    );

    await waitFor(() =>
      expect(previewProvisioning).toHaveBeenCalledWith(
        "/repo",
        "instance",
        { copy: [".env"], hardlink: [], symlink: [] },
        [],
        "HEAD",
        null,
      ),
    );
  });

  it("stacks mode lists until its own container is wide enough for three columns", () => {
    render(
      <ProvisioningRulesEditor
        level="run"
        repository=""
        rules={{ copy: [], hardlink: [], symlink: [] }}
        onChange={() => {}}
        defaultExpanded
      />,
    );

    expect(screen.getByTestId("provisioning-mode-grid")).toHaveClass(
      "grid-cols-1",
      "@[520px]:grid-cols-3",
    );
  });

  describe("collapsed by default (Notion #7)", () => {
    const SUBTITLE =
      "Bring files Git ignores (e.g. .env, local caches) from the main repository into this run's worktrees.";
    const toggle = () => screen.getByRole("button", { name: /^Provisioning/ });
    const EMPTY = { copy: [], hardlink: [], symlink: [] };

    beforeEach(() => {
      vi.mocked(previewProvisioning).mockReset();
      vi.mocked(previewProvisioning).mockResolvedValue({
        entries: [],
        rules: [],
        conflicts: [],
      });
    });

    it("opens collapsed with a 'none' summary and the explanation, previewing anyway", async () => {
      const onValidityChange = vi.fn();
      render(
        <ProvisioningRulesEditor
          level="run"
          repository="/repo"
          rules={EMPTY}
          onChange={() => {}}
          onValidityChange={onValidityChange}
        />,
      );

      expect(toggle()).toHaveAttribute("aria-expanded", "false");
      expect(screen.getByTestId("provisioning-summary")).toHaveTextContent("none");
      expect(toggle()).toHaveAttribute("title", SUBTITLE);
      expect(screen.queryByLabelText("Copy patterns")).not.toBeInTheDocument();
      await waitFor(() => expect(previewProvisioning).toHaveBeenCalled());
      await waitFor(() => expect(onValidityChange).toHaveBeenLastCalledWith(true));
      expect(toggle()).toHaveAttribute("aria-expanded", "false");
    });

    it("expands and collapses on demand, keeping the explanation visible", async () => {
      render(
        <ProvisioningRulesEditor
          level="run"
          repository=""
          rules={EMPTY}
          onChange={() => {}}
        />,
      );

      await userEvent.click(toggle());
      expect(toggle()).toHaveAttribute("aria-expanded", "true");
      expect(screen.getByLabelText("Copy patterns")).toBeInTheDocument();
      expect(screen.getByLabelText("Hardlink patterns")).toBeInTheDocument();
      expect(screen.getByLabelText("Symlink patterns")).toBeInTheDocument();
      expect(toggle()).toHaveAttribute("title", SUBTITLE);

      await userEvent.click(toggle());
      expect(toggle()).toHaveAttribute("aria-expanded", "false");
      expect(screen.queryByLabelText("Copy patterns")).not.toBeInTheDocument();
    });

    it("renders expanded when told to, with the leading and trailing slots", () => {
      render(
        <ProvisioningRulesEditor
          level="instance"
          repository=""
          rules={EMPTY}
          onChange={() => {}}
          defaultExpanded
          leading={<span>leading slot</span>}
          trailing={<span>trailing slot</span>}
        />,
      );

      expect(toggle()).toHaveAttribute("aria-expanded", "true");
      expect(screen.getByText("leading slot")).toBeInTheDocument();
      expect(screen.getByText("trailing slot")).toBeInTheDocument();
    });

    it("hides the slots while collapsed", () => {
      render(
        <ProvisioningRulesEditor
          level="project"
          repository=""
          rules={EMPTY}
          onChange={() => {}}
          leading={<span>leading slot</span>}
          trailing={<span>trailing slot</span>}
        />,
      );

      expect(screen.queryByText("leading slot")).not.toBeInTheDocument();
      expect(screen.queryByText("trailing slot")).not.toBeInTheDocument();
    });

    it("summarises inherited and own rules from the resolved plan", async () => {
      const rule = (scope: "instance" | "project" | "run", pattern: string) => ({
        scope,
        mode: "copy" as const,
        pattern,
        paths: [pattern],
        excluded_paths: [],
        unmatched: false,
      });
      vi.mocked(previewProvisioning).mockResolvedValue({
        entries: [],
        rules: [rule("instance", ".env"), rule("project", ".npmrc"), rule("run", "cache")],
        conflicts: [],
      });
      render(
        <ProvisioningRulesEditor
          level="run"
          repository="/repo"
          rules={{ copy: ["cache"], hardlink: [], symlink: [] }}
          onChange={() => {}}
        />,
      );

      await waitFor(() =>
        expect(screen.getByTestId("provisioning-summary")).toHaveTextContent("2 inherited · 1 at this level"),
      );
      // Valid, non-empty rules do not open the block.
      expect(toggle()).toHaveAttribute("aria-expanded", "false");
    });

    it("falls back to the local and inherited rule counts without a preview", () => {
      render(
        <ProvisioningRulesEditor
          level="run"
          repository=""
          rules={{ copy: [".env"], hardlink: [], symlink: ["node_modules"] }}
          inherited={[{ scope: "project", rules: { copy: [".npmrc"], hardlink: [], symlink: [] } }]}
          onChange={() => {}}
        />,
      );

      expect(screen.getByTestId("provisioning-summary")).toHaveTextContent("1 inherited · 2 at this level");
      expect(previewProvisioning).not.toHaveBeenCalled();
    });

    it("opens by itself on a mode conflict and reports why it blocks", async () => {
      vi.mocked(previewProvisioning).mockResolvedValue({
        entries: [],
        rules: [],
        conflicts: [{ scope: "run", relative_path: ".env", modes: ["copy", "symlink"] }],
      });
      const onValidityChange = vi.fn();
      render(
        <ProvisioningRulesEditor
          level="run"
          repository="/repo"
          rules={{ copy: [".env"], hardlink: [], symlink: [".env"] }}
          onChange={() => {}}
          onValidityChange={onValidityChange}
        />,
      );

      await waitFor(() => expect(toggle()).toHaveAttribute("aria-expanded", "true"));
      expect(screen.getByRole("alert")).toHaveTextContent("Mode conflict in Run — .env");
      expect(onValidityChange).toHaveBeenLastCalledWith(
        false,
        "Provisioning has a mode conflict",
      );

      // A blocking conflict cannot be hidden away.
      await userEvent.click(toggle());
      expect(toggle()).toHaveAttribute("aria-expanded", "true");
      expect(screen.getByLabelText("Copy patterns")).toBeInTheDocument();
    });

    it("opens by itself on a preview error and reports it", async () => {
      vi.mocked(previewProvisioning).mockRejectedValue(new Error("not a git repository"));
      const onValidityChange = vi.fn();
      render(
        <ProvisioningRulesEditor
          level="run"
          repository="/nowhere"
          rules={EMPTY}
          onChange={() => {}}
          onValidityChange={onValidityChange}
        />,
      );

      await waitFor(() => expect(toggle()).toHaveAttribute("aria-expanded", "true"));
      expect(screen.getByRole("alert")).toHaveTextContent("not a git repository");
      expect(onValidityChange).toHaveBeenLastCalledWith(
        false,
        "Provisioning preview failed: not a git repository",
      );
      // Unresolved inherited rules: the summary does not claim "none".
      expect(screen.getByTestId("provisioning-summary")).toHaveTextContent("0 at this level · inherited unknown");
    });

    it("stays open once the conflict is fixed, even if the header was clicked meanwhile", async () => {
      vi.mocked(previewProvisioning).mockImplementation(async (_repo, _level, rules) => ({
        entries: [],
        rules: [],
        conflicts:
          rules.copy.includes(".env") && rules.symlink.includes(".env")
            ? [{ scope: "run", relative_path: ".env", modes: ["copy", "symlink"] }]
            : [],
      }));
      function Host() {
        const [rules, setRules] = useState<ProvisioningRules>({
          copy: [".env"],
          hardlink: [],
          symlink: [".env"],
        });
        return (
          <ProvisioningRulesEditor level="run" repository="/repo" rules={rules} onChange={setRules} />
        );
      }
      render(<Host />);

      await waitFor(() => expect(toggle()).toHaveAttribute("aria-expanded", "true"));
      // Ignored while blocked — and not replayed as a collapse later.
      await userEvent.click(toggle());
      await userEvent.clear(screen.getByLabelText("Symlink patterns"));

      await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
      expect(toggle()).toHaveAttribute("aria-expanded", "true");
      expect(screen.getByLabelText("Symlink patterns")).toHaveFocus();
    });

    it("drops the header 'Resolve against' marker while the body shows its own field", async () => {
      render(
        <ProvisioningRulesEditor
          level="project"
          repository="/repo"
          rules={EMPTY}
          onChange={() => {}}
          leading={<span>leading slot</span>}
        />,
      );

      expect(screen.getByText("Resolve against /repo")).toBeInTheDocument();
      await userEvent.click(toggle());
      expect(screen.getByText("leading slot")).toBeInTheDocument();
      expect(screen.queryByText("Resolve against /repo")).not.toBeInTheDocument();
    });

    it("explains each mode and the level strip in keyboard-reachable tooltips", async () => {
      const user = userEvent.setup();
      render(
        <ProvisioningRulesEditor
          level="run"
          repository=""
          rules={EMPTY}
          onChange={() => {}}
          defaultExpanded
        />,
      );

      const hints: Array<[string, string]> = [
        [
          "About copy",
          "Independent copy. Edits in the worktree never touch the original. Uses disk space. Good for a .env you may tweak.",
        ],
        [
          "About hardlink",
          "Same file on disk, no extra space. In-place edits show up on both sides. Files only, same filesystem as the repository.",
        ],
        [
          "About symlink",
          "A link to the original path (a whole folder can be linked). Everything is shared, writes included. Good for large caches like node_modules.",
        ],
        [
          "About provisioning levels",
          "Rules add up Instance → Project → Run → Node. A finer level can change the mode or exclude (!) an inherited pattern.",
        ],
      ];
      for (const [label, text] of hints) {
        const trigger = screen.getByRole("button", { name: label });
        act(() => trigger.focus());
        await waitFor(() =>
          expect(screen.getByTestId("tooltip-content")).toHaveTextContent(text),
        );
        act(() => trigger.blur());
        await waitFor(() =>
          expect(screen.queryByTestId("tooltip-content")).not.toBeInTheDocument(),
        );
      }

      await user.hover(screen.getByRole("button", { name: "About hardlink" }));
      await waitFor(() =>
        expect(screen.getByTestId("tooltip-content")).toHaveTextContent(
          "Same file on disk, no extra space.",
        ),
      );
    });
  });
});
