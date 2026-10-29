import { useMemo, useRef, useState } from "react";
import { Bookmark, Check, ChevronDown, ChevronLeft, GitFork, SlidersHorizontal, TriangleAlert } from "lucide-react";
import type { AgentChoice } from "../../shared/generated/AgentChoice";
import type { AgentCombination } from "../model/settings";
import type { AgentProfile } from "../../shared/generated/AgentProfile";
import type { HarnessCatalog } from "../../harness/model/catalog";
import { findHarnessOption, effortOffer } from "../../harness/model/catalog";
import HarnessSelect from "../../harness/components/HarnessSelect";
import ModelPicker from "../../harness/components/ModelPicker";
import EffortPicker from "../../harness/components/EffortPicker";
import { AnchoredPanel } from "../../shared/ui/kit/layer";
import { Button } from "../../shared/ui/kit";
import { combinationLabel, resolveAgentChoice } from "../model/agentProfiles";

const EMPTY: AgentCombination = { harness: "claude", model: null, effort: null };

/**
 * The agent of a tier (instance, Projet, Run, node): inherit, a profile, or a
 * custom combination. The kit's dropdown look — the choice, then its
 * combination in mono at the end — opening a panel of choices.
 */
export default function AgentControl({
  choice,
  onChange,
  profiles,
  catalog,
  inherited = EMPTY,
  allowInherit = true,
  label = "Agent",
  showLabel = true,
  testId = "agent-control",
}: {
  choice?: AgentChoice | null;
  onChange: (choice: AgentChoice) => void;
  profiles: AgentProfile[];
  catalog: HarnessCatalog;
  inherited?: AgentCombination;
  allowInherit?: boolean;
  label?: string;
  /** The caps label above the control; off in a label-left form row. */
  showLabel?: boolean;
  testId?: string;
}) {
  const [open, setOpen] = useState(false);
  const [customPane, setCustomPane] = useState(false);
  const [custom, setCustom] = useState<AgentCombination>(() =>
    choice?.mode === "custom" ? choice : inherited,
  );
  const resolved = useMemo(
    () => resolveAgentChoice(choice, profiles, inherited),
    [choice, profiles, inherited],
  );
  const broken = resolved.brokenId != null;
  const modeLabel =
    broken ? resolved.brokenId
    : choice?.mode === "profile" ? resolved.profile?.name ?? "Profile"
    : choice?.mode === "custom" ? "Custom"
    : "Inherit";
  const Icon =
    broken ? TriangleAlert
    : choice?.mode === "profile" ? Bookmark
    : choice?.mode === "custom" ? SlidersHorizontal
    : GitFork;
  const harnessOption = findHarnessOption(catalog, custom.harness);
  // #798: the custom pane's effort offer is read against the model THIS pane has
  // selected (seeded from the choice when editing an existing custom, else from
  // the inherited combination it opens on) — per-model key authoritative
  // (`[]` included), missing key on the harness's global efforts; authoritative
  // ⇒ a stored effort outside the offer warns instead of silently persisting.
  const effort = effortOffer(harnessOption, custom.model);
  const summary = broken ? `missing · ${combinationLabel(resolved.combination)}` : combinationLabel(resolved.combination);

  const triggerRef = useRef<HTMLButtonElement>(null);

  const close = () => {
    setOpen(false);
    setCustomPane(false);
  };

  return (
    <div className="ln-agent" data-testid={`${testId}-root`}>
      {showLabel && <span className="d-caps ln-agent__label">{label}</span>}
      <button
        ref={triggerRef}
        type="button"
        data-testid={testId}
        // #824: what is chosen, machine-readable. The visible label is the
        // profile's NAME, which is renameable and sits next to a combination
        // summary in the same text node — nothing an observer can match on. The
        // id is fixed forever, so `profile:default` says exactly one thing.
        data-choice={
          choice?.mode === "profile"
            ? `profile:${choice.profile_id}`
            : (choice?.mode ?? "inherit")
        }
        aria-label={showLabel ? undefined : label}
        aria-expanded={open}
        aria-invalid={broken || undefined}
        title={broken ? `${modeLabel}: missing, resolution continues at the next tier` : `${modeLabel} · ${summary}`}
        onClick={() => (open ? close() : setOpen(true))}
        className="d-sel ln-agent__field"
      >
        <Icon className="d-sel__lead" size={13} strokeWidth={1.5} aria-hidden />
        <span className="d-sel__val">{modeLabel}</span>
        <span className="ln-sel__count ln-agent__sum">{summary}</span>
        <ChevronDown className="d-sel__chev" size={12} strokeWidth={1.5} aria-hidden />
      </button>

      <AnchoredPanel anchor={triggerRef} open={open} onClose={close} className="d-pop--panel ln-agent__pop">
        <div role="dialog" aria-label={label} data-testid={`${testId}-popover`} className="ln-agent__body">
          {customPane ? (
            <>
              <button type="button" onClick={() => setCustomPane(false)} className="d-pop__row ln-agent__back">
                <ChevronLeft size={12} strokeWidth={1.5} aria-hidden /> Custom combination
              </button>
              <div className="ln-agent__custom">
                <label className="ln-agent__f">
                  <span className="ln-faint">Harness</span>
                  <HarnessSelect
                    value={custom.harness}
                    onChange={(harness) => setCustom({ harness: harness || "claude", model: null, effort: null })}
                    catalog={catalog}
                    inheritLabel="Choose a harness"
                  />
                </label>
                <label className="ln-agent__f">
                  <span className="ln-faint">Model</span>
                  <ModelPicker
                    value={custom.model ?? null}
                    onChange={(model) => setCustom({ ...custom, model })}
                    models={harnessOption?.models ?? []}
                    contexts={harnessOption?.modelContexts}
                    testid={`${testId}-custom-model`}
                    subject={testId}
                  />
                </label>
                <label className="ln-agent__f">
                  <span className="ln-faint">Effort</span>
                  <EffortPicker
                    value={custom.effort ?? null}
                    onChange={(next) => setCustom({ ...custom, effort: next })}
                    efforts={effort.levels}
                    strict={effort.authoritative}
                    testid={`${testId}-custom-effort`}
                    disabled={!(harnessOption?.hasEffort ?? true)}
                  />
                </label>
                <div className="ln-line ln-line--end">
                  <Button
                    size="sm"
                    tone="primary"
                    onClick={() => {
                      onChange({ mode: "custom", ...custom });
                      close();
                    }}
                  >
                    Apply
                  </Button>
                </div>
              </div>
            </>
          ) : (
            <div className="d-pop__list">
              {allowInherit && (
                <ChoiceRow
                  testId={`${testId}-choice-inherit`}
                  name="Inherit"
                  summary={combinationLabel(inherited)}
                  selected={!choice || choice.mode === "inherit"}
                  onClick={() => {
                    onChange({ mode: "inherit" });
                    close();
                  }}
                />
              )}
              <div className="d-caps d-pop__group">Profiles</div>
              {profiles.map((profile) => (
                <ChoiceRow
                  key={profile.id}
                  // #824: one stable testid per profile, keyed on the id rather
                  // than the (renameable) name — a guided tour has to be able to
                  // aim at « Default », and the reserved id is the fixed part.
                  testId={`${testId}-choice-${profile.id}`}
                  name={profile.name}
                  summary={combinationLabel(profile)}
                  selected={choice?.mode === "profile" && choice.profile_id === profile.id}
                  onClick={() => {
                    onChange({ mode: "profile", profile_id: profile.id });
                    close();
                  }}
                />
              ))}
              <div className="d-pop__sep" />
              <ChoiceRow
                testId={`${testId}-choice-custom`}
                name="Custom…"
                summary=""
                selected={choice?.mode === "custom"}
                onClick={() => {
                  setCustom(choice?.mode === "custom" ? choice : inherited);
                  setCustomPane(true);
                }}
              />
            </div>
          )}
        </div>
      </AnchoredPanel>
    </div>
  );
}

function ChoiceRow({
  name,
  summary,
  selected,
  onClick,
  testId,
}: {
  name: string;
  summary: string;
  selected: boolean;
  onClick: () => void;
  testId?: string;
}) {
  return (
    <button
      type="button"
      data-testid={testId}
      onClick={onClick}
      aria-pressed={selected}
      className="d-pop__row"
      title={summary ? `${name} · ${summary} (— = the harness default)` : name}
    >
      <span className="d-pop__mark">{selected && <Check size={12} strokeWidth={2} aria-hidden />}</span>
      <span className="d-pop__label">{name}</span>
      {summary && <span className="d-pop__hint">{summary}</span>}
    </button>
  );
}
