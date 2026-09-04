/**
 * The Settings surface's map (#690, CONTEXT.md « Surface Settings »): four **categories**
 * on the rail, each a scrollable page cut into **sections** (the sub-categories the second
 * column lists and scroll-spies). Data, not JSX: the rail, the sub-column, the dirty
 * rollups and the confirm-close copy are all derived from this one list.
 */

export type SettingsCategoryId = "general" | "agents" | "sandbox" | "diagnostics";

export type SettingsSectionId =
  | "interface"
  | "runtime-limits"
  | "runs"
  | "harness-models"
  | "agent-profiles"
  | "skills"
  | "sandbox"
  | "staging-profiles"
  | "worktree-provisioning"
  | "price-table"
  | "harness-descriptors";

export interface SettingsSection {
  id: SettingsSectionId;
  label: string;
  description: string;
  /** Diagnostics: observed state, nothing to PUT. */
  readOnly?: boolean;
  /** Inline panel with its own REST resource: every edit is written at once, Save ignores it. */
  ownPersistence?: boolean;
  /** Needs the full pane width (two-column panels), not the 640px form column. */
  wide?: boolean;
}

export interface SettingsCategory {
  id: SettingsCategoryId;
  label: string;
  sections: SettingsSection[];
}

export const SETTINGS_CATEGORIES: SettingsCategory[] = [
  {
    id: "general",
    label: "General",
    sections: [
      {
        id: "interface",
        label: "Interface",
        description: "How this browser talks to PDO. Saved on this device only.",
      },
      {
        id: "runtime-limits",
        label: "Runtime limits",
        description: "Daemon-wide caps. Env variables override the stored value.",
      },
      {
        id: "runs",
        label: "Runs",
        description: "Defaults a new Run starts from. New Run and Triggers can override.",
      },
    ],
  },
  {
    id: "agents",
    label: "Agents",
    sections: [
      {
        id: "harness-models",
        label: "Harness & models",
        description:
          "What a work node runs on when neither the node nor a coarser tier decides.",
      },
      {
        id: "agent-profiles",
        label: "Agent profiles",
        description:
          "Named harness · model · effort presets a node, a Run or the instance can pick by name.",
        ownPersistence: true,
      },
      {
        id: "skills",
        label: "Skills",
        description:
          "The skills the instance tier hands to every node (saved with the form), and the bank they come from.",
      },
    ],
  },
  {
    id: "sandbox",
    label: "Sandbox & worktrees",
    sections: [
      {
        id: "sandbox",
        label: "Default sandbox",
        description:
          "Where a Run executes when neither the launch dialog nor a Trigger picks one.",
      },
      {
        id: "staging-profiles",
        label: "Staging profiles",
        description:
          "What a Docker sandbox receives from your home: the floor, the default entries, your extras.",
        ownPersistence: true,
      },
      {
        id: "worktree-provisioning",
        label: "Worktree provisioning",
        description:
          "Repository paths copied, hard-linked or symlinked into a Run's worktrees when Git leaves them absent.",
        ownPersistence: true,
      },
    ],
  },
  {
    id: "diagnostics",
    label: "Diagnostics",
    sections: [
      {
        id: "price-table",
        label: "Price table",
        description:
          "Where cost prices come from. Nothing here is edited: the paths are fixed, the content lives on disk.",
        readOnly: true,
      },
      {
        id: "harness-descriptors",
        label: "Harness descriptors",
        description:
          "Harnesses PDO knows how to launch. A refused descriptor is named here, and only here.",
        readOnly: true,
      },
    ],
  },
];

/**
 * Every editable field of the instance form, keyed to the section it lives in. The
 * dirty set is a `Set<SettingsFieldId>`; rollups to section and category derive from
 * this map, so a field can never be dirty "nowhere".
 */
export type SettingsFieldId =
  | "session-cap"
  | "reaper-ttl"
  | "guard-timeout"
  | "autocomplete-turn-end"
  | "default-auto-name"
  | "agent-choice"
  | "skills"
  | "default-model"
  | "default-harness"
  | "harness-models"
  | "default-sandbox";

export const FIELD_SECTION: Record<SettingsFieldId, SettingsSectionId> = {
  "session-cap": "runtime-limits",
  "reaper-ttl": "runtime-limits",
  "guard-timeout": "runtime-limits",
  "autocomplete-turn-end": "runs",
  "default-auto-name": "runs",
  "agent-choice": "harness-models",
  skills: "skills",
  "default-model": "harness-models",
  "default-harness": "harness-models",
  "harness-models": "harness-models",
  "default-sandbox": "sandbox",
};

export function categoryOf(section: SettingsSectionId): SettingsCategory {
  const found = SETTINGS_CATEGORIES.find((category) =>
    category.sections.some((item) => item.id === section),
  );
  if (!found) throw new Error(`unknown settings section ${section}`);
  return found;
}

export function findCategory(id: SettingsCategoryId): SettingsCategory {
  const found = SETTINGS_CATEGORIES.find((category) => category.id === id);
  if (!found) throw new Error(`unknown settings category ${id}`);
  return found;
}

export interface DirtyRollup {
  fields: Set<SettingsFieldId>;
  sections: Set<SettingsSectionId>;
  categories: Set<SettingsCategoryId>;
  /** Per category, how many fields are dirty (footer copy). */
  perCategory: Map<SettingsCategoryId, number>;
}

export function rollupDirty(fields: Set<SettingsFieldId>): DirtyRollup {
  const sections = new Set<SettingsSectionId>();
  const categories = new Set<SettingsCategoryId>();
  const perCategory = new Map<SettingsCategoryId, number>();
  for (const field of fields) {
    const section = FIELD_SECTION[field];
    sections.add(section);
    const category = categoryOf(section).id;
    categories.add(category);
    perCategory.set(category, (perCategory.get(category) ?? 0) + 1);
  }
  return { fields, sections, categories, perCategory };
}
