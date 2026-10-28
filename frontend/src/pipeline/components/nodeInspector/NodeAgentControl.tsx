import { TriangleAlert } from "lucide-react";
import type { AgentChoice } from "../../../shared/generated/AgentChoice";
import type { AgentProfile } from "../../../shared/generated/AgentProfile";
import type { AgentCombination } from "../../../settings/model/settings";
import { combinationLabel, resolveAgentChoice } from "../../../settings/model/agentProfiles";
import type { HarnessCatalog } from "../../../harness/model/catalog";
import { customDefaults, effortOffer, findHarnessOption } from "../../../harness/model/catalog";
import { useState } from "react";
import { Segmented, Select, TextInput } from "../../../shared/ui/kit";

type Mode = AgentChoice["mode"];

const MODES: { value: Mode; label: string; title: string }[] = [
  { value: "inherit", label: "Inherit", title: "Inherit from the Run" },
  { value: "profile", label: "Profile", title: "An agent profile" },
  { value: "custom", label: "Custom", title: "A combination for this node only" },
];

/** The value of a picker that stands for "the harness's default". */
const DEFAULT = "";
/** The model picker's entry that opens a field for an id the harness does not list. */
const OTHER = "\u0000other";

/**
 * The model of a Custom agent: the harness's models, or any id typed in a
 * field (a harness that lists none, or « other… »), kept on Enter or blur.
 */
function CustomModel({
  value,
  models,
  contexts,
  onChange,
  testId,
}: {
  value: string | null;
  models: string[];
  contexts: Record<string, string>;
  onChange: (model: string | null) => void;
  testId: string;
}) {
  const [draft, setDraft] = useState<string | null>(null);
  if (models.length === 0 || draft != null) {
    const commit = () => {
      if (draft == null) return;
      const next = draft.trim() || null;
      setDraft(null);
      if (next !== value) onChange(next);
    };
    return (
      <TextInput
        mono
        autoFocus={draft != null && models.length > 0}
        aria-label="Model"
        title="Model"
        placeholder="model"
        value={draft ?? value ?? ""}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "Enter") commit();
          else if (e.key === "Escape") setDraft(null);
        }}
        data-testid={testId}
      />
    );
  }
  return (
    <span className="ei-agent__slot" data-testid={testId}>
      <Select<string>
        label="Model"
        mono
        value={value ?? DEFAULT}
        onChange={(model) => {
          if (model === OTHER) setDraft(value ?? "");
          else onChange(model === DEFAULT ? null : model);
        }}
        options={[
          { value: DEFAULT, label: "default" },
          // A model the catalogue does not list stays shown, as it is stored.
          ...(value && !models.includes(value) ? [{ value, label: value }] : []),
          ...models.map((m) => ({ value: m, label: m, hint: contexts[m] })),
          { value: OTHER, label: "other…" },
        ]}
      />
    </span>
  );
}

/**
 * The agent of a node (`inspector.agentControl`): Inherit, Profile or Custom
 * as a segmented control, the picker of the chosen mode under it, the
 * resolved combination in mono.
 */
export default function NodeAgentControl({
  choice,
  onChange,
  profiles,
  catalog,
  inherited,
  readOnly,
  testId,
}: {
  choice?: AgentChoice | null;
  onChange: (choice: AgentChoice) => void;
  profiles: AgentProfile[];
  catalog: HarnessCatalog;
  inherited: AgentCombination;
  readOnly?: boolean;
  testId: string;
}) {
  const mode: Mode = choice?.mode ?? "inherit";
  const resolved = resolveAgentChoice(choice, profiles, inherited);
  const broken = resolved.brokenId != null;
  const custom: AgentCombination = choice?.mode === "custom" ? choice : inherited;
  const harnessOption = findHarnessOption(catalog, custom.harness);
  // #798: the effort offer is read against the model this choice has selected.
  const effort = effortOffer(harnessOption, custom.model);

  const switchMode = (next: Mode) => {
    if (next === mode) return;
    if (next === "inherit") onChange({ mode: "inherit" });
    else if (next === "profile") {
      const first = profiles[0];
      if (first) onChange({ mode: "profile", profile_id: first.id });
    } else onChange({ mode: "custom", ...customDefaults(catalog, inherited) });
  };

  return (
    <div
      className="ei-agent"
      data-testid={testId}
      // #824: what is chosen, machine-readable: the profile's id is fixed forever.
      data-choice={choice?.mode === "profile" ? `profile:${choice.profile_id}` : mode}
      data-broken={broken || undefined}
    >
      <Segmented<Mode>
        label="Agent"
        value={mode}
        disabled={readOnly}
        onChange={switchMode}
        items={MODES.map((m) => ({ ...m, disabled: m.value === "profile" && profiles.length === 0 }))}
      />
      {choice?.mode === "profile" && (
        <div className="ei-agent__pick">
          <Select<string>
            label="Profile"
            value={choice.profile_id}
            disabled={readOnly}
            placeholder={resolved.brokenId}
            onChange={(profile_id) => onChange({ mode: "profile", profile_id })}
            options={profiles.map((p) => ({ value: p.id, label: p.name, hint: combinationLabel(p) }))}
          />
        </div>
      )}
      {/* A read-only node shows the combination on the line below, no picker. */}
      {choice?.mode === "custom" && !readOnly && (
        <div className="ei-agent__pick" data-testid={`${testId}-custom`}>
          <Select<string>
            label="Harness"
            mono
            value={custom.harness}
            onChange={(harness) =>
              // A new harness starts from its own model and effort.
              onChange({ mode: "custom", ...customDefaults(catalog, { harness, model: null, effort: null }) })
            }
            options={[
              // A harness the catalogue no longer lists stays shown, as it is stored.
              ...(harnessOption ? [] : [{ value: custom.harness, label: custom.harness, hint: "unknown" }]),
              ...[...catalog.builtin, ...catalog.descriptors].map((h) => ({
                value: h.name,
                label: h.name,
                disabled: !h.installed,
                hint: h.installed ? undefined : "not installed",
              })),
            ]}
          />
          <CustomModel
            value={custom.model ?? null}
            models={harnessOption?.models ?? []}
            contexts={harnessOption?.modelContexts ?? {}}
            onChange={(model) => onChange({ mode: "custom", ...custom, model })}
            testId={`${testId}-custom-model`}
          />
          {(harnessOption?.hasEffort ?? true) && (effort.levels.length > 0 || custom.effort) && (
            <span className="ei-agent__slot" data-testid={`${testId}-custom-effort`}>
              <Select<string>
                label="Effort"
                mono
                value={custom.effort ?? DEFAULT}
                onChange={(next) => onChange({ mode: "custom", ...custom, effort: next === DEFAULT ? null : next })}
                options={[
                  { value: DEFAULT, label: "default" },
                  ...(custom.effort && !effort.levels.includes(custom.effort)
                    ? [{ value: custom.effort, label: custom.effort, hint: effort.authoritative ? "unsupported" : undefined }]
                    : []),
                  ...effort.levels.map((l) => ({ value: l, label: l })),
                ]}
              />
            </span>
          )}
        </div>
      )}
      <span
        className="ei-agent__line"
        title={broken ? `Profile ${resolved.brokenId} is missing: the next tier resolves` : "Resolved agent"}
      >
        {broken && <TriangleAlert size={10} strokeWidth={1.75} aria-hidden />}{" "}
        {broken ? `missing · ${combinationLabel(resolved.combination)}` : combinationLabel(resolved.combination)}
      </span>
    </div>
  );
}
