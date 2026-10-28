import type { ProvisioningMode } from "../../shared/generated/ProvisioningMode";
import type { ProvisioningScope } from "../../shared/generated/ProvisioningScope";

// The Provisioning block's labels and explanations (`ProvisioningRulesEditor`).

export const LEVEL_LABELS: Record<ProvisioningScope, string> = {
  instance: "Instance",
  project: "Project",
  run: "Run",
  isolated_node: "Node",
};

// Notion #7 / #951: the texts agreed at grilling (spec #950).
export const MODES: Array<{ key: ProvisioningMode; color: string; hint: string }> = [
  {
    key: "copy",
    color: "bg-st-done",
    hint: "Independent copy. Edits in the worktree never touch the original. Uses disk space. Good for a .env you may tweak.",
  },
  {
    key: "hardlink",
    color: "bg-st-running",
    hint: "Same file on disk, no extra space. In-place edits show up on both sides. Files only, same filesystem as the repository.",
  },
  {
    key: "symlink",
    color: "bg-st-blocked",
    hint: "A link to the original path (a whole folder can be linked). Everything is shared, writes included. Good for large caches like node_modules.",
  },
];

export const LEVELS_HINT =
  "Rules add up Instance → Project → Run → Node. A finer level can change the mode or exclude (!) an inherited pattern.";

export const SUBTITLE =
  "Bring files Git ignores (e.g. .env, local caches) from the main repository into this run's worktrees.";

export const PROVISIONING_CONFLICT_REASON = "Provisioning has a mode conflict";
