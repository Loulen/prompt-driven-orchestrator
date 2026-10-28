import { TriangleAlert } from "lucide-react";
import type { AgentChoice } from "../../../shared/generated/AgentChoice";
import type { AgentProfile } from "../../../shared/generated/AgentProfile";
import type { AgentCombination } from "../../../settings/model/settings";
import { combinationLabel, resolveAgentChoice } from "../../../settings/model/agentProfiles";
import type { HarnessCatalog } from "../../../harness/model/catalog";
import { effortOffer, findHarnessOption } from "../../../harness/model/catalog";
import HarnessSelect from "../../../harness/components/HarnessSelect";
import ModelPicker from "../../../harness/components/ModelPicker";
import EffortPicker from "../../../harness/components/EffortPicker";
import { Segmented, Select } from "../../../shared/ui/kit";

type Mode = AgentChoice["mode"];

const MODES: { value: Mode; label: string; title: string }[] = [
  { value: "inherit", label: "Inherit", title: "Inherit from the Run" },
  { value: "profile", label: "Profile", title: "An agent profile" },
  { value: "custom", label: "Custom", title: "A combination for this node only" },
];

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
    } else onChange({ mode: "custom", ...inherited });
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
      {choice?.mode === "custom" && (
        <div className="ei-agent__pick" data-testid={`${testId}-custom`}>
          <HarnessSelect
            value={custom.harness}
            onChange={(harness) => onChange({ mode: "custom", harness: harness || "claude", model: null, effort: null })}
            catalog={catalog}
            inheritLabel="Choose a harness"
          />
          <ModelPicker
            value={custom.model ?? null}
            onChange={(model) => onChange({ mode: "custom", ...custom, model })}
            models={harnessOption?.models ?? []}
            contexts={harnessOption?.modelContexts}
            testid={`${testId}-custom-model`}
            subject={testId}
          />
          <EffortPicker
            value={custom.effort ?? null}
            onChange={(next) => onChange({ mode: "custom", ...custom, effort: next })}
            efforts={effort.levels}
            strict={effort.authoritative}
            testid={`${testId}-custom-effort`}
            disabled={!(harnessOption?.hasEffort ?? true)}
          />
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
