/**
 * The Settings surface's map (#690, CONTEXT.md « Surface Settings »): the **categories**
 * on the rail, each a scrollable page cut into **sections** (the sub-categories the second
 * column lists and scroll-spies). Data, not JSX: the rail, the sub-column, the dirty
 * rollups and the confirm-close copy are all derived from this one list.
 */

export type SettingsCategoryId =
  | "general"
  | "appearance"
  | "notifications"
  | "agents"
  | "sandbox"
  | "projects"
  | "telemetry"
  | "diagnostics";

export type SettingsSectionId =
  | "theme"
  | "appearance"
  | "interface"
  | "waiting-alerts"
  | "public-url"
  | "runtime-limits"
  | "runs"
  | "version-update"
  | "tutorials"
  | "stats-absorptions"
  | "harness-models"
  | "agent-profiles"
  | "pipeline-manager"
  | "skills"
  | "sandbox"
  | "staging-profiles"
  | "worktree-provisioning"
  | "projects"
  | "clones-folder"
  | "otlp-export"
  | "prometheus-metrics"
  | "identity"
  | "what-is-sent"
  | "price-table"
  | "harness-descriptors";

export interface SettingsSection {
  id: SettingsSectionId;
  label: string;
  /** Diagnostics: observed state, nothing to PUT. */
  readOnly?: boolean;
  /**
   * Inline panel with its own REST resource (#691): every edit is written at once, the
   * form's Save ignores it. Rendered as a `saves as you go` badge next to the title.
   */
  ownPersistence?: boolean;
  /**
   * #823: the section edits nothing on the daemon — its state is this browser's
   * (localStorage), like the theme. Rendered as the `Device-local · saved
   * immediately` badge the Interface controls already wear.
   */
  deviceLocal?: boolean;
  /**
   * The section's choices belong to the Actor's Apparence (CONTEXT.md « Apparence »):
   * kept by the daemon for the Actor, written at once, out of the instance form.
   * Rendered as the `Your account, saved at once` icon next to the title.
   */
  account?: boolean;
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
        id: "public-url",
        label: "Public URL",
      },
      {
        id: "runtime-limits",
        label: "Runtime limits",
      },
      {
        id: "runs",
        label: "Runs",
      },
      {
        id: "version-update",
        label: "Version & update",
        // #697: « Check now » acts at once and the check switch is written at the
        // change — the form's Save does not apply here.
        ownPersistence: true,
      },
      {
        id: "tutorials",
        // #823: guided tours. Nothing here belongs to the instance form — the
        // checkmarks live in this browser — so the section contributes no field
        // to the dirty set and wears the device-local badge rather than the
        // "saves as you go" one (which speaks of a REST resource it has none of).
        label: "Tutorials",
        deviceLocal: true,
      },
      {
        id: "stats-absorptions",
        // #891 (ADR-0077): every combined row of Stats, the rename-born ones
        // included. Its own REST resource — each ✕ is written at once.
        label: "Stats absorptions",
        ownPersistence: true,
      },
    ],
  },
  {
    // The Apparence (CONTEXT.md « Apparence »): what changes the whole interface, the
    // blocks of each surface, then this browser's behaviour. Nothing here belongs to the
    // instance form: every choice is written at the change.
    id: "appearance",
    label: "Appearance",
    sections: [
      {
        // Each row wears its own scope: the Theme, the Dark mode and the colour-blind
        // status palette follow the Actor, the UI scale stays with this device.
        id: "theme",
        label: "Theme",
      },
      {
        id: "appearance",
        label: "Appearance",
        account: true,
      },
      {
        id: "interface",
        label: "Interface",
        deviceLocal: true,
      },
    ],
  },
  {
    // The Alerte d'attente (CONTEXT.md): this browser's notification and sound, and
    // its Portée d'alerte. Nothing here belongs to the instance form nor to the
    // Actor: the permission and the sound are the device's.
    id: "notifications",
    label: "Notifications",
    sections: [
      {
        id: "waiting-alerts",
        label: "Waiting alerts",
        deviceLocal: true,
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
      },
      {
        id: "agent-profiles",
        label: "Agent profiles",
        ownPersistence: true,
      },
      {
        id: "pipeline-manager",
        label: "Pipeline Manager",
      },
      {
        id: "skills",
        label: "Skills",
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
      },
      {
        id: "staging-profiles",
        label: "Staging profiles",
        ownPersistence: true,
      },
      {
        id: "worktree-provisioning",
        label: "Worktree provisioning",
        ownPersistence: true,
      },
    ],
  },
  {
    // CORE-96 (ADR-0087): the Projets and their repositories.
    id: "projects",
    label: "Projects",
    sections: [
      {
        id: "projects",
        label: "Projects",
        ownPersistence: true,
      },
      {
        // CORE-97: saved with the instance form, unlike the Projets above it.
        id: "clones-folder",
        label: "Clones folder",
      },
    ],
  },
  {
    // ADR-0090: the instance's metrics and traces, pushed to the organisation's collector.
    id: "telemetry",
    label: "Telemetry",
    sections: [
      {
        id: "otlp-export",
        label: "OTLP export",
      },
      {
        id: "prometheus-metrics",
        label: "Prometheus /metrics",
      },
      {
        id: "identity",
        label: "Identity",
      },
      {
        id: "what-is-sent",
        label: "What is sent",
        readOnly: true,
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
        readOnly: true,
      },
      {
        id: "harness-descriptors",
        label: "Harness descriptors",
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
  | "max-attachments-mb"
  | "autocomplete-turn-end"
  | "default-auto-name"
  | "review-agent-can-resolve"
  | "agent-choice"
  | "skills"
  | "default-model"
  | "default-harness"
  | "harness-models"
  | "default-sandbox"
  | "update-check"
  | "manager-enabled"
  | "manager-profile"
  | "telemetry-otlp"
  | "telemetry-endpoint"
  | "telemetry-headers"
  | "telemetry-prometheus"
  | "telemetry-include-actor"
  | "public-url"
  | "clones-folder";

export const FIELD_SECTION: Record<SettingsFieldId, SettingsSectionId> = {
  "session-cap": "runtime-limits",
  "reaper-ttl": "runtime-limits",
  "guard-timeout": "runtime-limits",
  "max-attachments-mb": "runtime-limits",
  "autocomplete-turn-end": "runs",
  "default-auto-name": "runs",
  "review-agent-can-resolve": "runs",
  "agent-choice": "harness-models",
  skills: "skills",
  "default-model": "harness-models",
  "default-harness": "harness-models",
  "harness-models": "harness-models",
  "default-sandbox": "sandbox",
  "update-check": "version-update",
  "manager-enabled": "pipeline-manager",
  "manager-profile": "pipeline-manager",
  "telemetry-otlp": "otlp-export",
  "telemetry-endpoint": "otlp-export",
  "telemetry-headers": "otlp-export",
  "telemetry-prometheus": "prometheus-metrics",
  "telemetry-include-actor": "identity",
  "public-url": "public-url",
  "clones-folder": "clones-folder",
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
